//! Bounded ICS import with recurrence exceptions and timezone-aware expansion.
use crate::state::AppState;
use chrono::{DateTime, Duration, NaiveDate, NaiveDateTime, TimeZone, Utc};
use ical::{parser::ical::component::IcalEvent, property::Property};
use rrule::{Frequency, RRuleSet, Tz};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::collections::{BTreeMap, BTreeSet};
use std::io::BufReader;
use tauri::State;

const MAX_EVENTS: usize = 10_000;
const MAX_EXPANDED_BYTES: usize = 16 * 1024 * 1024;
static IMPORT_JOB: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct Budget {
    bytes: usize,
    steps: u64,
    started: std::time::Instant,
}
impl Budget {
    fn check(&self) -> Result<(), String> {
        if self.started.elapsed() > std::time::Duration::from_secs(5) {
            return Err(
                "Calendar processing exceeded its time budget; import a smaller calendar".into(),
            );
        }
        Ok(())
    }
    fn push(
        &mut self,
        events: &mut Vec<CalendarEvent>,
        event: CalendarEvent,
    ) -> Result<(), String> {
        self.check()?;
        let bytes = [
            &event.id,
            &event.source,
            &event.uid,
            &event.recurrence_id,
            &event.title,
            &event.description,
            &event.starts_at,
            &event.ends_at,
            &event.timezone,
            &event.location,
            &event.participants,
        ]
        .iter()
        .map(|s| s.len())
        .sum::<usize>()
            + event.meeting_url.as_ref().map_or(0, String::len)
            + std::mem::size_of::<CalendarEvent>();
        if events.len() >= MAX_EVENTS || bytes > MAX_EXPANDED_BYTES.saturating_sub(self.bytes) {
            return Err(
                "Expanded calendar exceeds 10,000 events or 16 MiB; import a smaller calendar"
                    .into(),
            );
        }
        if event.ends_at < event.starts_at {
            return Err("A calendar event ends before it starts".into());
        }
        self.bytes += bytes;
        events.push(event);
        Ok(())
    }
}

fn validate_event(event: &IcalEvent) -> Result<(), String> {
    if event.properties.len() > 1024 {
        return Err("Calendar event contains too many properties".into());
    }
    for p in &event.properties {
        let limit = match p.name.as_str() {
            "UID" | "SUMMARY" => 1024,
            "URL" | "LOCATION" => 4096,
            "RRULE" | "EXRULE" => 4096,
            _ => 64 * 1024,
        };
        if p.name.len() > 128
            || p.value.as_ref().is_some_and(|v| v.len() > limit)
            || p.params.as_ref().is_some_and(|pairs| {
                pairs.len() > 32
                    || pairs.iter().any(|(key, values)| {
                        key.len() > 128
                            || values.len() > 32
                            || values.iter().any(|v| v.len() > 4096)
                    })
            })
        {
            return Err("Calendar event field exceeds its size limit".into());
        }
    }
    if event
        .properties
        .iter()
        .filter(|p| p.name == "RRULE")
        .count()
        > 8
    {
        return Err("Calendar event has too many recurrence rules".into());
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct CalendarEvent {
    pub id: String,
    pub source: String,
    pub uid: String,
    pub recurrence_id: String,
    pub title: String,
    pub description: String,
    pub starts_at: String,
    pub ends_at: String,
    pub timezone: String,
    pub all_day: bool,
    pub location: String,
    pub meeting_url: Option<String>,
    #[serde(default)]
    pub participants: String,
    /// Organizer address, lower case, when the calendar provides one.
    #[serde(default)]
    #[sqlx(default)]
    pub organizer: String,
    /// Identifies a recurring series across its occurrences; empty otherwise.
    #[serde(default)]
    #[sqlx(default)]
    pub series_id: String,
}
#[derive(Serialize)]
pub struct ImportReport {
    pub count: usize,
    pub warnings: Vec<String>,
}
fn prop<'a>(event: &'a IcalEvent, name: &str) -> Option<&'a Property> {
    event.properties.iter().find(|p| p.name == name)
}
fn value(event: &IcalEvent, name: &str) -> String {
    prop(event, name)
        .and_then(|p| p.value.clone())
        .unwrap_or_default()
}
fn unescape(value: &str) -> String {
    value
        .replace("\\n", "\n")
        .replace("\\N", "\n")
        .replace("\\,", ",")
        .replace("\\;", ";")
        .replace("\\\\", "\\")
}
pub fn meeting_link(value: &str) -> Option<String> {
    static LINKS: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r#"https://[^\s<>\"\\]+"#).unwrap());
    let regex = &*LINKS;
    let found = regex
        .find_iter(value)
        .map(|m| m.as_str().trim_end_matches(['.', ',', ')', ';']))
        .find(|link| {
            reqwest::Url::parse(link)
                .ok()
                .and_then(|u| u.host_str().map(str::to_owned))
                .is_some_and(|host| {
                    host == "meet.google.com"
                        || host.ends_with(".zoom.us")
                        || host == "zoom.us"
                        || host == "teams.microsoft.com"
                        || host == "teams.live.com"
                        || host.ends_with(".webex.com")
                })
        })
        .map(str::to_owned);
    found
}
#[tauri::command]
pub async fn workspace_join_event(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let link: Option<String> =
        sqlx::query_scalar("SELECT meeting_url FROM workspace_calendar WHERE id=?")
            .bind(id)
            .fetch_optional(state.db_manager.pool())
            .await
            .map_err(|e| e.to_string())?
            .flatten();
    crate::api::open_external_url(link.ok_or("This event has no meeting link")?).await
}
fn parameter<'a>(p: &'a Property, key: &str) -> Option<&'a str> {
    p.params
        .as_ref()?
        .iter()
        .find(|(k, _)| k == key)?
        .1
        .first()
        .map(String::as_str)
}
fn time(p: &Property) -> Result<(DateTime<Tz>, bool), String> {
    let raw = p.value.as_deref().ok_or("Calendar timestamp is empty")?;
    let all_day = parameter(p, "VALUE") == Some("DATE") || raw.len() == 8;
    let zone = if raw.ends_with('Z') {
        Tz::UTC
    } else if let Some(name) = parameter(p, "TZID") {
        Tz::Tz(name.parse::<chrono_tz::Tz>().map_err(|_| {
            format!("Unsupported calendar timezone '{name}'; export using an IANA timezone or UTC")
        })?)
    } else {
        Tz::LOCAL
    };
    let naive = if all_day {
        NaiveDate::parse_from_str(raw, "%Y%m%d")
            .map_err(|e| e.to_string())?
            .and_hms_opt(0, 0, 0)
            .unwrap()
    } else {
        NaiveDateTime::parse_from_str(raw.trim_end_matches('Z'), "%Y%m%dT%H%M%S")
            .map_err(|_| "Use complete ICS timestamps with seconds")?
    };
    // RFC5545 chooses the first occurrence when an hour repeats at DST end.
    let result = zone
        .from_local_datetime(&naive)
        .earliest()
        .ok_or("Calendar time falls in a daylight-saving gap; correct it in the calendar")?;
    Ok((result, all_day))
}
fn recurrence(
    event: &IcalEvent,
    start: DateTime<Tz>,
    after: DateTime<Utc>,
    before: DateTime<Utc>,
    budget: &mut Budget,
) -> Result<Vec<DateTime<Tz>>, String> {
    let mut set = RRuleSet::new(start);
    for property in &event.properties {
        match property.name.as_str() {
            "RRULE" => {
                let rule = property
                    .value
                    .as_deref()
                    .ok_or("Empty recurrence rule")?
                    .parse::<rrule::RRule<rrule::Unvalidated>>()
                    .map_err(|e| e.to_string())?
                    .validate(start)
                    .map_err(|e| e.to_string())?;
                // Bound history traversal as well as emitted dates: .all()
                // only caps dates inside the requested window.
                let seconds = match rule.get_freq() {
                    Frequency::Secondly => 1,
                    Frequency::Minutely => 60,
                    Frequency::Hourly => 3600,
                    Frequency::Daily => 86400,
                    Frequency::Weekly => 7 * 86400,
                    Frequency::Monthly => 28 * 86400,
                    Frequency::Yearly => 365 * 86400,
                };
                let periods = before.signed_duration_since(start).num_seconds().max(0) as u64
                    / (seconds * u64::from(rule.get_interval().max(1)))
                    + 1;
                let steps = rule
                    .get_count()
                    .map_or(periods, |count| periods.min(u64::from(count)));
                if steps > 100_000 || budget.steps.saturating_add(steps) > 1_000_000 {
                    return Err("Recurrence history exceeds the processing budget; export recent occurrences".into());
                }
                budget.steps += steps;
                budget.check()?;
                set = set.rrule(rule);
            }
            "RDATE" | "EXDATE" => {
                if parameter(property, "VALUE") == Some("PERIOD") {
                    return Err("RDATE periods are unsupported; export individual events".into());
                }
                for date in property.value.as_deref().unwrap_or("").split(',') {
                    let p = Property {
                        name: property.name.clone(),
                        params: property.params.clone(),
                        value: Some(date.into()),
                    };
                    let (dt, _) = time(&p)?;
                    set = if property.name == "RDATE" {
                        set.rdate(dt)
                    } else {
                        set.exdate(dt)
                    };
                }
            }
            "EXRULE" => return Err("Legacy EXRULE is unsupported; export using EXDATE".into()),
            _ => {}
        }
    }
    if !event
        .properties
        .iter()
        .any(|p| p.name == "RRULE" || p.name == "RDATE")
    {
        return Ok(
            if start.with_timezone(&Utc) >= after && start.with_timezone(&Utc) <= before {
                vec![start]
            } else {
                vec![]
            },
        );
    }
    // DTSTART is part of the recurrence set even when it doesn't fit the RRULE.
    set = set.rdate(start);
    let result = set
        .after(after.with_timezone(&Tz::UTC))
        .before(before.with_timezone(&Tz::UTC))
        .all(2048);
    if result.limited {
        return Err(
            "Calendar series exceeds 2,048 events in the import window; narrow it upstream".into(),
        );
    }
    let mut dates = result.dates;
    dates.sort();
    dates.dedup();
    Ok(dates)
}
fn build(
    event: &IcalEvent,
    source: &str,
    uid: &str,
    start: DateTime<Tz>,
    end: DateTime<Tz>,
    all_day: bool,
    recurrence_id: String,
) -> CalendarEvent {
    let url = value(event, "URL");
    let address = |p: &Property| {
        p.value
            .as_deref()
            .unwrap_or("")
            .trim()
            .trim_start_matches("mailto:")
            .trim_start_matches("MAILTO:")
            .to_lowercase()
    };
    let organizer = prop(event, "ORGANIZER").map(address).unwrap_or_default();
    let participants: Vec<_> = event
        .properties
        .iter()
        .filter(|p| p.name == "ATTENDEE")
        .take(200)
        .map(|p| {
            let email = address(p);
            serde_json::json!({
                "name": parameter(p, "CN").unwrap_or(""),
                "organizer": !email.is_empty() && email == organizer,
                "email": email,
                "status": parameter(p, "PARTSTAT").unwrap_or("NEEDS-ACTION")
            })
        })
        .collect();
    let recurring = ["RRULE", "RDATE", "RECURRENCE-ID"]
        .iter()
        .any(|name| prop(event, name).is_some());
    CalendarEvent {
        id: crate::security::storage::hex(&sha2::Sha256::digest(format!(
            "{source}\0{uid}\0{recurrence_id}"
        ))),
        source: source.into(),
        uid: uid.into(),
        recurrence_id,
        title: unescape(&value(event, "SUMMARY")),
        description: unescape(&value(event, "DESCRIPTION")),
        starts_at: start.with_timezone(&Utc).to_rfc3339(),
        ends_at: end.with_timezone(&Utc).to_rfc3339(),
        timezone: start.timezone().name().into(),
        all_day,
        location: unescape(&value(event, "LOCATION")),
        participants: serde_json::to_string(&participants).unwrap_or_else(|_| "[]".into()),
        organizer,
        series_id: if recurring { uid.into() } else { String::new() },
        meeting_url: if url.starts_with("https://") || url.starts_with("http://") {
            Some(url)
        } else {
            meeting_link(&format!(
                "{} {}",
                value(event, "LOCATION"),
                value(event, "DESCRIPTION")
            ))
        },
    }
}
use sha2::Digest;
pub fn parse_ics(
    bytes: &[u8],
    source: &str,
    now: DateTime<Utc>,
) -> Result<(Vec<CalendarEvent>, Vec<String>), String> {
    if bytes.len() > 8 * 1024 * 1024 {
        return Err("Calendar imports are limited to 8 MB".into());
    }
    if source.len() > 4096 || bytes.split(|b| *b == b'\n').take(100_001).count() > 100_000 {
        return Err("Calendar source or line count exceeds its size limit".into());
    }
    let mut budget = Budget {
        bytes: 0,
        steps: 0,
        started: std::time::Instant::now(),
    };
    let mut definitions = 0;
    let after = now - Duration::days(30);
    let before = now + Duration::days(366);
    let parser = ical::IcalParser::new(BufReader::new(bytes));
    let mut all = Vec::new();
    let mut warnings = Vec::new();
    for calendar in parser {
        budget.check()?;
        let calendar = calendar.map_err(|e| format!("Cannot parse ICS: {e}"))?;
        if !calendar.timezones.is_empty() {
            warnings.push("Named IANA timezones use the system timezone database; custom VTIMEZONE rules are not imported.".into());
        }
        let mut masters: BTreeMap<String, IcalEvent> = BTreeMap::new();
        let mut overrides: BTreeMap<(String, String), IcalEvent> = BTreeMap::new();
        definitions += calendar.events.len();
        if definitions > MAX_EVENTS {
            return Err("Calendar contains more than 10,000 event definitions".into());
        }
        for event in calendar.events {
            validate_event(&event)?;
            let uid = value(&event, "UID");
            if uid.is_empty() {
                warnings.push("An event without a UID was skipped.".into());
                continue;
            }
            if let Some(rid) = prop(&event, "RECURRENCE-ID") {
                if parameter(rid, "RANGE").is_some() {
                    warnings.push(format!("Skipped RANGE recurrence override for {uid}; export individual exceptions."));
                    continue;
                }
                let (date, _) = time(rid)?;
                overrides.insert((uid, date.with_timezone(&Utc).to_rfc3339()), event);
            } else {
                masters.insert(uid, event);
            }
        }
        let mut consumed = BTreeSet::new();
        for (uid, master) in masters {
            budget.check()?;
            if value(&master, "STATUS") == "CANCELLED" {
                continue;
            }
            let start_prop =
                prop(&master, "DTSTART").ok_or_else(|| format!("Event {uid} has no start time"))?;
            let (start, all_day) = time(start_prop)?;
            let end = match prop(&master, "DTEND") {
                Some(p) => time(p)?.0,
                None => {
                    if prop(&master, "DURATION").is_some() {
                        warnings.push(format!("Skipped DURATION event {uid}; export with DTEND."));
                        continue;
                    }
                    if all_day {
                        start + Duration::days(1)
                    } else {
                        start
                    }
                }
            };
            let duration = end.signed_duration_since(start);
            if duration < Duration::zero() {
                return Err(format!("Event {uid} ends before it starts"));
            }
            let dates = match recurrence(&master, start, after, before, &mut budget) {
                Ok(dates) => dates,
                Err(e) => {
                    warnings.push(format!("Skipped series {uid}: {e}"));
                    continue;
                }
            };
            let template = build(&master, source, &uid, start, end, all_day, String::new());
            for date in dates {
                let key = date.with_timezone(&Utc).to_rfc3339();
                let override_key = (uid.clone(), key.clone());
                if let Some(event) = overrides.get(&override_key) {
                    consumed.insert(override_key);
                    if value(event, "STATUS") == "CANCELLED" {
                        continue;
                    }
                    let (s, a) =
                        time(prop(event, "DTSTART").ok_or("An exception has no start time")?)?;
                    let e = if let Some(p) = prop(event, "DTEND") {
                        time(p)?.0
                    } else {
                        s + duration
                    };
                    budget.push(&mut all, build(event, source, &uid, s, e, a, key))?;
                } else {
                    let mut occurrence = template.clone();
                    occurrence.id = crate::security::storage::hex(&sha2::Sha256::digest(format!(
                        "{source}\0{uid}\0{key}"
                    )));
                    occurrence.recurrence_id = key;
                    occurrence.starts_at = date.with_timezone(&Utc).to_rfc3339();
                    occurrence.ends_at = (date + duration).with_timezone(&Utc).to_rfc3339();
                    budget.push(&mut all, occurrence)?;
                }
            }
        }
        // Include a detached exception moved INTO the window from outside it.
        for (key, event) in overrides {
            if consumed.contains(&key) || value(&event, "STATUS") == "CANCELLED" {
                continue;
            }
            if let Some(p) = prop(&event, "DTSTART") {
                let (s, a) = time(p)?;
                if s.with_timezone(&Utc) >= after && s.with_timezone(&Utc) <= before {
                    let e = if let Some(p) = prop(&event, "DTEND") {
                        time(p)?.0
                    } else {
                        if a {
                            s + Duration::days(1)
                        } else {
                            s
                        }
                    };
                    budget.push(&mut all, build(&event, source, &key.0, s, e, a, key.1))?;
                }
            }
        }
    }
    if all.len() > 10000 {
        return Err("Expanded calendar exceeds 10,000 events".into());
    }
    all.sort_by(|a, b| a.starts_at.cmp(&b.starts_at));
    warnings.sort();
    warnings.dedup();
    Ok((all, warnings))
}
#[tauri::command]
pub async fn workspace_import_calendar(
    app: tauri::AppHandle,
    path: String,
    state: State<'_, AppState>,
) -> Result<ImportReport, String> {
    // Keep the admission guard inside the blocking parser too. Cancelling the
    // async request must not admit another parser while this one is still running.
    let guard = IMPORT_JOB
        .try_lock()
        .map_err(|_| "A calendar import is already running")?;
    let canonical = std::path::Path::new(&path)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if canonical
        .extension()
        .and_then(|s| s.to_str())
        .is_none_or(|s| !s.eq_ignore_ascii_case("ics"))
    {
        return Err("Select an .ics calendar file".into());
    }
    let source = canonical.to_string_lossy().into_owned();
    let parse_source = source.clone();
    let (events, warnings, _guard) = tokio::task::spawn_blocking(move || {
        let data = crate::security::storage::read_limited(&canonical, 8 * 1024 * 1024)
            .map_err(|e| format!("Could not read calendar (8 MiB limit): {e}"))?;
        let (events, warnings) = parse_ics(&data, &parse_source, Utc::now())?;
        Ok::<_, String>((events, warnings, guard))
    })
    .await
    .map_err(|e| e.to_string())??;
    let count = events.len();
    let mut tx = state
        .db_manager
        .pool()
        .begin()
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM workspace_calendar WHERE source=?")
        .bind(&source)
        .execute(&mut *tx)
        .await
        .map_err(|e| e.to_string())?;
    for event in events {
        sqlx::query("INSERT INTO workspace_calendar(id,source,uid,recurrence_id,title,description,starts_at,ends_at,timezone,all_day,location,meeting_url,participants,organizer,series_id) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)")
            .bind(event.id)
            .bind(event.source)
            .bind(event.uid)
            .bind(event.recurrence_id)
            .bind(event.title)
            .bind(event.description)
            .bind(event.starts_at)
            .bind(event.ends_at)
            .bind(event.timezone)
            .bind(event.all_day)
            .bind(event.location)
            .bind(event.meeting_url)
            .bind(event.participants)
            .bind(event.organizer)
            .bind(event.series_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
    }
    tx.commit().await.map_err(|e| e.to_string())?;
    super::meeting_workflow::calendar_changed(&app, state.db_manager.pool()).await;
    Ok(ImportReport { count, warnings })
}
#[tauri::command]
pub async fn workspace_calendar_events(
    state: State<'_, AppState>,
) -> Result<Vec<CalendarEvent>, String> {
    sqlx::query_as(
        "SELECT * FROM workspace_calendar WHERE ends_at>=? ORDER BY starts_at LIMIT 1000",
    )
    .bind((Utc::now() - Duration::days(1)).to_rfc3339())
    .fetch_all(state.db_manager.pool())
    .await
    .map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn workspace_delete_calendar_event(
    id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    sqlx::query("DELETE FROM workspace_calendar WHERE id=?")
        .bind(id)
        .execute(state.db_manager.pool())
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn parse_fixture(events: &str) -> Result<(Vec<CalendarEvent>, Vec<String>), String> {
        parse_ics(
            format!("BEGIN:VCALENDAR\nVERSION:2.0\n{events}END:VCALENDAR\n").as_bytes(),
            "fixture",
            DateTime::parse_from_rfc3339("2026-10-18T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
        )
    }
    #[test]
    fn recurrence_cannot_multiply_large_payloads_past_the_import_budget() {
        let events = format!("BEGIN:VEVENT\nUID:budget\nDTSTART:20261018T100000Z\nRRULE:FREQ=HOURLY;COUNT=512\nDESCRIPTION:{}\nEND:VEVENT\n", "x".repeat(40 * 1024));
        assert!(parse_fixture(&events).is_err());
    }
    #[test]
    fn calendar_rejects_oversized_event_fields_before_expansion() {
        let events = format!(
            "BEGIN:VEVENT\nUID:large\nDTSTART:20261018T100000Z\nDESCRIPTION:{}\nEND:VEVENT\n",
            "x".repeat(65 * 1024)
        );
        assert!(parse_fixture(&events).is_err());
    }
    #[test]
    fn recurrence_exception_cannot_end_before_it_starts() {
        let events = "BEGIN:VEVENT\nUID:weekly\nDTSTART:20261018T100000Z\nDTEND:20261018T110000Z\nRRULE:FREQ=DAILY;COUNT=2\nEND:VEVENT\nBEGIN:VEVENT\nUID:weekly\nRECURRENCE-ID:20261019T100000Z\nDTSTART:20261019T140000Z\nDTEND:20261019T130000Z\nEND:VEVENT\n";
        assert!(parse_fixture(events).is_err());
    }
    #[test]
    fn old_high_frequency_series_is_rejected_before_history_iteration() {
        let events =
            "BEGIN:VEVENT\nUID:old\nDTSTART:19900101T100000Z\nRRULE:FREQ=SECONDLY\nEND:VEVENT\n";
        let (events, warnings) = parse_fixture(events).unwrap();
        assert!(events.is_empty());
        assert!(warnings.iter().any(|w| w.contains("history exceeds")));
    }
    #[test]
    fn occurrence_limit_applies_while_building_results() {
        let mut events = String::new();
        for i in 0..21 {
            events.push_str(&format!("BEGIN:VEVENT\nUID:s{i:02}\nDTSTART:20261018T100000Z\nRRULE:FREQ=HOURLY;COUNT=500\nEND:VEVENT\n"));
        }
        // This invalid definition must not be reached after the output budget
        // is exhausted. The original implementation only checked at the end.
        events.push_str("BEGIN:VEVENT\nUID:zzz\nEND:VEVENT\n");
        assert!(parse_fixture(&events)
            .unwrap_err()
            .contains("Expanded calendar"));
    }
    #[test]
    fn recurrence_copies_preserve_content_links_and_identity() {
        let (events, warnings) = parse_fixture("BEGIN:VEVENT\nUID:copies\nDTSTART:20261018T100000Z\nDTEND:20261018T110000Z\nRRULE:FREQ=DAILY;COUNT=3\nSUMMARY:Review\nDESCRIPTION:Join https://meet.google.com/abc-defg-hij\nATTENDEE;CN=Alex:mailto:alex@example.test\nEND:VEVENT\n").unwrap();
        assert!(warnings.is_empty());
        assert_eq!(events.len(), 3);
        assert_ne!(events[0].id, events[1].id);
        for event in events {
            assert_eq!(event.title, "Review");
            assert_eq!(
                event.meeting_url.as_deref(),
                Some("https://meet.google.com/abc-defg-hij")
            );
            assert!(event.participants.contains("Alex"));
            assert_eq!(
                DateTime::parse_from_rfc3339(&event.ends_at).unwrap()
                    - DateTime::parse_from_rfc3339(&event.starts_at).unwrap(),
                Duration::hours(1)
            );
        }
    }
    #[test]
    fn recurrence_keeps_dst_local_time_excludes_cancelled_and_moves_exceptions() {
        let ics = b"BEGIN:VCALENDAR\r\n\
                    VERSION:2.0\r\n\
                    BEGIN:VEVENT\r\n\
                    UID:weekly\r\n\
                    DTSTART;TZID=Europe/Berlin:20261020T100000\r\n\
                    DTEND;TZID=Europe/Berlin:20261020T110000\r\n\
                    RRULE:FREQ=WEEKLY;COUNT=3\r\n\
                    SUMMARY:Review\r\n\
                    END:VEVENT\r\n\
                    BEGIN:VEVENT\r\n\
                    UID:weekly\r\n\
                    RECURRENCE-ID;TZID=Europe/Berlin:20261027T100000\r\n\
                    DTSTART;TZID=Europe/Berlin:20261027T140000\r\n\
                    DTEND;TZID=Europe/Berlin:20261027T150000\r\n\
                    SUMMARY:Moved review\r\n\
                    END:VEVENT\r\n\
                    BEGIN:VEVENT\r\n\
                    UID:weekly\r\n\
                    RECURRENCE-ID;TZID=Europe/Berlin:20261103T100000\r\n\
                    STATUS:CANCELLED\r\n\
                    END:VEVENT\r\n\
                    END:VCALENDAR\r\n";
        let (events, _) = parse_ics(
            ics,
            "fixture",
            DateTime::parse_from_rfc3339("2026-09-30T10:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
        )
        .unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].starts_at, "2026-10-20T08:00:00+00:00");
        assert_eq!(events[1].starts_at, "2026-10-27T13:00:00+00:00");
        assert_eq!(events[1].title, "Moved review");
    }
    #[test]
    fn all_day_dtend_is_exclusive_and_floating_time_is_explicit() {
        let ics=b"BEGIN:VCALENDAR\nVERSION:2.0\nBEGIN:VEVENT\nUID:day\nDTSTART;VALUE=DATE:20261001\nDTEND;VALUE=DATE:20261003\nSUMMARY:Workshop\nEND:VEVENT\nEND:VCALENDAR\n";
        let (events, _) = parse_ics(
            ics,
            "fixture",
            DateTime::parse_from_rfc3339("2026-09-30T10:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
        )
        .unwrap();
        assert_eq!(events.len(), 1);
        assert!(events[0].all_day);
        assert_eq!(events[0].timezone, "Local");
        let start = DateTime::parse_from_rfc3339(&events[0].starts_at).unwrap();
        let end = DateTime::parse_from_rfc3339(&events[0].ends_at).unwrap();
        assert_eq!(end - start, Duration::days(2));
    }
}
