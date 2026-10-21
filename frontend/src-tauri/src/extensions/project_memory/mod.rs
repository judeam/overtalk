//! Automatic project filing and pre-meeting briefs.
//!
//! Upcoming calendar meetings are filed in their project before they start, and
//! calls with no invite when their transcript is ready; a brief is prepared about an
//! hour before each meeting. The engine owns every table written here and runs this
//! through the `Addon` seam, which other builds may still replace.
mod briefs;
mod learn;
mod matcher;
mod post;
mod signals;
mod upcoming;

#[cfg(test)]
mod tests;

use crate::extensions::meeting_workflow::{automation_settings, AutomationSettings};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use tauri::{AppHandle, Manager};

/// Install filing and briefs into the engine. Call before `crate::service_main`'s loop.
pub fn install() {
    let _ = crate::addon::install(Box::new(ProjectMemory));
}

pub(crate) fn pool(app: &AppHandle) -> Option<SqlitePool> {
    app.try_state::<crate::state::AppState>()
        .map(|state| state.db_manager.pool().clone())
}

pub(crate) fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// The person's choices for automatic filing, briefs and reminders.
pub(crate) fn settings(app: &AppHandle) -> AutomationSettings {
    automation_settings(app)
}

struct ProjectMemory;

#[async_trait::async_trait]
impl crate::addon::Addon for ProjectMemory {
    fn edition(&self) -> Value {
        json!({"name": "open", "label": "Overtalk", "capabilities": ["auto-assign", "briefs"]})
    }

    fn start(&self, app: AppHandle) {
        tauri::async_runtime::spawn(async move {
            // The engine opens its database during setup; wait for it.
            let pool = loop {
                if let Some(pool) = pool(&app) {
                    break pool;
                }
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            };
            upcoming::refresh(&app, &pool).await;
            post::backfill(&app, &pool).await;
            briefs::start(app, pool);
        });
    }

    async fn calendar_changed(&self, app: &AppHandle) {
        if let Some(pool) = pool(app) {
            upcoming::refresh(app, &pool).await;
        }
    }

    async fn project_for_event(&self, app: &AppHandle, event: &Value) -> Option<(String, String)> {
        upcoming::project_for_event(app, &pool(app)?, event).await
    }

    async fn transcript_ready(&self, app: &AppHandle, meeting_id: &str) {
        if let Some(pool) = pool(app) {
            post::transcript_ready(app, &pool, meeting_id).await;
        }
    }

    async fn filed(
        &self,
        app: &AppHandle,
        meeting_id: Option<&str>,
        event_id: Option<&str>,
        project_id: Option<&str>,
    ) {
        if let Some(pool) = pool(app) {
            learn::filed(&pool, meeting_id, event_id, project_id).await;
            upcoming::refresh(app, &pool).await;
            let (app, pool) = (app.clone(), pool.clone());
            tauri::async_runtime::spawn(async move {
                post::backfill(&app, &pool).await;
            });
        }
    }

    async fn command(&self, app: &AppHandle, name: &str, args: Value) -> Result<Value, String> {
        match name {
            "prepare_brief" => {
                let pool = pool(app).ok_or("The meeting library is not ready yet")?;
                let event = args["event_id"]
                    .as_str()
                    .ok_or("Choose an upcoming event")?;
                briefs::prepare_now(app, &pool, event).await
            }
            _ => Err("This command is not supported".into()),
        }
    }
}

pub(crate) fn log_error(message: &str) {
    // Through the application logger, whose privacy filter drops message text.
    log::warn!("Automatic filing and briefs: {message}");
}
