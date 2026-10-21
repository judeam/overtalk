//! Narrow hook points for a separately distributed edition of the service.
//! The open-source service installs no addon: every hook is then a no-op and
//! `workspace_edition` reports the open edition. Hooks may only add work; they
//! never gate capture, reading, export or recovery.
use serde_json::{json, Value};
use std::sync::OnceLock;
use tauri::{AppHandle, Emitter, Manager};

const UNAVAILABLE: &str = "This feature is not part of this edition of Overtalk.";

#[async_trait::async_trait]
pub trait Addon: Send + Sync + 'static {
    /// Name, capabilities and licence state, shown to the UI.
    fn edition(&self) -> Value;
    /// Start background work once the service and its database are ready.
    fn start(&self, _app: AppHandle) {}
    /// The calendar cache changed after a sync or an import.
    async fn calendar_changed(&self, _app: &AppHandle) {}
    /// A recording is starting for this calendar event and the person left its
    /// project undecided. Returns a project ID and the reason it was chosen.
    async fn project_for_event(
        &self,
        _app: &AppHandle,
        _event: &Value,
    ) -> Option<(String, String)> {
        None
    }
    /// A finished meeting has its final transcript. Runs before notes.
    async fn transcript_ready(&self, _app: &AppHandle, _meeting_id: &str) {}
    /// A person filed a meeting or an upcoming event by hand. `project_id` is
    /// `None` when they chose no project.
    async fn filed(
        &self,
        _app: &AppHandle,
        _meeting_id: Option<&str>,
        _event_id: Option<&str>,
        _project_id: Option<&str>,
    ) {
    }
    /// Commands only an installed edition understands.
    async fn command(&self, _app: &AppHandle, _name: &str, _args: Value) -> Result<Value, String> {
        Err(UNAVAILABLE.into())
    }
}

static ADDON: OnceLock<Box<dyn Addon>> = OnceLock::new();

/// Install an edition before the service starts. Only one can be installed.
pub fn install(addon: Box<dyn Addon>) -> Result<(), &'static str> {
    ADDON
        .set(addon)
        .map_err(|_| "An edition is already installed")
}

pub fn installed() -> Option<&'static dyn Addon> {
    ADDON.get().map(|addon| addon.as_ref())
}

pub(crate) fn start(app: AppHandle) {
    if let Some(addon) = installed() {
        addon.start(app);
    }
}

pub(crate) async fn calendar_changed(app: &AppHandle) {
    if let Some(addon) = installed() {
        addon.calendar_changed(app).await;
    }
}

pub(crate) async fn transcript_ready(app: &AppHandle, meeting_id: &str) {
    if let Some(state) = app.try_state::<crate::state::AppState>() {
        let pool = state.db_manager.pool();
        let has_text: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM transcripts WHERE meeting_id=? AND coalesce(is_partial,0)=0 AND trim(transcript)!='')")
            .bind(meeting_id).fetch_one(pool).await.unwrap_or(false);
        if has_text {
            crate::extensions::usage::count_once(pool, "transcript_ready", meeting_id).await;
        }
    }
    if let Some(addon) = installed() {
        addon.transcript_ready(app, meeting_id).await;
    }
}

pub(crate) async fn filed(
    app: &AppHandle,
    meeting_id: Option<&str>,
    event_id: Option<&str>,
    project_id: Option<&str>,
) {
    if let Some(addon) = installed() {
        addon.filed(app, meeting_id, event_id, project_id).await;
    }
}

/// Events an edition may send. The native adapter forwards only events that
/// the open-source service names, so editions emit through these helpers.
pub fn emit_projects_changed(app: &AppHandle) {
    let _ = app.emit("workspace-projects-changed", ());
}
pub fn emit_calendar_changed(app: &AppHandle) {
    let _ = app.emit("workspace-calendar-changed", ());
}
pub fn emit_brief_ready(app: &AppHandle, event_id: &str, title: &str, starts_at: &str) {
    let _ = app.emit(
        "workspace-brief-ready",
        json!({"event_id":event_id,"title":title,"starts_at":starts_at}),
    );
}

#[tauri::command]
pub async fn workspace_edition() -> Result<Value, String> {
    Ok(installed()
        .map(|addon| addon.edition())
        .unwrap_or_else(|| json!({"name":"open","capabilities":[]})))
}

#[tauri::command]
pub async fn workspace_addon(app: AppHandle, name: String, args: Value) -> Result<Value, String> {
    if name.is_empty() || name.len() > 64 {
        return Err("Invalid command".into());
    }
    match installed() {
        Some(addon) => addon.command(&app, &name, args).await,
        None => Err(UNAVAILABLE.into()),
    }
}
