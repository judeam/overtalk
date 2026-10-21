//! Continuous, recording-relative speaker-tagged ASR. The worker retains caches
//! for the entire meeting; VAD-filtered chunks must never enter this path.
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};

use base64::{engine::general_purpose::STANDARD, Engine};
use rubato::{FftFixedInOut, Resampler};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, Runtime};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::audio::recording_saver::TranscriptSegment;

static PREPARED: AtomicBool = AtomicBool::new(false);
static SESSION_LIFECYCLE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
const QUEUE_WINDOWS: usize = 8;

pub(crate) fn invalidate_prepared() {
    PREPARED.store(false, Ordering::SeqCst);
}

/// Every early recording-start failure releases the prepared model session.
pub(crate) struct StartupGuard<R: Runtime> {
    app: AppHandle<R>,
    armed: bool,
}
impl<R: Runtime> StartupGuard<R> {
    pub(crate) fn new(app: &AppHandle<R>) -> Self {
        Self {
            app: app.clone(),
            armed: PREPARED.load(Ordering::SeqCst),
        }
    }
    pub(crate) fn disarm(&mut self) {
        self.armed = false;
    }
}
impl<R: Runtime> Drop for StartupGuard<R> {
    fn drop(&mut self) {
        if self.armed && PREPARED.swap(false, Ordering::SeqCst) {
            let app = self.app.clone();
            // Reserve cleanup before another start can observe an unprepared
            // session. Preparing a new recording waits until this stop finishes.
            let reserved = SESSION_LIFECYCLE.try_lock().ok();
            tokio::spawn(async move {
                let _guard = match reserved {
                    Some(guard) => guard,
                    None => SESSION_LIFECYCLE.lock().await,
                };
                let _ = crate::speech::request(
                    &app,
                    json!({"operation":"stream_stop","offline":true}),
                    &CancellationToken::new(),
                )
                .await;
            });
        }
    }
}

pub(crate) async fn configured<R: Runtime>(app: &AppHandle<R>) -> Result<bool, String> {
    let settings = crate::speech::read_settings(app)?;
    if !settings.enabled || !settings.live_diarization {
        return Ok(false);
    }
    let config =
        crate::api::service::api_get_transcript_config(app.clone(), app.state(), None).await?;
    Ok(config.is_some_and(|config| config.provider == "localSpeech"))
}

/// Called during model validation, before the physical audio streams start.
pub(crate) async fn prepare<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let _guard = SESSION_LIFECYCLE.lock().await;
    if PREPARED.load(Ordering::SeqCst) {
        return Ok(());
    }
    let settings = crate::speech::read_settings(app)?;
    let response = crate::speech::request(
        app,
        json!({
            "operation": "stream_start", "offline": true,
            "device": settings.device, "asr_model": settings.streaming_asr_model,
            "max_speakers": 8,
            "language": super::speech::recognition_language(crate::get_language_preference_internal()),
        }),
        &CancellationToken::new(),
    )
    .await?;
    if response.get("started").and_then(Value::as_bool) != Some(true) {
        return Err("Streaming worker did not confirm model initialization".into());
    }
    PREPARED.store(true, Ordering::SeqCst);
    Ok(())
}

pub(crate) async fn abort_prepared<R: Runtime>(app: &AppHandle<R>) {
    let _guard = SESSION_LIFECYCLE.lock().await;
    if PREPARED.swap(false, Ordering::SeqCst) {
        let _ = crate::speech::request(
            app,
            json!({"operation":"stream_stop","offline":true}),
            &CancellationToken::new(),
        )
        .await;
    }
}

#[derive(Clone)]
pub(crate) struct LiveAudioSink {
    sender: mpsc::Sender<Vec<f32>>,
    failed: Arc<AtomicBool>,
    cancel: CancellationToken,
}

impl LiveAudioSink {
    /// Audio capture must not block behind model inference. Stop the live model
    /// on overload rather than silently omitting audio and shifting its clock.
    pub(crate) fn submit(&self, samples: Vec<f32>) {
        if self.failed.load(Ordering::SeqCst) {
            return;
        }
        if self.sender.try_send(samples).is_err() {
            self.fail();
        }
    }
    pub(crate) fn fail(&self) {
        self.failed.store(true, Ordering::SeqCst);
        self.cancel.cancel();
    }
    pub(crate) fn is_failed(&self) -> bool {
        self.failed.load(Ordering::SeqCst)
    }
}

/// One stateful, anti-aliased 48→16 kHz resampler. Its filter delay is removed,
/// and final convolution padding is cropped to the real captured duration.
struct ContinuousResampler {
    inner: FftFixedInOut<f32>,
    pending: VecDeque<f32>,
    discard_delay: usize,
    total_input: usize,
    total_output: usize,
}
impl ContinuousResampler {
    fn new() -> Result<Self, String> {
        let inner = FftFixedInOut::new(48000, 16000, 4800, 1).map_err(|e| e.to_string())?;
        let discard_delay = inner.output_delay();
        Ok(Self {
            inner,
            pending: VecDeque::new(),
            discard_delay,
            total_input: 0,
            total_output: 0,
        })
    }
    fn process(&mut self, samples: Vec<f32>) -> Result<Vec<f32>, String> {
        if samples.iter().any(|v| !v.is_finite()) {
            return Err("Audio contains non-finite samples".into());
        }
        self.total_input += samples.len();
        self.pending.extend(samples);
        let mut output = Vec::new();
        let size = self.inner.input_frames_next();
        while self.pending.len() >= size {
            let block: Vec<f32> = self.pending.drain(..size).collect();
            self.block(block, &mut output)?;
        }
        Ok(output)
    }
    fn block(&mut self, input: Vec<f32>, output: &mut Vec<f32>) -> Result<(), String> {
        let mut resampled = self
            .inner
            .process(&[input], None)
            .map_err(|e| e.to_string())?
            .remove(0);
        let discard = self.discard_delay.min(resampled.len());
        self.discard_delay -= discard;
        resampled.drain(..discard);
        let remaining = (self.total_input / 3).saturating_sub(self.total_output);
        resampled.truncate(remaining);
        self.total_output += resampled.len();
        output.extend(resampled);
        Ok(())
    }
    fn finish(&mut self) -> Result<Vec<f32>, String> {
        let mut output = Vec::new();
        let target = self.total_input / 3;
        let size = self.inner.input_frames_next();
        while self.total_output < target {
            let mut block: Vec<f32> = self.pending.drain(..).collect();
            block.resize(size, 0.0);
            self.block(block, &mut output)?;
        }
        Ok(output)
    }
}

#[derive(Deserialize)]
struct WorkerSegment {
    id: String,
    text: String,
    start: f64,
    end: f64,
    speaker: Option<String>,
    #[serde(default)]
    overlap: bool,
}

#[derive(Clone, Serialize)]
struct Snapshot {
    session_id: String,
    revision: u64,
    segments: Vec<TranscriptSegment>,
    is_partial: bool,
}

fn snapshot_segments(value: &Value, session_id: &str) -> Result<Vec<TranscriptSegment>, String> {
    let segments: Vec<WorkerSegment> = serde_json::from_value(
        value
            .get("segments")
            .cloned()
            .ok_or("Worker omitted streaming segments")?,
    )
    .map_err(|e| e.to_string())?;
    let partial = value
        .get("is_partial")
        .and_then(Value::as_bool)
        .ok_or("Worker omitted streaming finalization state")?;
    if segments.len() > 100_000 {
        return Err("Streaming transcript exceeded the supported segment limit".into());
    }
    let mut seen = std::collections::HashSet::new();
    let mut output = Vec::with_capacity(segments.len());
    for (index, segment) in segments.into_iter().enumerate() {
        if segment.id.is_empty()
            || segment.id.len() > 200
            || segment.text.len() > 1_000_000
            || segment.speaker.as_ref().is_some_and(|speaker| {
                !matches!(
                    speaker.as_str(),
                    "SPEAKER_00"
                        | "SPEAKER_01"
                        | "SPEAKER_02"
                        | "SPEAKER_03"
                        | "SPEAKER_04"
                        | "SPEAKER_05"
                        | "SPEAKER_06"
                        | "SPEAKER_07"
                )
            })
            || !segment.start.is_finite()
            || !segment.end.is_finite()
            || segment.start < 0.0
            || segment.end <= segment.start
            || !seen.insert(segment.id.clone())
        {
            return Err(
                "Worker returned invalid or duplicate streaming segment timestamps/IDs".into(),
            );
        }
        let seconds = segment.start.floor() as u64;
        output.push(TranscriptSegment {
            id: format!("live:{session_id}:{}", segment.id),
            text: segment.text,
            audio_start_time: segment.start,
            audio_end_time: segment.end,
            duration: segment.end - segment.start,
            display_time: format!("[{:02}:{:02}]", seconds / 60, seconds % 60),
            confidence: None,
            sequence_id: index as u64,
            speaker_id: segment.speaker,
            speaker_name: None,
            speaker_confidence: None,
            overlap: segment.overlap,
            is_partial: partial,
        });
    }
    Ok(output)
}

/// Starts the bounded audio pump after model validation. The recording store is
/// shared with its saver so final snapshots survive taking the manager at Stop.
pub(crate) fn start<R: Runtime>(
    app: AppHandle<R>,
    store: Arc<Mutex<Vec<TranscriptSegment>>>,
    folder: Arc<Mutex<Option<PathBuf>>>,
) -> Result<(LiveAudioSink, JoinHandle<()>), String> {
    if !PREPARED.load(Ordering::SeqCst) {
        return Err("Live speaker models have not been prepared".into());
    }
    let mut resampler = ContinuousResampler::new()?;
    let (sender, mut receiver) = mpsc::channel(QUEUE_WINDOWS);
    let failed = Arc::new(AtomicBool::new(false));
    let cancel = CancellationToken::new();
    let sink = LiveAudioSink {
        sender,
        failed: failed.clone(),
        cancel: cancel.clone(),
    };
    let session_id = uuid::Uuid::new_v4().to_string();
    let handle = tokio::spawn(async move {
        let mut revision = 0;
        let mut last_checkpoint = std::time::Instant::now();
        let mut publish = |response: Value| -> Result<(), String> {
            let segments = snapshot_segments(&response, &session_id)?;
            *store
                .lock()
                .map_err(|_| "Recording transcript store is unavailable")? = segments.clone();
            let partial = response
                .get("is_partial")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            revision += 1;
            if !partial || last_checkpoint.elapsed().as_secs() >= 5 {
                if let Some(folder) = folder
                    .lock()
                    .map_err(|_| "Recording folder is unavailable")?
                    .as_ref()
                {
                    let destination = folder.join("transcripts.json");
                    #[derive(serde::Serialize)]
                    struct Checkpoint<'a> {
                        version: &'static str,
                        segments: &'a [TranscriptSegment],
                        last_updated: String,
                    }
                    crate::security::storage::write_json_atomic(
                        &destination,
                        &Checkpoint {
                            version: "1.0",
                            segments: &segments,
                            last_updated: chrono::Utc::now().to_rfc3339(),
                        },
                    )
                    .map_err(|e| e.to_string())?;
                }
                last_checkpoint = std::time::Instant::now();
            }
            app.emit(
                "transcript-snapshot",
                Snapshot {
                    session_id: session_id.clone(),
                    revision,
                    segments,
                    is_partial: partial,
                },
            )
            .map_err(|e| e.to_string())
        };
        let result: Result<(), String> = async {
            loop {
                let input = tokio::select! { _ = cancel.cancelled() => return Err("Live speaker processing fell behind or audio capture lost samples".into()), input = receiver.recv() => input };
                let Some(input) = input else { break; };
                let output = resampler.process(input)?;
                for block in output.chunks(16000) {
                    let response = send_audio(&app, block, &cancel).await?;
                    publish(response)?;
                }
            }
            for block in resampler.finish()?.chunks(16000) {
                publish(send_audio(&app, block, &cancel).await?)?;
            }
            publish(crate::speech::request(&app, json!({"operation":"stream_stop","offline":true}), &cancel).await?)?;
            Ok(())
        }.await;
        PREPARED.store(false, Ordering::SeqCst);
        if let Err(error) = result {
            failed.store(true, Ordering::SeqCst);
            log::warn!("Continuous speaker processing stopped: {error}");
            let _ = app.emit("transcription-error", json!({"error":error, "userMessage":"Live speaker processing stopped. Recording continues; analyze the saved audio if audio auto-save is enabled.","actionable":false,"phase":"active"}));
            let _ = crate::speech::request(
                &app,
                json!({"operation":"stream_stop","offline":true}),
                &CancellationToken::new(),
            )
            .await;
        }
        let _ = app.emit("transcription-complete", ());
    });
    Ok((sink, handle))
}

async fn send_audio<R: Runtime>(
    app: &AppHandle<R>,
    samples: &[f32],
    cancel: &CancellationToken,
) -> Result<Value, String> {
    let bytes: Vec<u8> = samples
        .iter()
        .flat_map(|sample| ((sample.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes())
        .collect();
    crate::speech::request(app, json!({"operation":"stream_audio", "offline":true, "sample_rate":16000, "pcm16_base64":STANDARD.encode(bytes)}), cancel).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resampling_preserves_real_duration_and_chunk_boundaries() {
        let samples: Vec<f32> = (0..49003)
            .map(|index| {
                if (12000..24000).contains(&index) {
                    0.0
                } else {
                    (index as f32 / 40.0).sin() * 0.2
                }
            })
            .collect();
        let mut whole = ContinuousResampler::new().unwrap();
        let mut expected = whole.process(samples.clone()).unwrap();
        expected.extend(whole.finish().unwrap());
        let mut chunked = ContinuousResampler::new().unwrap();
        let mut actual = Vec::new();
        for block in samples.chunks(731) {
            actual.extend(chunked.process(block.to_vec()).unwrap());
        }
        actual.extend(chunked.finish().unwrap());
        assert_eq!(actual.len(), samples.len() / 3);
        assert_eq!(actual, expected);
        assert!(actual[5000..7000].iter().all(|sample| sample.abs() < 1e-5));
    }
    #[test]
    fn overload_marks_failure_instead_of_advancing_the_audio_clock() {
        let (sender, _receiver) = mpsc::channel(1);
        let sink = LiveAudioSink {
            sender,
            failed: Arc::new(AtomicBool::new(false)),
            cancel: CancellationToken::new(),
        };
        sink.submit(vec![0.0]);
        sink.submit(vec![0.0]);
        assert!(sink.failed.load(Ordering::SeqCst));
        assert!(sink.cancel.is_cancelled());
    }
    #[test]
    fn snapshots_preserve_overlap_and_session_isolation() {
        let value = json!({"segments":[{"id":"SPEAKER_00:0.000","text":"Hello","start":0.0,"end":1.0,"speaker":"SPEAKER_00","overlap":true},{"id":"SPEAKER_01:0.500","text":"Hi","start":0.5,"end":1.5,"speaker":"SPEAKER_01","overlap":true}],"is_partial":false});
        let segments = snapshot_segments(&value, "meeting-a").unwrap();
        assert!(segments
            .iter()
            .all(|segment| segment.overlap && !segment.is_partial));
        assert_ne!(
            segments[0].id,
            snapshot_segments(&value, "meeting-b").unwrap()[0].id
        );
        assert_eq!(segments[1].audio_start_time, 0.5);
    }
}
