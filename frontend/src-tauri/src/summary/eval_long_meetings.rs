//! Notes for the long test meetings in tests/long-meetings, from each small built-in model, once
//! with the whole transcript in one call ("full") and once as the app does it ("app": window by
//! window when the transcript exceeds the model's reading window). Score the output with
//! tests/long-meetings/score.py. Needs the downloaded models and the CUDA llama-helper:
//!
//! OVERTALK_EVAL_APP_DIR=<dir holding models/summary/*.gguf> SONEFOLD_LLAMA_HELPER=<llama-helper>
//! OVERTALK_EVAL_OUT=<dir> [OVERTALK_EVAL_MODELS=qwen3.5:2b,...] [OVERTALK_EVAL_MEETINGS=en-30min-product,...]
//! cargo test -p sonefold --lib eval_long_meetings -- --ignored --nocapture

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::Instant;

use crate::summary::llm_client::LLMProvider;
use crate::summary::processor::{
    generate_meeting_summary, EVAL_QUOTED_DECISIONS, EVAL_READING_WINDOW, EVAL_TOPIC_PLAN,
    FORCE_FULL_READ,
};
use crate::summary::summary_engine::get_model_by_name;
use crate::summary::templates::get_template;

fn list(name: &str, default: &str) -> Vec<String> {
    std::env::var(name)
        .unwrap_or_else(|_| default.to_string())
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs local models and a GPU; run by hand"]
async fn eval_long_meetings() {
    let app_dir =
        PathBuf::from(std::env::var("OVERTALK_EVAL_APP_DIR").expect("OVERTALK_EVAL_APP_DIR"));
    let out = PathBuf::from(std::env::var("OVERTALK_EVAL_OUT").expect("OVERTALK_EVAL_OUT"));
    std::fs::create_dir_all(&out).unwrap();
    let models = list(
        "OVERTALK_EVAL_MODELS",
        "qwen3.5:2b,qwen3.5:4b,gemma4:e2b,gemma4:e4b",
    );
    let meetings = list(
        "OVERTALK_EVAL_MEETINGS",
        "en-30min-product,en-60min-planning,en-90min-review,de-45min-team",
    );
    let modes = list("OVERTALK_EVAL_MODES", "full,app");
    let debug_root = std::env::var_os("OVERTALK_EVAL_DEBUG_DIR").map(PathBuf::from);
    let reading_window = std::env::var("OVERTALK_EVAL_READING_WINDOW")
        .ok()
        .map(|value| {
            let window: usize = value
                .parse()
                .expect("OVERTALK_EVAL_READING_WINDOW must be an integer");
            assert!(
                (256..=12000).contains(&window),
                "Reading window must be 256–12000 estimated tokens"
            );
            window
        });
    EVAL_READING_WINDOW.store(reading_window.unwrap_or(0), Ordering::SeqCst);
    EVAL_TOPIC_PLAN.store(
        std::env::var("OVERTALK_EVAL_TOPIC_PLAN").as_deref() == Ok("1"),
        Ordering::SeqCst,
    );
    EVAL_QUOTED_DECISIONS.store(
        std::env::var("OVERTALK_EVAL_QUOTED_DECISIONS").as_deref() == Ok("1"),
        Ordering::SeqCst,
    );
    super::grounding::BASELINE.store(
        std::env::var("OVERTALK_EVAL_BASELINE_PROMPT").as_deref() == Ok("1"),
        Ordering::SeqCst,
    );
    let template = get_template("standard_meeting").unwrap();
    let dir = std::env::var_os("OVERTALK_EVAL_INPUT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/long-meetings"));
    let provider = std::env::var("OVERTALK_EVAL_PROVIDER")
        .unwrap_or_else(|_| "builtin-ai".into())
        .parse::<LLMProvider>()
        .unwrap();
    assert!(matches!(
        provider,
        LLMProvider::BuiltInAI | LLMProvider::Privatemode
    ));
    let endpoint = if provider == LLMProvider::Privatemode {
        assert_eq!(
            std::env::var("OVERTALK_EVAL_FREE_PLAN_CONFIRMED").as_deref(),
            Ok("1"),
            "Remote evaluation requires confirmed Free plan and the budgeted comparison runner"
        );
        let value =
            std::env::var("OVERTALK_EVAL_PRIVATEMODE_ENDPOINT").expect("budgeted proxy endpoint");
        crate::summary::privatemode::endpoint(Some(&value)).unwrap();
        Some(value)
    } else {
        None
    };
    let max_tokens = if provider == LLMProvider::Privatemode {
        Some(
            std::env::var("OVERTALK_EVAL_MAX_COMPLETION_TOKENS")
                .ok()
                .map(|n| n.parse::<u32>().unwrap())
                .unwrap_or(crate::summary::privatemode::DEFAULT_MAX_TOKENS),
        )
    } else {
        None
    };
    for meeting in &meetings {
        let data: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(dir.join(format!("meeting-{meeting}.json"))).unwrap(),
        )
        .unwrap();
        let transcript = data["transcript"].as_str().unwrap();
        let language = data["language"].as_str().unwrap();
        for model in &models {
            let context = if provider == LLMProvider::Privatemode {
                crate::summary::privatemode::context_size(model)
            } else {
                get_model_by_name(model).expect("known model").context_size as usize
            };
            // As the app computes it in service.rs: context minus generation and prompt reserve.
            let budget = context
                - max_tokens
                    .map(|n| n as usize)
                    .unwrap_or_else(|| 4096.min(context / 3))
                - 256;
            for mode in &modes {
                let name = format!("{meeting}__{}__{mode}", model.replace(':', "-"));
                if let Some(root) = &debug_root {
                    // Equal transcript lengths must not overwrite another case's evidence.
                    std::env::set_var("OVERTALK_EVAL_DEBUG_DIR", root.join(&name));
                }
                FORCE_FULL_READ.store(mode == "full", Ordering::SeqCst);
                let started = Instant::now();
                let result = generate_meeting_summary(
                    &provider,
                    model,
                    "",
                    transcript,
                    "",
                    "standard_meeting",
                    &template,
                    budget,
                    endpoint.as_deref(),
                    None,
                    max_tokens,
                    None,
                    None,
                    Some(&app_dir),
                    None,
                    None,
                    Some(language),
                    None,
                )
                .await;
                let seconds = started.elapsed().as_secs_f64();
                let (body, calls) = match result {
                    Ok(summary) => (summary.final_markdown, summary.successful_chunk_count),
                    Err(error) => (format!("ERROR: {error}"), 0),
                };
                std::fs::write(out.join(format!("{name}.md")), &body).unwrap();
                std::fs::write(out.join(format!("{name}.json")), serde_json::json!({"seconds": seconds, "windows": calls, "reading_window_override": reading_window}).to_string()).unwrap();
                println!(
                    "{name}: {seconds:.1} s, {calls} windows, {} chars{}",
                    body.len(),
                    if body.starts_with("ERROR") {
                        " (error)"
                    } else {
                        ""
                    }
                );
            }
        }
    }
    FORCE_FULL_READ.store(false, Ordering::SeqCst);
    EVAL_READING_WINDOW.store(0, Ordering::SeqCst);
    EVAL_TOPIC_PLAN.store(false, Ordering::SeqCst);
    super::grounding::BASELINE.store(false, Ordering::SeqCst);
    EVAL_QUOTED_DECISIONS.store(false, Ordering::SeqCst);
    if let Some(root) = debug_root {
        std::env::set_var("OVERTALK_EVAL_DEBUG_DIR", root);
    }
}
