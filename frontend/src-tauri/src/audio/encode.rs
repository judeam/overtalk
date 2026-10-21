use super::ffmpeg::find_ffmpeg_path; // Correct path to encode module
use super::AudioDevice;
use std::io::{Read, Write};
use std::sync::Arc;
use std::{path::PathBuf, process::Stdio};
use tracing::{debug, error};

pub struct AudioInput {
    pub data: Arc<Vec<f32>>,
    pub sample_rate: u32,
    pub channels: u16,
    pub device: Arc<AudioDevice>,
}

pub fn encode_single_audio(
    data: &[u8],
    sample_rate: u32,
    channels: u16,
    output_path: &PathBuf,
) -> anyhow::Result<()> {
    debug!(
        "Starting FFmpeg process for {} bytes of audio data",
        data.len()
    );

    if data.is_empty() {
        return Err(anyhow::anyhow!("No audio data provided for encoding"));
    }

    let ffmpeg_path = find_ffmpeg_path().ok_or_else(|| {
        anyhow::anyhow!("FFmpeg not found. Please install FFmpeg to save recordings.")
    })?;

    debug!("Using FFmpeg at: {:?}", ffmpeg_path);

    let mut sandbox = overtalk_worker_sandbox::Sandbox::new(false)?;
    sandbox.output(output_path)?;
    let mut command = sandbox.command(&ffmpeg_path)?;
    command
        .args([
            "-y",
            "-nostdin",
            "-v",
            "error",
            "-threads",
            "2",
            "-protocol_whitelist",
            "file,pipe",
            "-f",
            "f32le",
            "-ar",
            &sample_rate.to_string(),
            "-ac",
            &channels.to_string(),
            "-i",
            "pipe:0",
            "-c:a",
            "aac",
            "-b:a",
            "192k", // Increased from 64k for better audio quality (especially for speech)
            "-profile:a",
            "aac_low", // Use AAC-LC profile for better compatibility
            "-movflags",
            "+faststart", // Optimize for web streaming
            "-f",
            "mp4",
        ])
        .arg(output_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    // Hide console window on Windows to prevent CMD popup during recording
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    debug!("FFmpeg command: {:?}", command);

    let mut ffmpeg = command.spawn()?;
    debug!("FFmpeg process spawned");
    let mut stdin = ffmpeg
        .stdin
        .take()
        .ok_or_else(|| anyhow::anyhow!("Missing encoder input"))?;
    let mut errors = ffmpeg
        .stderr
        .take()
        .ok_or_else(|| anyhow::anyhow!("Missing encoder error stream"))?;
    let error_reader = std::thread::spawn(move || {
        let mut kept = Vec::new();
        let mut buffer = [0u8; 4096];
        while let Ok(n) = errors.read(&mut buffer) {
            if n == 0 {
                break;
            }
            kept.extend_from_slice(&buffer[..n.min(16384usize.saturating_sub(kept.len()))]);
        }
        kept
    });
    let written = stdin.write_all(data);

    debug!("Dropping stdin");
    drop(stdin);
    debug!("Waiting for FFmpeg process to exit");
    let status = ffmpeg.wait()?;
    let bytes = error_reader.join().unwrap_or_default();
    let stderr = String::from_utf8_lossy(&bytes);

    debug!("FFmpeg process exited with status: {}", status);
    debug!("FFmpeg stderr: {}", stderr);

    if !status.success() {
        error!("FFmpeg process failed with status: {}", status);
        error!("FFmpeg stderr: {}", stderr);
        return Err(anyhow::anyhow!(
            "Audio encoding failed ({status}): {stderr}"
        ));
    }
    written?;

    Ok(())
}
