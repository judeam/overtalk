//! Local speech, speaker identities and opt-in workflow connections.

pub mod automation;
pub mod integrations;
pub(crate) mod loopback_http;
pub mod speakers;
pub mod speech;
pub mod speech_stream;
pub mod voice_memory;

pub mod knowledge;

pub mod retrieval;

pub mod calendar;

pub mod model_budget;

pub mod media;

pub mod archive;

pub mod diagnostics;

pub mod quality_profile;

pub mod timeline;

pub mod gpu;

pub mod backup_schedule;
pub mod connections;
pub mod jobs;
pub mod meeting_watch;
pub mod privacy;
pub mod projects;
pub mod reading;
pub mod recording_health;
pub mod review;
pub mod share;
pub mod video;
pub mod workspace;

#[cfg(target_os = "linux")]
pub(crate) mod media_stream;

pub mod connection_mcp;
pub mod connection_oauth;

pub mod connection_services;

pub mod first_run;
pub mod meeting_workflow;
pub mod project_memory;
pub mod usage;
