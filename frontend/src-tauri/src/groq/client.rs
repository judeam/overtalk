use crate::model_catalog::{CatalogAuth, ModelCatalog};
use serde::{Deserialize, Serialize};
use tauri::command;

/// Groq model information returned to frontend
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GroqModel {
    pub id: String,
    pub owned_by: Option<String>,
}

static CATALOG: ModelCatalog<'static, GroqModel> = ModelCatalog::new(
    "Groq",
    "https://api.groq.com/openai/v1/models",
    CatalogAuth::Bearer,
);

/// Fallback models when API fetch fails (matches frontend hardcoded values)
const FALLBACK_MODELS: &[&str] = &["llama-3.3-70b-versatile"];

/// Get fallback models as GroqModel vec
fn get_fallback_models() -> Vec<GroqModel> {
    FALLBACK_MODELS
        .iter()
        .map(|id| GroqModel {
            id: id.to_string(),
            owned_by: None,
        })
        .collect()
}

/// Check if model is a chat-capable model (filter out whisper, etc.)
fn is_chat_model(model_id: &str) -> bool {
    let id = model_id.to_lowercase();
    // Exclude whisper, tool-use specific models, and embedding models
    !id.contains("whisper")
        && !id.contains("embed")
        && !id.contains("guard")
        && !id.contains("tool-use")
}

/// Available chat models for this credential, with an offline fallback.
#[command]
pub async fn get_groq_models(api_key: Option<String>) -> Result<Vec<GroqModel>, String> {
    CATALOG
        .get(api_key, get_fallback_models, |model| {
            is_chat_model(&model.id)
        })
        .await
}
