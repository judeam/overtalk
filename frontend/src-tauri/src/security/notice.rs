//! The notice people hear or read before a recording, and the per-meeting record
//! of how it was given. The wording follows where meeting content goes when the
//! recording starts. A record is the operator's statement, not a legal assessment
//! or proof that every participant saw the notice.
use super::privacy::Policy;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::SqlitePool;

pub const LOCAL_NOTICE: &str = "I'm recording this call to take notes. It is recorded and transcribed on my computer; nothing is sent to a cloud service.";
/// `{services}` becomes what is sent where, for example "the transcript is sent
/// to Anthropic to write the notes".
pub const EXTERNAL_NOTICE: &str =
    "I'm recording this call to take notes. It is recorded on my computer, and {services}.";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Method {
    Copied,
    ReadAloud,
    /// Participants were informed some other way, as the earlier dialog asked.
    Confirmed,
    Skipped,
}
impl Method {
    fn as_str(self) -> &'static str {
        match self {
            Self::Copied => "copied",
            Self::ReadAloud => "read_aloud",
            Self::Confirmed => "confirmed",
            Self::Skipped => "skipped",
        }
    }
}

/// Services outside this computer that receive meeting content; `None` is local.
#[derive(Debug, Default, PartialEq)]
pub struct Route {
    pub transcription: Option<String>,
    pub notes: Option<String>,
    /// The transcript is encrypted on this computer before it leaves, as
    /// Privatemode's local proxy does for Privatemode and Overtalk cloud notes.
    pub notes_encrypted: bool,
}
impl Route {
    pub fn is_local(&self) -> bool {
        self.transcription.is_none() && self.notes.is_none()
    }
    fn services(&self) -> String {
        match (&self.transcription, &self.notes) {
            (Some(t), Some(n)) if t == n => {
                format!("the audio is sent to {t} for transcription and notes")
            }
            (Some(t), Some(n)) if self.notes_encrypted => format!(
                "the audio is sent to {t} for transcription; the transcript is encrypted before it is sent to {n} to write the notes"
            ),
            (Some(t), Some(n)) => format!(
                "the audio is sent to {t} for transcription and the transcript to {n} to write the notes"
            ),
            (Some(t), None) => format!("the audio is sent to {t} for transcription"),
            (None, Some(n)) if self.notes_encrypted => format!(
                "the transcript is encrypted before it is sent to {n} to write the notes"
            ),
            (None, Some(n)) => format!("the transcript is sent to {n} to write the notes"),
            (None, None) => String::new(),
        }
    }
}

/// The person's wording for this route, or the default when they left it blank.
pub fn text(policy: &Policy, route: &Route) -> String {
    let chosen = |custom: &str, default: &str| {
        if custom.trim().is_empty() {
            default.to_owned()
        } else {
            custom.trim().to_owned()
        }
    };
    if route.is_local() {
        chosen(&policy.recording_notice_local, LOCAL_NOTICE)
    } else {
        chosen(&policy.recording_notice_external, EXTERNAL_NOTICE)
            .replace("{services}", &route.services())
    }
}

fn host(endpoint: &str) -> Option<String> {
    let url = super::transport::endpoint(endpoint).ok()?;
    (!super::transport::is_loopback(&url)).then(|| url.host_str().unwrap_or(endpoint).to_owned())
}

fn speech_service(provider: &str) -> Option<String> {
    match provider {
        "localWhisper" | "parakeet" | "localSpeech" => None,
        "openai" => Some("OpenAI".into()),
        "groq" => Some("Groq".into()),
        "deepgram" => Some("Deepgram".into()),
        "elevenLabs" => Some("ElevenLabs".into()),
        other => Some(other.into()),
    }
}

/// Ollama and OpenAI-compatible servers on this computer stay local; elsewhere
/// they are named by host. Privatemode's proxy forwards to its cloud wherever it
/// runs, and Overtalk cloud notes go through the same proxy.
fn notes_service(provider: &str, ollama: Option<&str>, custom: Option<&str>) -> Option<String> {
    match provider {
        "builtin-ai" | "local-llama" | "localllama" => None,
        "ollama" => match ollama.filter(|e| !e.trim().is_empty()) {
            None => None,
            Some(endpoint) => host(endpoint).or_else(|| {
                super::transport::endpoint(endpoint)
                    .is_err()
                    .then(|| "an Ollama server".into())
            }),
        },
        "custom-openai" => match custom.filter(|e| !e.trim().is_empty()) {
            Some(endpoint) if super::transport::endpoint(endpoint).is_ok() => host(endpoint),
            _ => Some("an OpenAI-compatible service".into()),
        },
        "openai" => Some("OpenAI".into()),
        "claude" => Some("Anthropic".into()),
        "groq" => Some("Groq".into()),
        "openrouter" => Some("OpenRouter".into()),
        "privatemode" | "overtalk-cloud" => Some("Privatemode".into()),
        other => Some(other.into()),
    }
}

/// Where a recording started now would go. Notes count only when they are
/// written automatically after this recording.
pub async fn route(pool: &SqlitePool, policy: &Policy, auto_notes: bool) -> Result<Route, String> {
    if policy.local_only {
        return Ok(Route::default());
    }
    let speech: Option<String> =
        sqlx::query_scalar("SELECT provider FROM transcript_settings LIMIT 1")
            .fetch_optional(pool)
            .await
            .map_err(|_| "Could not read speech settings")?;
    let notes: Option<(String, String, Option<String>, Option<String>)> = sqlx::query_as("SELECT provider,model,ollamaEndpoint,CASE WHEN json_valid(customOpenAIConfig) THEN json_extract(customOpenAIConfig,'$.endpoint') END FROM settings LIMIT 1")
        .fetch_optional(pool)
        .await
        .map_err(|_| "Could not read notes settings")?;
    let notes = notes.filter(|(_, model, _, _)| auto_notes && !model.trim().is_empty());
    Ok(Route {
        transcription: speech.as_deref().and_then(speech_service),
        notes_encrypted: notes.as_ref().is_some_and(|(provider, ..)| {
            matches!(provider.as_str(), "privatemode" | "overtalk-cloud")
        }),
        notes: notes.and_then(|(provider, _, ollama, custom)| {
            notes_service(&provider, ollama.as_deref(), custom.as_deref())
        }),
    })
}

/// What the notice step shows before a recording starts.
pub async fn preview(
    pool: &SqlitePool,
    policy: &Policy,
    auto_notes: bool,
) -> Result<Value, String> {
    let route = route(pool, policy, auto_notes).await?;
    Ok(json!({
        "text": text(policy, &route),
        "route": if route.is_local() { "local" } else { "external" },
        "detail": if route.is_local() {
            "Your setup: speech and notes stay on this computer.".to_owned()
        } else {
            format!("Your setup: {}.", route.services())
        },
        "required": policy.require_recording_notice,
    }))
}

/// How the notice was given for the next recording, kept until capture starts.
#[derive(Clone, Debug)]
pub struct Given {
    pub method: Method,
    local: bool,
    notice: String,
    at: chrono::DateTime<chrono::Utc>,
}

pub async fn given(
    pool: &SqlitePool,
    policy: &Policy,
    method: Method,
    auto_notes: bool,
) -> Result<Given, String> {
    let route = route(pool, policy, auto_notes).await?;
    Ok(Given {
        method,
        local: route.is_local(),
        notice: if method == Method::Skipped {
            String::new()
        } else {
            text(policy, &route)
        },
        at: chrono::Utc::now(),
    })
}

/// Store the notice for recording `id` before its audio starts. Without a notice
/// step (turned off in settings) the recording is stored as skipped.
pub async fn record(pool: &SqlitePool, id: &str, auto_notes: bool) -> Result<(), String> {
    let policy = super::privacy::policy(pool).await?;
    let given = match super::privacy::pending_notice().await {
        Some(given) => given,
        None if policy.require_recording_notice => {
            return Err(super::privacy::NOTICE_REQUIRED.into())
        }
        None => given(pool, &policy, Method::Skipped, auto_notes).await?,
    };
    save(pool, id, &given, policy.require_recording_notice).await
}

pub async fn save(
    pool: &SqlitePool,
    id: &str,
    given: &Given,
    required: bool,
) -> Result<(), String> {
    sqlx::query("INSERT INTO meeting_notices(meeting_id,method,route,notice,required,given_at) VALUES(?,?,?,?,?,?)")
        .bind(id)
        .bind(given.method.as_str())
        .bind(if given.local { "local" } else { "external" })
        .bind(&given.notice)
        .bind(required)
        .bind(given.at.to_rfc3339())
        .execute(pool)
        .await
        .map_err(|_| "Could not save the recording notice")?;
    Ok(())
}

/// One line for meeting details and exports, in local time. A record that came with a
/// shared file says so: it is what the sender reported, not a notice given here.
pub async fn summary(pool: &SqlitePool, id: &str) -> Result<Option<String>, String> {
    let row: Option<(String, String, bool, String)> = sqlx::query_as(
        "SELECT method,given_at,required,source FROM meeting_notices WHERE meeting_id=?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|_| "Could not read the recording notice")?;
    Ok(row.map(|(method, at, required, source)| {
        let at = chrono::DateTime::parse_from_rfc3339(&at)
            .map(|t| t.with_timezone(&chrono::Local).format("%H:%M").to_string())
            .unwrap_or(at);
        let line = match method.as_str() {
            "copied" => format!("Participants were notified at {at} (notice copied to chat)"),
            "read_aloud" => format!("Participants were notified at {at} (notice read aloud)"),
            "confirmed" => {
                format!("Participants were notified at {at} (confirmed before recording)")
            }
            _ if required => format!("Participant notice skipped at {at}"),
            _ => format!("Participant notice skipped at {at} (notice step turned off)"),
        };
        if source == "shared-file" {
            format!("{SHARED_FILE_PREFIX}{line}")
        } else {
            line
        }
    }))
}

/// How a notice line reads when the record arrived in a shared file.
pub const SHARED_FILE_PREFIX: &str = "Reported in the shared file: ";

#[cfg(test)]
mod tests {
    use super::*;
    async fn database() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }
    async fn configure(pool: &SqlitePool, speech: &str, notes: &str, endpoint: Option<&str>) {
        sqlx::query("INSERT INTO transcript_settings(id,provider,model) VALUES('1',?,'model') ON CONFLICT(id) DO UPDATE SET provider=excluded.provider")
            .bind(speech).execute(pool).await.unwrap();
        sqlx::query("INSERT INTO settings(id,provider,model,whisperModel,ollamaEndpoint) VALUES('1',?,'model','',?) ON CONFLICT(id) DO UPDATE SET provider=excluded.provider,ollamaEndpoint=excluded.ollamaEndpoint")
            .bind(notes).bind(endpoint).execute(pool).await.unwrap();
    }

    #[tokio::test]
    async fn a_notice_from_a_shared_file_reads_as_the_senders_report() {
        let pool = database().await;
        for (id, source) in [("here", "this-computer"), ("shared", "shared-file")] {
            sqlx::query("INSERT INTO meeting_notices(meeting_id,method,route,notice,required,given_at,source) VALUES(?,'copied','local','I am recording.',1,'2026-10-18T09:00:00+00:00',?)")
                .bind(id).bind(source).execute(&pool).await.unwrap();
        }
        let here = summary(&pool, "here").await.unwrap().unwrap();
        let shared = summary(&pool, "shared").await.unwrap().unwrap();
        assert!(here.starts_with("Participants were notified at "));
        assert_eq!(shared, format!("{SHARED_FILE_PREFIX}{here}"));
        // Only the two sources exist.
        assert!(
            sqlx::query("UPDATE meeting_notices SET source='forged' WHERE meeting_id='here'")
                .execute(&pool)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn wording_follows_where_meeting_content_goes() {
        let pool = database().await;
        let policy = Policy::default();
        assert_eq!(
            text(&policy, &route(&pool, &policy, true).await.unwrap()),
            LOCAL_NOTICE
        );
        configure(&pool, "parakeet", "builtin-ai", None).await;
        assert!(route(&pool, &policy, true).await.unwrap().is_local());
        configure(&pool, "parakeet", "ollama", Some("http://localhost:11434")).await;
        assert_eq!(
            text(&policy, &route(&pool, &policy, true).await.unwrap()),
            LOCAL_NOTICE
        );

        configure(&pool, "parakeet", "claude", None).await;
        assert_eq!(
            text(&policy, &route(&pool, &policy, true).await.unwrap()),
            "I'm recording this call to take notes. It is recorded on my computer, and the transcript is sent to Anthropic to write the notes."
        );
        // Notes count only when this recording writes them automatically.
        assert!(route(&pool, &policy, false).await.unwrap().is_local());
        configure(&pool, "parakeet", "ollama", Some("https://llm.example.org")).await;
        assert!(text(&policy, &route(&pool, &policy, true).await.unwrap())
            .ends_with("the transcript is sent to llm.example.org to write the notes."));
        configure(&pool, "deepgram", "builtin-ai", None).await;
        assert!(text(&policy, &route(&pool, &policy, true).await.unwrap())
            .ends_with("the audio is sent to Deepgram for transcription."));
        configure(
            &pool,
            "deepgram",
            "privatemode",
            Some("http://127.0.0.1:8080/v1"),
        )
        .await;
        assert!(text(&policy, &route(&pool, &policy, true).await.unwrap()).ends_with(
            "the audio is sent to Deepgram for transcription; the transcript is encrypted before it is sent to Privatemode to write the notes."
        ));
        // Overtalk cloud notes go through the same local Privatemode proxy.
        configure(&pool, "parakeet", "overtalk-cloud", None).await;
        assert!(text(&policy, &route(&pool, &policy, true).await.unwrap()).ends_with(
            "and the transcript is encrypted before it is sent to Privatemode to write the notes."
        ));
        configure(&pool, "openai", "openai", None).await;
        assert!(text(&policy, &route(&pool, &policy, true).await.unwrap())
            .ends_with("the audio is sent to OpenAI for transcription and notes."));

        // Local-only blocks every outside destination, so nothing can be sent.
        let local_only = Policy {
            local_only: true,
            ..Default::default()
        };
        assert!(route(&pool, &local_only, true).await.unwrap().is_local());

        // The person's wording replaces the default; blank restores it.
        let custom = Policy {
            recording_notice_local: "Heads up: I'm taking notes on my laptop.".into(),
            recording_notice_external: "Heads up: {services}.".into(),
            ..Default::default()
        };
        assert_eq!(
            text(&custom, &route(&pool, &custom, true).await.unwrap()),
            "Heads up: the audio is sent to OpenAI for transcription and notes."
        );
        assert_eq!(
            text(&custom, &Route::default()),
            "Heads up: I'm taking notes on my laptop."
        );
        let preview = preview(&pool, &policy, true).await.unwrap();
        assert_eq!(preview["route"], "external");
        assert_eq!(preview["required"], true);
    }

    #[tokio::test]
    async fn each_meeting_keeps_how_and_when_participants_were_told() {
        let pool = database().await;
        let policy = Policy::default();
        for (id, method) in [
            ("copied", Method::Copied),
            ("aloud", Method::ReadAloud),
            ("skipped", Method::Skipped),
        ] {
            let given = given(&pool, &policy, method, true).await.unwrap();
            save(&pool, id, &given, true).await.unwrap();
        }
        let off = given(&pool, &policy, Method::Skipped, true).await.unwrap();
        save(&pool, "off", &off, false).await.unwrap();

        let stored: (String, String) =
            sqlx::query_as("SELECT route,notice FROM meeting_notices WHERE meeting_id='copied'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(
            (stored.0.as_str(), stored.1.as_str()),
            ("local", LOCAL_NOTICE)
        );
        let at = off.at.with_timezone(&chrono::Local).format("%H:%M");
        assert_eq!(
            summary(&pool, "copied").await.unwrap().unwrap(),
            format!("Participants were notified at {at} (notice copied to chat)")
        );
        assert!(summary(&pool, "aloud")
            .await
            .unwrap()
            .unwrap()
            .ends_with("(notice read aloud)"));
        assert_eq!(
            summary(&pool, "skipped").await.unwrap().unwrap(),
            format!("Participant notice skipped at {at}")
        );
        assert!(summary(&pool, "off")
            .await
            .unwrap()
            .unwrap()
            .ends_with("(notice step turned off)"));
        let skipped: String =
            sqlx::query_scalar("SELECT notice FROM meeting_notices WHERE meeting_id='skipped'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(skipped.is_empty(), "A skipped notice was not given");
        assert!(summary(&pool, "older-meeting").await.unwrap().is_none());

        // Deleting the meeting deletes its notice.
        sqlx::query("INSERT INTO meetings(id,title,created_at,updated_at) VALUES('copied','Call',datetime('now'),datetime('now'))").execute(&pool).await.unwrap();
        sqlx::query("DELETE FROM meetings WHERE id='copied'")
            .execute(&pool)
            .await
            .unwrap();
        assert!(summary(&pool, "copied").await.unwrap().is_none());
    }
}
