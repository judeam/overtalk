//! Opt-in loopback automation. All mutations call the same native app services.
use std::{convert::Infallible, net::TcpListener, sync::Arc, time::Duration};

use hyper::{
    body::HttpBody,
    service::{make_service_fn, service_fn},
    Body, Method, Request, Response, Server, StatusCode,
};
use once_cell::sync::Lazy;
use serde_json::{json, Value};
use sqlx::Row;
use subtle::ConstantTimeEq;
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tokio::{
    sync::{Mutex, Semaphore},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

use crate::integrations::IntegrationSettings;
struct RunningAutomation {
    cancellation: CancellationToken,
    task: JoinHandle<()>,
}

static SERVER: Lazy<Mutex<Option<RunningAutomation>>> = Lazy::new(|| Mutex::new(None));
static RESTART_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));
const MAX_BODY: usize = 1024 * 1024;

pub(crate) async fn shutdown() {
    if let Some(RunningAutomation {
        cancellation: token,
        mut task,
    }) = SERVER.lock().await.take()
    {
        token.cancel();
        if tokio::time::timeout(Duration::from_secs(2), &mut task)
            .await
            .is_err()
        {
            task.abort();
        }
    }
}

pub(crate) async fn restart<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    let _restart = RESTART_LOCK.lock().await;
    shutdown().await;
    let path = crate::integrations::configuration_path()
        .ok_or("Integrations configuration is unavailable")?;
    let settings = crate::integrations::read_settings(&path)?;
    if !settings.api_enabled && !settings.cli_enabled && !settings.mcp_enabled {
        return Ok(());
    }
    let port = if settings.api_port == 0 {
        8421
    } else {
        settings.api_port
    };
    let listener = TcpListener::bind(("127.0.0.1", port))
        .map_err(|e| format!("Cannot listen on local API port {port}: {e}"))?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let limit = Arc::new(Semaphore::new(8));
    let connections = Arc::new(Semaphore::new(32));
    let services = make_service_fn(move |_| {
        let app = app.clone();
        let path = path.clone();
        let limit = limit.clone();
        // Reserve before HTTP headers are parsed. A request-only limit does
        // not bound idle sockets or slow clients that never finish headers.
        let connection = connections.clone().try_acquire_owned();
        async move {
            let connection = connection
                .map_err(|_| std::io::Error::other("Local API connection limit reached"))?;
            Ok::<_, std::io::Error>(service_fn(move |request| {
                let _keep_alive = &connection;
                let app = app.clone();
                let path = path.clone();
                let limit = limit.clone();
                async move {
                    let Ok(_permit) = limit.try_acquire_owned() else {
                        return Ok::<_, Infallible>(reply(
                            StatusCode::TOO_MANY_REQUESTS,
                            json!({"error":"Too many active requests"}),
                        ));
                    };
                    let response = handle(app, path, port, request).await;
                    Ok::<_, Infallible>(response)
                }
            }))
        }
    });
    let token = CancellationToken::new();
    let stopped = token.clone();
    let server = Server::from_tcp(listener)
        .map_err(|e| e.to_string())?
        .http1_max_buf_size(64 * 1024)
        .http1_header_read_timeout(Duration::from_secs(10))
        .serve(services)
        .with_graceful_shutdown(async move { stopped.cancelled().await });
    let task = tokio::spawn(async move {
        if let Err(error) = server.await {
            log::warn!("Local automation listener stopped: {error}");
        }
    });
    *SERVER.lock().await = Some(RunningAutomation {
        cancellation: token,
        task,
    });
    Ok(())
}

fn authorized(settings: &IntegrationSettings, token: &str) -> bool {
    if token.len() < 32 {
        return false;
    }
    [
        (settings.api_enabled, &settings.api_token),
        (settings.cli_enabled, &settings.cli_token),
        (settings.mcp_enabled, &settings.mcp_token),
    ]
    .into_iter()
    .any(|(enabled, secret)| {
        enabled && !secret.is_empty() && bool::from(secret.as_bytes().ct_eq(token.as_bytes()))
    })
}

/// The answer to a client's nonce: an HMAC under that client's token, if it is on.
fn proof_reply(
    settings: &IntegrationSettings,
    port: u16,
    client: &str,
    nonce: &str,
) -> (StatusCode, Value) {
    let secret = match client {
        "api" if settings.api_enabled => settings.api_token.as_str(),
        "cli" if settings.cli_enabled => settings.cli_token.as_str(),
        "mcp" if settings.mcp_enabled => settings.mcp_token.as_str(),
        _ => "",
    };
    if secret.len() < 32 || !overtalk_library_reads::proof::valid_nonce(nonce) {
        return (
            StatusCode::NOT_FOUND,
            json!({"error":"This client is not turned on"}),
        );
    }
    (
        StatusCode::OK,
        json!({"proof": overtalk_library_reads::proof::proof(secret, port, nonce)}),
    )
}

fn reply(status: StatusCode, value: Value) -> Response<Body> {
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .header("cache-control", "no-store")
        .header("x-content-type-options", "nosniff")
        .body(Body::from(value.to_string()))
        .expect("fixed response headers")
}

async fn handle<R: Runtime>(
    app: AppHandle<R>,
    path: std::path::PathBuf,
    port: u16,
    mut request: Request<Body>,
) -> Response<Body> {
    let allowed_host = format!("127.0.0.1:{port}");
    let local_host = format!("localhost:{port}");
    if request.headers().contains_key("origin")
        || !request
            .headers()
            .get("host")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v == allowed_host || v == local_host)
    {
        return reply(
            StatusCode::FORBIDDEN,
            json!({"error":"Browser origins and non-loopback hosts are not allowed"}),
        );
    }
    let settings = match crate::integrations::read_settings(&path) {
        Ok(value) => value,
        Err(_) => {
            return reply(
                StatusCode::SERVICE_UNAVAILABLE,
                json!({"error":"Integrations configuration unavailable"}),
            )
        }
    };
    // A client asks for proof that this is Overtalk before it sends its token.
    if request.method() == Method::GET && request.uri().path() == "/v1/proof" {
        let header = |name: &str| {
            request
                .headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
        };
        let (status, body) = proof_reply(
            &settings,
            port,
            header("x-overtalk-client"),
            header("x-overtalk-nonce"),
        );
        return reply(status, body);
    }
    let token = request
        .headers()
        .get("authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .unwrap_or("");
    if !authorized(&settings, token) {
        return reply(
            StatusCode::UNAUTHORIZED,
            json!({"error":"Invalid or revoked local API token"}),
        );
    }
    if request.method() == Method::GET && request.uri().path() == "/health" {
        return reply(
            StatusCode::OK,
            json!({"version":1,"read_meetings":settings.read_meetings,"read_transcripts":settings.read_transcripts,"read_summaries":settings.read_summaries,"record_control":settings.record_control,"write_notes":settings.write_notes}),
        );
    }
    if request.method() != Method::POST || request.uri().path() != "/v1/call" {
        return reply(
            StatusCode::NOT_FOUND,
            json!({"error":"Use POST /v1/call with {name, arguments}"}),
        );
    }
    if request
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_none_or(|v| v.split(';').next() != Some("application/json"))
    {
        return reply(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            json!({"error":"Use application/json"}),
        );
    }
    let read = async {
        let mut bytes = Vec::new();
        while let Some(chunk) = request.body_mut().data().await {
            let chunk = chunk.map_err(|_| "Request body disconnected")?;
            if bytes.len().saturating_add(chunk.len()) > MAX_BODY {
                return Err("Request body exceeds 1 MiB");
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice::<Value>(&bytes).map_err(|_| "Invalid JSON body")
    };
    let value = match tokio::time::timeout(Duration::from_secs(10), read).await {
        Ok(Ok(value)) => value,
        Ok(Err(error)) => return reply(StatusCode::BAD_REQUEST, json!({"error":error})),
        Err(_) => {
            return reply(
                StatusCode::REQUEST_TIMEOUT,
                json!({"error":"Request body timed out"}),
            )
        }
    };
    let Some(name) = value.get("name").and_then(Value::as_str) else {
        return reply(
            StatusCode::BAD_REQUEST,
            json!({"error":"Operation name required"}),
        );
    };
    let args = value.get("arguments").cloned().unwrap_or(json!({}));
    if !args.is_object() {
        return reply(
            StatusCode::BAD_REQUEST,
            json!({"error":"Arguments must be an object"}),
        );
    }
    let result = audited_dispatch(
        app.state::<crate::state::AppState>().db_manager.pool(),
        name,
        &args,
        dispatch(&app, &settings, name, &args),
    )
    .await;
    match result {
        Ok(value) => reply(StatusCode::OK, json!({"result":value})),
        Err(error) => reply(StatusCode::BAD_REQUEST, json!({"error":error})),
    }
}

// Apply the same fail-closed audit boundary used by native and web IPC.
// Constructing a future does not execute it; no data is read before preflight.
async fn audited_dispatch<F>(
    pool: &sqlx::SqlitePool,
    name: &str,
    args: &Value,
    operation: F,
) -> Result<Value, String>
where
    F: std::future::Future<Output = Result<Value, String>>,
{
    if name.len() > 64 || !name.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') {
        return Err("Invalid automation operation".into());
    }
    let command = format!("automation:{name}");
    let object = crate::security::audit::object_id(args);
    crate::security::audit::preflight(pool, &command, &object).await?;
    let result = operation.await;
    crate::security::audit::record_outcome(pool, &command, &object, result.is_ok()).await?;
    result
}

async fn meeting_details(
    pool: &sqlx::SqlitePool,
    settings: &IntegrationSettings,
    args: &Value,
) -> Result<Value, String> {
    let include = |key: &str| -> Result<bool, String> {
        match args.get(key) {
            None => Ok(true),
            Some(Value::Bool(value)) => Ok(*value),
            _ => Err(format!("{key} must be a boolean")),
        }
    };
    let transcripts = include("include_transcripts")?;
    let summary = include("include_summary")?;
    if !settings.read_meetings
        || (transcripts && !settings.read_transcripts)
        || (summary && !settings.read_summaries)
    {
        return Err("The requested meeting content has not been granted".into());
    }
    let (limit, offset) = paging(args)?;
    let id = string(args, "meeting_id")?;
    let row = sqlx::query("SELECT id,title,created_at,updated_at FROM meetings WHERE id=?")
        .bind(&id)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Meeting not found")?;
    let mut result = json!({
        "id": row.get::<String, _>("id"),
        "title": row.get::<String, _>("title"),
        "created_at": row.get::<String, _>("created_at"),
        "updated_at": row.get::<String, _>("updated_at")
    });
    if transcripts {
        let rows = sqlx::query(
            "SELECT id,transcript,timestamp,audio_start_time,audio_end_time,speaker_id,\
             speaker_confidence,overlap FROM transcripts WHERE meeting_id=? \
             ORDER BY audio_start_time,id LIMIT ? OFFSET ?",
        )
        .bind(&id)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?;
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM transcripts WHERE meeting_id=?")
            .bind(&id)
            .fetch_one(pool)
            .await
            .map_err(|e| e.to_string())?;
        result["has_more"] = json!(offset + (rows.len() as i64) < count);
        result["total_count"] = json!(count);
        result["transcripts"] = json!(rows
            .iter()
            .map(|r| json!({
                "id": r.get::<String, _>("id"),
                "text": r.get::<String, _>("transcript"),
                "timestamp": r.get::<String, _>("timestamp"),
                "audio_start_time": r.get::<Option<f64>, _>("audio_start_time"),
                "audio_end_time": r.get::<Option<f64>, _>("audio_end_time"),
                "speaker_id": r.get::<Option<String>, _>("speaker_id"),
                "speaker_confidence": r.get::<Option<f64>, _>("speaker_confidence"),
                "overlap": r.get::<bool, _>("overlap")
            }))
            .collect::<Vec<_>>());
        let rows =
            sqlx::query("SELECT id,name FROM meeting_speakers WHERE meeting_id=? ORDER BY id")
                .bind(&id)
                .fetch_all(pool)
                .await
                .map_err(|e| e.to_string())?;
        result["speakers"] = json!(rows
            .iter()
            .map(|r| json!({"speaker_id":r.get::<String,_>("id"),"name":r.get::<String,_>("name")}))
            .collect::<Vec<_>>());
    }
    if summary {
        let value: Option<Option<String>> =
            sqlx::query_scalar("SELECT result FROM summary_processes WHERE meeting_id=?")
                .bind(&id)
                .fetch_optional(pool)
                .await
                .map_err(|e| e.to_string())?;
        result["summary"] = value
            .flatten()
            .map(|v| serde_json::from_str(&v).map_err(|_| "Saved summary is invalid".to_string()))
            .transpose()?
            .unwrap_or(Value::Null);
    }
    Ok(result)
}

/// The library reads overtalk-cli also runs on its own while the app is
/// closed, here over the app's pool and behind the audit gate.
struct PoolStore<'a>(&'a sqlx::SqlitePool);

impl overtalk_library_reads::Store for PoolStore<'_> {
    fn rows(
        &mut self,
        sql: &str,
        args: &[overtalk_library_reads::Arg],
    ) -> impl std::future::Future<Output = Result<Vec<Value>, String>> + Send {
        let pool = self.0;
        async move {
            let mut query = sqlx::query_scalar::<_, String>(sql);
            for arg in args {
                query = match arg {
                    overtalk_library_reads::Arg::Null => query.bind(None::<String>),
                    overtalk_library_reads::Arg::Int(value) => query.bind(*value),
                    overtalk_library_reads::Arg::Text(value) => query.bind(value.as_str()),
                };
            }
            let rows = query.fetch_all(pool).await.map_err(|e| e.to_string())?;
            rows.iter()
                .map(|row| serde_json::from_str(row).map_err(|_| "Invalid library row".to_string()))
                .collect()
        }
    }
}

fn grants(settings: &IntegrationSettings) -> overtalk_library_reads::Grants {
    overtalk_library_reads::Grants {
        read_meetings: settings.read_meetings,
        read_transcripts: settings.read_transcripts,
        read_summaries: settings.read_summaries,
        record_control: settings.record_control,
        write_notes: settings.write_notes,
    }
}

fn string(args: &Value, name: &str) -> Result<String, String> {
    args.get(name)
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty() && v.len() <= 4096)
        .map(str::to_owned)
        .ok_or_else(|| format!("A nonempty {name} string is required"))
}
fn paging(args: &Value) -> Result<(i64, i64), String> {
    let limit = match args.get("limit") {
        None => 100,
        Some(v) => v.as_i64().ok_or("limit must be an integer")?,
    };
    let offset = match args.get("offset") {
        None => 0,
        Some(v) => v.as_i64().ok_or("offset must be an integer")?,
    };
    if !(1..=500).contains(&limit) || offset < 0 {
        return Err("Use limit 1–500 and nonnegative offset".into());
    }
    Ok((limit, offset))
}

async fn dispatch<R: Runtime>(
    app: &AppHandle<R>,
    settings: &IntegrationSettings,
    name: &str,
    args: &Value,
) -> Result<Value, String> {
    if !settings.read_meetings {
        return Err("Meeting access is disabled".into());
    }
    let recording = matches!(
        name,
        "start_recording" | "stop_recording" | "pause_recording" | "resume_recording"
    );
    let writing = matches!(
        name,
        "rename_meeting"
            | "rename_speaker"
            | "merge_speakers"
            | "regenerate_summary"
            | "cancel_summary"
            | "set_summary"
    );
    if recording && !settings.record_control {
        return Err("Recording control is disabled in Integrations".into());
    }
    if writing && !settings.write_notes {
        return Err("Notes editing is disabled in Integrations".into());
    }
    let state = app
        .try_state::<crate::state::AppState>()
        .ok_or("Database is not ready")?;
    let pool = state.db_manager.pool();
    // find_meetings, show_meeting, read_transcript, search_meetings,
    // list_projects, list_actions and upcoming_briefs check their own grants.
    if overtalk_library_reads::operation(name).is_some() {
        return overtalk_library_reads::call(&mut PoolStore(pool), grants(settings), name, args)
            .await;
    }
    match name {
        "get_meeting_details" => meeting_details(pool, settings, args).await,
        "list_meetings" => {
            let (limit, offset) = paging(args)?;
            let rows = sqlx::query(
                "SELECT id,title,created_at,updated_at FROM meetings ORDER BY created_at DESC,id \
                 LIMIT ? OFFSET ?",
            )
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await
            .map_err(|e| e.to_string())?;
            Ok(json!(rows
                .iter()
                .map(|r| json!({
                    "id": r.get::<String, _>("id"),
                    "title": r.get::<String, _>("title"),
                    "created_at": r.get::<String, _>("created_at"),
                    "updated_at": r.get::<String, _>("updated_at")
                }))
                .collect::<Vec<_>>()))
        }
        "get_transcript" => {
            if !settings.read_transcripts {
                return Err("Transcript access is disabled".into());
            }
            let (limit, offset) = paging(args)?;
            let result = crate::api::service::api_get_meeting_transcripts(
                app.clone(),
                string(args, "meeting_id")?,
                limit,
                offset,
                state,
            )
            .await?;
            serde_json::to_value(result).map_err(|e| e.to_string())
        }
        "get_meeting" => {
            let row = sqlx::query("SELECT id,title,created_at,updated_at FROM meetings WHERE id=?")
                .bind(string(args, "meeting_id")?)
                .fetch_optional(pool)
                .await
                .map_err(|e| e.to_string())?
                .ok_or("Meeting not found")?;
            Ok(json!({
                "id": row.get::<String, _>("id"),
                "title": row.get::<String, _>("title"),
                "created_at": row.get::<String, _>("created_at"),
                "updated_at": row.get::<String, _>("updated_at")
            }))
        }
        "search_library" => {
            if !settings.read_transcripts || !settings.read_summaries {
                return Err(
                    "Grant transcript and summary access before searching the library".into(),
                );
            }
            let request: super::knowledge::SearchRequest = serde_json::from_value(args.clone())
                .map_err(|_| "Invalid library search filters")?;
            let response =
                super::retrieval::hybrid(app, pool, &request, &CancellationToken::new()).await?;
            serde_json::to_value(response).map_err(|e| e.to_string())
        }
        "search_transcripts" => {
            if !settings.read_transcripts {
                return Err("Transcript access is disabled".into());
            }
            let query = string(args, "query")?;
            let (limit, offset) = paging(args)?;
            let pattern = format!(
                "%{}%",
                query
                    .replace('\\', "\\\\")
                    .replace('%', "\\%")
                    .replace('_', "\\_")
            );
            let rows = sqlx::query(
                "SELECT m.id AS meeting_id,m.title,t.id,t.transcript,t.timestamp,t.speaker_id,\
                 s.name AS speaker_name FROM transcripts t JOIN meetings m ON m.id=t.meeting_id \
                 LEFT JOIN meeting_speakers s ON s.meeting_id=t.meeting_id AND s.id=t.speaker_id \
                 WHERE t.transcript LIKE ? ESCAPE '\\' ORDER BY m.created_at \
                 DESC,t.audio_start_time,t.id LIMIT ? OFFSET ?",
            )
            .bind(pattern)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await
            .map_err(|e| e.to_string())?;
            Ok(json!(rows
                .iter()
                .map(|r| json!({
                    "meeting_id": r.get::<String, _>("meeting_id"),
                    "title": r.get::<String, _>("title"),
                    "transcript_id": r.get::<String, _>("id"),
                    "text": r.get::<String, _>("transcript"),
                    "timestamp": r.get::<String, _>("timestamp"),
                    "speaker_id": r.get::<Option<String>, _>("speaker_id"),
                    "speaker_name": r.get::<Option<String>, _>("speaker_name")
                }))
                .collect::<Vec<_>>()))
        }
        "get_summary" => {
            if !settings.read_summaries {
                return Err("Summary access is disabled".into());
            }
            let row = sqlx::query(
                "SELECT status,result,error,updated_at FROM summary_processes WHERE meeting_id=?",
            )
            .bind(string(args, "meeting_id")?)
            .fetch_optional(pool)
            .await
            .map_err(|e| e.to_string())?;
            Ok(row
                .map(|r| {
                    json!({
                        "status": r.get::<String, _>("status"),
                        "summary": r
                            .get::<Option<String>, _>("result")
                            .and_then(|s| serde_json::from_str::<Value>(&s).ok()),
                        "error": r.get::<Option<String>, _>("error"),
                        "updated_at": r.get::<String, _>("updated_at")
                    })
                })
                .unwrap_or(Value::Null))
        }
        "get_recording_state" => Ok(crate::audio::recording_commands::get_recording_state().await),
        "start_recording" => {
            let title = args
                .get("title")
                .map(|_| string(args, "title"))
                .transpose()?;
            // The normal native start path validates models, devices and active recordings.
            crate::audio::recording_commands::start_recording_with_meeting_name(app.clone(), title)
                .await?;
            Ok(json!({"started":true}))
        }
        "stop_recording" => {
            crate::audio::recording_commands::stop_recording(
                app.clone(),
                crate::audio::recording_commands::RecordingArgs {
                    save_path: String::new(),
                },
            )
            .await?;
            Ok(json!({"stopped":true}))
        }
        "pause_recording" => {
            crate::audio::recording_commands::pause_recording(app.clone()).await?;
            Ok(json!({"paused":true}))
        }
        "resume_recording" => {
            crate::audio::recording_commands::resume_recording(app.clone()).await?;
            Ok(json!({"resumed":true}))
        }
        "rename_meeting" => {
            let id = string(args, "meeting_id")?;
            let title = string(args, "title")?;
            crate::api::service::api_save_meeting_title(
                app.clone(),
                state,
                id.clone(),
                title,
                None,
            )
            .await?;
            let _ = app.emit("meeting-updated", id);
            Ok(json!({"updated":true}))
        }
        "rename_speaker" => {
            let id = string(args, "meeting_id")?;
            crate::speakers::rename_speaker(
                pool,
                &id,
                &string(args, "speaker_id")?,
                &string(args, "name")?,
            )
            .await?;
            let _ = app.emit("meeting-speakers-updated", id);
            Ok(json!({"updated":true}))
        }
        "merge_speakers" => {
            let id = string(args, "meeting_id")?;
            crate::speakers::merge_speakers(
                pool,
                &id,
                &string(args, "from_id")?,
                &string(args, "into_id")?,
            )
            .await?;
            let _ = app.emit("meeting-speakers-updated", id);
            Ok(json!({"updated":true}))
        }
        "set_summary" => {
            if !settings.read_summaries {
                return Err("Summary access is disabled".into());
            }
            let markdown = string(args, "markdown")?;
            crate::summary::commands::api_save_meeting_summary(
                app.clone(),
                state,
                string(args, "meeting_id")?,
                json!({"markdown":markdown}),
                None,
            )
            .await
        }
        "cancel_summary" => {
            crate::summary::commands::api_cancel_summary(
                app.clone(),
                state,
                string(args, "meeting_id")?,
                string(args, "process_id")?,
            )
            .await
        }
        "regenerate_summary" => {
            if !settings.read_transcripts || !settings.read_summaries {
                return Err("Grant transcript and summary access before regenerating".into());
            }
            let id = string(args, "meeting_id")?;
            let rows = sqlx::query(
                "SELECT t.transcript,t.timestamp,t.speaker_id,s.name AS speaker_name \
                 FROM transcripts t LEFT JOIN meeting_speakers s ON s.meeting_id=t.meeting_id \
                 AND s.id=t.speaker_id WHERE t.meeting_id=? ORDER BY t.audio_start_time,t.id",
            )
            .bind(&id)
            .fetch_all(pool)
            .await
            .map_err(|e| e.to_string())?;
            if rows.is_empty() {
                return Err("Meeting has no transcript".into());
            }
            let text = rows
                .iter()
                .map(|row| {
                    let name = row
                        .get::<Option<String>, _>("speaker_name")
                        .or_else(|| row.get::<Option<String>, _>("speaker_id"));
                    format!(
                        "[{}] {}{}",
                        row.get::<String, _>("timestamp"),
                        name.map(|s| format!("{s}: ")).unwrap_or_default(),
                        row.get::<String, _>("transcript")
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            if text.len() > 16 * 1024 * 1024 {
                return Err("Transcript exceeds the automation limit".into());
            }
            let config: Option<(String, String)> =
                sqlx::query_as("SELECT provider,model FROM settings LIMIT 1")
                    .fetch_optional(pool)
                    .await
                    .map_err(|e| e.to_string())?;
            let (provider, model) = config.ok_or("Select a summary model in Overtalk first")?;
            let template = args
                .get("template_id")
                .map(|_| string(args, "template_id"))
                .transpose()?;
            let result = crate::summary::commands::api_process_transcript(
                app.clone(),
                state,
                text,
                provider,
                model,
                Some(id),
                None,
                None,
                None,
                template,
                None,
                None,
            )
            .await?;
            serde_json::to_value(result).map_err(|e| e.to_string())
        }
        _ => Err("Unknown automation operation".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_app_proves_only_an_enabled_clients_token() {
        let nonce = "0123456789abcdef0123456789abcdef";
        let mut settings = IntegrationSettings {
            cli_enabled: true,
            cli_token: "c".repeat(64),
            mcp_token: "m".repeat(64),
            ..Default::default()
        };
        let (status, body) = proof_reply(&settings, 8421, "cli", nonce);
        assert_eq!(status, StatusCode::OK);
        assert!(overtalk_library_reads::proof::verify(
            &"c".repeat(64),
            8421,
            nonce,
            body["proof"].as_str().unwrap()
        ));
        // A client that is off, an unknown client and a malformed nonce learn nothing.
        assert_eq!(
            proof_reply(&settings, 8421, "mcp", nonce).0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            proof_reply(&settings, 8421, "other", nonce).0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            proof_reply(&settings, 8421, "cli", "short").0,
            StatusCode::NOT_FOUND
        );
        settings.cli_token = "short".into();
        assert_eq!(
            proof_reply(&settings, 8421, "cli", nonce).0,
            StatusCode::NOT_FOUND
        );
    }
    #[tokio::test]
    async fn protected_automation_records_access_and_fails_closed() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        sqlx::query("UPDATE privacy_settings SET policy=? WHERE id=1")
            .bind(r#"{"protected_mode":true}"#)
            .execute(&pool)
            .await
            .unwrap();
        let args = json!({"meeting_id":"private name, never log this"});
        audited_dispatch(&pool, "get_summary", &args, async {
            Ok(json!({"ok":true}))
        })
        .await
        .unwrap();
        let events: Vec<(String, String)> =
            sqlx::query_as("SELECT operation,object_id FROM privacy_events ORDER BY sequence")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(
            events,
            vec![
                ("requested:automation:get_summary".into(), "".into()),
                ("automation:get_summary".into(), "".into())
            ]
        );
        assert!(crate::security::audit::verify(&pool).await.unwrap());
        sqlx::query("DROP TABLE privacy_events")
            .execute(&pool)
            .await
            .unwrap();
        let ran = std::sync::atomic::AtomicBool::new(false);
        assert!(audited_dispatch(&pool, "get_summary", &args, async {
            ran.store(true, std::sync::atomic::Ordering::Relaxed);
            Ok(Value::Null)
        })
        .await
        .is_err());
        assert!(!ran.load(std::sync::atomic::Ordering::Relaxed));
    }
    #[tokio::test]
    async fn gateway_meeting_details_preserve_content_grants() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        sqlx::query("INSERT INTO meetings(id,title,created_at,updated_at) VALUES('one','Synthetic','now','now')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO transcripts(id,meeting_id,transcript,timestamp,audio_start_time) VALUES('t','one','Synthetic words','00:01',1)").execute(&pool).await.unwrap();
        let mut settings = IntegrationSettings {
            read_meetings: true,
            ..Default::default()
        };
        let request =
            json!({"meeting_id":"one","include_transcripts":false,"include_summary":false});
        let metadata = meeting_details(&pool, &settings, &request).await.unwrap();
        assert!(metadata.get("transcripts").is_none());
        assert!(metadata.get("summary").is_none());
        assert!(
            meeting_details(&pool, &settings, &json!({"meeting_id":"one"}))
                .await
                .is_err()
        );
        settings.read_transcripts = true;
        settings.read_summaries = true;
        let result = meeting_details(&pool, &settings, &json!({"meeting_id":"one"}))
            .await
            .unwrap();
        assert_eq!(result["transcripts"][0]["text"], "Synthetic words");
        assert_eq!(result["total_count"], 1);
        assert_eq!(result["has_more"], false);
        assert!(meeting_details(
            &pool,
            &settings,
            &json!({"meeting_id":"one","include_transcripts":"false"})
        )
        .await
        .is_err());
    }
    #[tokio::test]
    async fn library_reads_run_on_the_app_pool_under_its_grants() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        sqlx::raw_sql(
            "INSERT INTO meetings(id,title,created_at,updated_at)
              VALUES('m1','Synthetic sync',strftime('%Y-%m-%dT%H:%M:%SZ','now','-1 day'),'now');
             INSERT INTO transcripts(id,meeting_id,transcript,timestamp,audio_start_time,audio_end_time)
              VALUES('t1','m1','We ship on Friday.','00:04',4,7),('t2','m1','Priya sends the notes.','00:09',9,12);
             INSERT INTO summary_processes(meeting_id,status,created_at,updated_at,result)
              VALUES('m1','completed','now','now',
              '{\"markdown\":\"## Decisions\\n- Ship on Friday [S1]\\n## Action Items\\n- **Priya:** Send the notes [S2]\"}');
             INSERT INTO projects(id,name,created_at,updated_at) VALUES('p1','Synthetic project','now','now');
             INSERT INTO project_meetings(meeting_id,project_id) VALUES('m1','p1');
             INSERT INTO workspace_tasks(id,meeting_id,title,owner,due_date,status,created_at,updated_at)
              VALUES('k1','m1','Send the notes','Priya',date('now','localtime','-2 days'),'open','now','now');
             INSERT INTO workspace_calendar(id,source,uid,title,starts_at,ends_at,timezone)
              VALUES('e1','local','u1','Next sync',strftime('%Y-%m-%dT%H:%M:%SZ','now','+1 day'),
              strftime('%Y-%m-%dT%H:%M:%SZ','now','+2 days'),'UTC');
             INSERT INTO meeting_briefs(event_id,project_id,title,starts_at,body,status,created_at,updated_at)
              VALUES('e1','p1','Next sync',strftime('%Y-%m-%dT%H:%M:%SZ','now','+1 day'),
              '{\"summary\":\"Second sync.\",\"agenda\":[{\"text\":\"Check the release\",\"source\":{\"meeting_id\":\"m1\",\"start\":4}}]}',
              'ready','now','now');",
        )
        .execute(&pool)
        .await
        .unwrap();
        let settings = IntegrationSettings {
            read_meetings: true,
            read_summaries: true,
            ..Default::default()
        };
        let call = |name: &'static str, args: Value| {
            let (pool, grants) = (&pool, grants(&settings));
            async move { overtalk_library_reads::call(&mut PoolStore(pool), grants, name, &args).await }
        };
        let found = call("find_meetings", json!({"project":"synthetic PROJECT"}))
            .await
            .unwrap();
        assert_eq!(found["meetings"][0]["id"], "m1");
        let shown = call("show_meeting", json!({"meeting_id":"m1"}))
            .await
            .unwrap();
        assert_eq!(shown["decisions"][0]["time"], "00:04");
        assert_eq!(shown["actions"][0]["owner"], "Priya");
        assert!(shown["decisions"][0]["sources"][0].get("quote").is_none());
        let actions = call("list_actions", json!({"due":"overdue"}))
            .await
            .unwrap();
        assert_eq!(actions["tracked"][0]["overdue"], true);
        assert_eq!(actions["from_notes"][0]["time"], "00:09");
        let briefs = call("upcoming_briefs", json!({})).await.unwrap();
        assert_eq!(
            briefs["events"][0]["brief"]["agenda"][0]["source"]["meeting_title"],
            "Synthetic sync"
        );
        let projects = call("list_projects", json!({})).await.unwrap();
        assert_eq!(projects["projects"][0]["open_actions"], 1);
        let search = call("search_meetings", json!({"query":"friday"}))
            .await
            .unwrap();
        assert_eq!(search["results"][0]["kind"], "notes");
        assert_eq!(search["not_searched"][0]["content"], "transcripts");
        assert_eq!(
            call("read_transcript", json!({"meeting_id":"m1"}))
                .await
                .unwrap_err(),
            "Turn on “Allow reading transcripts” in Overtalk → Settings → Automation → Connections."
        );
    }
    #[test]
    fn tokens_are_independent_revocable_and_disabled_by_default() {
        let mut settings = IntegrationSettings {
            api_token: "a".repeat(64),
            cli_token: "b".repeat(64),
            mcp_token: "c".repeat(64),
            ..Default::default()
        };
        assert!(!authorized(&settings, &settings.api_token));
        settings.cli_enabled = true;
        assert!(authorized(&settings, &settings.cli_token));
        assert!(!authorized(&settings, &settings.api_token));
        assert!(!authorized(&settings, &settings.mcp_token));
        settings.cli_enabled = false;
        assert!(!authorized(&settings, &settings.cli_token));
        settings.api_enabled = true;
        assert!(!authorized(&settings, "a"));
        assert!(!authorized(&settings, &"z".repeat(64)));
    }
    #[test]
    fn paging_rejects_unbounded_negative_and_noninteger_inputs() {
        assert_eq!(
            paging(&json!({"limit":500,"offset":503})).unwrap(),
            (500, 503)
        );
        for value in [
            json!({"limit":501}),
            json!({"limit":0}),
            json!({"offset":-1}),
            json!({"limit":true}),
            json!({"offset":0.5}),
        ] {
            assert!(paging(&value).is_err());
        }
    }
}
