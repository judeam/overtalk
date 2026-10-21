//! Meetings, their notes with sources, transcripts and literal search.
use std::collections::HashMap;

use serde_json::{json, Value};

use crate::args::Args;
use crate::notes::{self, clock, notes_markdown, Item, Kind};
use crate::{Arg, Grant, Grants, Store};

const FIND: &str = concat!(
    "SELECT json_object(",
    meeting_fields!(),
    ",'has_notes',json(iif(EXISTS(SELECT 1 FROM summary_processes s \
     WHERE s.meeting_id=m.id AND coalesce(s.result,'')<>''),'true','false')),\
     'has_transcript',json(iif(EXISTS(SELECT 1 FROM transcripts t \
     WHERE t.meeting_id=m.id),'true','false')))",
    meeting_rows!(""),
    " AND (?1 IS NULL OR p.id=?1) \
     AND (?2 IS NULL OR date(m.created_at,'localtime')>=?2) \
     AND (?3 IS NULL OR date(m.created_at,'localtime')<=?3) \
     AND (?4 IS NULL OR instr(lower(m.title),lower(?4))>0) \
     ORDER BY julianday(m.created_at) DESC,m.id LIMIT ?5 OFFSET ?6"
);

const MEETING: &str = concat!(
    "SELECT json_object(",
    meeting_fields!(),
    ")",
    meeting_rows!(""),
    " AND m.id=?1"
);

const PROJECT: &str = "SELECT json_object('id',id,'name',name) FROM projects \
    WHERE id=?1 OR lower(name)=lower(?1) ORDER BY id=?1 DESC,archived,id LIMIT 1";

const NOTES: &str = "SELECT json_object('status',status,'updated_at',updated_at,\
    'result',json(iif(json_valid(result),result,NULL))) \
    FROM summary_processes WHERE meeting_id=?1";

/// Lines are numbered as notes generation numbers them: finished lines in time order.
const CITED: &str = "WITH numbered AS (SELECT \
    row_number() OVER (ORDER BY t.audio_start_time,t.id) AS n,t.audio_start_time AS start,\
    t.audio_end_time AS stop,t.timestamp AS stamp,coalesce(s.name,t.speaker_id) AS speaker,\
    t.transcript AS words FROM transcripts t LEFT JOIN meeting_speakers s \
    ON s.meeting_id=t.meeting_id AND s.id=t.speaker_id \
    WHERE t.meeting_id=?1 AND coalesce(t.is_partial,0)=0) \
    SELECT json_object('n',n,'start',start,'end',stop,'stamp',stamp,'speaker',speaker,\
    'quote',iif(?3,words,NULL)) FROM numbered WHERE n IN (SELECT value FROM json_each(?2))";

const LINES: &str = "SELECT json_object('start',t.audio_start_time,'end',t.audio_end_time,\
    'stamp',t.timestamp,'speaker',coalesce(s.name,t.speaker_id),'text',t.transcript) \
    FROM transcripts t LEFT JOIN meeting_speakers s \
    ON s.meeting_id=t.meeting_id AND s.id=t.speaker_id \
    WHERE t.meeting_id=?1 AND coalesce(t.is_partial,0)=0 \
    ORDER BY t.audio_start_time,t.id LIMIT ?2 OFFSET ?3";

const LINE_COUNT: &str = "SELECT json_object('count',count(*)) FROM transcripts \
    WHERE meeting_id=?1 AND coalesce(is_partial,0)=0";

const SPEAKERS: &str = "SELECT json_object('name',name) FROM meeting_speakers \
    WHERE meeting_id=?1 AND trim(name)<>'' ORDER BY id";

const INVITEES: &str = "SELECT json_object('organizer',organizer,\
    'participants',json(iif(json_valid(participants),participants,'[]'))) \
    FROM meeting_events WHERE meeting_id=?1";

const TITLE_SEARCH: &str = concat!(
    "SELECT json_object('kind','title',",
    meeting_fields!(),
    ")",
    meeting_rows!(""),
    " AND instr(lower(m.title),lower(?1))>0 \
     ORDER BY julianday(m.created_at) DESC,m.id LIMIT ?2"
);

const NOTES_SEARCH: &str = concat!(
    "SELECT json_object(",
    meeting_fields!(),
    ",'notes',json((SELECT iif(json_valid(s.result),s.result,NULL) \
     FROM summary_processes s WHERE s.meeting_id=m.id)))",
    meeting_rows!(""),
    " AND EXISTS(SELECT 1 FROM summary_processes s \
     WHERE s.meeting_id=m.id AND instr(lower(s.result),lower(?1))>0) \
     ORDER BY julianday(m.created_at) DESC,m.id LIMIT ?2"
);

const TRANSCRIPT_SEARCH: &str = concat!(
    "WITH numbered AS (SELECT t.meeting_id,t.id,t.audio_start_time AS start,\
     t.timestamp AS stamp,coalesce(s.name,t.speaker_id) AS speaker,t.transcript AS words,\
     row_number() OVER (PARTITION BY t.meeting_id ORDER BY t.audio_start_time,t.id) AS n \
     FROM transcripts t LEFT JOIN meeting_speakers s \
     ON s.meeting_id=t.meeting_id AND s.id=t.speaker_id \
     WHERE coalesce(t.is_partial,0)=0 AND t.meeting_id IN \
     (SELECT meeting_id FROM transcripts WHERE instr(lower(transcript),lower(?1))>0)) \
     SELECT json_object('kind','transcript',",
    meeting_fields!(),
    ",'source','S'||x.n,'start',x.start,'stamp',x.stamp,'speaker',x.speaker,'text',x.words)",
    meeting_rows!("JOIN numbered x ON x.meeting_id=m.id"),
    " AND instr(lower(x.words),lower(?1))>0 \
     ORDER BY julianday(m.created_at) DESC,m.id,x.n LIMIT ?2"
);

/// The project named or identified by `name`; an unknown name is an error, not an empty list.
pub(crate) async fn project<S: Store>(
    store: &mut S,
    name: Option<String>,
) -> Result<Option<Value>, String> {
    let Some(name) = name else {
        return Ok(None);
    };
    match store.rows(PROJECT, &[name.as_str().into()]).await?.pop() {
        Some(project) => Ok(Some(project)),
        None => Err(format!(
            "No project is called “{name}”. List projects to see their names."
        )),
    }
}

pub(crate) fn project_id(project: &Option<Value>) -> Arg {
    project
        .as_ref()
        .and_then(|project| project["id"].as_str())
        .map_or(Arg::Null, Arg::from)
}

pub(crate) async fn find<S: Store>(store: &mut S, args: &Value) -> Result<Value, String> {
    let args = Args::new(
        args,
        &["project", "since", "until", "title", "limit", "offset"],
    )?;
    let limit = args.number("limit", 50, 1..=500)?;
    let offset = args.number("offset", 0, 0..=1_000_000)?;
    let (since, until, title) = (
        args.date("since")?,
        args.date("until")?,
        args.text("title")?,
    );
    let project = project(store, args.text("project")?).await?;
    let mut meetings = store
        .rows(
            FIND,
            &[
                project_id(&project),
                since.into(),
                until.into(),
                title.into(),
                Arg::Int(limit + 1),
                Arg::Int(offset),
            ],
        )
        .await?;
    let has_more = meetings.len() as i64 > limit;
    meetings.truncate(limit as usize);
    Ok(json!({"meetings": meetings, "has_more": has_more}))
}

async fn meeting<S: Store>(store: &mut S, id: &str) -> Result<Value, String> {
    store
        .rows(MEETING, &[id.into()])
        .await?
        .pop()
        .ok_or_else(|| "Meeting not found".to_owned())
}

/// Time, speaker and, with transcript access, the words of each cited line.
pub(crate) async fn cite<S: Store>(
    store: &mut S,
    meeting: &str,
    numbers: impl IntoIterator<Item = u64>,
    quotes: bool,
) -> Result<HashMap<u64, Value>, String> {
    let mut numbers: Vec<u64> = numbers.into_iter().collect();
    numbers.sort_unstable();
    numbers.dedup();
    if numbers.is_empty() {
        return Ok(HashMap::new());
    }
    let rows = store
        .rows(
            CITED,
            &[
                meeting.into(),
                Arg::Text(json!(numbers).to_string()),
                Arg::Int(quotes.into()),
            ],
        )
        .await?;
    Ok(rows
        .into_iter()
        .filter_map(|row| {
            let number = row["n"].as_u64()?;
            let mut source = json!({
                "id": format!("S{number}"),
                "start": row["start"],
                "end": row["end"],
                "time": line_time(&row),
                "speaker": row["speaker"],
            });
            if !row["quote"].is_null() {
                source["quote"] = row["quote"].clone();
            }
            Some((number, source))
        })
        .collect())
}

/// A line's place in the recording, or the time it was written at for older lines.
fn line_time(row: &Value) -> Value {
    match row["start"].as_f64() {
        Some(start) => json!(clock(start)),
        None => row["stamp"].clone(),
    }
}

/// Text with its sources, timed by the first cited line or else by a written time.
fn cited_json(
    text: &str,
    numbers: &[u64],
    written: Option<f64>,
    cited: &HashMap<u64, Value>,
) -> Value {
    let sources: Vec<Value> = numbers
        .iter()
        .filter_map(|number| cited.get(number).cloned())
        .collect();
    let time = sources
        .first()
        .map(|source| source["time"].clone())
        .or_else(|| written.map(|time| json!(clock(time))))
        .unwrap_or(Value::Null);
    json!({"text": text, "time": time, "sources": sources})
}

pub(crate) fn item_json(item: &Item, cited: &HashMap<u64, Value>) -> Value {
    let mut value = cited_json(&item.text, &item.sources, item.written_time, cited);
    if item.kind == Kind::Action {
        value["owner"] = json!(item.owner);
        value["due"] = json!(item.due);
    }
    value
}

/// Named speakers first, then invitees of the linked calendar event.
async fn people<S: Store>(store: &mut S, id: &str) -> Result<Vec<Value>, String> {
    let mut people: Vec<Value> = store
        .rows(SPEAKERS, &[id.into()])
        .await?
        .into_iter()
        .map(|row| json!({"name": row["name"], "spoke": true, "invited": false}))
        .collect();
    for event in store.rows(INVITEES, &[id.into()]).await? {
        for person in event["participants"].as_array().into_iter().flatten() {
            let (name, email) = match person {
                Value::String(name) => (name.trim(), ""),
                _ => (
                    person["name"].as_str().unwrap_or_default().trim(),
                    person["email"].as_str().unwrap_or_default().trim(),
                ),
            };
            let name = if name.is_empty() { email } else { name };
            if name.is_empty() {
                continue;
            }
            let known = people.iter_mut().find(|known| {
                known["name"]
                    .as_str()
                    .is_some_and(|known| known.to_lowercase() == name.to_lowercase())
            });
            let entry = match known {
                Some(known) => known,
                None => {
                    people.push(json!({"name": name, "spoke": false}));
                    people.last_mut().expect("just pushed")
                }
            };
            entry["invited"] = json!(true);
            if !email.is_empty() {
                entry["email"] = json!(email);
            }
        }
    }
    Ok(people)
}

pub(crate) async fn show<S: Store>(
    store: &mut S,
    grants: Grants,
    args: &Value,
) -> Result<Value, String> {
    let args = Args::new(args, &["meeting_id"])?;
    let id = args.required("meeting_id")?;
    let mut meeting = meeting(store, &id).await?;
    let saved = store.rows(NOTES, &[id.as_str().into()]).await?.pop();
    let markdown = saved
        .as_ref()
        .map(|saved| notes_markdown(&saved["result"]))
        .unwrap_or_default();
    let items = notes::items(&markdown);
    let cited = cite(
        store,
        &id,
        items.iter().flat_map(|item| item.sources.iter().copied()),
        grants.read_transcripts,
    )
    .await?;
    let of = |kind: Kind| -> Vec<Value> {
        items
            .iter()
            .filter(|item| item.kind == kind)
            .map(|item| item_json(item, &cited))
            .collect()
    };
    meeting["decisions"] = json!(of(Kind::Decision));
    meeting["actions"] = json!(of(Kind::Action));
    meeting["open_questions"] = json!(of(Kind::Question));
    meeting["people"] = json!(people(store, &id).await?);
    meeting["notes"] = saved.map_or(Value::Null, |saved| {
        json!({"status": saved["status"], "updated_at": saved["updated_at"], "markdown": markdown})
    });
    Ok(meeting)
}

pub(crate) async fn transcript<S: Store>(store: &mut S, args: &Value) -> Result<Value, String> {
    let args = Args::new(args, &["meeting_id", "limit", "offset"])?;
    let id = args.required("meeting_id")?;
    let limit = args.number("limit", 500, 1..=500)?;
    let offset = args.number("offset", 0, 0..=1_000_000)?;
    let mut meeting = meeting(store, &id).await?;
    let total = store
        .rows(LINE_COUNT, &[id.as_str().into()])
        .await?
        .pop()
        .and_then(|row| row["count"].as_i64())
        .unwrap_or(0);
    let rows = store
        .rows(
            LINES,
            &[id.as_str().into(), Arg::Int(limit), Arg::Int(offset)],
        )
        .await?;
    let lines: Vec<Value> = rows
        .iter()
        .zip(offset + 1..)
        .map(|(row, number)| {
            json!({
                "source": format!("S{number}"),
                "start": row["start"],
                "end": row["end"],
                "time": line_time(row),
                "speaker": row["speaker"],
                "text": row["text"],
            })
        })
        .collect();
    meeting["has_more"] = json!(offset + (lines.len() as i64) < total);
    meeting["total_count"] = json!(total);
    meeting["lines"] = json!(lines);
    Ok(meeting)
}

/// Literal search in what the grants allow: titles always, notes and transcripts when granted.
pub(crate) async fn search<S: Store>(
    store: &mut S,
    grants: Grants,
    args: &Value,
) -> Result<Value, String> {
    let args = Args::new(args, &["query", "limit"])?;
    let query = args.required("query")?;
    let limit = args.number("limit", 30, 1..=200)?;
    let needle = query.to_lowercase();
    let mut results = store
        .rows(TITLE_SEARCH, &[query.as_str().into(), Arg::Int(limit)])
        .await?;
    let mut not_searched = Vec::new();
    if grants.read_summaries {
        for mut meeting in store
            .rows(NOTES_SEARCH, &[query.as_str().into(), Arg::Int(limit)])
            .await?
        {
            let markdown = notes_markdown(&meeting["notes"]);
            if let Value::Object(fields) = &mut meeting {
                fields.remove("notes");
            }
            // The database matched the stored document; keep only lines that say it.
            let lines: Vec<_> = markdown
                .lines()
                .filter(|line| line.to_lowercase().contains(&needle))
                .take(5)
                .map(notes::line)
                .filter(|(text, _, _)| !text.is_empty())
                .collect();
            let id = meeting["id"].as_str().unwrap_or_default().to_owned();
            let numbers = lines
                .iter()
                .flat_map(|(_, numbers, _)| numbers.iter().copied());
            let cited = cite(store, &id, numbers, grants.read_transcripts).await?;
            for (text, numbers, written) in &lines {
                let mut hit = meeting.clone();
                hit["kind"] = json!("notes");
                for (key, value) in cited_json(text, numbers, *written, &cited)
                    .as_object()
                    .into_iter()
                    .flatten()
                {
                    hit[key] = value.clone();
                }
                results.push(hit);
            }
        }
    } else {
        not_searched.push(json!({"content": "notes", "reason": Grant::Notes.missing()}));
    }
    if grants.read_transcripts {
        for mut hit in store
            .rows(TRANSCRIPT_SEARCH, &[query.as_str().into(), Arg::Int(limit)])
            .await?
        {
            hit["time"] = line_time(&hit);
            if let Value::Object(fields) = &mut hit {
                fields.remove("stamp");
            }
            results.push(hit);
        }
    } else {
        not_searched
            .push(json!({"content": "transcripts", "reason": Grant::Transcripts.missing()}));
    }
    Ok(json!({"query": query, "results": results, "not_searched": not_searched}))
}
