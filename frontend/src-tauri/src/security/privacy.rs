//! Local privacy policy and reviewable rights workflows. Recording acknowledgments
//! record the operator's attestation, not a legal conclusion or a subject signature.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{Row, SqlitePool};
use tauri::{AppHandle, Manager, Runtime, State};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Policy {
    pub local_only: bool,
    pub protected_mode: bool,
    pub require_recording_notice: bool,
    /// Notice wording when meeting content stays on this computer.
    pub recording_notice_local: String,
    /// Notice wording when a service outside this computer is used.
    pub recording_notice_external: String,
    pub retention_days: u32,
    pub automatic_retention: bool,
    pub audit_retention_days: u32,
    pub diagnostic_retention_days: u32,
    pub approved_hosts: Vec<String>,
    pub organization: String,
    pub privacy_contact: String,
    pub recording_basis: String,
    pub agreement_reference: String,
    /// Recognising remembered voices also analyses everyone else's voice in the
    /// meeting, so it runs after transcription only when the person turns it on.
    pub voice_recognition_automatic: bool,
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            local_only: false,
            protected_mode: false,
            require_recording_notice: true,
            recording_notice_local: super::notice::LOCAL_NOTICE.into(),
            recording_notice_external: super::notice::EXTERNAL_NOTICE.into(),
            retention_days: 0,
            automatic_retention: false,
            audit_retention_days: 90,
            diagnostic_retention_days: 7,
            approved_hosts: vec![],
            organization: String::new(),
            privacy_contact: String::new(),
            recording_basis: String::new(),
            agreement_reference: String::new(),
            voice_recognition_automatic: false,
        }
    }
}
static PROCESS_POLICY: std::sync::LazyLock<std::sync::RwLock<Policy>> =
    std::sync::LazyLock::new(|| std::sync::RwLock::new(Policy::default()));

pub fn guard_process_destination(destination: &str) -> Result<(), String> {
    let url = super::transport::endpoint(destination)?;
    let p = PROCESS_POLICY
        .read()
        .map_err(|_| "Privacy policy unavailable")?;
    let host = url.host_str().unwrap_or("");
    let local = super::transport::is_loopback(&url);
    if p.local_only && !local {
        return Err("Local-only policy blocks sending meeting content outside this device.".into());
    }
    if p.protected_mode && !local && !p.approved_hosts.iter().any(|h| h == host) {
        return Err("This destination has not been approved in Privacy settings.".into());
    }
    Ok(())
}

pub async fn initialize(pool: &SqlitePool) -> Result<(), String> {
    let loaded = policy(pool).await?;
    *PROCESS_POLICY
        .write()
        .map_err(|_| "Privacy policy unavailable")? = loaded;
    Ok(())
}

pub async fn policy(pool: &SqlitePool) -> Result<Policy, String> {
    let encoded: Option<String> =
        sqlx::query_scalar("SELECT policy FROM privacy_settings WHERE id=1")
            .fetch_optional(pool)
            .await
            .map_err(|_| "Privacy policy unavailable")?;
    serde_json::from_str(encoded.as_deref().unwrap_or("{}"))
        .map_err(|_| "Privacy policy is invalid".into())
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Action {
    Status,
    SavePolicy {
        policy: Policy,
    },
    /// `auto_notes` says whether this recording writes notes automatically; the
    /// notes provider counts toward the wording only then. Missing counts it.
    RecordingNotice {
        auto_notes: Option<bool>,
    },
    AcknowledgeRecording {
        method: Option<super::notice::Method>,
        auto_notes: Option<bool>,
    },
    RequestRecordingConsent,
    AuthorizeVoice {
        name: String,
        basis: String,
        reference: String,
        days: u32,
    },
    WithdrawAuthorization {
        id: String,
    },
    SubjectPreview {
        name: String,
    },
    ExportSubject {
        name: String,
        expected_ids: Vec<String>,
    },
    EraseSubject {
        name: String,
        expected_ids: Vec<String>,
    },
    Audit,
    VerifyAudit,
    VerifyDatabase,
    RetryDeletion,
    ClearDiagnosticLogs,
    MigrateCredentials,
    ApplyRetention {
        expected_ids: Vec<String>,
    },
    Hold {
        meeting_id: String,
        reason: String,
    },
    ReleaseHold {
        meeting_id: String,
    },
}
/// The notice confirmed for the next recording. Capture consumes it.
static CAPTURE_ACK: tokio::sync::Mutex<Option<(std::time::Instant, super::notice::Given)>> =
    tokio::sync::Mutex::const_new(None);
const NOTICE_LIFETIME: std::time::Duration = std::time::Duration::from_secs(600);
pub const NOTICE_REQUIRED: &str = "Before recording, confirm that participants have been informed and your recording basis is appropriate. Open the recorder to review the notice.";

/// The notice for a recording about to start, without consuming it.
pub async fn pending_notice() -> Option<super::notice::Given> {
    CAPTURE_ACK
        .lock()
        .await
        .as_ref()
        .filter(|(at, _)| at.elapsed() < NOTICE_LIFETIME)
        .map(|(_, given)| given.clone())
}

async fn acknowledge(
    pool: &SqlitePool,
    method: super::notice::Method,
    auto_notes: bool,
) -> Result<(), String> {
    let given = super::notice::given(pool, &policy(pool).await?, method, auto_notes).await?;
    let operation = if method == super::notice::Method::Skipped {
        "recording_notice_skipped"
    } else {
        "recording_notice_acknowledged"
    };
    super::audit::append(pool, operation, "", true).await?;
    *CAPTURE_ACK.lock().await = Some((std::time::Instant::now(), given));
    Ok(())
}

pub async fn guard_capture<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let pool = app
        .state::<crate::state::AppState>()
        .db_manager
        .pool()
        .clone();
    let p = policy(&pool).await?;
    guard_device(app, &p).await?;
    // Every capture consumes the notice, so one confirmation covers one recording.
    let acknowledged = CAPTURE_ACK
        .lock()
        .await
        .take()
        .is_some_and(|(at, _)| at.elapsed() < NOTICE_LIFETIME);
    if p.require_recording_notice && !acknowledged {
        return Err(NOTICE_REQUIRED.into());
    }
    Ok(())
}

pub async fn guard_destination(pool: &SqlitePool, url: &str) -> Result<(), String> {
    let url = super::transport::endpoint(url)?;
    let host = url.host_str().unwrap_or("");
    let local = super::transport::is_loopback(&url);
    let p = policy(pool).await?;
    if p.local_only && !local {
        return Err(
            "Local-only policy blocks sending meeting content to an external service.".into(),
        );
    }
    if p.protected_mode && !local && !p.approved_hosts.iter().any(|h| h == host) {
        return Err("Protected mode requires this destination to be approved in Privacy settings, with the applicable agreement recorded.".into());
    }
    Ok(())
}

async fn device<R: Runtime>(app: &AppHandle<R>) -> Result<Vec<Value>, String> {
    let mut paths = super::erasure::roots(app).await?;
    paths.push(
        app.path()
            .app_data_dir()
            .map_err(|_| "App data unavailable")?,
    );
    paths.push(std::env::temp_dir());
    paths.retain(|p| p.exists());
    tokio::task::spawn_blocking(move || {
        paths
            .into_iter()
            .map(|p| {
                let evidence = super::encryption::inspect(&p);
                json!({"path":p,"verified":evidence.verified,"detail":evidence.detail})
            })
            .collect()
    })
    .await
    .map_err(|_| "Storage check stopped".into())
}

pub async fn guard_device<R: Runtime>(app: &AppHandle<R>, p: &Policy) -> Result<(), String> {
    if p.protected_mode && device(app).await?.iter().any(|r| r["verified"] != true) {
        return Err("Protected mode is locked: device encryption is not verified for every storage location. Check Privacy settings.".into());
    }
    Ok(())
}

pub async fn voice_authorization(pool: &SqlitePool, name: &str) -> Result<String, String> {
    let authorization: Option<String> = sqlx::query_scalar(
        "SELECT id FROM privacy_authorizations WHERE lower(trim(subject_name))=lower(trim(?)) \
         AND purpose='voice_identification' AND withdrawn_at IS NULL AND (expires_at IS NULL \
         OR julianday(expires_at)>julianday('now')) ORDER BY recorded_at DESC LIMIT 1",
    )
    .bind(name)
    .fetch_optional(pool)
    .await
    .map_err(|_| "Could not check voice authorization")?;
    authorization.ok_or("Record how this person agreed before remembering their voice.".into())
}

/// Record a person's explicit consent to voice recognition, given while remembering their
/// voice from a meeting. `method` is how they agreed: `in_meeting` (the recording holds
/// it), `in_writing` or `other`; the last two need `note` to say where it is recorded.
pub async fn record_voice_consent(
    pool: &SqlitePool,
    name: &str,
    method: &str,
    note: &str,
    meeting_id: &str,
) -> Result<String, String> {
    let note = note.trim();
    if note.chars().count() > 500 || note.chars().any(char::is_control) {
        return Err("Keep the consent note to one line of up to 500 characters".into());
    }
    let meeting: Option<(String, String)> =
        sqlx::query_as("SELECT title,created_at FROM meetings WHERE id=?")
            .bind(meeting_id)
            .fetch_optional(pool)
            .await
            .map_err(|_| "Could not read the meeting")?;
    let (title, at) = meeting.ok_or("This meeting no longer exists")?;
    let day = chrono::DateTime::parse_from_rfc3339(&at)
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_default();
    let reference = match (method, note.is_empty()) {
        ("in_meeting", true) => format!("Agreed in the meeting \"{title}\" ({day})"),
        ("in_meeting", false) => format!("Agreed in the meeting \"{title}\" ({day}): {note}"),
        ("in_writing", false) => format!("Agreed in writing: {note}"),
        ("other", false) => format!("Agreed: {note}"),
        ("in_writing" | "other", true) => {
            return Err("Say where their agreement is recorded".into())
        }
        _ => return Err("Choose how they agreed".into()),
    };
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now();
    sqlx::query("INSERT INTO privacy_authorizations(id,subject_name,purpose,basis,notice_version,recorded_at,expires_at) VALUES(?,?,'voice_identification','explicit_consent',?,?,?)")
        .bind(&id).bind(name.trim()).bind(&reference).bind(now.to_rfc3339())
        .bind((now + chrono::Duration::days(365)).to_rfc3339())
        .execute(pool).await.map_err(|_| "Could not record consent")?;
    super::audit::append(pool, "voice_authorization_recorded", &id, true).await?;
    Ok(id)
}

async fn retention_candidates(pool: &SqlitePool, p: &Policy) -> Result<Vec<String>, String> {
    if p.retention_days == 0 {
        return Ok(vec![]);
    }
    sqlx::query_scalar("SELECT id FROM meetings WHERE julianday(created_at)<julianday('now',?) AND NOT EXISTS(SELECT 1 FROM privacy_holds WHERE meeting_id=meetings.id) ORDER BY id")
        .bind(format!("-{} days",p.retention_days)).fetch_all(pool).await.map_err(|_|"Could not check retention".into())
}

async fn subject_ids(pool: &SqlitePool, name: &str) -> Result<Vec<String>, String> {
    if name.trim().is_empty() || name.len() > 160 {
        return Err("Enter a person's name (up to 160 characters)".into());
    }
    sqlx::query_scalar(
        "SELECT DISTINCT m.id FROM meetings m WHERE EXISTS(SELECT 1 FROM transcripts t \
         WHERE t.meeting_id=m.id AND instr(lower(t.transcript),lower(?1))>0) OR EXISTS(SELECT 1 \
         FROM speech_transcript_backups b WHERE b.meeting_id=m.id \
         AND (instr(lower(b.transcripts),lower(?1))>0 OR instr(lower(b.speakers),lower(?1))>0)) \
         OR EXISTS(SELECT 1 FROM meeting_notes n WHERE n.meeting_id=m.id \
         AND instr(lower(n.notes_markdown),lower(?1))>0) OR EXISTS(SELECT 1 \
         FROM meeting_speakers s WHERE s.meeting_id=m.id AND lower(trim(s.name))=lower(trim(?1))) \
         OR EXISTS(SELECT 1 FROM workspace_documents d WHERE d.meeting_id=m.id \
         AND (instr(lower(d.body),lower(?1))>0 OR instr(lower(d.title),lower(?1))>0)) \
         OR EXISTS(SELECT 1 FROM workspace_tasks t WHERE t.meeting_id=m.id \
         AND (lower(trim(t.owner))=lower(trim(?1)) OR instr(lower(t.title),lower(?1))>0)) \
         OR EXISTS(SELECT 1 FROM note_versions n WHERE n.meeting_id=m.id \
         AND instr(lower(n.content),lower(?1))>0) OR EXISTS(SELECT 1 FROM workspace_notepads n \
         WHERE (n.id=m.id OR n.id='followup:'||m.id) AND instr(lower(n.body),lower(?1))>0) \
         OR EXISTS(SELECT 1 FROM project_facts f WHERE f.source_meeting_id=m.id \
         AND (instr(lower(f.body),lower(?1))>0 OR instr(lower(f.source_quote),lower(?1))>0)) \
         ORDER BY m.id",
    )
    .bind(name)
    .fetch_all(pool)
    .await
    .map_err(|_| "Could not find subject records".into())
}

async fn erase_many<R: Runtime>(
    app: &AppHandle<R>,
    pool: &SqlitePool,
    ids: &[String],
) -> Result<(), String> {
    let roots = super::erasure::roots(app).await?;
    for id in ids {
        if crate::audio::recording_commands::is_recording().await {
            return Err("Stop recording before applying erasure".into());
        }
        super::erasure::delete(pool, id, &roots).await?;
        super::audit::append(pool, "meeting_erased", id, true).await?;
    }
    Ok(())
}

#[tauri::command]
pub async fn workspace_privacy<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, crate::state::AppState>,
    action: Action,
) -> Result<Value, String> {
    let pool = state.db_manager.pool();
    match action {
        Action::Status => {
            let p = policy(pool).await?;
            type AuthorizationRow = (String, String, String, Option<String>, Option<String>);
            let authorizations: Vec<AuthorizationRow> = sqlx::query_as(
                "SELECT id,subject_name,purpose,expires_at,withdrawn_at \
                 FROM privacy_authorizations ORDER BY recorded_at DESC LIMIT 500",
            )
            .fetch_all(pool)
            .await
            .map_err(|e| e.to_string())?;
            let pending: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM privacy_erasures WHERE pending_paths!='[]'",
            )
            .fetch_one(pool)
            .await
            .map_err(|e| e.to_string())?;
            let holds: Vec<(String, String, String)> = sqlx::query_as(
                "SELECT h.meeting_id,m.title,h.reason FROM privacy_holds h JOIN meetings m \
                 ON m.id=h.meeting_id ORDER BY m.created_at DESC",
            )
            .fetch_all(pool)
            .await
            .map_err(|e| e.to_string())?;
            let meetings: Vec<(String, String)> =
                sqlx::query_as("SELECT id,title FROM meetings ORDER BY created_at DESC LIMIT 1000")
                    .fetch_all(pool)
                    .await
                    .map_err(|e| e.to_string())?;
            Ok(json!({
                "policy": p,
                "storage": device(&app).await?,
                "pending_deletions": pending,
                "pending_credentials": super::credentials::pending_count(pool)
                    .await
                    .map_err(|_| "Could not check credential migration")?,
                "retention_candidates": retention_candidates(pool, &p).await?,
                "holds": holds
                    .into_iter()
                    .map(|(id, title, reason)| json!({
                        "id": id,
                        "title": title,
                        "reason": reason
                    }))
                    .collect::<Vec<_>>(),
                "meetings": meetings
                    .into_iter()
                    .map(|(id, title)| json!({"id": id, "title": title}))
                    .collect::<Vec<_>>(),
                "authorizations": authorizations
                    .into_iter()
                    .map(|(id, name, purpose, expires, withdrawn)| json!({
                        "id": id,
                        "name": name,
                        "purpose": purpose,
                        "expires": expires,
                        "withdrawn": withdrawn
                    }))
                    .collect::<Vec<_>>()
            }))
        }
        Action::SavePolicy { policy: p } => {
            if p.retention_days > 36500
                || (p.automatic_retention && p.retention_days == 0)
                || !(1..=3650).contains(&p.audit_retention_days)
                || p.diagnostic_retention_days > 365
                || p.organization.len() > 240
                || p.privacy_contact.len() > 320
                || p.agreement_reference.len() > 2000
                || p.recording_notice_local.len() > 500
                || p.recording_notice_external.len() > 500
                || p.approved_hosts.len() > 100
            {
                return Err("Check privacy settings limits".into());
            }
            for host in &p.approved_hosts {
                let url = super::transport::endpoint(&format!("https://{host}"))?;
                if url.host_str() != Some(host.as_str())
                    || url.path() != "/"
                    || url.port().is_some()
                {
                    return Err("Approve hostnames only, without a URL path or port".into());
                }
            }
            if p.protected_mode
                && (p.organization.trim().is_empty()
                    || p.privacy_contact.trim().is_empty()
                    || p.recording_basis.trim().is_empty())
            {
                return Err(
                    "Protected mode requires an organization, privacy contact and documented \
                     recording basis"
                        .into(),
                );
            }
            if p.protected_mode
                && !p.approved_hosts.is_empty()
                && p.agreement_reference.trim().is_empty()
            {
                return Err("Record the agreement reference for approved external services".into());
            }
            guard_device(&app, &p).await?;
            super::audit::append(pool, "privacy_policy_changed", "", true).await?;
            sqlx::query("UPDATE privacy_settings SET policy=? WHERE id=1")
                .bind(serde_json::to_string(&p).unwrap())
                .execute(pool)
                .await
                .map_err(|e| e.to_string())?;
            *PROCESS_POLICY
                .write()
                .map_err(|_| "Privacy policy unavailable")? = p;
            Ok(json!({"saved":true}))
        }
        Action::RecordingNotice { auto_notes } => {
            super::notice::preview(pool, &policy(pool).await?, auto_notes.unwrap_or(true)).await
        }
        Action::AcknowledgeRecording { method, auto_notes } => {
            acknowledge(
                pool,
                method.unwrap_or(super::notice::Method::Confirmed),
                auto_notes.unwrap_or(true),
            )
            .await?;
            Ok(json!({"acknowledged":true}))
        }
        Action::RequestRecordingConsent => {
            use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
            let accepted = tokio::task::spawn_blocking(move || {
                app.dialog()
                    .message(
                        "Inform participants that this conversation will be recorded, explain its \
                         purpose, and confirm that recording is permitted. System audio may \
                         include other sounds on this computer.",
                    )
                    .title("Before you record")
                    .buttons(MessageDialogButtons::OkCancelCustom(
                        "Participants informed · continue".into(),
                        "Cancel".into(),
                    ))
                    .blocking_show()
            })
            .await
            .map_err(|_| "Recording notice closed unexpectedly")?;
            if accepted {
                acknowledge(pool, super::notice::Method::Confirmed, true).await?;
            }
            Ok(json!({"acknowledged":accepted}))
        }
        Action::AuthorizeVoice {
            name,
            basis,
            reference,
            days,
        } => {
            if name.trim().is_empty()
                || name.len() > 160
                || reference.trim().is_empty()
                || reference.len() > 1000
                || days == 0
                || days > 3650
                || !matches!(basis.as_str(), "explicit_consent" | "applicable_law")
            {
                return Err(
                    "Provide the person's name, authorization reference, basis and a duration of \
                     1–3650 days"
                        .into(),
                );
            }
            let id = uuid::Uuid::new_v4().to_string();
            let now = chrono::Utc::now();
            sqlx::query(
                "INSERT INTO privacy_authorizations(id,subject_name,purpose,basis,notice_version,\
                 recorded_at,expires_at) VALUES(?,?,'voice_identification',?,?,?,?)",
            )
            .bind(&id)
            .bind(name.trim())
            .bind(basis)
            .bind(reference)
            .bind(now.to_rfc3339())
            .bind((now + chrono::Duration::days(days as i64)).to_rfc3339())
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
            super::audit::append(pool, "voice_authorization_recorded", &id, true).await?;
            Ok(json!({"id":id}))
        }
        Action::WithdrawAuthorization { id } => {
            let name: Option<String> =
                sqlx::query_scalar("SELECT subject_name FROM privacy_authorizations WHERE id=?")
                    .bind(&id)
                    .fetch_optional(pool)
                    .await
                    .map_err(|e| e.to_string())?;
            let mut tx = pool.begin().await.map_err(|e| e.to_string())?;
            sqlx::query("UPDATE privacy_authorizations SET withdrawn_at=? WHERE id=?")
                .bind(chrono::Utc::now().to_rfc3339())
                .bind(&id)
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
            if let Some(name) = name {
                sqlx::query(
                    "DELETE FROM voice_profiles WHERE lower(trim(name))=lower(trim(?)) \
                     OR id IN(SELECT profile_id FROM project_people \
                     WHERE lower(trim(name))=lower(trim(?)))",
                )
                .bind(&name)
                .bind(&name)
                .execute(&mut *tx)
                .await
                .map_err(|e| e.to_string())?;
            }
            tx.commit().await.map_err(|e| e.to_string())?;
            super::audit::append(pool, "voice_authorization_withdrawn", &id, true).await?;
            Ok(json!({"withdrawn":true}))
        }
        Action::SubjectPreview { name } => {
            let ids = subject_ids(pool, &name).await?;
            let mut meetings = Vec::new();
            for id in &ids {
                let title: String = sqlx::query_scalar("SELECT title FROM meetings WHERE id=?")
                    .bind(id)
                    .fetch_one(pool)
                    .await
                    .map_err(|e| e.to_string())?;
                meetings.push(json!({"id":id,"title":title}));
            }
            Ok(json!({
                "name": name,
                "ids": ids,
                "meetings": meetings,
                "notice": "Name matches are candidates, not verified identities. Erasure removes \
                           complete meetings and may affect other people. Review before \
                           confirming. Exports and backups held elsewhere require separate \
                           handling."
            }))
        }
        Action::ExportSubject {
            name,
            mut expected_ids,
        } => {
            use tauri_plugin_dialog::DialogExt;
            let ids = subject_ids(pool, &name).await?;
            expected_ids.sort();
            expected_ids.dedup();
            if ids != expected_ids {
                return Err(
                    "The matching records changed. Review them again before export.".into(),
                );
            }
            guard_device(&app, &policy(pool).await?).await?;
            // Bound materialization before copying transcript/history into JSON.
            // The allowance includes headroom for JSON escaping and metadata.
            let bytes: i64 = sqlx::query_scalar(
                "SELECT
                (SELECT coalesce(sum(length(CAST(transcript AS BLOB))),0) FROM transcripts WHERE \
                    meeting_id IN(SELECT value FROM json_each(?1))) +
                (SELECT coalesce(sum(length(CAST(notes_markdown AS BLOB))),0) FROM meeting_notes \
                    WHERE meeting_id IN(SELECT value FROM json_each(?1))) +
                (SELECT coalesce(sum(length(CAST(transcripts AS BLOB))+length(CAST(speakers AS \
                    BLOB))+length(CAST(turns AS BLOB))),0) FROM speech_transcript_backups WHERE \
                    meeting_id IN(SELECT value FROM json_each(?1))) +
                (SELECT coalesce(sum(length(CAST(body AS BLOB))),0) FROM workspace_documents WHERE \
                    meeting_id IN(SELECT value FROM json_each(?1))) +
                (SELECT coalesce(sum(length(CAST(content AS BLOB))),0) FROM note_versions WHERE \
                    meeting_id IN(SELECT value FROM json_each(?1))) +
                (SELECT coalesce(sum(length(CAST(body AS BLOB))),0) FROM workspace_notepads WHERE \
                    id IN(SELECT value FROM json_each(?1)) OR substr(id,10) IN(SELECT value FROM \
                    json_each(?1))) +
                (SELECT coalesce(sum(length(title)+length(owner)+length(evidence_json)),0) FROM \
                    workspace_tasks WHERE meeting_id IN(SELECT value FROM json_each(?1))) +
                (SELECT coalesce(sum(length(body)+length(source_quote)),0) FROM project_facts \
                    WHERE source_meeting_id IN(SELECT value FROM json_each(?1)))",
            )
            .bind(serde_json::to_string(&ids).unwrap())
            .fetch_one(pool)
            .await
            .map_err(|e| e.to_string())?;
            if bytes > 4 * 1024 * 1024 {
                return Err(
                    "This request is too large for a single review file. Use individual meeting \
                     exports."
                        .into(),
                );
            }
            let mut meetings = Vec::new();
            for id in &ids {
                let data: Option<String> = sqlx::query_scalar(
                    "SELECT json_object('id',id,'title',title,'recorded_at',created_at,\
                     'audio_folder',folder_path) FROM meetings WHERE id=?",
                )
                .bind(id)
                .fetch_optional(pool)
                .await
                .map_err(|e| e.to_string())?;
                let transcript: Vec<String> = sqlx::query_scalar(
                    "SELECT json_object('text',transcript,'start',audio_start_time,'end',\
                     audio_end_time,'speaker',speaker_id) FROM transcripts WHERE meeting_id=? \
                     ORDER BY audio_start_time,id",
                )
                .bind(id)
                .fetch_all(pool)
                .await
                .map_err(|e| e.to_string())?;
                let notes: Option<String> = sqlx::query_scalar(
                    "SELECT notes_markdown FROM meeting_notes WHERE meeting_id=?",
                )
                .bind(id)
                .fetch_optional(pool)
                .await
                .map_err(|e| e.to_string())?;
                let speakers: Vec<String> = sqlx::query_scalar(
                    "SELECT json_object('id',id,'name',name) FROM meeting_speakers \
                     WHERE meeting_id=?",
                )
                .bind(id)
                .fetch_all(pool)
                .await
                .map_err(|e| e.to_string())?;
                let tasks: Vec<String> = sqlx::query_scalar(
                    "SELECT json_object('title',title,'owner',owner,'due_date',due_date,'status',\
                     status,'evidence',evidence_json) FROM workspace_tasks WHERE meeting_id=?",
                )
                .bind(id)
                .fetch_all(pool)
                .await
                .map_err(|e| e.to_string())?;
                let recovery: Vec<String> = sqlx::query_scalar(
                    "SELECT json_object('created_at',created_at,'transcripts',transcripts,\
                     'speakers',speakers,'turns',turns) FROM speech_transcript_backups \
                     WHERE meeting_id=? ORDER BY id",
                )
                .bind(id)
                .fetch_all(pool)
                .await
                .map_err(|e| e.to_string())?;
                let notes_history: Vec<String> = sqlx::query_scalar(
                    "SELECT json_object('content',content,'created_at',created_at,'origin',origin) \
                     FROM note_versions WHERE meeting_id=? ORDER BY id",
                )
                .bind(id)
                .fetch_all(pool)
                .await
                .map_err(|e| e.to_string())?;
                let personal_notes: Vec<String> = sqlx::query_scalar(
                    "SELECT json_object('id',id,'body',body,'updated_at',updated_at) \
                     FROM workspace_notepads WHERE id=?1 OR id='followup:'||?1",
                )
                .bind(id)
                .fetch_all(pool)
                .await
                .map_err(|e| e.to_string())?;
                let facts: Vec<String> = sqlx::query_scalar(
                    "SELECT json_object('body',body,'status',status,'source_quote',source_quote,\
                     'project_id',project_id) FROM project_facts WHERE source_meeting_id=?",
                )
                .bind(id)
                .fetch_all(pool)
                .await
                .map_err(|e| e.to_string())?;
                let parse = |items: Vec<String>| {
                    items
                        .into_iter()
                        .filter_map(|v| serde_json::from_str::<Value>(&v).ok())
                        .collect::<Vec<_>>()
                };
                meetings.push(json!({
                    "meeting": data.and_then(|v| serde_json::from_str::<Value>(&v).ok()),
                    "transcript": parse(transcript),
                    "notes": notes,
                    "speakers": parse(speakers),
                    "tasks": parse(tasks),
                    "transcript_recovery": parse(recovery),
                    "notes_history": parse(notes_history),
                    "personal_notes": parse(personal_notes),
                    "project_facts": parse(facts)
                }));
            }
            let identities: Vec<String> = sqlx::query_scalar(
                "SELECT json_object('project_id',project_id,'name',name,'role',role,\
                 'has_voice_profile',profile_id IS NOT NULL) FROM project_people \
                 WHERE lower(trim(name))=lower(trim(?))",
            )
            .bind(&name)
            .fetch_all(pool)
            .await
            .map_err(|e| e.to_string())?;
            let authorizations: Vec<String> = sqlx::query_scalar(
                "SELECT json_object('purpose',purpose,'basis',basis,'reference',notice_version,\
                 'recorded_at',recorded_at,'expires_at',expires_at,'withdrawn_at',withdrawn_at) \
                 FROM privacy_authorizations WHERE lower(trim(subject_name))=lower(trim(?))",
            )
            .bind(&name)
            .fetch_all(pool)
            .await
            .map_err(|e| e.to_string())?;
            // The voice profile itself: when it was made and where it was used. Its numeric
            // template stays out of the export; forgetting the voice deletes it.
            let voices: Vec<String> = sqlx::query_scalar(
                "SELECT json_object('created_at',v.created_at,'model',v.model,'samples',\
                 json_array_length(v.embeddings),'project_id',v.scope_project_id,'stored',\
                 'this computer only','meetings_recognized',(SELECT COUNT(*) \
                 FROM meeting_voice_matches m WHERE m.profile_id=v.id)) FROM voice_profiles v \
                 WHERE lower(trim(v.name))=lower(trim(?1)) OR v.id IN(SELECT profile_id \
                 FROM project_people WHERE lower(trim(name))=lower(trim(?1)))",
            )
            .bind(&name)
            .fetch_all(pool)
            .await
            .map_err(|e| e.to_string())?;
            let parse = |items: Vec<String>| {
                items
                    .into_iter()
                    .filter_map(|v| serde_json::from_str::<Value>(&v).ok())
                    .collect::<Vec<_>>()
            };
            let body = serde_json::to_vec_pretty(&json!({
                "format": "overtalk-subject-review-v1",
                "subject": name,
                "review_required": "This export contains full matching meetings, including other \
                                    people. Review and redact before disclosing. It is a review \
                                    aid, not an exhaustive verified-subject export. Audio remains \
                                    at the listed local folders and can be provided through \
                                    meeting exports. Separately review saved answers, transcript \
                                    recovery copies, connected-service records and external \
                                    backups.",
                "meetings": meetings,
                "project_identities": parse(identities),
                "voice_authorizations": parse(authorizations),
                "voice_profiles": parse(voices)
            }))
            .map_err(|_| "Could not create export")?;
            if body.len() > 32 * 1024 * 1024 {
                return Err(
                    "This request exceeds 32 MiB; export matching meetings in smaller groups."
                        .into(),
                );
            }
            let chosen = tokio::task::spawn_blocking(move || {
                app.dialog()
                    .file()
                    .set_file_name("data-request-review.json")
                    .blocking_save_file()
            })
            .await
            .map_err(|_| "File picker stopped")?;
            let Some(chosen) = chosen else {
                return Ok(json!({"cancelled":true}));
            };
            let path = chosen.into_path().map_err(|_| "Choose a local file")?;
            let parent = path.parent().ok_or("Choose an export folder")?;
            let mut temp = tempfile::NamedTempFile::new_in(parent)
                .map_err(|_| "Could not create private export")?;
            use std::io::Write;
            temp.write_all(&body)
                .map_err(|_| "Could not write export")?;
            temp.as_file()
                .sync_all()
                .map_err(|_| "Could not save export")?;
            temp.persist(path).map_err(|_| "Could not finish export")?;
            super::audit::append(pool, "subject_review_export", "", true).await?;
            Ok(json!({"exported":true}))
        }
        Action::EraseSubject {
            name,
            mut expected_ids,
        } => {
            let current = subject_ids(pool, &name).await?;
            expected_ids.sort();
            expected_ids.dedup();
            if expected_ids != current {
                return Err(
                    "Subject records changed. Review the preview again before erasing.".into(),
                );
            }
            erase_many(&app, pool, &current).await?;
            sqlx::query(
                "DELETE FROM voice_profiles WHERE lower(trim(name))=lower(trim(?)) \
                 OR id IN(SELECT profile_id FROM project_people \
                 WHERE lower(trim(name))=lower(trim(?)))",
            )
            .bind(&name)
            .bind(&name)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
            sqlx::query("DELETE FROM project_people WHERE lower(trim(name))=lower(trim(?))")
                .bind(&name)
                .execute(pool)
                .await
                .map_err(|e| e.to_string())?;
            sqlx::query(
                "DELETE FROM privacy_authorizations WHERE lower(trim(subject_name))=lower(trim(?))",
            )
            .bind(name)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
            Ok(json!({"erased_meetings":current.len()}))
        }
        Action::ApplyRetention { mut expected_ids } => {
            let current = retention_candidates(pool, &policy(pool).await?).await?;
            expected_ids.sort();
            expected_ids.dedup();
            if expected_ids != current {
                return Err(
                    "Retention candidates changed. Review them again before deleting.".into(),
                );
            }
            erase_many(&app, pool, &current).await?;
            Ok(json!({"erased_meetings":current.len()}))
        }
        Action::Hold { meeting_id, reason } => {
            if reason.trim().is_empty() || reason.len() > 1000 {
                return Err("Enter a retention-hold reason".into());
            }
            sqlx::query(
                "INSERT INTO privacy_holds(meeting_id,reason,created_at) VALUES(?,?,?) \
                 ON CONFLICT(meeting_id) DO UPDATE SET reason=excluded.reason",
            )
            .bind(&meeting_id)
            .bind(reason)
            .bind(chrono::Utc::now().to_rfc3339())
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
            super::audit::append(pool, "retention_hold_set", &meeting_id, true).await?;
            Ok(Value::Null)
        }
        Action::ReleaseHold { meeting_id } => {
            sqlx::query("DELETE FROM privacy_holds WHERE meeting_id=?")
                .bind(&meeting_id)
                .execute(pool)
                .await
                .map_err(|e| e.to_string())?;
            super::audit::append(pool, "retention_hold_released", &meeting_id, true).await?;
            Ok(Value::Null)
        }
        Action::RetryDeletion => {
            super::erasure::resume(pool, &super::erasure::roots(&app).await?).await?;
            Ok(Value::Null)
        }
        Action::MigrateCredentials => {
            let count = super::credentials::migrate(pool)
                .await
                .map_err(|e| e.to_string())?;
            if count > 0 {
                sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
                    .execute(pool)
                    .await
                    .map_err(|_| "Could not checkpoint migrated settings")?;
                sqlx::query("VACUUM")
                    .execute(pool)
                    .await
                    .map_err(|_| "Could not compact migrated settings")?;
            }
            Ok(json!({"migrated":count}))
        }
        Action::ClearDiagnosticLogs => {
            let count = clear_logs(0).await?;
            super::audit::append(pool, "diagnostic_logs_cleared", "", true).await?;
            Ok(json!({"removed":count}))
        }
        Action::VerifyAudit => Ok(json!({"valid":super::audit::verify(pool).await?})),
        Action::VerifyDatabase => {
            let integrity: Vec<String> = sqlx::query_scalar("PRAGMA integrity_check(10)")
                .fetch_all(pool)
                .await
                .map_err(|_| "Could not check database integrity")?;
            let foreign_keys: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM pragma_foreign_key_check")
                    .fetch_one(pool)
                    .await
                    .map_err(|_| "Could not check database references")?;
            let version: String = sqlx::query_scalar("SELECT sqlite_version()")
                .fetch_one(pool)
                .await
                .map_err(|_| "Could not read database engine version")?;
            let fts_valid = sqlx::query(
                "INSERT INTO workspace_fts(workspace_fts,rank) VALUES('integrity-check',1)",
            )
            .execute(pool)
            .await
            .is_ok();
            Ok(
                json!({"valid":integrity == ["ok"] && foreign_keys == 0 && fts_valid,
                "integrity":integrity,"foreign_key_violations":foreign_keys,
                "search_index_valid":fts_valid,"sqlite_version":version}),
            )
        }
        Action::Audit => {
            let rows = sqlx::query(
                "SELECT sequence,occurred_at,actor,operation,object_id,outcome FROM privacy_events \
                 ORDER BY sequence DESC LIMIT 1000",
            )
            .fetch_all(pool)
            .await
            .map_err(|e| e.to_string())?;
            Ok(json!(rows
                .iter()
                .map(|r| json!({
                    "sequence": r.get::<i64, _>("sequence"),
                    "at": r.get::<String, _>("occurred_at"),
                    "actor": r.get::<String, _>("actor"),
                    "operation": r.get::<String, _>("operation"),
                    "object_id": r.get::<String, _>("object_id"),
                    "outcome": r.get::<String, _>("outcome")
                }))
                .collect::<Vec<_>>()))
        }
    }
}

async fn clear_logs(days: u32) -> Result<usize, String> {
    let directory = std::env::var_os("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| dirs::home_dir().map(|p| p.join(".local/state")))
        .ok_or("Diagnostic directory unavailable")?
        .join("overtalk-slint");
    tokio::task::spawn_blocking(move || {
        if !directory.exists() {
            return Ok(0);
        }
        if std::fs::symlink_metadata(&directory)
            .map_err(|_| "Cannot inspect log directory")?
            .file_type()
            .is_symlink()
        {
            return Err("Diagnostic directory must not be a link".to_owned());
        }
        let mut removed = 0;
        for entry in std::fs::read_dir(directory)
            .map_err(|_| "Cannot open log directory")?
            .flatten()
        {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let metadata = entry.metadata().map_err(|_| "Cannot inspect log file")?;
            if name.starts_with("app-")
                && name.ends_with(".log")
                && entry.file_type().is_ok_and(|t| t.is_file())
                && (days == 0
                    || metadata
                        .modified()
                        .ok()
                        .and_then(|t| t.elapsed().ok())
                        .is_some_and(|age| age.as_secs() > days as u64 * 86400))
            {
                std::fs::remove_file(entry.path()).map_err(|_| "Cannot remove a diagnostic log")?;
                removed += 1;
            }
        }
        Ok(removed)
    })
    .await
    .map_err(|_| "Log cleanup worker stopped")?
}

pub fn start_maintenance<R: Runtime>(app: AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        loop {
            // Delay startup work so opening the notebook stays responsive.
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            let pool = app
                .state::<crate::state::AppState>()
                .db_manager
                .pool()
                .clone();
            if let Ok(p) = policy(&pool).await {
                let _ = clear_logs(p.diagnostic_retention_days.max(1)).await;
                let _ = sqlx::query(
                    "DELETE FROM privacy_events WHERE julianday(occurred_at)<julianday('now',?)",
                )
                .bind(format!("-{} days", p.audit_retention_days))
                .execute(&pool)
                .await;
                // Expired/withdrawn authorizations cannot continue recognizing a voice.
                let _ = sqlx::query(
                    "DELETE FROM voice_profiles WHERE lower(trim(name)) IN(SELECT \
                     lower(trim(subject_name)) FROM privacy_authorizations \
                     WHERE withdrawn_at IS NOT NULL OR (expires_at IS NOT NULL \
                     AND julianday(expires_at)<=julianday('now'))) \
                     AND lower(trim(name)) NOT IN(SELECT lower(trim(subject_name)) \
                     FROM privacy_authorizations WHERE withdrawn_at IS NULL \
                     AND (expires_at IS NULL OR julianday(expires_at)>julianday('now')))",
                )
                .execute(&pool)
                .await;
                if p.automatic_retention && p.retention_days > 0 {
                    if let Ok(ids) = retention_candidates(&pool, &p).await {
                        let _ = erase_many(&app, &pool, &ids).await;
                    }
                }
                if let Ok(roots) = super::erasure::roots(&app).await {
                    let _ = super::erasure::resume(&pool, &roots).await;
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(3540)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    async fn database() -> SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }
    #[tokio::test]
    async fn local_only_policy_blocks_external_and_lookalike_hosts() {
        let pool = database().await;
        sqlx::query("UPDATE privacy_settings SET policy=?")
            .bind(
                serde_json::to_string(&Policy {
                    local_only: true,
                    ..Default::default()
                })
                .unwrap(),
            )
            .execute(&pool)
            .await
            .unwrap();
        assert!(guard_destination(&pool, "http://127.0.0.1:11434")
            .await
            .is_ok());
        assert!(guard_destination(&pool, "http://[::1]:11434").await.is_ok());
        for url in [
            "https://api.privatemode.ai",
            "https://localhost.attacker.example",
            "http://192.168.1.1",
        ] {
            assert!(guard_destination(&pool, url).await.is_err());
        }
    }
    #[tokio::test]
    async fn protected_mode_approves_exact_destinations_only() {
        let pool = database().await;
        let policy = Policy {
            protected_mode: true,
            approved_hosts: vec!["api.example.org".into()],
            ..Default::default()
        };
        sqlx::query("UPDATE privacy_settings SET policy=?")
            .bind(serde_json::to_string(&policy).unwrap())
            .execute(&pool)
            .await
            .unwrap();
        assert!(guard_destination(&pool, "https://api.example.org/v1")
            .await
            .is_ok());
        assert!(
            guard_destination(&pool, "https://api.example.org.attacker.test/v1")
                .await
                .is_err()
        );
        assert!(guard_destination(&pool, "http://api.example.org/v1")
            .await
            .is_err());
    }
    #[tokio::test]
    async fn voice_permission_must_be_current_and_not_withdrawn() {
        let pool = database().await;
        assert!(voice_authorization(&pool, "Alice").await.is_err());
        sqlx::query("INSERT INTO privacy_authorizations(id,subject_name,purpose,basis,notice_version,recorded_at,expires_at) VALUES('a','Alice','voice_identification','explicit_consent','reference',datetime('now'),datetime('now','+1 day'))").execute(&pool).await.unwrap();
        assert_eq!(voice_authorization(&pool, "Alice").await.unwrap(), "a");
        sqlx::query("UPDATE privacy_authorizations SET withdrawn_at=datetime('now')")
            .execute(&pool)
            .await
            .unwrap();
        assert!(voice_authorization(&pool, "Alice").await.is_err());
        sqlx::query("UPDATE privacy_authorizations SET withdrawn_at=NULL,expires_at=datetime('now','-1 day')").execute(&pool).await.unwrap();
        assert!(voice_authorization(&pool, "Alice").await.is_err());
    }

    #[tokio::test]
    async fn consent_given_while_remembering_a_voice_is_recorded() {
        let pool = database().await;
        sqlx::query("INSERT INTO meetings(id,title,created_at,updated_at) VALUES('m','Weekly sync','2026-10-20T09:00:00+00:00',datetime('now'))").execute(&pool).await.unwrap();
        // Written or other consent must say where it is recorded; unknown methods are refused.
        assert!(record_voice_consent(&pool, "Alice", "in_writing", " ", "m")
            .await
            .is_err());
        assert!(record_voice_consent(&pool, "Alice", "nodded", "", "m")
            .await
            .is_err());
        let id = record_voice_consent(&pool, "Alice", "in_meeting", "", "m")
            .await
            .unwrap();
        assert_eq!(voice_authorization(&pool, " alice ").await.unwrap(), id);
        let (basis, reference): (String, String) =
            sqlx::query_as("SELECT basis,notice_version FROM privacy_authorizations WHERE id=?")
                .bind(&id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(basis, "explicit_consent");
        assert_eq!(
            reference,
            "Agreed in the meeting \"Weekly sync\" (2026-10-20)"
        );
        // Recognition after transcription stays off until the person turns it on.
        assert!(!Policy::default().voice_recognition_automatic);
    }

    // The only test that uses the process-wide pending notice.
    #[tokio::test]
    async fn a_required_notice_must_be_given_and_skipping_is_recorded() {
        use super::super::notice;
        let pool = database().await;
        *CAPTURE_ACK.lock().await = None;
        assert_eq!(
            notice::record(&pool, "silent", true).await.unwrap_err(),
            NOTICE_REQUIRED
        );
        acknowledge(&pool, notice::Method::Skipped, true)
            .await
            .unwrap();
        notice::record(&pool, "skipped", true).await.unwrap();
        let line = notice::summary(&pool, "skipped").await.unwrap().unwrap();
        assert!(line.starts_with("Participant notice skipped at") && !line.contains("turned off"));
        acknowledge(&pool, notice::Method::Copied, true)
            .await
            .unwrap();
        notice::record(&pool, "copied", true).await.unwrap();
        assert!(notice::summary(&pool, "copied")
            .await
            .unwrap()
            .unwrap()
            .ends_with("(notice copied to chat)"));
        let audit: Vec<String> =
            sqlx::query_scalar("SELECT operation FROM privacy_events ORDER BY sequence")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(
            audit,
            ["recording_notice_skipped", "recording_notice_acknowledged"]
        );

        // Without the step, a recording is still recorded, as skipped.
        *CAPTURE_ACK.lock().await = None;
        sqlx::query("UPDATE privacy_settings SET policy=?")
            .bind(
                serde_json::to_string(&Policy {
                    require_recording_notice: false,
                    ..Default::default()
                })
                .unwrap(),
            )
            .execute(&pool)
            .await
            .unwrap();
        notice::record(&pool, "off", false).await.unwrap();
        assert!(notice::summary(&pool, "off")
            .await
            .unwrap()
            .unwrap()
            .ends_with("(notice step turned off)"));
    }

    #[tokio::test]
    async fn subject_review_includes_personal_notes_and_old_note_versions() {
        let pool = database().await;
        for id in ["notes", "history", "unrelated"] {
            sqlx::query("INSERT INTO meetings(id,title,created_at,updated_at) VALUES(?,?,datetime('now'),datetime('now'))").bind(id).bind("Synthetic meeting").execute(&pool).await.unwrap();
        }
        sqlx::query("INSERT INTO workspace_notepads VALUES('notes','Follow up with Alice',0,datetime('now'))").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO note_versions(meeting_id,content,created_at) VALUES('history','Alice asked for a review',datetime('now'))").execute(&pool).await.unwrap();
        assert_eq!(
            subject_ids(&pool, "Alice").await.unwrap(),
            vec!["history", "notes"]
        );
    }

    #[tokio::test]
    async fn subject_search_does_not_depend_on_a_fresh_search_index() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        sqlx::query("INSERT INTO meetings(id,title,created_at,updated_at) VALUES('raw','synthetic',datetime('now'),datetime('now'))").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO transcripts(id,meeting_id,transcript,timestamp) VALUES('line','raw','Alice asked for a review','00:00')").execute(&pool).await.unwrap();
        assert_eq!(subject_ids(&pool, "Alice").await.unwrap(), vec!["raw"]);
    }
}
