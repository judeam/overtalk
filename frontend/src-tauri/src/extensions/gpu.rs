//! Who is using the GPU, in words a person can act on, and a way to free memory that other
//! local apps keep pinned (Ollama models loaded with an endless keep-alive).

use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct GpuProcess {
    pub pid: u32,
    pub label: String,
    pub used_mb: u64,
    /// Overtalk itself or its speech worker.
    pub ours: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct OllamaModel {
    pub name: String,
    pub vram_mb: u64,
}

#[derive(Clone, Debug, Serialize, Default)]
pub struct GpuReport {
    /// False when no NVIDIA driver tooling answered; the rest is then empty.
    pub available: bool,
    pub total_mb: u64,
    pub used_mb: u64,
    pub processes: Vec<GpuProcess>,
    pub ollama: Vec<OllamaModel>,
}

/// Why a recording runs without a live transcript, for the recorder and the saved meeting.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct SpeechNotice {
    /// "gpu-memory" or "engine".
    pub reason: String,
    pub message: String,
    pub detail: String,
    pub can_free: bool,
}

pub fn is_out_of_memory(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    lower.contains("out of memory")
        || lower.contains("outofmemory")
        || lower.contains("cuda_error_out_of_memory")
}

fn gb(mb: u64) -> String {
    format!("{:.1} GB", mb as f64 / 1024.0)
}

/// "Ollama (qwen3.8:27b) is using 19.5 GB of 23.9 GB." from the largest process that is not ours.
pub fn culprit_sentence(report: &GpuReport) -> Option<String> {
    let mut others: Vec<&GpuProcess> = report.processes.iter().filter(|p| !p.ours).collect();
    others.sort_by(|a, b| b.used_mb.cmp(&a.used_mb));
    let top = others.first()?;
    let total = if report.total_mb > 0 {
        format!(" of {}", gb(report.total_mb))
    } else {
        String::new()
    };
    Some(format!(
        "{} is using {}{}.",
        top.label,
        gb(top.used_mb),
        total
    ))
}

pub fn notice_for(error: &str, report: &GpuReport) -> SpeechNotice {
    let first_line = error.lines().next().unwrap_or(error);
    let short: String = first_line.chars().take(220).collect();
    if is_out_of_memory(error) {
        let culprit = culprit_sentence(report)
            .map(|c| format!("GPU memory is full. {c}"))
            .unwrap_or_else(|| "GPU memory is full.".to_string());
        SpeechNotice {
            reason: "gpu-memory".into(),
            message: culprit,
            detail: short,
            can_free: !report.ollama.is_empty(),
        }
    } else {
        SpeechNotice {
            reason: "engine".into(),
            message: "The speech model could not start.".into(),
            detail: short,
            can_free: false,
        }
    }
}

pub fn parse_compute_apps(text: &str) -> Vec<(u32, String, u64)> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split(',').map(str::trim);
            let pid = parts.next()?.parse().ok()?;
            let name = parts.next()?.to_string();
            let used = parts.next()?.trim_end_matches("MiB").trim().parse().ok()?;
            Some((pid, name, used))
        })
        .collect()
}

async fn nvidia_smi(args: &[&str]) -> Option<String> {
    let mut command = std::process::Command::new("nvidia-smi");
    command.args(args);
    let output = tokio::task::spawn_blocking(move || {
        overtalk_worker_sandbox::run_bounded(command, 256 * 1024, Duration::from_secs(3))
    })
    .await
    .ok()?
    .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn ollama_base() -> String {
    std::env::var("OLLAMA_HOST")
        .ok()
        .filter(|h| !h.trim().is_empty())
        .map(|h| {
            if h.starts_with("http") {
                h
            } else {
                format!("http://{h}")
            }
        })
        .unwrap_or_else(|| "http://127.0.0.1:11434".to_string())
}

#[derive(Deserialize)]
struct OllamaPs {
    #[serde(default)]
    models: Vec<OllamaPsModel>,
}
#[derive(Deserialize)]
struct OllamaPsModel {
    name: String,
    #[serde(default)]
    size_vram: u64,
}

async fn ollama_loaded() -> Vec<OllamaModel> {
    let client = match crate::security::transport::client() {
        Ok(client) => client,
        Err(_) => return Vec::new(),
    };
    let Ok(base) = local_ollama_base(&ollama_base()) else {
        return Vec::new();
    };
    let Ok(response) = client
        .get(format!("{}/api/ps", base.as_str().trim_end_matches('/')))
        .timeout(Duration::from_millis(1500))
        .send()
        .await
    else {
        return Vec::new();
    };
    if !response.status().is_success() {
        return Vec::new();
    }
    let Ok(body) = crate::security::transport::response_text(response, 256 * 1024).await else {
        return Vec::new();
    };
    let Ok(ps) = serde_json::from_str::<OllamaPs>(&body) else {
        return Vec::new();
    };
    ps.models
        .into_iter()
        .filter(|m| {
            !m.name.is_empty() && m.name.len() <= 512 && !m.name.chars().any(char::is_control)
        })
        .take(128)
        .map(|m| OllamaModel {
            name: m.name,
            vram_mb: m.size_vram / (1024 * 1024),
        })
        .collect()
}

fn local_ollama_base(input: &str) -> Result<reqwest::Url, String> {
    let url = crate::security::transport::endpoint(input)?;
    if !crate::security::transport::is_loopback(&url) {
        return Err("Only a local Ollama server can free memory on this device".into());
    }
    Ok(url)
}

/// Parent chain of a process, so the speech worker counts as Overtalk.
fn descends_from(pid: u32, ancestor: u32) -> bool {
    let mut current = pid;
    for _ in 0..6 {
        if current == ancestor {
            return true;
        }
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{current}/stat")) else {
            return false;
        };
        // The command name may contain spaces; fields resume after the closing parenthesis.
        let Some(after) = stat.rsplit_once(')').map(|(_, rest)| rest) else {
            return false;
        };
        let Some(parent) = after
            .split_whitespace()
            .nth(1)
            .and_then(|p| p.parse::<u32>().ok())
        else {
            return false;
        };
        if parent <= 1 {
            return false;
        }
        current = parent;
    }
    false
}

fn label_for(name: &str, ours: bool, ollama: &[OllamaModel]) -> String {
    if ours {
        return "Overtalk".into();
    }
    let base = name.rsplit('/').next().unwrap_or(name);
    if name.contains("ollama") || base.starts_with("llama-server") {
        return if ollama.is_empty() {
            "Ollama".into()
        } else {
            format!(
                "Ollama ({})",
                ollama
                    .iter()
                    .map(|m| m.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
    }
    base.to_string()
}

pub async fn report() -> GpuReport {
    let Some(totals) = nvidia_smi(&[
        "--query-gpu=memory.total,memory.used",
        "--format=csv,noheader,nounits",
    ])
    .await
    else {
        return GpuReport::default();
    };
    let mut first = totals
        .lines()
        .next()
        .unwrap_or("")
        .split(',')
        .map(str::trim);
    let total_mb = first.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    let used_mb = first.next().and_then(|v| v.parse().ok()).unwrap_or(0);
    let apps = nvidia_smi(&[
        "--query-compute-apps=pid,process_name,used_memory",
        "--format=csv,noheader,nounits",
    ])
    .await
    .map(|text| parse_compute_apps(&text))
    .unwrap_or_default();
    let ollama = ollama_loaded().await;
    let me = std::process::id();
    let processes = apps
        .into_iter()
        .map(|(pid, name, used_mb)| {
            let ours = descends_from(pid, me);
            GpuProcess {
                pid,
                label: label_for(&name, ours, &ollama),
                used_mb,
                ours,
            }
        })
        .collect();
    GpuReport {
        available: true,
        total_mb,
        used_mb,
        processes,
        ollama,
    }
}

pub async fn notice(error: &str) -> SpeechNotice {
    let report = if is_out_of_memory(error) {
        report().await
    } else {
        GpuReport::default()
    };
    notice_for(error, &report)
}

#[tauri::command]
pub async fn gpu_memory_report() -> GpuReport {
    report().await
}

#[tauri::command]
pub async fn gpu_explain_error(error: String) -> SpeechNotice {
    notice(&error).await
}

/// Unloads every model Ollama holds in memory. Ollama loads them again on the next request.
#[tauri::command]
pub async fn gpu_free_ollama() -> Result<Vec<String>, String> {
    let base = local_ollama_base(&ollama_base())?;
    let loaded = ollama_loaded().await;
    let client = crate::security::transport::client()?;
    tokio::time::timeout(Duration::from_secs(15), async {
        let mut freed = Vec::new();
        for model in loaded {
            let request = client
                .post(format!(
                    "{}/api/generate",
                    base.as_str().trim_end_matches('/')
                ))
                .timeout(Duration::from_secs(3))
                .json(&serde_json::json!({ "model": model.name, "keep_alive": 0 }));
            match request.send().await {
                Ok(response) if response.status().is_success() => freed.push(model.name),
                Ok(response) => log::warn!(
                    "Ollama refused to unload {}: {}",
                    model.name,
                    response.status()
                ),
                Err(error) => {
                    log::warn!("Could not ask Ollama to unload {}: {}", model.name, error)
                }
            }
        }
        Ok(freed)
    })
    .await
    .map_err(|_| "Ollama did not finish freeing memory in time".to_owned())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freeing_device_memory_never_targets_a_remote_ollama_server() {
        assert!(local_ollama_base("http://localhost:11434").is_ok());
        assert!(local_ollama_base("http://[::1]:11434").is_ok());
        for endpoint in [
            "https://remote.example",
            "http://127.0.0.1.example",
            "https://example/localhost",
        ] {
            assert!(local_ollama_base(endpoint).is_err());
        }
    }

    fn report() -> GpuReport {
        GpuReport {
            available: true,
            total_mb: 24_463,
            used_mb: 22_617,
            processes: vec![
                GpuProcess {
                    pid: 1,
                    label: "Overtalk".into(),
                    used_mb: 842,
                    ours: true,
                },
                GpuProcess {
                    pid: 2,
                    label: "Ollama (qwen3.8:27b)".into(),
                    used_mb: 20_010,
                    ours: false,
                },
                GpuProcess {
                    pid: 3,
                    label: "Xorg".into(),
                    used_mb: 52,
                    ours: false,
                },
            ],
            ollama: vec![OllamaModel {
                name: "qwen3.8:27b".into(),
                vram_mb: 17_400,
            }],
        }
    }

    #[test]
    fn out_of_memory_names_the_largest_other_process_and_offers_to_free_ollama() {
        let error = "Speech analysis failed (OutOfMemoryError): CUDA out of memory. Tried to allocate 594.00 MiB.";
        let notice = notice_for(error, &report());
        assert_eq!(notice.reason, "gpu-memory");
        assert_eq!(
            notice.message,
            "GPU memory is full. Ollama (qwen3.8:27b) is using 19.5 GB of 23.9 GB."
        );
        assert!(notice.can_free);
    }

    #[test]
    fn other_failures_are_reported_without_a_gpu_claim() {
        let notice = notice_for("Model file missing\nsecond line", &GpuReport::default());
        assert_eq!(notice.reason, "engine");
        assert_eq!(notice.detail, "Model file missing");
        assert!(!notice.can_free);
    }

    #[test]
    fn compute_apps_parse_from_nvidia_smi_csv() {
        let rows = parse_compute_apps(
            "820618, /usr/local/lib/ollama/llama-server, 20010\nbad line\n12, python3, 842 MiB\n",
        );
        assert_eq!(
            rows,
            vec![
                (820618, "/usr/local/lib/ollama/llama-server".into(), 20010),
                (12, "python3".into(), 842)
            ]
        );
        assert_eq!(
            label_for(
                "/usr/local/lib/ollama/llama-server",
                false,
                &report().ollama
            ),
            "Ollama (qwen3.8:27b)"
        );
    }
}
