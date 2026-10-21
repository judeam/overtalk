// audio/transcription/mod.rs
//
// Transcription module: provider trait, engine management, and worker pool.

pub mod engine;
pub mod provider;
pub mod worker;

// Re-export commonly used types
pub use engine::{
    get_or_init_transcription_engine, get_or_init_whisper, validate_transcription_model_ready,
    TranscriptionEngine,
};
pub use provider::{TranscriptResult, TranscriptionError, TranscriptionProvider};
pub use worker::{reset_speech_detected_flag, start_transcription_task, TranscriptUpdate};
