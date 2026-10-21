use crate::model_catalog::{CatalogAuth, ModelCatalog};
use serde::{Deserialize, Serialize};
use tauri::command;

/// OpenAI model information returned to frontend
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OpenAIModel {
    pub id: String,
}

static CATALOG: ModelCatalog<'static, OpenAIModel> = ModelCatalog::new(
    "OpenAI",
    "https://api.openai.com/v1/models",
    CatalogAuth::Bearer,
);

/// Fallback models when API fetch fails (matches frontend hardcoded values)
const FALLBACK_MODELS: &[&str] = &[
    "gpt-5",
    "gpt-5-mini",
    "gpt-4o",
    "gpt-4.1",
    "gpt-4-turbo",
    "gpt-3.5-turbo",
    "gpt-4o-2024-11-20",
    "gpt-4o-2024-08-06",
    "gpt-4o-mini-2024-07-18",
    "gpt-4.1-2025-04-14",
    "gpt-4.1-nano-2025-04-14",
    "gpt-4.1-mini-2025-04-14",
    "o4-mini-2025-04-16",
    "o3-2025-04-16",
    "o3-mini-2025-01-31",
    "o1-2024-12-17",
    "o1-mini-2024-09-12",
    "gpt-4-turbo-2024-04-09",
    "gpt-4-0125-Preview",
    "gpt-4-vision-preview",
    "gpt-4-1106-Preview",
    "gpt-3.5-turbo-0125",
    "gpt-3.5-turbo-1106",
];

/// Get fallback models as OpenAIModel vec
fn get_fallback_models() -> Vec<OpenAIModel> {
    FALLBACK_MODELS
        .iter()
        .map(|id| OpenAIModel { id: id.to_string() })
        .collect()
}

/// Check if model is a chat-capable model (filter out embedding, tts, etc.)
fn is_chat_model(model_id: &str) -> bool {
    let id = model_id.to_lowercase();
    // Include gpt-*, o1-*, o3-*, o4-* models
    // Exclude embedding, tts, whisper, dall-e, babbage, davinci (non-chat models)
    (id.starts_with("gpt-")
        || id.starts_with("o1-")
        || id.starts_with("o3-")
        || id.starts_with("o4-")
        || id.starts_with("chatgpt-"))
        && !id.contains("embedding")
        && !id.contains("tts")
        && !id.contains("whisper")
        && !id.contains("dall-e")
        && !id.contains("babbage")
        && !id.contains("davinci")
        && !id.contains("instruct")
        && !id.contains("realtime")
        && !id.contains("audio")
}

/// Available chat models for this credential, with an offline fallback.
#[command]
pub async fn get_openai_models(api_key: Option<String>) -> Result<Vec<OpenAIModel>, String> {
    CATALOG
        .get(api_key, get_fallback_models, |model| {
            is_chat_model(&model.id)
        })
        .await
}
