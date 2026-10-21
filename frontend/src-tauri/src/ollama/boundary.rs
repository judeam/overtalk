//! Shared bounds for model management, including older UI command routes.
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::{
    collections::HashSet,
    sync::{LazyLock, Mutex},
};

pub(super) fn endpoint(input: Option<&str>) -> Result<reqwest::Url, String> {
    let input = input
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("http://127.0.0.1:11434");
    if input.len() > 2048 {
        return Err("Ollama endpoint is too long".into());
    }
    crate::security::transport::endpoint(input)
}

pub(super) fn model_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.len() > 512
        || name.chars().any(char::is_whitespace)
        || name.chars().any(char::is_control)
    {
        return Err("Enter a model name of at most 512 characters without whitespace".into());
    }
    Ok(())
}

pub(super) fn url(endpoint: &reqwest::Url, operation: &str) -> String {
    format!(
        "{}/api/{operation}",
        endpoint.as_str().trim_end_matches('/')
    )
}

pub(super) async fn json<T: DeserializeOwned>(response: reqwest::Response) -> Result<T, String> {
    let text = crate::security::transport::response_text(response, 4 * 1024 * 1024).await?;
    serde_json::from_str(&text).map_err(|_| "Ollama returned invalid JSON".into())
}

static JOBS: LazyLock<Mutex<HashSet<(String, String)>>> = LazyLock::new(Mutex::default);
pub(super) struct ModelJob((String, String));
impl ModelJob {
    pub(super) fn acquire(endpoint: &reqwest::Url, name: &str) -> Result<Self, String> {
        model_name(name)?;
        let key = (
            endpoint.as_str().trim_end_matches('/').to_owned(),
            name.to_owned(),
        );
        let mut jobs = JOBS
            .lock()
            .map_err(|_| "Model operations are unavailable")?;
        if jobs.contains(&key) {
            return Err("This model is already being changed".into());
        }
        if jobs.len() >= 2 {
            return Err("Wait for an active model operation to finish".into());
        }
        jobs.insert(key.clone());
        Ok(Self(key))
    }
}
impl Drop for ModelJob {
    fn drop(&mut self) {
        if let Ok(mut jobs) = JOBS.lock() {
            jobs.remove(&self.0);
        }
    }
}

#[derive(Default)]
pub(super) struct Progress {
    line: Vec<u8>,
    bytes: usize,
    success: bool,
    last: Option<u8>,
}
impl Progress {
    pub(super) fn feed(&mut self, bytes: &[u8], mut emit: impl FnMut(u8)) -> Result<(), String> {
        // Progress JSON is tiny compared with model weights, which Ollama
        // downloads itself. Bound both an unterminated line and the full stream.
        if bytes.len() > (32 * 1024 * 1024usize).saturating_sub(self.bytes) {
            return Err("Ollama progress stream exceeds its size limit".into());
        }
        self.bytes += bytes.len();
        for part in bytes.split_inclusive(|b| *b == b'\n') {
            if self.line.len() + part.len() > 64 * 1024 {
                return Err("Ollama progress line exceeds its size limit".into());
            }
            self.line.extend_from_slice(part);
            if part.last() == Some(&b'\n') {
                self.flush(&mut emit)?;
            }
        }
        Ok(())
    }

    fn flush(&mut self, emit: &mut impl FnMut(u8)) -> Result<(), String> {
        if self.line.iter().all(u8::is_ascii_whitespace) {
            self.line.clear();
            return Ok(());
        }
        let value: Value =
            serde_json::from_slice(&self.line).map_err(|_| "Ollama returned malformed progress")?;
        self.line.clear();
        if let Some(error) = value.get("error").and_then(Value::as_str) {
            return Err(format!(
                "Ollama error: {}",
                error.chars().take(512).collect::<String>()
            ));
        }
        if value["status"] == "success" {
            self.success = true;
        }
        if let (Some(done), Some(total)) = (value["completed"].as_u64(), value["total"].as_u64()) {
            if total > 0 {
                let percent = (done.min(total) as f64 / total as f64 * 100.) as u8;
                if self.last != Some(percent) {
                    self.last = Some(percent);
                    emit(percent);
                }
            }
        }
        Ok(())
    }

    pub(super) fn finish(&mut self, mut emit: impl FnMut(u8)) -> Result<(), String> {
        self.flush(&mut emit)?;
        if self.success {
            Ok(())
        } else {
            Err("Ollama disconnected before confirming the download".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn progress_handles_fragmented_utf8_regressing_layers_and_final_line() {
        let data = "{\"status\":\"pulling 日本語\",\"completed\":95,\"total\":100}\n{\"completed\":1,\"total\":100}\n{\"status\":\"success\"}".as_bytes();
        for size in 1..=data.len() {
            let mut progress = Progress::default();
            let mut events = Vec::new();
            for chunk in data.chunks(size) {
                progress.feed(chunk, |n| events.push(n)).unwrap();
            }
            progress.finish(|n| events.push(n)).unwrap();
            assert_eq!(events, [95, 1]);
        }
    }
    #[test]
    fn progress_rejects_incomplete_malformed_and_flooded_streams() {
        let mut progress = Progress::default();
        progress
            .feed(b"{\"completed\":1,\"total\":100}\n", |_| {})
            .unwrap();
        assert!(progress.finish(|_| {}).is_err());
        assert!(Progress::default().feed(b"invalid\n", |_| {}).is_err());
        assert!(Progress::default()
            .feed(&vec![b'x'; 65537], |_| {})
            .is_err());
        let mut progress = Progress {
            bytes: 32 * 1024 * 1024,
            ..Default::default()
        };
        assert!(progress.feed(b"\n", |_| {}).is_err());
    }
    #[test]
    fn failed_or_cancelled_operations_release_admission() {
        let endpoint = endpoint(None).unwrap();
        let first = ModelJob::acquire(&endpoint, "synthetic-one").unwrap();
        assert!(ModelJob::acquire(&endpoint, "synthetic-one").is_err());
        let second = ModelJob::acquire(&endpoint, "synthetic-two").unwrap();
        assert!(ModelJob::acquire(&endpoint, "synthetic-three").is_err());
        drop(first);
        assert!(ModelJob::acquire(&endpoint, "synthetic-one").is_ok());
        drop(second);
    }
}
