// Model definitions and prompt templates for built-in AI summary generation
// Designed for easy extension - just add new entries to get_available_models()

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ============================================================================
// Model Definitions
// ============================================================================

/// Sampling parameters supported by the built-in AI -> llama-helper pipeline.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SamplingParams {
    /// Temperature - 0.0 triggers greedy decoding in llama-helper.
    pub temperature: f32,

    /// Top-K sampling - limits vocabulary to top K tokens.
    pub top_k: i32,

    /// Top-P (nucleus) sampling - cumulative probability threshold.
    pub top_p: f32,

    /// Presence penalty - discourages reusing tokens that already appeared in the generated output.
    pub presence_penalty: f32,

    /// Frequency penalty - discourages repeated token frequency in the generated output.
    pub frequency_penalty: f32,

    /// Repeat penalty - llama.cpp repeat penalty, 1.0 disables it.
    pub repeat_penalty: f32,

    /// Number of recent generated tokens to apply penalties over, 0 disables penalties.
    pub penalty_last_n: i32,

    /// Stop tokens - generation stops when any of these appear in output
    pub stop_tokens: Vec<String>,
}

impl SamplingParams {
    /// Restrained near-greedy preset for fuller but still conservative output.
    pub fn tight_structured(stop_tokens: Vec<String>) -> Self {
        Self {
            temperature: 0.1,
            top_k: 20,
            top_p: 0.88,
            presence_penalty: 0.0,
            frequency_penalty: 0.0,
            repeat_penalty: 1.0,
            penalty_last_n: 0,
            stop_tokens,
        }
    }

    /// Summary-tuned Qwen 3.5 preset: non-greedy with mild repetition controls.
    pub fn qwen35_summary(stop_tokens: Vec<String>) -> Self {
        Self {
            temperature: 0.5,
            top_k: 20,
            top_p: 0.8,
            presence_penalty: 0.3,
            frequency_penalty: 0.0,
            repeat_penalty: 1.05,
            penalty_last_n: 256,
            stop_tokens,
        }
    }

    /// Near-greedy preset for extracting facts line by line; the model's stop tokens are added
    /// by the caller.
    pub fn extraction() -> Self {
        Self {
            temperature: 0.2,
            top_k: 20,
            top_p: 0.9,
            presence_penalty: 0.0,
            frequency_penalty: 0.0,
            repeat_penalty: 1.0,
            penalty_last_n: 0,
            stop_tokens: Vec::new(),
        }
    }

    /// Gemma 3 and Gemma 4 instruct preset (temperature 1.0, top-k 64, top-p 0.95 per both model cards).
    pub fn gemma3_instruct(stop_tokens: Vec<String>) -> Self {
        Self {
            temperature: 1.0,
            top_k: 64,
            top_p: 0.95,
            presence_penalty: 0.0,
            frequency_penalty: 0.0,
            repeat_penalty: 1.0,
            penalty_last_n: 0,
            stop_tokens,
        }
    }

    /// Normalize built-in presets to the subset supported by llama-helper.
    pub fn sanitize_for_llama_helper(&self) -> Self {
        let temperature = if self.temperature.is_finite() {
            self.temperature.max(0.0)
        } else {
            0.0
        };
        let top_k = self.top_k.max(0);
        let top_p = if self.top_p.is_finite() && self.top_p > 0.0 && self.top_p <= 1.0 {
            self.top_p
        } else {
            1.0
        };
        let presence_penalty = if self.presence_penalty.is_finite() {
            self.presence_penalty.max(0.0)
        } else {
            0.0
        };
        let frequency_penalty = if self.frequency_penalty.is_finite() {
            self.frequency_penalty.max(0.0)
        } else {
            0.0
        };
        let repeat_penalty = if self.repeat_penalty.is_finite() && self.repeat_penalty > 0.0 {
            self.repeat_penalty
        } else {
            1.0
        };
        let penalty_last_n = self.penalty_last_n.max(0);

        Self {
            temperature,
            top_k,
            top_p,
            presence_penalty,
            frequency_penalty,
            repeat_penalty,
            penalty_last_n,
            stop_tokens: self.stop_tokens.clone(),
        }
    }
}

/// Definition of a built-in AI model with all metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelDef {
    /// Model name in format "family:variant" (e.g., "gemma3:1b")
    /// This is what's stored in database as model field when provider="builtin-ai"
    pub name: String,

    /// Display name for UI (e.g., "Gemma 3 1B (Fast)")
    pub display_name: String,

    /// GGUF filename on disk (e.g., "gemma-3-1b-it-q4_0.gguf")
    pub gguf_file: String,

    /// Template name for prompt formatting (e.g., "gemma3")
    pub template: String,

    /// Download URL (HuggingFace or other source)
    pub download_url: String,

    /// File size in MiB. The field name is kept for API compatibility.
    pub size_mb: u64,

    /// Context window size in tokens (configurable per model!)
    /// This is used for chunking in processor.rs
    pub context_size: u32,

    /// Model layer count (for GPU offloading calculation)
    pub layer_count: u32,

    /// Sampling parameters for this model
    pub sampling: SamplingParams,

    /// Short description for UI
    pub description: String,
    /// Hash for models downloaded from a pinned upstream revision.
    pub expected_sha256: Option<String>,

    /// Transcript a small model reads per call (estimated tokens, see `rough_token_count`) when
    /// notes for a long meeting are built window by window. None: read as much as fits.
    #[serde(default)]
    pub reading_window: Option<u32>,
}

/// Get all available built-in AI models
/// Add new models here - the system will automatically detect and manage them
pub fn get_available_models() -> Vec<ModelDef> {
    vec![
        ModelDef {
            name: "qwen3.8:27b".into(), display_name: "Qwen 3.8 27B · calibrated IQ4_XS".into(),
            gguf_file: "Qwen3.8-27B-AP-IQ4_XS.gguf".into(), template: "qwen3.5_nonthinking".into(),
            download_url: "https://huggingface.co/agentionai/Qwen3.8-27B-AP-GGUF/resolve/fdb2d87d4f730dd4c395156cf3361acd0d045033/Qwen3.8-27B-AP-IQ4_XS.gguf".into(),
            size_mb: 13593, context_size: 16384, layer_count: 64,
            sampling: SamplingParams::qwen35_summary(vec!["<|im_end|>".into()]),
            description: "Calibrated mixed-precision quality profile. 13.3 GiB weights; leaves more room for context and the desktop. Pinned and verified before availability.".into(),
            expected_sha256: Some("2074fd5c3c7f6540913c2f62ad02c50b3f7dde7880d18b3acb02432f2edcab67".into()),
            reading_window: Some(6000),
        },
        ModelDef {
            name: "qwen3.8:27b-compact".into(), display_name: "Qwen 3.8 27B · GSQ-RCO compact".into(),
            gguf_file: "Qwen3.8-27B-GSQ-RCO-IQ3_S.gguf".into(), template: "qwen3.5_nonthinking".into(),
            download_url: "https://huggingface.co/ISTA-DASLab/Qwen3.8-27B-GSQ-RCO-GGUF/resolve/d562806dbafae37109975e970aae91b43e73b440/Qwen3.8-27B-GSQ-RCO-IQ3_S.gguf".into(),
            size_mb: 11227, context_size: 16384, layer_count: 64,
            sampling: SamplingParams::qwen35_summary(vec!["<|im_end|>".into()]),
            description: "Smaller calibrated alternative. 11 GiB weights; review factuality before choosing this more aggressive quantization.".into(),
            expected_sha256: Some("64b53b64c7aa39f20a7e54bd80582fe595b1d745624ee8a72e92508c0326d810".into()),
            reading_window: Some(6000),
        },
        ModelDef {
            name: "qwen3.8:27b-q4km".into(), display_name: "Qwen 3.8 27B · Q4_K_M reference".into(),
            gguf_file: "Qwen3.8-27B-Q4_K_M.gguf".into(), template: "qwen3.5_nonthinking".into(),
            download_url: "https://huggingface.co/ggml-org/Qwen3.8-27B-GGUF/resolve/71bc7b627595dc8a91039addd9c791ae548d6747/Qwen3.8-27B-Q4_K_M.gguf".into(),
            size_mb: 18095, context_size: 16384, layer_count: 64,
            sampling: SamplingParams::qwen35_summary(vec!["<|im_end|>".into()]),
            description: "Quality model for capable hardware. About 18 GiB of weights plus context memory; use serialized inference on a 24 GB GPU. Benchmark meeting factuality before selecting it.".into(),
            expected_sha256: Some("c600de0300ae8a0eb3a6c0b8b5561b8b96f16bd2c863c2a66c42de29d391a747".into()),
            reading_window: Some(6000),
        },
        // Qwen 3.5 2B - Balanced tier
        ModelDef {
            expected_sha256: Some("aaf42c8b7c3cab2bf3d69c355048d4a0ee9973d48f16c731c0520ee914699223".into()),
            name: "qwen3.5:2b".to_string(),
            display_name: "Qwen 3.5 2B (Balanced)".to_string(),
            gguf_file: "Qwen3.5-2B-Q4_K_M.gguf".to_string(),
            template: "qwen3.5_nonthinking".to_string(),
            download_url: "https://huggingface.co/unsloth/Qwen3.5-2B-GGUF/resolve/f6d5376be1edb4d416d56da11e5397a961aca8ae/Qwen3.5-2B-Q4_K_M.gguf".to_string(),
            size_mb: 1221,
            context_size: 32768,
            layer_count: 24,
            sampling: SamplingParams::qwen35_summary(vec!["<|im_end|>".to_string()]),
            description: "Smallest Qwen 3.5 model, fine for short meetings. For meetings longer than about 20 minutes, Qwen 3.5 4B finds far more of the decisions and action items.".to_string(),
            reading_window: Some(4000),
        },
        // Qwen 3.5 4B - High quality tier
        ModelDef {
            expected_sha256: Some("00fe7986ff5f6b463e62455821146049db6f9313603938a70800d1fb69ef11a4".into()),
            name: "qwen3.5:4b".to_string(),
            display_name: "Qwen 3.5 4B (High Quality)".to_string(),
            gguf_file: "Qwen3.5-4B-Q4_K_M.gguf".to_string(),
            template: "qwen3.5_nonthinking".to_string(),
            download_url: "https://huggingface.co/unsloth/Qwen3.5-4B-GGUF/resolve/e87f176479d0855a907a41277aca2f8ee7a09523/Qwen3.5-4B-Q4_K_M.gguf".to_string(),
            size_mb: 2614,
            context_size: 32768,
            layer_count: 32,
            sampling: SamplingParams::qwen35_summary(vec!["<|im_end|>".to_string()]),
            description: "High-quality Qwen 3.5 model for built-in summaries. Compact option with modest memory requirements.".to_string(),
            reading_window: Some(6000),
        },
        // Gemma 4 E4B and E2B: Google's quantization-aware-trained Q4_0 files, text weights only
        // (the vision projector ships separately and is not needed). Per-layer embedding tables make
        // them larger than the names suggest; llama.cpp keeps those tables in system RAM.
        ModelDef {
            name: "gemma4:e4b".into(), display_name: "Gemma 4 E4B".into(),
            gguf_file: "gemma-4-E4B_q4_0-it.gguf".into(), template: "gemma4".into(),
            download_url: "https://huggingface.co/google/gemma-4-E4B-it-qat-q4_0-gguf/resolve/4b4a2c1d584be7264f87aac328a1bc739ce81b6c/gemma-4-E4B_q4_0-it.gguf".into(),
            size_mb: 4916, context_size: 32768, layer_count: 42,
            sampling: SamplingParams::gemma3_instruct(vec!["<turn|>".into()]),
            description: "Google's newest small model (April 2026). Follows formatting closely and invents the least; weaker than Qwen 3.5 4B on very long meetings. Needs about 6 GB of memory.".into(),
            expected_sha256: Some("676c35070db6dbe52f93e9c864ee0fba4eddea94b9c875d9cb10daff453fbaee".into()),
            reading_window: Some(8000),
        },
        ModelDef {
            name: "gemma4:e2b".into(), display_name: "Gemma 4 E2B".into(),
            gguf_file: "gemma-4-E2B_q4_0-it.gguf".into(), template: "gemma4".into(),
            download_url: "https://huggingface.co/google/gemma-4-E2B-it-qat-q4_0-gguf/resolve/675cff42a74c774d6cb76f76d8eacb49b48c9b93/gemma-4-E2B_q4_0-it.gguf".into(),
            size_mb: 3194, context_size: 32768, layer_count: 35,
            sampling: SamplingParams::gemma3_instruct(vec!["<turn|>".into()]),
            description: "Google's newest small model (April 2026). More accurate than Qwen 3.5 2B but needs about 4 GB of memory, as much as Qwen 3.5 4B.".into(),
            expected_sha256: Some("fa401b55b07ee70a54c6dae3903c783a6e65064312529ea57175cb5f8dec6634".into()),
            reading_window: Some(6000),
        },
        // Gemma 3 4B - Legacy alternative retained for users who prefer Gemma output.
        ModelDef {
            expected_sha256: Some("4996030242583a40aa151ff93f49ed787ac8c25e4120c3ae4588b2e2a7d1ae94".into()),
            name: "gemma3:4b".to_string(),
            display_name: "Gemma 3 4B (Balanced)".to_string(),
            gguf_file: "gemma-3-4b-it-Q4_K_M.gguf".to_string(),
            template: "gemma3".to_string(),
            download_url: "https://huggingface.co/bartowski/google_gemma-3-4b-it-GGUF/resolve/71506238f970075ca85125cd749c28b1b0eee84e/google_gemma-3-4b-it-Q4_K_M.gguf".to_string(),
            size_mb: 2374,
            context_size: 32768,
            layer_count: 35,
            sampling: SamplingParams::gemma3_instruct(vec!["<end_of_turn>".to_string()]),
            description: "Balanced model. Great quality/speed trade-off. Requires ~3.5GB RAM.".to_string(),
            reading_window: Some(6000),
        },
        // Gemma 3 1B - Visible legacy tier retained for already-shipped users.
        ModelDef {
            expected_sha256: Some("375e12a4a18929a641f9744b060d4a7cf4e279530750555828ec0c117870bc96".into()),
            name: "gemma3:1b".to_string(),
            display_name: "Gemma 3 1B (Fast)".to_string(),
            gguf_file: "gemma-3-1b-it-Q8_0.gguf".to_string(),
            template: "gemma3".to_string(),
            download_url: "https://huggingface.co/bartowski/google_gemma-3-1b-it-GGUF/resolve/116f76234503685a98f572982177b11d44ec8ff1/google_gemma-3-1b-it-Q8_0.gguf".to_string(),
            size_mb: 1019,
            context_size: 32768,
            layer_count: 26,
            sampling: SamplingParams::gemma3_instruct(vec!["<end_of_turn>".to_string()]),
            description: "Fastest model. Runs on any hardware with ~1GB RAM. Good for quick summaries.".to_string(),
            reading_window: Some(3400),
        },
    ]
}

/// Get a specific model by name
pub fn get_model_by_name(name: &str) -> Option<ModelDef> {
    get_available_models().into_iter().find(|m| m.name == name)
}

/// Get the strongest supported default for the detected hardware.
pub fn get_default_model() -> ModelDef {
    get_model_by_name(crate::extensions::quality_profile::detected().summary_model)
        .expect("Default model must be defined")
}

/// Resolve model name to full file path in the models directory
pub fn get_model_path(app_data_dir: &std::path::Path, model_name: &str) -> Result<PathBuf> {
    let model =
        get_model_by_name(model_name).ok_or_else(|| anyhow!("Unknown model: {}", model_name))?;

    let models_dir = get_models_directory(app_data_dir);
    let model_path = models_dir.join(&model.gguf_file);

    Ok(model_path)
}

/// Get the models directory path for built-in AI
pub fn get_models_directory(app_data_dir: &std::path::Path) -> PathBuf {
    app_data_dir.join("models").join("summary")
}

// ============================================================================
// Prompt Templates (Model-Specific Formatting)
// ============================================================================

/// Gemma 3 chat template format
pub const GEMMA3_TEMPLATE: &str = "\
<start_of_turn>user
{system_prompt}<end_of_turn>
<start_of_turn>user
{user_prompt}<end_of_turn>
<start_of_turn>model
";

/// Gemma 4 chat template with thinking off: no `<|think|>` token in the system turn.
/// llama-helper adds the BOS token itself.
pub const GEMMA4_TEMPLATE: &str = "\
<|turn>system
{system_prompt}<turn|>
<|turn>user
{user_prompt}<turn|>
<|turn>model
";

/// Qwen 3.5 non-thinking chat template format.
/// This starts the assistant turn with an empty think block so generation begins
/// in direct-response mode for summaries.
pub const QWEN35_NONTHINKING_TEMPLATE: &str = "\
<|im_start|>system
{system_prompt}<|im_end|>
<|im_start|>user
{user_prompt}<|im_end|>
<|im_start|>assistant
<think>

</think>

";

fn escape_user_prompt_control_markers(user_prompt: &str) -> String {
    user_prompt
        .replace("<|im_start|>", "< |im_start| >")
        .replace("<|im_end|>", "< |im_end| >")
        .replace("<start_of_turn>", "< start_of_turn >")
        .replace("<end_of_turn>", "< end_of_turn >")
        .replace("<|turn>", "< |turn >")
        .replace("<turn|>", "< turn| >")
        .replace("<|channel>", "< |channel >")
        .replace("<channel|>", "< channel| >")
        .replace("<|think|>", "< |think| >")
        .replace("<think>", "< think >")
        .replace("</think>", "< /think >")
}

/// Format a prompt using the specified template
///
/// # Arguments
/// * `template_name` - Template identifier (e.g., "gemma3", "chatml", "llama3")
/// * `system_prompt` - System message (instructions for the model)
/// * `user_prompt` - User message (actual task/question)
///
/// # Returns
/// Formatted prompt string ready to send to llama-helper
pub fn format_prompt(
    template_name: &str,
    system_prompt: &str,
    user_prompt: &str,
) -> Result<String> {
    let template = match template_name {
        "gemma3" => GEMMA3_TEMPLATE,
        "gemma4" => GEMMA4_TEMPLATE,
        "qwen3.5_nonthinking" => QWEN35_NONTHINKING_TEMPLATE,
        _ => return Err(anyhow!("Unknown template: {}", template_name)),
    };

    let escaped_user_prompt = escape_user_prompt_control_markers(user_prompt);

    let formatted = template
        .replace("{system_prompt}", system_prompt)
        .replace("{user_prompt}", &escaped_user_prompt);

    Ok(formatted)
}

// ============================================================================
// Configuration Constants
// ============================================================================

/// Default max tokens for generation (increased for better summary quality)
pub const DEFAULT_MAX_TOKENS: i32 = 4096;

/// Idle timeout for sidecar (seconds) - can be overridden via LLAMA_IDLE_TIMEOUT env var
pub const DEFAULT_IDLE_TIMEOUT_SECS: u64 = 300; // 5 minutes

/// Generation timeout (how long to wait for a response)
pub const GENERATION_TIMEOUT_SECS: u64 = 900; // 15 minutes

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qwen35_models_are_registered_with_expected_metadata() {
        let qwen_2b = get_model_by_name("qwen3.5:2b").expect("qwen 2b model should exist");
        assert_eq!(qwen_2b.display_name, "Qwen 3.5 2B (Balanced)");
        assert_eq!(qwen_2b.gguf_file, "Qwen3.5-2B-Q4_K_M.gguf");
        assert_eq!(qwen_2b.template, "qwen3.5_nonthinking");
        assert_eq!(
            qwen_2b.download_url,
            "https://huggingface.co/unsloth/Qwen3.5-2B-GGUF/resolve/f6d5376be1edb4d416d56da11e5397a961aca8ae/Qwen3.5-2B-Q4_K_M.gguf"
        );
        assert_eq!(qwen_2b.size_mb, 1221);
        assert_eq!(qwen_2b.context_size, 32768);
        assert_eq!(qwen_2b.layer_count, 24);
        assert_eq!(
            qwen_2b.sampling,
            SamplingParams::qwen35_summary(vec!["<|im_end|>".to_string()])
        );

        let qwen_4b = get_model_by_name("qwen3.5:4b").expect("qwen 4b model should exist");
        assert_eq!(qwen_4b.display_name, "Qwen 3.5 4B (High Quality)");
        assert_eq!(qwen_4b.gguf_file, "Qwen3.5-4B-Q4_K_M.gguf");
        assert_eq!(qwen_4b.template, "qwen3.5_nonthinking");
        assert_eq!(
            qwen_4b.download_url,
            "https://huggingface.co/unsloth/Qwen3.5-4B-GGUF/resolve/e87f176479d0855a907a41277aca2f8ee7a09523/Qwen3.5-4B-Q4_K_M.gguf"
        );
        assert_eq!(qwen_4b.size_mb, 2614);
        assert_eq!(qwen_4b.context_size, 32768);
        assert_eq!(qwen_4b.layer_count, 32);
        assert_eq!(
            qwen_4b.sampling,
            SamplingParams::qwen35_summary(vec!["<|im_end|>".to_string()])
        );
    }

    #[test]
    fn gemma_models_use_huggingface_urls_and_gemma3_instruct_sampling() {
        let gemma_1b = get_model_by_name("gemma3:1b").expect("gemma 1b model should exist");
        assert_eq!(gemma_1b.gguf_file, "gemma-3-1b-it-Q8_0.gguf");
        assert_eq!(
            gemma_1b.download_url,
            "https://huggingface.co/bartowski/google_gemma-3-1b-it-GGUF/resolve/116f76234503685a98f572982177b11d44ec8ff1/google_gemma-3-1b-it-Q8_0.gguf"
        );
        assert_eq!(
            gemma_1b.sampling,
            SamplingParams::gemma3_instruct(vec!["<end_of_turn>".to_string()])
        );
        assert_eq!(gemma_1b.sampling.temperature, 1.0);
        assert_eq!(gemma_1b.sampling.top_k, 64);
        assert_eq!(gemma_1b.sampling.top_p, 0.95);
        assert_eq!(gemma_1b.sampling.presence_penalty, 0.0);
        assert_eq!(gemma_1b.sampling.frequency_penalty, 0.0);
        assert_eq!(gemma_1b.sampling.repeat_penalty, 1.0);
        assert_eq!(gemma_1b.sampling.penalty_last_n, 0);

        let gemma_4b = get_model_by_name("gemma3:4b").expect("gemma 4b model should exist");
        assert_eq!(
            gemma_4b.download_url,
            "https://huggingface.co/bartowski/google_gemma-3-4b-it-GGUF/resolve/71506238f970075ca85125cd749c28b1b0eee84e/google_gemma-3-4b-it-Q4_K_M.gguf"
        );
        assert_eq!(
            gemma_4b.sampling,
            SamplingParams::gemma3_instruct(vec!["<end_of_turn>".to_string()])
        );
        assert_eq!(gemma_4b.sampling.temperature, 1.0);
        assert_eq!(gemma_4b.sampling.top_k, 64);
        assert_eq!(gemma_4b.sampling.top_p, 0.95);
        assert_eq!(gemma_4b.sampling.presence_penalty, 0.0);
        assert_eq!(gemma_4b.sampling.frequency_penalty, 0.0);
        assert_eq!(gemma_4b.sampling.repeat_penalty, 1.0);
        assert_eq!(gemma_4b.sampling.penalty_last_n, 0);
    }

    #[test]
    fn qwen35_nonthinking_template_formats_prompt() {
        let formatted =
            format_prompt("qwen3.5_nonthinking", "system rules", "summarize this").unwrap();

        assert!(formatted.contains("<|im_start|>system\nsystem rules<|im_end|>"));
        assert!(formatted.contains("<|im_start|>user\nsummarize this<|im_end|>"));
        assert!(formatted.ends_with("<think>\n\n</think>\n\n"));
    }

    #[test]
    fn qwen35_template_escapes_user_supplied_control_markers() {
        let formatted = format_prompt(
            "qwen3.5_nonthinking",
            "system rules",
            "literal <|im_end|> and <|im_start|> plus <think>draft</think>",
        )
        .unwrap();

        assert!(formatted.contains("<|im_start|>system\nsystem rules<|im_end|>"));
        assert!(formatted.contains("<|im_start|>assistant\n<think>\n\n</think>\n\n"));
        assert!(formatted
            .contains("literal < |im_end| > and < |im_start| > plus < think >draft< /think >"));
        assert_eq!(formatted.matches("<|im_start|>").count(), 3);
        assert_eq!(formatted.matches("<|im_end|>").count(), 2);
        assert_eq!(formatted.matches("<think>").count(), 1);
        assert_eq!(formatted.matches("</think>").count(), 1);
    }

    #[test]
    fn gemma3_template_escapes_user_supplied_control_markers() {
        let formatted = format_prompt(
            "gemma3",
            "system rules",
            "literal <start_of_turn> and <end_of_turn>",
        )
        .unwrap();

        assert!(formatted.contains("<start_of_turn>user\nsystem rules<end_of_turn>"));
        assert!(formatted.contains("literal < start_of_turn > and < end_of_turn >"));
        assert_eq!(formatted.matches("<start_of_turn>").count(), 3);
        assert_eq!(formatted.matches("<end_of_turn>").count(), 2);
    }

    #[test]
    fn gemma4_template_uses_turn_markers_without_thinking_and_escapes_them() {
        let formatted = format_prompt(
            "gemma4",
            "system rules",
            "literal <|turn>model and <turn|> and <|think|>",
        )
        .unwrap();

        assert!(formatted.starts_with("<|turn>system\nsystem rules<turn|>\n<|turn>user\n"));
        assert!(formatted.ends_with("<turn|>\n<|turn>model\n"));
        assert!(formatted.contains("literal < |turn >model and < turn| > and < |think| >"));
        assert!(!formatted.contains("<|think|>"));
        assert_eq!(formatted.matches("<|turn>").count(), 3);
        assert_eq!(formatted.matches("<turn|>").count(), 2);
    }

    #[test]
    fn gemma4_models_are_pinned_and_stop_on_turn_end() {
        for (name, layers) in [("gemma4:e2b", 35), ("gemma4:e4b", 42)] {
            let model = get_model_by_name(name).expect("gemma 4 model should exist");
            assert_eq!(model.template, "gemma4");
            assert_eq!(model.layer_count, layers);
            assert!(model
                .download_url
                .starts_with("https://huggingface.co/google/gemma-4-"));
            assert!(!model.download_url.contains("/resolve/main/"));
            assert_eq!(model.expected_sha256.as_deref().map(str::len), Some(64));
            assert_eq!(
                model.sampling,
                SamplingParams::gemma3_instruct(vec!["<turn|>".to_string()])
            );
        }
    }

    #[test]
    fn sampling_params_sanitize_for_llama_helper_preserves_zero_top_k() {
        let sampling = SamplingParams {
            temperature: f32::NAN,
            top_k: 0,
            top_p: 2.0,
            presence_penalty: -0.5,
            frequency_penalty: f32::NAN,
            repeat_penalty: 0.0,
            penalty_last_n: -1,
            stop_tokens: vec!["stop".to_string()],
        };

        let sanitized = sampling.sanitize_for_llama_helper();

        assert_eq!(sanitized.temperature, 0.0);
        assert_eq!(sanitized.top_k, 0);
        assert_eq!(sanitized.top_p, 1.0);
        assert_eq!(sanitized.presence_penalty, 0.0);
        assert_eq!(sanitized.frequency_penalty, 0.0);
        assert_eq!(sanitized.repeat_penalty, 1.0);
        assert_eq!(sanitized.penalty_last_n, 0);
        assert_eq!(sanitized.stop_tokens, vec!["stop".to_string()]);
    }

    #[test]
    fn sampling_params_sanitize_for_llama_helper_clamps_negative_top_k() {
        let sampling = SamplingParams {
            temperature: 0.7,
            top_k: -5,
            top_p: 0.8,
            presence_penalty: 0.3,
            frequency_penalty: 0.0,
            repeat_penalty: 1.05,
            penalty_last_n: 256,
            stop_tokens: vec!["stop".to_string()],
        };

        let sanitized = sampling.sanitize_for_llama_helper();

        assert_eq!(sanitized.top_k, 0);
        assert_eq!(sanitized.temperature, 0.7);
        assert_eq!(sanitized.top_p, 0.8);
        assert_eq!(sanitized.presence_penalty, 0.3);
        assert_eq!(sanitized.repeat_penalty, 1.05);
        assert_eq!(sanitized.penalty_last_n, 256);
    }

    #[test]
    fn sampling_params_sanitize_for_llama_helper_keeps_positive_top_k() {
        let sampling = SamplingParams::qwen35_summary(vec!["stop".to_string()]);

        let sanitized = sampling.sanitize_for_llama_helper();

        assert_eq!(sanitized.top_k, 20);
    }
}

#[cfg(test)]
mod supply_chain_tests {
    #[test]
    fn all_downloadable_models_have_immutable_revisions_and_sha256() {
        for model in super::get_available_models() {
            let digest = model
                .expected_sha256
                .as_deref()
                .expect("Every built-in model needs a digest");
            assert_eq!(digest.len(), 64, "{}", model.name);
            assert!(digest.bytes().all(|b| b.is_ascii_hexdigit()));
            let (_, revision) = model
                .download_url
                .split_once("/resolve/")
                .expect("Pinned model URL");
            let revision = revision.split('/').next().unwrap();
            assert_eq!(revision.len(), 40, "{}", model.name);
            assert!(revision.bytes().all(|b| b.is_ascii_hexdigit()));
        }
    }
}
