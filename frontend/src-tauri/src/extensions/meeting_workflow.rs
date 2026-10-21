//! Capture-to-notes handoff and personal writing, independent of the open view.
use crate::state::AppState;
use futures_util::{stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{Row, SqlitePool};
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum WorkflowAction {
    Dashboard {
        refresh: bool,
    },
    Load {
        id: String,
    },
    Save {
        id: String,
        body: String,
        revision: i64,
    },
    Readiness,
    /// Start the model downloads a first meeting needs; recording can begin meanwhile.
    FirstRunDownloads,
    Retry {
        meeting_id: String,
    },
    /// Change the project of a recording in progress, or of a saved meeting.
    CaptureProject {
        meeting_id: String,
        project_id: Option<String>,
    },
    Brief {
        event_id: String,
    },
    Automation {
        settings: Option<AutomationSettings>,
    },
}

/// Preferences an installed edition reads. The open edition only stores them.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AutomationSettings {
    /// `confident`, `suggest` or `off`.
    pub auto_assign: String,
    /// `projects`, `all` or `off`.
    pub briefs: String,
    /// Minutes before an event to remind; 0 turns reminders off.
    pub reminder_minutes: u32,
}
impl Default for AutomationSettings {
    fn default() -> Self {
        Self {
            auto_assign: "confident".into(),
            briefs: "projects".into(),
            reminder_minutes: 10,
        }
    }
}
fn automation_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("automation.json"))
}
pub fn automation_settings(app: &AppHandle) -> AutomationSettings {
    automation_path(app)
        .ok()
        .and_then(|p| crate::security::storage::read_limited(&p, 8192).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// The calendar event a recording belongs to: the one chosen with Join, or a
/// call with invitees or a meeting link happening now.
async fn capture_event(
    pool: &SqlitePool,
    options: &Value,
) -> Result<Option<(Value, &'static str)>, String> {
    if let Some(id) = options["event_id"].as_str().filter(|s| !s.is_empty()) {
        let event: Option<super::calendar::CalendarEvent> =
            sqlx::query_as("SELECT * FROM workspace_calendar WHERE id=?")
                .bind(id)
                .fetch_optional(pool)
                .await
                .map_err(|e| e.to_string())?;
        return Ok(event.map(|e| (json!(e), "join")));
    }
    if options["link_event"] == false {
        return Ok(None);
    }
    let now = chrono::Utc::now();
    let event: Option<super::calendar::CalendarEvent> = sqlx::query_as(
        "SELECT * FROM workspace_calendar WHERE all_day=0 AND julianday(starts_at)<=julianday(?) \
         AND julianday(ends_at)>=julianday(?) AND (meeting_url IS NOT NULL OR participants<>'[]') \
         ORDER BY abs(julianday(starts_at)-julianday(?)) LIMIT 1",
    )
    .bind((now + chrono::Duration::minutes(10)).to_rfc3339())
    .bind(now.to_rfc3339())
    .bind(now.to_rfc3339())
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(event.map(|e| (json!(e), "time")))
}

pub async fn load(pool: &SqlitePool, id: &str) -> Result<Value, String> {
    let pad: Option<(String, i64)> =
        sqlx::query_as("SELECT body,revision FROM workspace_notepads WHERE id=?")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?;
    let jobs: Vec<(String,String,String,String)> = sqlx::query_as("SELECT id,kind,state,detail FROM workspace_jobs WHERE meeting_id=? AND json_extract(payload,'$.automatic')=1 ORDER BY created_at,id")
        .bind(id).fetch_all(pool).await.map_err(|e|e.to_string())?;
    let (body, revision) = pad.unwrap_or_default();
    Ok(
        json!({"body":body,"revision":revision,"jobs":jobs.into_iter().map(|(id,kind,state,detail)|json!({"id":id,"kind":kind,"state":state,"detail":detail})).collect::<Vec<_>>()}),
    )
}

pub async fn save(pool: &SqlitePool, id: &str, body: &str, revision: i64) -> Result<Value, String> {
    if id.is_empty() || id.len() > 200 || body.len() > 128 * 1024 || revision < 0 {
        return Err("Keep personal notes below 128 KB".into());
    }
    let changed = sqlx::query("INSERT INTO workspace_notepads(id,body,revision,updated_at) SELECT ?,?,1,? WHERE ?=0 ON CONFLICT(id) DO UPDATE SET body=excluded.body,revision=workspace_notepads.revision+1,updated_at=excluded.updated_at WHERE workspace_notepads.revision=?")
        .bind(id).bind(body).bind(chrono::Utc::now().to_rfc3339()).bind(revision).bind(revision)
        .execute(pool).await.map_err(|e|e.to_string())?;
    // For an existing pad the SELECT must run even when the expected revision
    // is nonzero, so the update path is explicit below.
    if changed.rows_affected() == 0 && revision > 0 {
        let changed=sqlx::query("UPDATE workspace_notepads SET body=?,revision=revision+1,updated_at=? WHERE id=? AND revision=?")
            .bind(body).bind(chrono::Utc::now().to_rfc3339()).bind(id).bind(revision).execute(pool).await.map_err(|e|e.to_string())?;
        if changed.rows_affected() == 0 {
            return Err("These personal notes changed in another window. Your draft is kept; reopen the meeting before merging it.".into());
        }
    } else if changed.rows_affected() == 0 {
        return Err("These personal notes changed in another window. Your draft is kept; reopen the meeting before merging it.".into());
    }
    Ok(json!({"revision":revision+1}))
}

/// The UI supplies intent before recording starts. Never schedule the library
/// retroactively, and never depend on a particular UI view being mounted.
///
/// Project intent: a `project_id` is the person's choice; `project_mode:
/// "none"` means they chose no project; otherwise a choice made for the linked
/// calendar event applies, and the project stays undecided without one.
pub async fn prepare(pool: &SqlitePool, id: &str, options: &Value) -> Result<Value, String> {
    let template = options["template_id"]
        .as_str()
        .unwrap_or("standard_meeting");
    let language = options["language"].as_str().unwrap_or("auto");
    if template.len() > 200 || language.len() > 80 {
        return Err("Invalid recording options".into());
    }
    let event = capture_event(pool, options).await?;
    let event_id = event
        .as_ref()
        .map(|(e, _)| e["id"].as_str().unwrap_or("").to_owned());
    let (project, origin, reason) = if let Some(project) =
        options["project_id"].as_str().filter(|s| !s.is_empty())
    {
        (Some(project.to_owned()), "manual", String::new())
    } else if options["project_mode"] == "none" {
        (None, "manual", String::new())
    } else {
        let chosen: Option<(Option<String>, String, String)> = match &event_id {
            Some(event) => sqlx::query_as("SELECT project_id,origin,reason FROM event_projects WHERE event_id=? AND origin IN ('manual','automatic')")
                .bind(event).fetch_optional(pool).await.map_err(|e| e.to_string())?,
            None => None,
        };
        match chosen {
            Some((project, origin, _)) if origin == "manual" => {
                let reason = if project.is_some() {
                    "Chosen for this calendar event"
                } else {
                    ""
                };
                (project, "manual", reason.to_owned())
            }
            Some((Some(project), _, reason)) => (Some(project), "automatic", reason),
            _ => (None, "undecided", String::new()),
        }
    };
    let now = chrono::Utc::now().to_rfc3339();
    let mut tx = crate::database::begin_write(pool)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("INSERT INTO workspace_capture_workflows(meeting_id,event_id,project_id,auto_notes,template_id,language,created_at,project_origin,project_reason) VALUES(?,?,?,?,?,?,?,?,?)")
        .bind(id).bind(&event_id).bind(&project).bind(options["auto_notes"].as_bool().unwrap_or(false))
        .bind(template).bind(language).bind(&now).bind(origin).bind(&reason).execute(&mut *tx).await.map_err(|e|e.to_string())?;
    if let Some((event, linked_by)) = &event {
        let text = |key: &str| event[key].as_str().unwrap_or("").to_owned();
        sqlx::query("INSERT INTO meeting_events(meeting_id,event_id,source,series_id,title,starts_at,ends_at,organizer,participants,linked_by,created_at) VALUES(?,?,?,?,?,?,?,?,?,?,?)")
            .bind(id).bind(text("id")).bind(text("source")).bind(text("series_id")).bind(text("title")).bind(text("starts_at")).bind(text("ends_at"))
            .bind(text("organizer")).bind(text("participants")).bind(*linked_by).bind(&now).execute(&mut *tx).await.map_err(|e|e.to_string())?;
    }
    tx.commit().await.map_err(|e| e.to_string())?;
    // Snapshot pre-call writing before audio starts. Edits during capture use
    // this same ID, and therefore need no fragile copy-on-stop operation.
    // Without any, a prepared agenda for this event becomes the starting page.
    let personal = options["personal_notes"].as_str().unwrap_or("").to_owned();
    let personal = if personal.trim().is_empty() {
        match &event_id {
            Some(event) => agenda(pool, event).await?,
            None => String::new(),
        }
    } else {
        personal
    };
    if !personal.is_empty() {
        save(pool, id, &personal, 0).await?;
    }
    Ok(capture_state(pool, id).await?.unwrap_or(Value::Null))
}

async fn agenda(pool: &SqlitePool, event: &str) -> Result<String, String> {
    let body: Option<String> = sqlx::query_scalar(
        "SELECT body FROM meeting_briefs WHERE event_id=? AND status IN ('facts','ready')",
    )
    .bind(event)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;
    let body: Value = body
        .and_then(|b| serde_json::from_str(&b).ok())
        .unwrap_or_default();
    let items: Vec<String> = body["agenda"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["text"].as_str())
        .filter(|text| !text.trim().is_empty())
        .take(12)
        .map(|text| format!("• {}", text.chars().take(300).collect::<String>()))
        .collect();
    Ok(if items.is_empty() {
        String::new()
    } else {
        format!("Agenda\n{}\n", items.join("\n"))
    })
}

/// What the recorder shows: the linked event and how its project was chosen.
pub async fn capture_state(pool: &SqlitePool, id: &str) -> Result<Option<Value>, String> {
    type CaptureRow = (
        Option<String>,
        String,
        String,
        Option<String>,
        Option<String>,
    );
    let row: Option<CaptureRow> = sqlx::query_as("SELECT w.project_id,w.project_origin,w.project_reason,p.name,e.title FROM workspace_capture_workflows w LEFT JOIN projects p ON p.id=w.project_id LEFT JOIN meeting_events e ON e.meeting_id=w.meeting_id WHERE w.meeting_id=?")
        .bind(id).fetch_optional(pool).await.map_err(|e| e.to_string())?;
    Ok(row.map(|(project, origin, reason, name, event)| {
        json!({"meeting_id":id,"project_id":project,"project_name":name,"project_origin":origin,"project_reason":reason,"event_title":event})
    }))
}

/// Ask an installed edition to file an undecided recording from its event.
pub async fn resolve_with_addon(
    app: &AppHandle,
    pool: &SqlitePool,
    id: &str,
) -> Result<(), String> {
    let Some(addon) = crate::addon::installed() else {
        return Ok(());
    };
    let event: Option<String> = sqlx::query_scalar(
        "SELECT json_object('id',e.event_id,'source',e.source,'series_id',e.series_id,'title',\
         e.title,'starts_at',e.starts_at,'ends_at',e.ends_at,'organizer',e.organizer,\
         'participants',e.participants) FROM meeting_events e JOIN workspace_capture_workflows w \
         ON w.meeting_id=e.meeting_id WHERE e.meeting_id=? AND w.project_origin='undecided' \
         AND w.finalized=0",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;
    let Some(event) = event.and_then(|e| serde_json::from_str::<Value>(&e).ok()) else {
        return Ok(());
    };
    if let Some((project, reason)) = addon.project_for_event(app, &event).await {
        sqlx::query(
            "UPDATE workspace_capture_workflows SET project_id=?,project_origin='automatic',\
             project_reason=? WHERE meeting_id=? AND project_origin='undecided' AND finalized=0 \
             AND EXISTS(SELECT 1 FROM projects WHERE id=? AND archived=0)",
        )
        .bind(&project)
        .bind(reason.chars().take(300).collect::<String>())
        .bind(id)
        .bind(&project)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub async fn finalize(pool: &SqlitePool, id: &str) -> Result<(), String> {
    let mut tx = crate::database::begin_write(pool)
        .await
        .map_err(|e| e.to_string())?;
    let row=sqlx::query("SELECT auto_notes,template_id,language,project_id,project_origin,project_reason FROM workspace_capture_workflows WHERE meeting_id=? AND finalized=0")
        .bind(id).fetch_optional(&mut *tx).await.map_err(|e|e.to_string())?;
    if let Some(row) = row {
        if let Some(project) = row.get::<Option<String>, _>("project_id") {
            let automatic = row.get::<String, _>("project_origin") == "automatic";
            sqlx::query(
                "INSERT OR IGNORE INTO project_meetings(meeting_id,project_id,origin,reason) VALUES(?,?,?,?)",
            )
            .bind(id)
            .bind(project)
            .bind(if automatic { "automatic" } else { "manual" })
            .bind(row.get::<String, _>("project_reason"))
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
        }
        // A recording with no live transcript (for example while the speech model was
        // still downloading) is always queued for transcription; notes follow if asked for.
        let has_transcript:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM transcripts WHERE meeting_id=? AND coalesce(is_partial,0)=0 AND trim(transcript)!='')")
            .bind(id).fetch_one(&mut *tx).await.map_err(|e|e.to_string())?;
        let auto_notes = row.get::<bool, _>("auto_notes");
        let now = chrono::Utc::now().to_rfc3339();
        let dependency = (!has_transcript).then(|| format!("auto-transcribe:{id}"));
        for (kind,job_id,payload) in (!has_transcript).then(||("transcribe",format!("auto-transcribe:{id}"),json!({"automatic":true}))).into_iter()
            .chain(auto_notes.then(||("notes",format!("auto-notes:{id}"),json!({"automatic":true,"depends_on":dependency,"template_id":row.get::<String,_>("template_id"),"language":row.get::<String,_>("language")})))) {
            sqlx::query("INSERT OR IGNORE INTO workspace_jobs(id,meeting_id,kind,state,detail,payload,created_at,updated_at) VALUES(?,?,?,'queued','Ready after recording',?,?,?)")
                .bind(job_id).bind(id).bind(kind).bind(payload.to_string()).bind(&now).bind(&now).execute(&mut *tx).await.map_err(|e|e.to_string())?;
        }
        sqlx::query("UPDATE workspace_capture_workflows SET finalized=1 WHERE meeting_id=?")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|e| e.to_string())?;
    }
    tx.commit().await.map_err(|e| e.to_string())
}

/// Sync every connected calendar account, two at a time, then tell the UI and
/// any installed edition that the cache changed.
async fn sync_accounts(app: &AppHandle, state: &State<'_, AppState>) -> Result<(), String> {
    static SYNCING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let Ok(_syncing) = SYNCING.try_lock() else {
        return Ok(());
    };
    let pool = state.db_manager.pool();
    let accounts: Vec<String> = sqlx::query_scalar("SELECT id FROM workspace_connections WHERE provider IN ('google','microsoft') ORDER BY name LIMIT 20")
        .fetch_all(pool).await.map_err(|e| e.to_string())?;
    if accounts.is_empty() {
        return Ok(());
    }
    let tasks:Vec<_>=accounts.into_iter().map(|id| {
        let state=state.clone();
        let pool=pool.clone();
        async move {
            let result=super::connection_services::workspace_sync_calendar(state,id.clone()).await;
            let (success,error)=match result {Ok(_)=>(Some(chrono::Utc::now().to_rfc3339()),None),Err(e)=>(None,Some(e))};
            sqlx::query("INSERT INTO workspace_calendar_sync(connection_id,synced_at,error) VALUES(?,?,?) ON CONFLICT(connection_id) DO UPDATE SET synced_at=coalesce(excluded.synced_at,workspace_calendar_sync.synced_at),error=excluded.error")
                .bind(id).bind(success).bind(error).execute(&pool).await
        }
    }).collect();
    stream::iter(tasks)
        .buffer_unordered(2)
        .collect::<Vec<_>>()
        .await;
    calendar_changed(app, pool).await;
    Ok(())
}

/// Forget choices and briefs for events that ended, then notify listeners.
pub async fn calendar_changed(app: &AppHandle, pool: &SqlitePool) {
    let cutoff = (chrono::Utc::now() - chrono::Duration::days(2)).to_rfc3339();
    let _ = sqlx::query("DELETE FROM event_projects WHERE event_id NOT IN (SELECT id FROM workspace_calendar WHERE julianday(ends_at)>=julianday(?))")
        .bind(&cutoff).execute(pool).await;
    let _ = sqlx::query("DELETE FROM meeting_briefs WHERE julianday(starts_at)<julianday(?)")
        .bind(&cutoff)
        .execute(pool)
        .await;
    crate::addon::calendar_changed(app).await;
    let _ = app.emit("workspace-calendar-changed", ());
}

/// Keep upcoming meetings current without any particular view being open.
pub fn start_calendar_sync(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(20)).await;
        loop {
            if !crate::audio::recording_commands::is_recording().await {
                if let Some(state) = app.try_state::<AppState>() {
                    let _ = sync_accounts(&app, &state).await;
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(15 * 60)).await;
        }
    });
}

async fn dashboard(
    app: &AppHandle,
    state: &State<'_, AppState>,
    refresh: bool,
) -> Result<Value, String> {
    let pool = state.db_manager.pool();
    if refresh {
        sync_accounts(app, state).await?;
    }
    let accounts:Vec<(String,String,String)>=sqlx::query_as("SELECT id,provider,name FROM workspace_connections WHERE provider IN ('google','microsoft') ORDER BY name LIMIT 20")
        .fetch_all(pool).await.map_err(|e|e.to_string())?;
    let now = chrono::Utc::now();
    let events:Vec<super::calendar::CalendarEvent>=sqlx::query_as("SELECT * FROM workspace_calendar WHERE julianday(ends_at)>=julianday(?) AND julianday(starts_at)<=julianday(?) ORDER BY julianday(starts_at) LIMIT 12")
        .bind(now.to_rfc3339()).bind((now+chrono::Duration::days(7)).to_rfc3339()).fetch_all(pool).await.map_err(|e|e.to_string())?;
    let mut rows = Vec::with_capacity(events.len());
    for event in events {
        let project: Option<(Option<String>, String, String, Option<String>)> = sqlx::query_as("SELECT e.project_id,e.origin,e.reason,p.name FROM event_projects e LEFT JOIN projects p ON p.id=e.project_id WHERE e.event_id=?")
            .bind(&event.id).fetch_optional(pool).await.map_err(|e| e.to_string())?;
        let brief: Option<String> =
            sqlx::query_scalar("SELECT status FROM meeting_briefs WHERE event_id=?")
                .bind(&event.id)
                .fetch_optional(pool)
                .await
                .map_err(|e| e.to_string())?;
        let mut row = json!(event);
        if let Some((id, origin, reason, name)) = project {
            row["project"] = json!({"id":id,"name":name,"origin":origin,"reason":reason});
        }
        row["brief"] = json!(brief);
        rows.push(row);
    }
    let sync: Vec<(String, Option<String>, Option<String>)> =
        sqlx::query_as("SELECT connection_id,synced_at,error FROM workspace_calendar_sync")
            .fetch_all(pool)
            .await
            .map_err(|e| e.to_string())?;
    Ok(
        json!({"events":rows,"accounts":accounts.into_iter().map(|(id,provider,name)|json!({"id":id,"provider":provider,"name":name})).collect::<Vec<_>>(),"sync":sync.into_iter().map(|(id,at,error)|json!({"id":id,"at":at,"error":error})).collect::<Vec<_>>()}),
    )
}

#[tauri::command]
pub async fn workspace_workflow(
    app: AppHandle,
    state: State<'_, AppState>,
    action: WorkflowAction,
) -> Result<Value, String> {
    let pool = state.db_manager.pool();
    match action {
        WorkflowAction::Dashboard { refresh } => dashboard(&app, &state, refresh).await,
        WorkflowAction::Load { id } => load(pool, &id).await,
        WorkflowAction::Save { id, body, revision } => save(pool, &id, &body, revision).await,
        WorkflowAction::Retry { meeting_id } => {
            sqlx::query(
                "UPDATE workspace_jobs SET state='queued',detail='Retry queued',updated_at=? \
                 WHERE meeting_id=? AND json_extract(payload,'$.automatic')=1 \
                 AND state IN ('failed','interrupted','cancelled')",
            )
            .bind(chrono::Utc::now().to_rfc3339())
            .bind(&meeting_id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
            load(pool, &meeting_id).await
        }
        WorkflowAction::CaptureProject {
            meeting_id,
            project_id,
        } => {
            let project_id = project_id.filter(|p| !p.is_empty());
            let changed = sqlx::query(
                "UPDATE workspace_capture_workflows SET project_id=?,project_origin='manual',\
                 project_reason='' WHERE meeting_id=? AND finalized=0",
            )
            .bind(&project_id)
            .bind(&meeting_id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
            if changed.rows_affected() == 0 {
                // Already saved: the same as filing the meeting by hand.
                super::projects::apply(
                    pool,
                    super::projects::ProjectAction::Assign {
                        meeting_id: meeting_id.clone(),
                        project_id: project_id.clone(),
                        series: None,
                    },
                )
                .await?;
                let _ = app.emit("workspace-projects-changed", ());
            }
            crate::addon::filed(&app, Some(&meeting_id), None, project_id.as_deref()).await;
            Ok(capture_state(pool, &meeting_id)
                .await?
                .unwrap_or(Value::Null))
        }
        WorkflowAction::Brief { event_id } => {
            let brief: Option<(Option<String>, String, String, String, String, String)> =
                sqlx::query_as(
                    "SELECT b.project_id,p.name,b.title,b.starts_at,b.body,b.status \
                     FROM meeting_briefs b LEFT JOIN projects p ON p.id=b.project_id \
                     WHERE b.event_id=?",
                )
                .bind(&event_id)
                .fetch_optional(pool)
                .await
                .map_err(|e| e.to_string())?;
            Ok(brief.map_or(
                Value::Null,
                |(project_id, project, title, starts_at, body, status)| {
                    json!({
                        "event_id": event_id,
                        "project_id": project_id,
                        "project_name": project,
                        "title": title,
                        "starts_at": starts_at,
                        "status": status,
                        "body": serde_json::from_str::<Value>(&body).unwrap_or_default()
                    })
                },
            ))
        }
        WorkflowAction::Automation { settings } => {
            if let Some(settings) = settings {
                if !matches!(
                    settings.auto_assign.as_str(),
                    "confident" | "suggest" | "off"
                ) || !matches!(settings.briefs.as_str(), "projects" | "all" | "off")
                    || settings.reminder_minutes > 120
                {
                    return Err("Invalid automation settings".into());
                }
                super::integrations::write_private_json(&automation_path(&app)?, &settings)?;
                crate::addon::calendar_changed(&app).await;
            }
            Ok(json!(automation_settings(&app)))
        }
        WorkflowAction::FirstRunDownloads => super::first_run::start_downloads(&app, pool).await,
        WorkflowAction::Readiness => {
            let transcript =
                crate::database::repositories::setting::SettingsRepository::get_transcript_config(
                    pool,
                )
                .await
                .map_err(|e| e.to_string())?;
            let speech: Result<Value, String> = match transcript
                .as_ref()
                .map(|c| c.provider.as_str())
            {
                Some("localSpeech") => super::speech::speech_health(app.clone()).await,
                Some("localWhisper") => {
                    crate::whisper_engine::commands::whisper_get_available_models()
                        .await
                        .map(|models| {
                            json!({
                                "ready": models.iter().any(|m| Some(&m.name)
                                    == transcript.as_ref().map(|c| &c.model)
                                    && matches!(
                                        m.status,
                                        crate::whisper_engine::ModelStatus::Available
                                    ))
                            })
                        })
                }
                Some("parakeet") => {
                    crate::parakeet_engine::commands::parakeet_init().await?;
                    crate::parakeet_engine::commands::parakeet_get_available_models()
                        .await
                        .map(|models| {
                            json!({
                                "ready": models.iter().any(|m| Some(&m.name)
                                    == transcript.as_ref().map(|c| &c.model)
                                    && matches!(
                                        m.status,
                                        crate::parakeet_engine::ModelStatus::Available
                                    ))
                            })
                        })
                }
                Some(_) => Err(
                    "Cloud transcription configured. Test it with a short sound check before a \
                     real call."
                        .into(),
                ),
                None => Err("Choose a speech model in Transcription settings.".into()),
            };
            let config =
                crate::database::repositories::setting::SettingsRepository::get_model_config(pool)
                    .await
                    .map_err(|e| e.to_string())?;
            let models = crate::summary::summary_engine::commands::builtin_ai_list_models(
                app.clone(),
                app.state(),
            )
            .await
            .unwrap_or_default();
            let local_ready = config.as_ref().is_some_and(|c| {
                models.iter().any(|m| {
                    m.name == c.model
                        && matches!(
                            m.status,
                            crate::summary::summary_engine::model_manager::ModelStatus::Available
                        )
                })
            });
            let remote_configured = config.as_ref().is_some_and(|c| {
                !matches!(
                    c.provider.as_str(),
                    "builtin-ai" | "local-llama" | "localllama"
                ) && !c.model.is_empty()
            });
            // Downloads in progress, so the first-run checklist can show them.
            let (_, _, speech_now) = super::first_run::speech(pool).await;
            let speech_progress = match speech_now {
                super::first_run::Speech::Downloading(progress) => Some(progress),
                _ => None,
            };
            let notes_progress = config.as_ref().and_then(|c| {
                models.iter().find(|m| m.name == c.model).and_then(|m| match m.status {
                    crate::summary::summary_engine::model_manager::ModelStatus::Downloading { progress } => Some(progress),
                    _ => None,
                })
            });
            Ok(json!({
                "speech": speech.as_ref().ok(),
                "speech_error": speech.as_ref().err(),
                "speech_progress": speech_progress,
                "notes_ready": local_ready,
                "notes_configured": local_ready || remote_configured,
                "notes_progress": notes_progress,
                "remote": remote_configured,
                "model_label": config.as_ref().map(|c| super::first_run::label(&c.model)),
                "model": config.map(|c| c.model)
            }))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    async fn pool() -> SqlitePool {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        pool
    }
    #[tokio::test]
    async fn notepad_conflicts_preserve_writing_and_revision() {
        let p = pool().await;
        save(&p, "draft", "first", 0).await.unwrap();
        assert!(save(&p, "draft", "stale", 0).await.is_err());
        save(&p, "draft", "second", 1).await.unwrap();
        assert!(save(&p, "draft", "stale", 1).await.is_err());
        assert_eq!(load(&p, "draft").await.unwrap()["body"], "second");
    }
    #[tokio::test]
    async fn recovered_recording_schedules_exactly_once_and_waits_for_transcript() {
        let p = pool().await;
        // Minimal meeting row, same order as recording journal persistence.
        sqlx::query("INSERT INTO meetings(id,title,created_at,updated_at) VALUES('m','Call',datetime('now'),datetime('now'))").execute(&p).await.unwrap();
        prepare(&p,"m",&json!({"auto_notes":true,"personal_notes":"Remember question","template_id":"standard_meeting"})).await.unwrap();
        finalize(&p, "m").await.unwrap();
        finalize(&p, "m").await.unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM workspace_jobs")
                .fetch_one(&p)
                .await
                .unwrap(),
            2
        );
        let payload: String =
            sqlx::query_scalar("SELECT payload FROM workspace_jobs WHERE kind='notes'")
                .fetch_one(&p)
                .await
                .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&payload).unwrap()["depends_on"],
            "auto-transcribe:m"
        );
        assert_eq!(load(&p, "m").await.unwrap()["body"], "Remember question");
    }

    #[tokio::test]
    async fn a_recording_without_a_transcript_is_queued_even_without_automatic_notes() {
        // For example one made while the speech model was still downloading.
        let p = pool().await;
        meeting(&p, "m").await;
        prepare(&p, "m", &json!({"auto_notes":false}))
            .await
            .unwrap();
        finalize(&p, "m").await.unwrap();
        let kinds: Vec<String> =
            sqlx::query_scalar("SELECT kind FROM workspace_jobs WHERE meeting_id='m'")
                .fetch_all(&p)
                .await
                .unwrap();
        assert_eq!(kinds, ["transcribe"]);
    }

    async fn event(p: &SqlitePool, id: &str, minutes_from_now: i64, participants: &str) {
        let start = chrono::Utc::now() + chrono::Duration::minutes(minutes_from_now);
        sqlx::query("INSERT INTO workspace_calendar(id,source,uid,recurrence_id,title,description,starts_at,ends_at,timezone,all_day,location,meeting_url,participants,organizer,series_id) VALUES(?,'google:c',?,'',?,'',?,?,'UTC',0,'',NULL,?,'maya@acme.example','series-1')")
            .bind(id).bind(id).bind(format!("Event {id}")).bind(start.to_rfc3339()).bind((start + chrono::Duration::minutes(30)).to_rfc3339()).bind(participants)
            .execute(p).await.unwrap();
    }
    async fn project(p: &SqlitePool, id: &str) {
        sqlx::query("INSERT INTO projects(id,name,created_at,updated_at) VALUES(?,?,datetime('now'),datetime('now'))")
            .bind(id).bind(format!("Project {id}")).execute(p).await.unwrap();
    }
    async fn meeting(p: &SqlitePool, id: &str) {
        sqlx::query("INSERT INTO meetings(id,title,created_at,updated_at) VALUES(?,'Call',datetime('now'),datetime('now'))")
            .bind(id).execute(p).await.unwrap();
    }
    async fn filed(p: &SqlitePool, id: &str) -> Option<(String, String, String)> {
        sqlx::query_as("SELECT project_id,origin,reason FROM project_meetings WHERE meeting_id=?")
            .bind(id)
            .fetch_optional(p)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn joined_event_is_kept_with_the_meeting_and_its_project_choice_applies() {
        let p = pool().await;
        project(&p, "orion").await;
        event(
            &p,
            "e1",
            60,
            r#"[{"name":"Maya","email":"maya@acme.example"}]"#,
        )
        .await;
        sqlx::query("INSERT INTO event_projects(event_id,project_id,origin,updated_at) VALUES('e1','orion','manual',datetime('now'))").execute(&p).await.unwrap();
        let state = prepare(&p, "m", &json!({"event_id":"e1"})).await.unwrap();
        assert_eq!(state["project_name"], "Project orion");
        assert_eq!(state["event_title"], "Event e1");
        // A later sync replaces the calendar row; the meeting keeps its copy.
        sqlx::query("DELETE FROM workspace_calendar")
            .execute(&p)
            .await
            .unwrap();
        meeting(&p, "m").await;
        finalize(&p, "m").await.unwrap();
        let (project, origin, _) = filed(&p, "m").await.unwrap();
        assert_eq!((project.as_str(), origin.as_str()), ("orion", "manual"));
        let kept: (String, String, String) = sqlx::query_as(
            "SELECT participants,organizer,linked_by FROM meeting_events WHERE meeting_id='m'",
        )
        .fetch_one(&p)
        .await
        .unwrap();
        assert!(kept.0.contains("maya@acme.example"));
        assert_eq!(
            (kept.1.as_str(), kept.2.as_str()),
            ("maya@acme.example", "join")
        );
    }

    #[tokio::test]
    async fn recording_during_a_call_links_it_and_keeps_automatic_choices_explained() {
        let p = pool().await;
        project(&p, "orion").await;
        event(&p, "solo", -5, "[]").await;
        event(
            &p,
            "call",
            -2,
            r#"[{"name":"Maya","email":"maya@acme.example"}]"#,
        )
        .await;
        sqlx::query("INSERT INTO event_projects(event_id,project_id,origin,reason,updated_at) VALUES('call','orion','automatic','Same series as 14 Oct',datetime('now'))").execute(&p).await.unwrap();
        let state = prepare(&p, "m", &json!({})).await.unwrap();
        assert_eq!(state["project_origin"], "automatic");
        meeting(&p, "m").await;
        finalize(&p, "m").await.unwrap();
        assert_eq!(
            filed(&p, "m").await.unwrap(),
            (
                "orion".into(),
                "automatic".into(),
                "Same series as 14 Oct".into()
            )
        );
        // A sound check never links to the call.
        prepare(&p, "check", &json!({"link_event":false}))
            .await
            .unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT count(*) FROM meeting_events WHERE meeting_id='check'"
            )
            .fetch_one(&p)
            .await
            .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn a_person_choosing_no_project_wins_over_the_event() {
        let p = pool().await;
        project(&p, "orion").await;
        event(&p, "call", -2, r#"[{"email":"maya@acme.example"}]"#).await;
        sqlx::query("INSERT INTO event_projects(event_id,project_id,origin,updated_at) VALUES('call','orion','automatic',datetime('now'))").execute(&p).await.unwrap();
        let state = prepare(&p, "m", &json!({"project_mode":"none"}))
            .await
            .unwrap();
        assert_eq!(state["project_origin"], "manual");
        assert!(state["project_id"].is_null());
        meeting(&p, "m").await;
        finalize(&p, "m").await.unwrap();
        assert!(filed(&p, "m").await.is_none());
    }

    #[tokio::test]
    async fn a_prepared_agenda_starts_the_notes_page_only_when_it_is_empty() {
        let p = pool().await;
        event(&p, "call", 30, r#"[{"email":"maya@acme.example"}]"#).await;
        sqlx::query("INSERT INTO meeting_briefs(event_id,title,starts_at,body,status,created_at,updated_at) VALUES('call','Call',datetime('now'),?,'ready',datetime('now'),datetime('now'))")
            .bind(r#"{"agenda":[{"text":"Confirm the release date"},{"text":""}]}"#).execute(&p).await.unwrap();
        prepare(&p, "a", &json!({"event_id":"call"})).await.unwrap();
        assert_eq!(
            load(&p, "a").await.unwrap()["body"],
            "Agenda\n• Confirm the release date\n"
        );
        prepare(&p, "b", &json!({"event_id":"call","personal_notes":"Mine"}))
            .await
            .unwrap();
        assert_eq!(load(&p, "b").await.unwrap()["body"], "Mine");
    }
}
