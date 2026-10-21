//! Quality-first defaults selected from hardware capacity, never temporary GPU load.
use serde::Serialize;
use std::sync::OnceLock;

#[derive(Clone, Debug, Serialize)]
pub struct LocalQualityProfile {
    pub system_ram_mib: u64,
    pub cuda_vram_mib: Option<u64>,
    pub summary_model: &'static str,
    pub speech_model: &'static str,
    pub embedding_family: &'static str,
    pub embedding_size: &'static str,
    pub reranker_size: &'static str,
    pub device: &'static str,
}

pub(crate) fn select(system_ram_mib: u64, cuda_vram_mib: Option<u64>) -> LocalQualityProfile {
    let vram = cuda_vram_mib.unwrap_or(0);
    // Allow weight staging, context/activation memory, and at least 2.5 GiB headroom.
    let summary_model = if system_ram_mib >= 22 * 1024
        || (vram >= 12 * 1024 - 128 && system_ram_mib >= 14 * 1024)
    {
        "qwen3.8:27b"
    } else {
        crate::summary::summary_engine::commands::recommend_summary_model(
            cfg!(target_os = "macos"),
            system_ram_mib / 1024,
        )
    };
    let (embedding_family, embedding_size, reranker_size) = if (vram >= 22 * 1024
        && system_ram_mib >= 24 * 1024)
        || (vram == 0 && system_ram_mib >= 60 * 1024)
    {
        ("nemotron3", "8B", "4B")
    } else if (vram >= 12 * 1024 - 128 && system_ram_mib >= 16 * 1024)
        || (vram == 0 && system_ram_mib >= 30 * 1024)
    {
        ("qwen3", "4B", "4B")
    } else {
        ("qwen3", "0.6B", "0.6B")
    };
    LocalQualityProfile {
        system_ram_mib,
        cuda_vram_mib,
        summary_model,
        speech_model: if vram >= 6 * 1024 - 128 || system_ram_mib >= 16 * 1024 {
            "qwen3-asr-1.7b"
        } else {
            "qwen3-asr-0.6b"
        },
        embedding_family,
        embedding_size,
        reranker_size,
        device: if vram > 0 { "cuda" } else { "cpu" },
    }
}

pub(crate) fn detected() -> &'static LocalQualityProfile {
    static PROFILE: OnceLock<LocalQualityProfile> = OnceLock::new();
    PROFILE.get_or_init(|| {
        let mut system = sysinfo::System::new();
        system.refresh_memory();
        select(system.total_memory() / (1024 * 1024), nvidia_memory_mib())
    })
}

fn nvidia_memory_mib() -> Option<u64> {
    use std::process::Command;
    use std::time::Duration;
    let mut command = Command::new("nvidia-smi");
    command.args(["--query-gpu=memory.total", "--format=csv,noheader,nounits"]);
    // Match the worker's first visible CUDA device; never add separate GPU memories.
    if let Some(device) = std::env::var("CUDA_VISIBLE_DEVICES")
        .ok()
        .and_then(|s| s.split(',').next().map(str::to_owned))
        .filter(|s| !s.is_empty())
    {
        command.arg(format!("--id={}", device));
    }
    let output =
        overtalk_worker_sandbox::run_bounded(command, 1024, Duration::from_secs(1)).ok()?;
    if !output.status.success() {
        return None;
    }
    std::str::from_utf8(&output.stdout)
        .ok()?
        .lines()
        .next()?
        .trim()
        .parse()
        .ok()
}

#[tauri::command]
pub fn get_local_quality_profile() -> LocalQualityProfile {
    detected().clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn workstation_automatically_uses_quality_models() {
        let p = select(64 * 1024, Some(24 * 1024));
        assert_eq!(p.summary_model, "qwen3.8:27b");
        assert_eq!(p.speech_model, "qwen3-asr-1.7b");
        assert_eq!(
            (p.embedding_family, p.embedding_size, p.reranker_size),
            ("nemotron3", "8B", "4B")
        );
        assert_eq!(p.device, "cuda");
    }
    #[test]
    fn memory_limits_choose_fitting_profiles_instead_of_largest_weights() {
        let medium = select(16 * 1024, Some(12 * 1024));
        assert_eq!(medium.summary_model, "qwen3.8:27b");
        assert_eq!(medium.embedding_size, "4B");
        let small = select(8 * 1024, Some(4 * 1024));
        assert_eq!(small.speech_model, "qwen3-asr-0.6b");
        assert_eq!(small.embedding_size, "0.6B");
        assert_eq!(small.reranker_size, "0.6B");
    }
    #[test]
    fn cpu_capacity_uses_quality_without_claiming_cuda() {
        let p = select(64 * 1024, None);
        assert_eq!(p.summary_model, "qwen3.8:27b");
        assert_eq!(p.embedding_size, "8B");
        assert_eq!(p.device, "cpu");
        assert_eq!(select(32 * 1024, None).embedding_size, "4B");
    }
}
