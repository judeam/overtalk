use crate::model_catalog::{CatalogAuth, ModelCatalog};
use serde::{Deserialize, Serialize};
use tauri::command;

/// Anthropic (Claude) model information returned to frontend
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AnthropicModel {
    pub id: String,
    pub display_name: Option<String>,
}

static CATALOG: ModelCatalog<'static, AnthropicModel> = ModelCatalog::new(
    "Anthropic",
    "https://api.anthropic.com/v1/models",
    CatalogAuth::Anthropic,
);

/// Fallback models when API fetch fails (matches frontend hardcoded values)
const FALLBACK_MODELS: &[(&str, &str)] = &[
    ("claude-sonnet-4-5-20250929", "Claude 4.5 Sonnet"),
    ("claude-haiku-4-5-20251001", "Claude 4.5 Haiku"),
    ("claude-opus-4-1-20250805", "Claude 4.1 Opus"),
    ("claude-sonnet-4-20250514", "Claude 4 Sonnet"),
];

/// Get fallback models as AnthropicModel vec
fn get_fallback_models() -> Vec<AnthropicModel> {
    FALLBACK_MODELS
        .iter()
        .map(|(id, name)| AnthropicModel {
            id: id.to_string(),
            display_name: Some(name.to_string()),
        })
        .collect()
}

/// Check if model is a chat-capable model
fn is_chat_model(model_id: &str) -> bool {
    let id = model_id.to_lowercase();
    // Include Claude models only
    id.starts_with("claude-")
}

/// Available chat models for this credential, with an offline fallback.
#[command]
pub async fn get_anthropic_models(api_key: Option<String>) -> Result<Vec<AnthropicModel>, String> {
    CATALOG
        .get(api_key, get_fallback_models, |model| {
            is_chat_model(&model.id)
        })
        .await
}
