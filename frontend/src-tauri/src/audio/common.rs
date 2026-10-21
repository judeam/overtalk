use crate::api::TranscriptSegment;
use anyhow::{anyhow, Result};
use log::{debug, info};
use once_cell::sync::Lazy;
use std::path::Path;
use std::sync::Arc;
use tauri::{AppHandle, Runtime};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub(crate) enum BatchTranscriber<R: Runtime> {
    Whisper(Arc<crate::whisper_engine::WhisperEngine>),
    Parakeet(Arc<crate::parakeet_engine::ParakeetEngine>),
    Local(crate::speech::SpeechProvider<R>),
}

impl<R: Runtime> BatchTranscriber<R> {
    pub(crate) async fn local(
        app: &AppHandle<R>,
        model: Option<String>,
        token: &CancellationToken,
    ) -> Result<Self> {
        let selected_model = match model.filter(|m| !m.is_empty()) {
            Some(model) => model,
            None => {
                crate::speech::speech_get_settings(app.clone())
                    .await
                    .map_err(anyhow::Error::msg)?
                    .model
            }
        };
        let engine = crate::speech::SpeechProvider {
            app: app.clone(),
            model: selected_model,
        };
        engine.prepare(token).await.map_err(anyhow::Error::msg)?;
        Ok(Self::Local(engine))
    }

    pub(crate) async fn transcribe(
        &self,
        segment: &crate::audio::vad::SpeechSegment,
        language: Option<String>,
        token: &CancellationToken,
        index: usize,
    ) -> Result<(String, Option<f32>)> {
        match self {
            Self::Local(engine) => {
                let result = engine
                    .transcribe_with_cancellation(segment.samples.clone(), language, token)
                    .await
                    .map_err(|e| anyhow!("Local speech failed on segment {}: {}", index, e))?;
                Ok((result.text, result.confidence))
            }
            Self::Parakeet(engine) => {
                let text = engine
                    .transcribe_audio(segment.samples.clone())
                    .await
                    .map_err(|e| {
                        anyhow!("Parakeet transcription failed on segment {}: {}", index, e)
                    })?;
                Ok((text, None))
            }
            Self::Whisper(engine) => {
                let (text, confidence, _) = engine
                    .transcribe_audio_with_confidence(segment.samples.clone(), language)
                    .await
                    .map_err(|e| {
                        anyhow!("Whisper transcription failed on segment {}: {}", index, e)
                    })?;
                Ok((text, Some(confidence)))
            }
        }
    }
}

/// Reuse short segments' sample buffers; release each original long buffer as
/// soon as it has been split. Import and retranscription use the same boundaries.
pub(crate) fn prepare_speech_segments(
    segments: Vec<crate::audio::vad::SpeechSegment>,
) -> Vec<crate::audio::vad::SpeechSegment> {
    const MAX_SEGMENT_SAMPLES: usize = 25 * 16000;
    let mut prepared = Vec::with_capacity(segments.len());
    for segment in segments {
        if segment.samples.len() > MAX_SEGMENT_SAMPLES {
            prepared.extend(split_segment_at_silence(&segment, MAX_SEGMENT_SAMPLES));
        } else {
            prepared.push(segment);
        }
    }
    prepared
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum BatchProvider {
    Whisper,
    Parakeet,
    LocalSpeech,
}
impl BatchProvider {
    pub(crate) fn resolve(provider: Option<&str>) -> Result<Self> {
        match provider {
            None | Some("localWhisper" | "whisper") => Ok(Self::Whisper),
            Some("parakeet") => Ok(Self::Parakeet),
            Some("localSpeech") => Ok(Self::LocalSpeech),
            Some(value) => Err(anyhow!(
                "Provider '{}' does not support local audio import",
                value
            )),
        }
    }
}

static ENGINE_LIFECYCLE_LOCK: Lazy<Arc<AsyncMutex<()>>> =
    Lazy::new(|| Arc::new(AsyncMutex::new(())));

pub(crate) async fn acquire_engine_lifecycle_lock() -> OwnedMutexGuard<()> {
    ENGINE_LIFECYCLE_LOCK.clone().lock_owned().await
}

/// Unload the transcription engine after a batch job (import or retranscription).
/// Skips unloading if a live recording is currently in progress, since recording
/// uses the same global engine instances.
pub(crate) async fn unload_engine_after_batch(use_parakeet: bool) {
    let _engine_lifecycle_guard = acquire_engine_lifecycle_lock().await;

    if crate::audio::recording_commands::is_recording().await {
        log::info!("Skipping model unload after batch: recording in progress");
        return;
    }

    if use_parakeet {
        use crate::parakeet_engine::commands::PARAKEET_ENGINE;
        let engine = {
            let guard = PARAKEET_ENGINE.lock().unwrap_or_else(|e| e.into_inner());
            guard.as_ref().cloned()
        };
        if let Some(e) = engine {
            e.unload_model().await;
        }
    } else {
        use crate::whisper_engine::commands::WHISPER_ENGINE;
        let engine = {
            let guard = WHISPER_ENGINE.lock().unwrap_or_else(|e| e.into_inner());
            guard.as_ref().cloned()
        };
        if let Some(e) = engine {
            e.unload_model().await;
        }
    }
}

/// Shared diagnostics for the import and retranscription VAD output.
pub(crate) fn log_speech_segments(
    speech_segments: &[crate::audio::vad::SpeechSegment],
    duration_seconds: f64,
) {
    let total_segments = speech_segments.len();
    // Diagnostic: log segment duration distribution
    if !speech_segments.is_empty() {
        let durations_ms: Vec<f64> = speech_segments
            .iter()
            .map(|s| s.end_timestamp_ms - s.start_timestamp_ms)
            .collect();
        let total_speech_ms: f64 = durations_ms.iter().sum();
        let avg_duration = total_speech_ms / durations_ms.len() as f64;
        let min_duration = durations_ms.iter().cloned().fold(f64::INFINITY, f64::min);
        let max_duration = durations_ms
            .iter()
            .cloned()
            .fold(f64::NEG_INFINITY, f64::max);
        info!(
            "VAD segment stats: avg={:.0}ms, min={:.0}ms, max={:.0}ms, total_speech={:.1}s/{:.1}s ({:.0}%)",
            avg_duration, min_duration, max_duration,
            total_speech_ms / 1000.0, duration_seconds,
            (total_speech_ms / 1000.0 / duration_seconds) * 100.0
        );
        // Log first 10 segments for detailed inspection
        for (i, seg) in speech_segments.iter().take(10).enumerate() {
            let dur = seg.end_timestamp_ms - seg.start_timestamp_ms;
            debug!(
                "  Segment {}: {:.0}ms-{:.0}ms ({:.0}ms, {} samples)",
                i,
                seg.start_timestamp_ms,
                seg.end_timestamp_ms,
                dur,
                seg.samples.len()
            );
        }
        if total_segments > 10 {
            debug!("  ... and {} more segments", total_segments - 10);
        }
    }
}

/// Create transcript segments from transcription results.
/// Each tuple is (text, start_ms, end_ms) from VAD timestamps.
pub(crate) fn create_transcript_segments(
    transcripts: &[(String, f64, f64)],
) -> Vec<TranscriptSegment> {
    transcripts
        .iter()
        .map(|(text, start_ms, end_ms)| {
            let start_seconds = start_ms / 1000.0;
            let end_seconds = end_ms / 1000.0;
            let duration = end_seconds - start_seconds;

            TranscriptSegment {
                id: format!("transcript-{}", Uuid::new_v4()),
                text: text.trim().to_string(),
                timestamp: chrono::Utc::now().to_rfc3339(),
                audio_start_time: Some(start_seconds),
                audio_end_time: Some(end_seconds),
                duration: Some(duration),
                speaker: None,
                speaker_id: None,
                speaker_name: None,
                speaker_confidence: None,
                overlap: false,
                is_partial: false,
            }
        })
        .collect()
}

/// Write transcripts.json to a meeting folder (atomic write with temp file)
pub(crate) fn write_transcripts_json(folder: &Path, segments: &[TranscriptSegment]) -> Result<()> {
    let transcript_path = folder.join("transcripts.json");
    #[derive(serde::Serialize)]
    struct Checkpoint<'a> {
        version: &'static str,
        last_updated: String,
        total_segments: usize,
        segments: SegmentList<'a>,
    }
    #[derive(serde::Serialize)]
    struct Segment<'a> {
        id: &'a str,
        text: &'a str,
        timestamp: &'a str,
        audio_start_time: Option<f64>,
        audio_end_time: Option<f64>,
        duration: Option<f64>,
        sequence_id: usize,
    }
    struct SegmentList<'a>(&'a [TranscriptSegment]);
    impl serde::Serialize for SegmentList<'_> {
        fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            serializer.collect_seq(self.0.iter().enumerate().map(|(i, s)| Segment {
                id: &s.id,
                text: &s.text,
                timestamp: &s.timestamp,
                audio_start_time: s.audio_start_time,
                audio_end_time: s.audio_end_time,
                duration: s.duration,
                sequence_id: i,
            }))
        }
    }
    crate::security::storage::write_json_atomic(
        &transcript_path,
        &Checkpoint {
            version: "1.0",
            last_updated: chrono::Utc::now().to_rfc3339(),
            total_segments: segments.len(),
            segments: SegmentList(segments),
        },
    )?;

    info!(
        "Wrote transcripts.json with {} segments to {}",
        segments.len(),
        transcript_path.display()
    );
    Ok(())
}

/// Split a long speech segment at the lowest-energy (silence) point near the target size.
///
/// Scans for 100ms windows with minimal RMS energy within +/-3 seconds of each target
/// split point. If no clear silence is found, falls back to a 1-second overlap split
/// to avoid cutting words at boundaries.
pub(crate) fn split_segment_at_silence(
    segment: &crate::audio::vad::SpeechSegment,
    max_samples: usize,
) -> Vec<crate::audio::vad::SpeechSegment> {
    const SAMPLE_RATE: usize = 16000;
    // 100ms window for energy measurement (1600 samples at 16kHz)
    const ENERGY_WINDOW: usize = SAMPLE_RATE / 10;
    // Search +/-3 seconds around the target split point
    const SEARCH_RADIUS: usize = SAMPLE_RATE * 3;
    // RMS threshold below which we consider a window "silent"
    const SILENCE_RMS_THRESHOLD: f32 = 0.02;
    // Overlap to use when no silence boundary is found (1 second)
    const FALLBACK_OVERLAP: usize = SAMPLE_RATE;

    let total = segment.samples.len();
    if total <= max_samples {
        return vec![segment.clone()];
    }

    let ms_per_sample =
        (segment.end_timestamp_ms - segment.start_timestamp_ms) / segment.samples.len() as f64;
    let mut result = Vec::new();
    let mut pos = 0usize;

    while pos < total {
        let remaining = total - pos;
        if remaining <= max_samples {
            // Last chunk - take everything remaining
            let chunk_samples = segment.samples[pos..].to_vec();
            let chunk_start_ms = segment.start_timestamp_ms + (pos as f64 * ms_per_sample);
            let chunk_end_ms = segment.end_timestamp_ms;
            result.push(crate::audio::vad::SpeechSegment {
                samples: chunk_samples,
                start_timestamp_ms: chunk_start_ms,
                end_timestamp_ms: chunk_end_ms,
                confidence: segment.confidence,
            });
            break;
        }

        // Target split point
        let target = pos + max_samples;

        // Search window: [target - SEARCH_RADIUS, target + SEARCH_RADIUS]
        let search_start = target.saturating_sub(SEARCH_RADIUS).max(pos + SAMPLE_RATE);
        let search_end = (target + SEARCH_RADIUS).min(total.saturating_sub(ENERGY_WINDOW));

        // Find the lowest-energy 100ms window in the search range
        let mut best_split = target.min(total); // fallback: exact target
        let mut best_rms = f32::MAX;

        if search_start + ENERGY_WINDOW <= search_end {
            let mut idx = search_start;
            while idx + ENERGY_WINDOW <= search_end {
                let window = &segment.samples[idx..idx + ENERGY_WINDOW];
                let rms = (window.iter().map(|s| s * s).sum::<f32>() / ENERGY_WINDOW as f32).sqrt();
                if rms < best_rms {
                    best_rms = rms;
                    best_split = idx + ENERGY_WINDOW / 2; // split at center of quiet window
                }
                // Step by 10ms (160 samples) for efficiency
                idx += SAMPLE_RATE / 100;
            }
        }

        let split_at = best_split;
        if best_rms <= SILENCE_RMS_THRESHOLD {
            debug!(
                "Splitting at silence boundary: sample {} (RMS={:.4})",
                split_at, best_rms
            );
        } else {
            debug!(
                "No silence found near target (best RMS={:.4}), splitting with overlap at sample {}",
                best_rms, split_at
            );
        }

        // Determine the actual end of this chunk (with overlap if no silence)
        let chunk_end = if best_rms > SILENCE_RMS_THRESHOLD {
            (split_at + FALLBACK_OVERLAP).min(total)
        } else {
            split_at
        };

        let chunk_samples = segment.samples[pos..chunk_end].to_vec();
        let chunk_start_ms = segment.start_timestamp_ms + (pos as f64 * ms_per_sample);
        let chunk_end_ms = segment.start_timestamp_ms + (chunk_end as f64 * ms_per_sample);

        result.push(crate::audio::vad::SpeechSegment {
            samples: chunk_samples,
            start_timestamp_ms: chunk_start_ms,
            end_timestamp_ms: chunk_end_ms,
            confidence: segment.confidence,
        });

        // Advance position to where the current chunk actually ends
        // to avoid transcribing the overlap region twice
        pos = chunk_end;
    }

    result
}

#[cfg(test)]
mod tests;
