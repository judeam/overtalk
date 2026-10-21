// Built-in AI summary engine module
// Provides local LLM inference via llama-helper sidecar

pub mod client;
pub mod commands;
mod download;
mod manager_state;
pub mod model_manager;
pub mod models;
pub mod sidecar;

// Re-export commonly used types
pub use client::{
    force_shutdown_sidecar, generate_batch_with_builtin, generate_with_builtin,
    generate_with_builtin_options,
};
pub use commands::ModelManagerState;
pub use models::get_model_by_name;
