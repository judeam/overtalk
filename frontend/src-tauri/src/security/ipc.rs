//! Audited typed dispatch for the retained web interface. The Tauri allowlist
//! still gates incoming commands; every registered command has a typed adapter.
use serde::de::DeserializeOwned;
use serde_json::Value;
use tauri::{ipc::Invoke, Manager};

#[path = "web_dispatch.rs"]
mod dispatch;

fn argument<T: DeserializeOwned>(args: &Value, name: &str) -> Result<T, String> {
    serde_json::from_value(args.get(name).cloned().unwrap_or(Value::Null))
        .map_err(|_| format!("Invalid or missing argument: {name}"))
}

pub fn handler<F>(_registered: F) -> impl Fn(Invoke) -> bool + Send + Sync + 'static
where
    F: Fn(Invoke) -> bool + Send + Sync + 'static,
{
    move |invoke| {
        let command = invoke.message.command().to_owned();
        if !dispatch::COMMANDS.contains(&command.as_str()) {
            return false;
        }
        let app = invoke.message.webview().app_handle().clone();
        let args = match invoke.message.payload() {
            tauri::ipc::InvokeBody::Json(value) => value.clone(),
            _ => {
                invoke.resolver.reject("Expected JSON command arguments");
                return true;
            }
        };
        invoke.resolver.respond_async(async move {
            let result: Result<Value, String> = async {
                let pool = app
                    .state::<crate::state::AppState>()
                    .db_manager
                    .pool()
                    .clone();
                let object = super::audit::object_id(&args);
                super::audit::preflight(&pool, &command, &object).await?;
                let outcome = dispatch::dispatch(&app, &command, &args).await;
                if super::audit::sensitive(&command) {
                    super::audit::record_outcome(&pool, &command, &object, outcome.is_ok()).await?;
                }
                outcome
            }
            .await;
            result.map_err(tauri::ipc::InvokeError::from)
        });
        true
    }
}
