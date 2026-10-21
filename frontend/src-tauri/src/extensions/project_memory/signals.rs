//! Normalised evidence from calendar invites and transcripts.
use serde_json::Value;
use sqlx::SqlitePool;
use std::collections::BTreeSet;

/// Free mail providers say nothing about which organisation someone is from.
const PUBLIC_DOMAINS: &[&str] = &[
    "aol.com",
    "fastmail.com",
    "freenet.de",
    "gmail.com",
    "gmx.at",
    "gmx.ch",
    "gmx.de",
    "gmx.net",
    "googlemail.com",
    "hey.com",
    "hotmail.com",
    "icloud.com",
    "live.com",
    "mac.com",
    "mail.com",
    "mailbox.org",
    "me.com",
    "msn.com",
    "outlook.com",
    "posteo.de",
    "proton.me",
    "protonmail.com",
    "t-online.de",
    "web.de",
    "yahoo.com",
    "yandex.com",
    "zoho.com",
];

pub fn domain(email: &str) -> Option<String> {
    let (_, domain) = email.rsplit_once('@')?;
    let domain = domain.trim().to_lowercase();
    (domain.contains('.') && !domain.contains(' ')).then_some(domain)
}

pub fn is_public(domain: &str) -> bool {
    PUBLIC_DOMAINS.contains(&domain)
}

/// The person's own addresses and organisations: their own calendar entries
/// and the accounts they connected. Their own domain never identifies a client.
#[derive(Clone, Debug, Default)]
pub struct Own {
    pub emails: BTreeSet<String>,
    pub domains: BTreeSet<String>,
}

impl Own {
    pub async fn load(pool: &SqlitePool) -> Self {
        let mut own = Own::default();
        let accounts: Vec<String> = sqlx::query_scalar(
            "SELECT lower(username) FROM workspace_connections WHERE provider IN ('google','microsoft') AND username LIKE '%@%'",
        )
        .fetch_all(pool)
        .await
        .unwrap_or_default();
        let selves: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT lower(json_extract(p.value,'$.email')) FROM workspace_calendar c, json_each(c.participants) p WHERE json_extract(p.value,'$.self')=1",
        )
        .fetch_all(pool)
        .await
        .unwrap_or_default();
        // Imported calendars name nobody as "self". The person organises or is
        // invited to nearly every event on their calendar; a series counts once,
        // so a weekly one-to-one cannot make a colleague look like the person.
        let appearances = "SELECT lower(json_extract(p.value,'$.email')) AS email,coalesce(nullif(c.series_id,''),c.id) AS invite FROM workspace_calendar c, json_each(c.participants) p UNION SELECT lower(organizer),coalesce(nullif(series_id,''),id) FROM workspace_calendar WHERE organizer<>''";
        let invites: i64 = sqlx::query_scalar(&format!(
            "SELECT count(DISTINCT invite) FROM ({appearances})"
        ))
        .fetch_one(pool)
        .await
        .unwrap_or(0);
        let frequent: Option<(String, i64)> = sqlx::query_as(&format!("SELECT email,count(DISTINCT invite) AS n FROM ({appearances}) WHERE email LIKE '%@%' GROUP BY email ORDER BY n DESC LIMIT 1"))
            .fetch_optional(pool).await.unwrap_or(None);
        let inferred = frequent
            .filter(|(_, n)| *n >= 3 && *n * 10 >= invites * 8)
            .map(|(email, _)| email);
        for email in accounts.into_iter().chain(selves).chain(inferred) {
            if let Some(domain) = domain(&email) {
                if !is_public(&domain) {
                    own.domains.insert(domain);
                }
            }
            own.emails.insert(email);
        }
        own
    }
    pub fn is_own(&self, email: &str) -> bool {
        self.emails.contains(email)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Person {
    pub name: String,
    pub email: String,
    pub declined: bool,
    pub own: bool,
}

impl Person {
    pub fn label(&self) -> String {
        if self.name.trim().is_empty() {
            self.email.clone()
        } else {
            self.name.trim().to_owned()
        }
    }
}

/// An upcoming event or the copy a meeting kept of the event it came from.
#[derive(Clone, Debug, Default)]
pub struct Event {
    pub id: String,
    pub title: String,
    pub source: String,
    pub series_id: String,
    pub starts_at: String,
    pub has_link: bool,
    pub people: Vec<Person>,
}

impl Event {
    pub fn from_row(row: &Value, own: &Own) -> Self {
        let text = |key: &str| row[key].as_str().unwrap_or("").to_owned();
        let participants: Value =
            serde_json::from_str(row["participants"].as_str().unwrap_or("[]")).unwrap_or_default();
        let people = participants
            .as_array()
            .into_iter()
            .flatten()
            .take(200)
            .map(|p| {
                let email = p["email"].as_str().unwrap_or("").trim().to_lowercase();
                let status = p["status"].as_str().unwrap_or("").to_lowercase();
                Person {
                    name: p["name"].as_str().unwrap_or("").to_owned(),
                    own: p["self"] == true || own.is_own(&email),
                    declined: status == "declined",
                    email,
                }
            })
            .collect();
        Event {
            id: text("id"),
            title: text("title"),
            source: text("source"),
            series_id: text("series_id"),
            starts_at: text("starts_at"),
            has_link: row["meeting_url"].as_str().is_some_and(|u| !u.is_empty()),
            people,
        }
    }
    /// Invitees other than the person, who did not decline.
    pub fn guests(&self) -> impl Iterator<Item = &Person> {
        self.people
            .iter()
            .filter(|p| !p.own && !p.declined && !p.email.is_empty())
    }
    pub fn declined_by_me(&self) -> bool {
        self.people.iter().any(|p| p.own && p.declined)
    }
    /// Organisations of the invitees, excluding free mail and the person's own.
    pub fn domains(&self, own: &Own) -> BTreeSet<String> {
        self.guests()
            .filter_map(|p| domain(&p.email))
            .filter(|d| !is_public(d) && !own.domains.contains(d))
            .collect()
    }
}

/// Lower case words with dates, numbers and punctuation removed, so "Orion
/// weekly — 14/10" and "Orion Weekly" compare equal.
pub fn normalise(text: &str) -> String {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty() && !w.chars().all(|c| c.is_ascii_digit()))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whole-word match of an already normalised phrase.
pub fn has_phrase(normalised_text: &str, normalised_phrase: &str) -> bool {
    if normalised_phrase.is_empty() {
        return false;
    }
    format!(" {normalised_text} ").contains(&format!(" {normalised_phrase} "))
}

pub fn count_phrase(normalised_text: &str, normalised_phrase: &str) -> usize {
    let phrase: Vec<&str> = normalised_phrase
        .split(' ')
        .filter(|w| !w.is_empty())
        .collect();
    if phrase.is_empty() {
        return 0;
    }
    let words: Vec<&str> = normalised_text.split(' ').collect();
    words
        .windows(phrase.len())
        .filter(|w| *w == phrase.as_slice())
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalisation_ignores_dates_case_and_punctuation() {
        assert_eq!(normalise("Orion weekly — 14/10, RC2!"), "orion weekly rc2");
        assert!(has_phrase(
            &normalise("Re: Orion weekly sync"),
            "orion weekly"
        ));
        assert!(!has_phrase(&normalise("Orionis launch"), "orion"));
        assert_eq!(count_phrase("for orion orion ships", "orion"), 2);
        assert_eq!(
            count_phrase("orion weekly and orion weekly", "orion weekly"),
            2
        );
        assert_eq!(domain("Maya@Acme.Example").as_deref(), Some("acme.example"));
        assert!(is_public("gmail.com"));
    }
}
