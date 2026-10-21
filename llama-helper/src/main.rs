use std::io::{self, BufRead, Write};
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::pin::pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel};
use serde::{Deserialize, Serialize};

// ============================================================================
// Protocol Messages (JSON over stdin/stdout)
// ============================================================================

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Request {
    Generate {
        prompt: String,
        max_tokens: Option<i32>,
        context_size: Option<u32>,
        model_path: Option<String>,
        // Sampling parameters
        temperature: Option<f32>,
        top_k: Option<i32>,
        top_p: Option<f32>,
        presence_penalty: Option<f32>,
        frequency_penalty: Option<f32>,
        repeat_penalty: Option<f32>,
        penalty_last_n: Option<i32>,
        stop_tokens: Option<Vec<String>>,
        /// Stop when a finished line repeats an earlier one (digits ignored): a looping model.
        #[serde(default)]
        stop_on_repeated_line: bool,
    },
    /// Several independent prompts generated together as parallel sequences: decoding several
    /// sequences costs little more than one, so this is much faster than one request each.
    GenerateBatch {
        prompts: Vec<String>,
        max_tokens: Option<i32>,
        context_size: Option<u32>,
        model_path: Option<String>,
        temperature: Option<f32>,
        top_k: Option<i32>,
        top_p: Option<f32>,
        presence_penalty: Option<f32>,
        frequency_penalty: Option<f32>,
        repeat_penalty: Option<f32>,
        penalty_last_n: Option<i32>,
        stop_tokens: Option<Vec<String>>,
        #[serde(default)]
        stop_on_repeated_line: bool,
    },
    Ping,
    Shutdown,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Response {
    #[serde(rename = "response")]
    Completion {
        text: String,
        error: Option<String>,
    },
    #[serde(rename = "batch_response")]
    BatchCompletion {
        texts: Vec<String>,
        error: Option<String>,
    },
    Pong,
    Goodbye,
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct SamplingConfig {
    temperature: f32,
    top_k: i32,
    top_p: f32,
    presence_penalty: f32,
    frequency_penalty: f32,
    repeat_penalty: f32,
    penalty_last_n: i32,
}

impl SamplingConfig {
    fn from_request(
        temperature: Option<f32>,
        top_k: Option<i32>,
        top_p: Option<f32>,
        presence_penalty: Option<f32>,
        frequency_penalty: Option<f32>,
        repeat_penalty: Option<f32>,
        penalty_last_n: Option<i32>,
    ) -> Self {
        let temperature = temperature.unwrap_or(1.0);
        let temperature = if temperature.is_finite() {
            temperature.max(0.0)
        } else {
            0.0
        };
        let top_k = top_k.unwrap_or(64).max(1);
        let top_p = top_p.unwrap_or(0.95);
        let top_p = if top_p.is_finite() && top_p > 0.0 && top_p <= 1.0 {
            top_p
        } else {
            1.0
        };
        let presence_penalty = presence_penalty.unwrap_or(0.0);
        let presence_penalty = if presence_penalty.is_finite() {
            presence_penalty.max(0.0)
        } else {
            0.0
        };
        let frequency_penalty = frequency_penalty.unwrap_or(0.0);
        let frequency_penalty = if frequency_penalty.is_finite() {
            frequency_penalty.max(0.0)
        } else {
            0.0
        };
        let repeat_penalty = repeat_penalty.unwrap_or(1.0);
        let repeat_penalty = if repeat_penalty.is_finite() && repeat_penalty > 0.0 {
            repeat_penalty
        } else {
            1.0
        };
        let penalty_last_n = penalty_last_n.unwrap_or(0).max(0);

        Self {
            temperature,
            top_k,
            top_p,
            presence_penalty,
            frequency_penalty,
            repeat_penalty,
            penalty_last_n,
        }
    }

    fn uses_penalties(&self) -> bool {
        self.penalty_last_n > 0
            && (self.presence_penalty > 0.0
                || self.frequency_penalty > 0.0
                || (self.repeat_penalty - 1.0).abs() > f32::EPSILON)
    }
}

// ============================================================================
// VRAM Detection and GPU Layer Calculation
// ============================================================================

/// Detect available VRAM in GB
fn detect_vram_gb() -> f32 {
    #[cfg(feature = "metal")]
    {
        // macOS Metal: Query recommended max working set size
        if let Some(vram) = detect_metal_vram() {
            eprintln!("Metal VRAM detected: {:.2} GB", vram);
            return vram;
        }
    }

    #[cfg(feature = "cuda")]
    {
        // NVIDIA CUDA: Query device memory
        if let Some(vram) = detect_cuda_vram() {
            eprintln!("CUDA VRAM detected: {:.2} GB", vram);
            return vram;
        }
    }

    // TODO: Vulkan VRAM detection

    eprintln!("VRAM detection not available, using conservative estimate");
    4.0 // Conservative fallback
}

#[cfg(feature = "metal")]
fn detect_metal_vram() -> Option<f32> {
    if let Ok(output) = std::process::Command::new("sysctl")
        .arg("hw.memsize")
        .output()
    {
        if let Ok(stdout) = String::from_utf8(output.stdout) {
            if let Some(bytes_str) = stdout.split(':').nth(1) {
                if let Ok(bytes) = bytes_str.trim().parse::<u64>() {
                    let gb = bytes as f32 / (1024.0 * 1024.0 * 1024.0);
                    // Assume GPU can use ~60% of system memory on Apple Silicon
                    return Some(gb * 0.6);
                }
            }
        }
    }
    None
}

#[cfg(feature = "cuda")]
fn detect_cuda_vram() -> Option<f32> {
    // Use nvidia-smi to query VRAM
    if let Ok(output) = std::process::Command::new("nvidia-smi")
        .args(["--query-gpu=memory.free", "--format=csv,noheader,nounits"])
        .output()
    {
        if let Ok(stdout) = String::from_utf8(output.stdout) {
            if let Ok(mb) = stdout.trim().parse::<f32>() {
                return Some(mb / 1024.0); // Convert MB to GB
            }
        }
    }
    None
}

/// Calculate safe GPU layer count based on VRAM, model file size, and context size
fn calculate_gpu_layers(
    model_path: &PathBuf,
    model_layers: u32,
    vram_gb: f32,
    context_size: u32,
    exact_kv_gb: Option<f32>,
) -> u32 {
    let file_size_gb = std::fs::metadata(model_path)
        .map(|m| m.len() as f32 / 1024.0 / 1024.0 / 1024.0)
        .unwrap_or(0.0);

    if file_size_gb == 0.0 || model_layers == 0 {
        eprintln!("⚠️ Could not determine model file size, using conservative default");
        return 0;
    }

    // Heuristic: Estimate KV cache size
    // 7B models (approx > 2.5GB) usually have 4096 hidden dim -> ~256MB per 1k context
    // 1B models (approx < 2.5GB) usually have 2048 hidden dim -> ~128MB per 1k context
    let kv_per_1k_gb = if file_size_gb > 2.5 { 0.25 } else { 0.12 };
    let total_kv_gb = exact_kv_gb.unwrap_or((context_size as f32 / 1000.0) * kv_per_1k_gb);

    // Reserve display, recurrent state and bounded prefill workspace.
    let safe_vram = vram_gb - 2.5; // Display, recurrent state and bounded prefill workspace.

    // For debugging
    eprintln!("📊 VRAM Analysis:");
    eprintln!("   • Available: {:.2} GB", vram_gb);
    eprintln!("   • Safe Limit: {:.2} GB", safe_vram);
    eprintln!("   • Model Weights: {:.2} GB", file_size_gb);
    eprintln!(
        "   • KV Cache ({} ctx): {:.2} GB",
        context_size, total_kv_gb
    );

    if safe_vram <= 0.0 {
        eprintln!("⚠️ No safe VRAM available, using CPU only");
        return 0;
    }

    // Calculate cost per layer
    let weight_per_layer = file_size_gb / model_layers as f32;
    let kv_per_layer = total_kv_gb / model_layers as f32;
    let total_per_layer = weight_per_layer + kv_per_layer;

    // Calculate how many layers fit
    let safe_layers = (safe_vram / total_per_layer).floor() as u32;
    let layers = safe_layers.min(model_layers);

    eprintln!(
        "   • Cost per layer: {:.2} MB (Weights) + {:.2} MB (KV) = {:.2} MB",
        weight_per_layer * 1024.0,
        kv_per_layer * 1024.0,
        total_per_layer * 1024.0
    );

    if layers < model_layers {
        eprintln!(
            "⚠️ Memory constrained. Offloading {}/{} layers ({:.1}%)",
            layers,
            model_layers,
            (layers as f32 / model_layers as f32) * 100.0
        );
    } else {
        // llama.cpp counts the output layer as well as the repeating blocks.
        // Passing only block_count leaves one recurrent block on the CPU.
        eprintln!(
            "✅ Full offload possible ({} blocks plus output layer)",
            layers
        );
        return model_layers.saturating_add(1);
    }

    layers
}

/// Get default GPU layer count with smart detection
fn get_default_gpu_layers(
    backend: &LlamaBackend,
    model_path: &PathBuf,
    context_size: u32,
) -> Result<u32> {
    let params = LlamaModelParams::default()
        .with_vocab_only(true)
        .with_n_gpu_layers(0);
    let params = pin!(params);
    let metadata = LlamaModel::load_from_file(backend, model_path, &params)?;
    // vocab_only exits before llama.cpp initializes hparams: n_layer() is zero.
    // GGUF metadata is available, so read the real architecture block count.
    let architecture = metadata.meta_val_str("general.architecture")?;
    let numeric = |key: &str| metadata.meta_val_str(key).ok()?.parse::<u32>().ok();
    let layers = numeric(&format!("{architecture}.block_count"))
        .filter(|count| *count > 0)
        .ok_or_else(|| anyhow::anyhow!("Model metadata has no positive block count"))?;
    let kv = if architecture == "qwen35" {
        match (
            numeric("qwen35.attention.head_count_kv"),
            numeric("qwen35.attention.key_length"),
            numeric("qwen35.attention.value_length"),
            numeric("qwen35.full_attention_interval"),
        ) {
            (Some(heads), Some(key), Some(value), Some(interval)) if interval > 0 => Some(
                (layers / interval) as f32
                    * heads as f32
                    * (key + value) as f32
                    * context_size as f32
                    * 2.0
                    / (1024.0 * 1024.0 * 1024.0),
            ),
            _ => None,
        }
    } else {
        None
    };
    drop(metadata);
    Ok(calculate_gpu_layers(
        model_path,
        layers,
        detect_vram_gb(),
        context_size,
        kv,
    ))
}

// ============================================================================
// Model State Management
// ============================================================================

struct ModelState {
    backend: LlamaBackend,
    model: Option<LlamaModel>,
    model_path: Option<PathBuf>,
    context_size: u32,
    last_activity: Arc<AtomicU64>,
}

impl ModelState {
    fn new() -> Result<Self> {
        let backend = LlamaBackend::init().context("Failed to init LlamaBackend")?;
        Ok(Self {
            backend,
            model: None,
            model_path: None,
            context_size: 2048,
            last_activity: Arc::new(AtomicU64::new(Self::current_timestamp())),
        })
    }

    fn current_timestamp() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    fn update_activity(&self) {
        self.last_activity
            .store(Self::current_timestamp(), Ordering::SeqCst);
    }

    fn seconds_since_activity(&self) -> u64 {
        Self::current_timestamp() - self.last_activity.load(Ordering::SeqCst)
    }

    fn load_model_if_needed(&mut self, model_path: PathBuf, context_size: u32) -> Result<()> {
        // Check if model is already loaded
        if let Some(ref loaded_path) = self.model_path {
            if loaded_path == &model_path && self.context_size == context_size {
                eprintln!("✓ Model already loaded");
                self.update_activity();
                return Ok(());
            }
        }

        eprintln!("📥 Loading model: {}", model_path.display());

        // Detect GPU layers
        self.model = None;
        self.model_path = None; // Release old GPU weights before changing models.
        let gpu_layers = get_default_gpu_layers(&self.backend, &model_path, context_size)?;

        // Configure model parameters with GPU offload
        let model_params = LlamaModelParams::default().with_n_gpu_layers(gpu_layers);
        let model_params = pin!(model_params);

        let model = LlamaModel::load_from_file(&self.backend, model_path.clone(), &model_params)
            .with_context(|| format!("unable to load model at {:?}", model_path))?;

        self.model = Some(model);
        self.model_path = Some(model_path);
        self.context_size = context_size;
        self.update_activity();

        eprintln!("✅ Model loaded successfully");
        Ok(())
    }

    fn generate(
        &mut self,
        prompt: String,
        max_tokens: i32,
        sampling: SamplingConfig,
        stop_tokens: Vec<String>,
        stop_on_repeated_line: bool,
    ) -> Result<String> {
        let start_time = Instant::now();
        let model = self.model.as_ref().context("Model not loaded")?;

        // Calculate thread count (conservative default: max(1, (Cores / 2) + 2))
        // This ensures the UI thread is never starved
        let threads: i32 = std::thread::available_parallelism()
            .map(|n| {
                let cores = n.get() as i32;
                ((cores / 2) + 2).max(1)
            })
            .unwrap_or(2);

        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(Some(
                NonZeroU32::new(self.context_size).context("Invalid ctx size")?,
            ))
            .with_n_batch(self.context_size.min(prefill_chunk()))
            .with_n_ubatch(self.context_size.min(prefill_chunk()))
            .with_n_threads(threads)
            .with_n_threads_batch(threads);

        let mut ctx = model
            .new_context(&self.backend, ctx_params)
            .context("unable to create the llama_context")?;

        let tokens_list = model
            .str_to_token(&prompt, AddBos::Always)
            .with_context(|| "failed to tokenize prompt")?;

        eprintln!("📝 Tokenized prompt: {} tokens", tokens_list.len());
        validate_context_budget(tokens_list.len(), max_tokens, self.context_size)?;

        // Bound prefill activations independently from the full context window.
        let chunk_size = self.context_size.min(prefill_chunk()) as usize;
        let mut batch = LlamaBatch::new(chunk_size, 1);
        let n_prompt_tokens = i32::try_from(tokens_list.len())?;
        for (chunk_index, chunk) in tokens_list.chunks(chunk_size).enumerate() {
            batch.clear();
            for (index, token) in chunk.iter().enumerate() {
                let position = (chunk_index * chunk_size + index) as i32;
                batch
                    .add(*token, position, &[0], position == n_prompt_tokens - 1)
                    .context("Failed to add prompt token")?;
            }
            ctx.decode(&mut batch)
                .context("llama prompt prefill failed")?;
        }
        let prompt_time = start_time.elapsed();
        let mut n_cur = n_prompt_tokens;
        let mut decoder = encoding_rs::UTF_8.new_decoder();
        let mut output = String::new();

        eprintln!("🔄 Starting generation (max_tokens: {})", max_tokens);

        use llama_cpp_2::sampling::LlamaSampler;

        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u32;
        let sampler = if sampling.temperature <= 0.0 {
            if sampling.uses_penalties() {
                LlamaSampler::chain_simple([
                    LlamaSampler::penalties(
                        model.n_vocab(),
                        sampling.penalty_last_n,
                        sampling.repeat_penalty,
                        sampling.frequency_penalty,
                        sampling.presence_penalty,
                    ),
                    LlamaSampler::greedy(),
                ])
            } else {
                LlamaSampler::chain_simple([LlamaSampler::greedy()])
            }
        } else if sampling.uses_penalties() {
            LlamaSampler::chain_simple([
                LlamaSampler::penalties(
                    model.n_vocab(),
                    sampling.penalty_last_n,
                    sampling.repeat_penalty,
                    sampling.frequency_penalty,
                    sampling.presence_penalty,
                ),
                LlamaSampler::top_k(sampling.top_k),
                LlamaSampler::top_p(sampling.top_p, 1),
                LlamaSampler::temp(sampling.temperature),
                LlamaSampler::dist(seed),
            ])
        } else {
            LlamaSampler::chain_simple([
                LlamaSampler::top_k(sampling.top_k),
                LlamaSampler::top_p(sampling.top_p, 1),
                LlamaSampler::temp(sampling.temperature),
                LlamaSampler::dist(seed),
            ])
        };
        let mut sampler = pin!(sampler);

        loop {
            // Check if we've generated enough tokens
            if (n_cur - n_prompt_tokens) >= max_tokens {
                eprintln!("✓ Reached max_tokens limit");
                break;
            }

            let token = sampler.as_mut().sample(&ctx, batch.n_tokens() - 1);
            sampler.as_mut().accept(token);

            if model.is_eog_token(token) {
                eprintln!(
                    "✓ End-of-generation token reached (generated {} chars)",
                    output.len()
                );
                break;
            }

            let output_bytes = match model.token_to_piece_bytes(token, 32, true, None) {
                Err(llama_cpp_2::TokenToStringError::InsufficientBufferSpace(size)) => {
                    let required_size: usize = size
                        .checked_neg()
                        .context("Invalid token piece buffer size")?
                        .try_into()
                        .context("Invalid token piece buffer size")?;
                    model.token_to_piece_bytes(token, required_size, true, None)
                }
                result => result,
            }
            .context("Failed to convert token to bytes")?;

            let mut token_text = String::with_capacity(32);
            let _ = decoder.decode_to_string(&output_bytes, &mut token_text, false);
            output.push_str(&token_text);

            // Check for model-specific stop tokens
            let mut should_stop = false;
            for stop_token in &stop_tokens {
                if output.contains(stop_token) {
                    eprintln!(
                        "✓ Stop token '{}' detected (generated {} chars)",
                        stop_token,
                        output.len()
                    );
                    // Remove the stop token from output
                    output = output.replace(stop_token, "").trim_end().to_string();
                    should_stop = true;
                    break;
                }
            }
            if should_stop {
                break;
            }
            if stop_on_repeated_line && token_text.contains('\n') {
                if let Some(kept) = drop_repeated_last_line(&output) {
                    eprintln!(
                        "✓ Repeated line detected (generated {} chars)",
                        output.len()
                    );
                    output = kept;
                    break;
                }
            }

            batch.clear();
            batch
                .add(token, n_cur, &[0], true)
                .context("Failed to add generated token to batch")?;
            n_cur += 1;
            ctx.decode(&mut batch).context("failed to eval")?;
        }

        // Generation statistics
        let total_time = start_time.elapsed();
        let gen_time = total_time.saturating_sub(prompt_time);
        let output_tokens = (n_cur - n_prompt_tokens) as u64;
        let prompt_tokens = n_prompt_tokens as u64;

        let tokens_per_sec = if gen_time.as_secs_f64() > 0.0 {
            output_tokens as f64 / gen_time.as_secs_f64()
        } else {
            0.0
        };

        eprintln!("📊 Generation Statistics:");
        eprintln!("   • Prompt tokens: {}", prompt_tokens);
        eprintln!("   • Output tokens: {}", output_tokens);
        eprintln!("   • Prompt processing: {:.2}s", prompt_time.as_secs_f64());
        eprintln!("   • Generation time: {:.2}s", gen_time.as_secs_f64());
        eprintln!("   • Total time: {:.2}s", total_time.as_secs_f64());
        eprintln!("   • Speed: {:.2} tokens/sec", tokens_per_sec);

        self.update_activity();
        Ok(output)
    }

    /// Generates every prompt as its own sequence in one context. Each prompt is prefilled in
    /// turn and its first token sampled at once; then one decode per step advances all
    /// unfinished sequences together.
    fn generate_batch(
        &mut self,
        prompts: Vec<String>,
        max_tokens: i32,
        sampling: SamplingConfig,
        stop_tokens: Vec<String>,
        stop_on_repeated_line: bool,
    ) -> Result<Vec<String>> {
        let start_time = Instant::now();
        let model = self.model.as_ref().context("Model not loaded")?;
        anyhow::ensure!(!prompts.is_empty(), "No prompts to generate");
        let n = prompts.len();
        let per_sequence = self.context_size / n as u32;
        let token_lists = prompts
            .iter()
            .map(|prompt| {
                model
                    .str_to_token(prompt, AddBos::Always)
                    .context("failed to tokenize prompt")
            })
            .collect::<Result<Vec<_>>>()?;
        for tokens in &token_lists {
            validate_context_budget(tokens.len(), max_tokens, per_sequence)?;
        }
        let threads: i32 = std::thread::available_parallelism()
            .map(|n| ((n.get() as i32 / 2) + 2).max(1))
            .unwrap_or(2);
        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(Some(
                NonZeroU32::new(per_sequence * n as u32).context("Invalid ctx size")?,
            ))
            .with_n_batch(prefill_chunk())
            .with_n_ubatch(prefill_chunk())
            .with_n_seq_max(n as u32)
            .with_kv_unified(false)
            .with_n_threads(threads)
            .with_n_threads_batch(threads);
        let mut ctx = model
            .new_context(&self.backend, ctx_params)
            .context("unable to create the llama_context")?;
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u32;
        let mut samplers: Vec<_> = (0..n)
            .map(|i| build_sampler(model, &sampling, seed.wrapping_add(i as u32)))
            .collect();
        let mut decoders: Vec<_> = (0..n).map(|_| encoding_rs::UTF_8.new_decoder()).collect();
        let mut outputs = vec![String::new(); n];
        let mut positions = vec![0i32; n];
        let mut next = vec![None; n];
        let mut batch = LlamaBatch::new((prefill_chunk() as usize).max(n), n as i32);

        for (seq, tokens) in token_lists.iter().enumerate() {
            for chunk in tokens.chunks(prefill_chunk() as usize) {
                batch.clear();
                for token in chunk {
                    let last = positions[seq] as usize == tokens.len() - 1;
                    batch
                        .add(*token, positions[seq], &[seq as i32], last)
                        .context("Failed to add prompt token")?;
                    positions[seq] += 1;
                }
                ctx.decode(&mut batch)
                    .context("llama prompt prefill failed")?;
            }
            let token = samplers[seq].sample(&ctx, batch.n_tokens() - 1);
            samplers[seq].accept(token);
            next[seq] = Some(token);
        }
        let prompt_time = start_time.elapsed();

        let mut generated = vec![0i32; n];
        loop {
            batch.clear();
            let mut fed = Vec::new();
            for seq in 0..n {
                let Some(token) = next[seq].take() else {
                    continue;
                };
                if model.is_eog_token(token) || generated[seq] >= max_tokens {
                    continue;
                }
                let text = token_text(model, token, &mut decoders[seq])?;
                generated[seq] += 1;
                if append_and_check(
                    &mut outputs[seq],
                    &text,
                    &stop_tokens,
                    stop_on_repeated_line,
                ) {
                    continue;
                }
                batch
                    .add(token, positions[seq], &[seq as i32], true)
                    .context("Failed to add generated token")?;
                positions[seq] += 1;
                fed.push(seq);
            }
            if fed.is_empty() {
                break;
            }
            ctx.decode(&mut batch).context("failed to eval")?;
            for (index, seq) in fed.into_iter().enumerate() {
                let token = samplers[seq].sample(&ctx, index as i32);
                samplers[seq].accept(token);
                next[seq] = Some(token);
            }
        }

        let total_time = start_time.elapsed();
        eprintln!(
            "📊 Batch of {}: {} prompt tokens in {:.2}s, {} output tokens in {:.2}s",
            n,
            token_lists.iter().map(Vec::len).sum::<usize>(),
            prompt_time.as_secs_f64(),
            generated.iter().sum::<i32>(),
            total_time.saturating_sub(prompt_time).as_secs_f64()
        );
        self.update_activity();
        Ok(outputs)
    }
}

/// Prompt tokens decoded per step. Larger steps make prefill faster on a GPU but need a larger
/// compute buffer. LLAMA_PREFILL_CHUNK overrides it.
fn prefill_chunk() -> u32 {
    std::env::var("LLAMA_PREFILL_CHUNK")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|v| (32..=8192).contains(v))
        .unwrap_or(512)
}

fn build_sampler(
    model: &LlamaModel,
    sampling: &SamplingConfig,
    seed: u32,
) -> llama_cpp_2::sampling::LlamaSampler {
    use llama_cpp_2::sampling::LlamaSampler;
    let penalties = || {
        LlamaSampler::penalties(
            model.n_vocab(),
            sampling.penalty_last_n,
            sampling.repeat_penalty,
            sampling.frequency_penalty,
            sampling.presence_penalty,
        )
    };
    match (sampling.temperature <= 0.0, sampling.uses_penalties()) {
        (true, true) => LlamaSampler::chain_simple([penalties(), LlamaSampler::greedy()]),
        (true, false) => LlamaSampler::chain_simple([LlamaSampler::greedy()]),
        (false, true) => LlamaSampler::chain_simple([
            penalties(),
            LlamaSampler::top_k(sampling.top_k),
            LlamaSampler::top_p(sampling.top_p, 1),
            LlamaSampler::temp(sampling.temperature),
            LlamaSampler::dist(seed),
        ]),
        (false, false) => LlamaSampler::chain_simple([
            LlamaSampler::top_k(sampling.top_k),
            LlamaSampler::top_p(sampling.top_p, 1),
            LlamaSampler::temp(sampling.temperature),
            LlamaSampler::dist(seed),
        ]),
    }
}

fn token_text(
    model: &LlamaModel,
    token: llama_cpp_2::token::LlamaToken,
    decoder: &mut encoding_rs::Decoder,
) -> Result<String> {
    let bytes = match model.token_to_piece_bytes(token, 32, true, None) {
        Err(llama_cpp_2::TokenToStringError::InsufficientBufferSpace(size)) => {
            let required: usize = size
                .checked_neg()
                .context("Invalid token piece buffer size")?
                .try_into()?;
            model.token_to_piece_bytes(token, required, true, None)
        }
        result => result,
    }
    .context("Failed to convert token to bytes")?;
    let mut text = String::with_capacity(32);
    let _ = decoder.decode_to_string(&bytes, &mut text, false);
    Ok(text)
}

/// Appends a token's text; true when generation should stop (stop string or repeated line).
fn append_and_check(
    output: &mut String,
    text: &str,
    stop_tokens: &[String],
    stop_on_repeated_line: bool,
) -> bool {
    output.push_str(text);
    if let Some(stop) = stop_tokens
        .iter()
        .find(|stop| output.contains(stop.as_str()))
    {
        *output = output.replace(stop.as_str(), "").trim_end().to_string();
        return true;
    }
    if stop_on_repeated_line && text.contains('\n') {
        if let Some(kept) = drop_repeated_last_line(output) {
            *output = kept;
            return true;
        }
    }
    false
}

/// When the last finished line of `output` repeats an earlier line (digits ignored, so a loop
/// that only changes line numbers counts), the output without that line.
fn drop_repeated_last_line(output: &str) -> Option<String> {
    let complete = &output[..output.rfind('\n')?];
    let (earlier, last) = complete.rsplit_once('\n').unwrap_or(("", complete));
    let key = |line: &str| {
        line.chars()
            .filter(|c| !c.is_ascii_digit())
            .collect::<String>()
            .trim()
            .to_string()
    };
    let last_key = key(last);
    if last_key.is_empty() {
        return None;
    }
    earlier
        .lines()
        .any(|line| key(line) == last_key)
        .then(|| earlier.to_string())
}

fn validate_context_budget(
    prompt_tokens: usize,
    output_tokens: i32,
    context_size: u32,
) -> Result<()> {
    anyhow::ensure!(prompt_tokens > 0, "Prompt contains no tokens");
    anyhow::ensure!(output_tokens > 0, "max_tokens must be positive");
    anyhow::ensure!(prompt_tokens.saturating_add(output_tokens as usize) <= context_size as usize,
        "Prompt ({} tokens) plus requested output ({} tokens) exceeds context ({}). Reduce the chunk size or output budget.",
        prompt_tokens, output_tokens, context_size);
    Ok(())
}

// ============================================================================
// Main Loop with Keep-Alive Protocol
// ============================================================================

fn send_response(response: &Response) -> Result<()> {
    let json = serde_json::to_string(response)?;
    println!("{}", json);
    io::stdout().flush()?;
    Ok(())
}

fn main() -> Result<()> {
    // Get idle timeout from environment variable (default 5 minutes)
    let idle_timeout_secs = std::env::var("LLAMA_IDLE_TIMEOUT")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(300); // 5 minutes default

    eprintln!(
        "🦙 llama-helper starting (idle timeout: {}s)",
        idle_timeout_secs
    );

    let mut state = ModelState::new()?;

    let stdin = io::stdin();
    let mut stdin_lock = stdin.lock();
    let mut buffer = String::new();

    loop {
        // Check idle timeout
        if state.seconds_since_activity() > idle_timeout_secs {
            eprintln!("💤 Idle timeout reached, shutting down");
            send_response(&Response::Goodbye)?;
            break;
        }

        // Read line from stdin
        buffer.clear();
        match stdin_lock.read_line(&mut buffer) {
            Ok(0) => {
                // EOF reached
                eprintln!("📪 EOF received, shutting down");
                break;
            }
            Ok(_) => {
                let line = buffer.trim();
                if line.is_empty() {
                    continue;
                }

                // Parse request
                match serde_json::from_str::<Request>(line) {
                    Ok(Request::Generate {
                        prompt,
                        max_tokens,
                        context_size,
                        model_path,
                        temperature,
                        top_k,
                        top_p,
                        presence_penalty,
                        frequency_penalty,
                        repeat_penalty,
                        penalty_last_n,
                        stop_tokens,
                        stop_on_repeated_line,
                    }) => {
                        let max_tokens = max_tokens.unwrap_or(512);
                        let context_size = context_size.unwrap_or(2048);

                        let sampling = SamplingConfig::from_request(
                            temperature,
                            top_k,
                            top_p,
                            presence_penalty,
                            frequency_penalty,
                            repeat_penalty,
                            penalty_last_n,
                        );
                        let stop_tokens = stop_tokens.unwrap_or_else(Vec::new);

                        // Load model if path provided
                        if let Some(path_str) = model_path {
                            let path = PathBuf::from(path_str);
                            if let Err(e) = state.load_model_if_needed(path, context_size) {
                                send_response(&Response::Completion {
                                    text: String::new(),
                                    error: Some(format!("Failed to load model: {}", e)),
                                })?;
                                continue;
                            }
                        }

                        // Generate response with sampling parameters
                        match state.generate(
                            prompt,
                            max_tokens,
                            sampling,
                            stop_tokens,
                            stop_on_repeated_line,
                        ) {
                            Ok(text) => {
                                send_response(&Response::Completion { text, error: None })?;
                            }
                            Err(e) => {
                                send_response(&Response::Completion {
                                    text: String::new(),
                                    error: Some(format!("Generation failed: {}", e)),
                                })?;
                            }
                        }
                    }
                    Ok(Request::GenerateBatch {
                        prompts,
                        max_tokens,
                        context_size,
                        model_path,
                        temperature,
                        top_k,
                        top_p,
                        presence_penalty,
                        frequency_penalty,
                        repeat_penalty,
                        penalty_last_n,
                        stop_tokens,
                        stop_on_repeated_line,
                    }) => {
                        let sampling = SamplingConfig::from_request(
                            temperature,
                            top_k,
                            top_p,
                            presence_penalty,
                            frequency_penalty,
                            repeat_penalty,
                            penalty_last_n,
                        );
                        if let Some(path_str) = model_path {
                            if let Err(e) = state.load_model_if_needed(
                                PathBuf::from(path_str),
                                context_size.unwrap_or(2048),
                            ) {
                                send_response(&Response::BatchCompletion {
                                    texts: Vec::new(),
                                    error: Some(format!("Failed to load model: {}", e)),
                                })?;
                                continue;
                            }
                        }
                        let result = state.generate_batch(
                            prompts,
                            max_tokens.unwrap_or(512),
                            sampling,
                            stop_tokens.unwrap_or_default(),
                            stop_on_repeated_line,
                        );
                        match result {
                            Ok(texts) => {
                                send_response(&Response::BatchCompletion { texts, error: None })?
                            }
                            Err(e) => send_response(&Response::BatchCompletion {
                                texts: Vec::new(),
                                error: Some(format!("Generation failed: {}", e)),
                            })?,
                        }
                    }
                    Ok(Request::Ping) => {
                        state.update_activity();
                        send_response(&Response::Pong)?;
                    }
                    Ok(Request::Shutdown) => {
                        eprintln!("🛑 Shutdown requested");
                        send_response(&Response::Goodbye)?;
                        break;
                    }
                    Err(e) => {
                        eprintln!("❌ Failed to parse request: {}", e);
                        send_response(&Response::Error {
                            message: format!("Invalid request: {}", e),
                        })?;
                    }
                }
            }
            Err(e) => {
                eprintln!("❌ Error reading stdin: {}", e);
                break;
            }
        }
    }

    eprintln!("👋 llama-helper exiting");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_block_count_never_enters_layer_division() {
        let path =
            std::env::temp_dir().join(format!("sonefold-layer-budget-{}", std::process::id()));
        std::fs::write(&path, [0u8; 1024]).unwrap();
        assert_eq!(calculate_gpu_layers(&path, 0, 24.0, 16384, Some(1.0)), 0);
        assert_eq!(calculate_gpu_layers(&path, 64, 24.0, 16384, Some(1.0)), 65);
        assert_eq!(calculate_gpu_layers(&path, 64, 2.0, 16384, Some(1.0)), 0);
        // A partial budget must remain bounded rather than opting into all layers.
        let partial = calculate_gpu_layers(&path, 64, 3.0, 16384, Some(1.0));
        assert!(partial > 0 && partial < 64);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn a_line_repeated_with_other_numbers_ends_generation() {
        let looping =
            "TOPIC: audio cut out [S337]\nTOPIC: bramble [S12]\nTOPIC: audio cut out [S338]\n";
        assert_eq!(
            drop_repeated_last_line(looping).as_deref(),
            Some("TOPIC: audio cut out [S337]\nTOPIC: bramble [S12]")
        );
        assert_eq!(
            drop_repeated_last_line("DECISION: a [S1]\nACTION: b [S2]\n"),
            None
        );
        assert_eq!(drop_repeated_last_line("no newline yet"), None);
    }

    #[test]
    fn context_budget_reserves_output_and_rejects_overflow() {
        assert!(validate_context_budget(1536, 512, 2048).is_ok());
        assert!(validate_context_budget(1537, 512, 2048).is_err());
        assert!(validate_context_budget(0, 512, 2048).is_err());
        assert!(validate_context_budget(10, 0, 2048).is_err());
        assert!(validate_context_budget(10, -1, 2048).is_err());
        assert!(validate_context_budget(usize::MAX, 512, 2048).is_err());
    }

    #[test]
    fn generate_request_defaults_penalties_when_omitted() {
        let json =
            r#"{"type":"generate","prompt":"summarize","temperature":0.5,"top_k":20,"top_p":0.8}"#;
        let request: Request = serde_json::from_str(json).unwrap();
        let Request::Generate {
            temperature,
            top_k,
            top_p,
            presence_penalty,
            frequency_penalty,
            repeat_penalty,
            penalty_last_n,
            ..
        } = request
        else {
            panic!("expected generate request");
        };

        let sampling = SamplingConfig::from_request(
            temperature,
            top_k,
            top_p,
            presence_penalty,
            frequency_penalty,
            repeat_penalty,
            penalty_last_n,
        );

        assert_eq!(sampling.presence_penalty, 0.0);
        assert_eq!(sampling.frequency_penalty, 0.0);
        assert_eq!(sampling.repeat_penalty, 1.0);
        assert_eq!(sampling.penalty_last_n, 0);
        assert!(!sampling.uses_penalties());
    }

    #[test]
    fn generate_request_deserializes_qwen_penalties() {
        let json = r#"{"type":"generate","prompt":"summarize","temperature":0.5,"top_k":20,"top_p":0.8,"presence_penalty":0.3,"frequency_penalty":0.0,"repeat_penalty":1.05,"penalty_last_n":256}"#;
        let request: Request = serde_json::from_str(json).unwrap();
        let Request::Generate {
            temperature,
            top_k,
            top_p,
            presence_penalty,
            frequency_penalty,
            repeat_penalty,
            penalty_last_n,
            ..
        } = request
        else {
            panic!("expected generate request");
        };

        let sampling = SamplingConfig::from_request(
            temperature,
            top_k,
            top_p,
            presence_penalty,
            frequency_penalty,
            repeat_penalty,
            penalty_last_n,
        );

        assert_eq!(sampling.temperature, 0.5);
        assert_eq!(sampling.top_k, 20);
        assert_eq!(sampling.top_p, 0.8);
        assert_eq!(sampling.presence_penalty, 0.3);
        assert_eq!(sampling.frequency_penalty, 0.0);
        assert_eq!(sampling.repeat_penalty, 1.05);
        assert_eq!(sampling.penalty_last_n, 256);
        assert!(sampling.uses_penalties());
    }
}
