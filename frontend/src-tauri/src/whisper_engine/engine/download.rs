//! Download ownership, verification, progress and cancellation.
use super::{
    ActiveDownload, CancelDownloadOutcome, DownloadCancelled, ModelStatus, WhisperEngine,
    CANCEL_DOWNLOAD_CLEANUP_TIMEOUT,
};
use crate::config::WHISPER_MODEL_CATALOG;
use anyhow::{anyhow, Result};
use reqwest::Client;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

impl WhisperEngine {
    pub(super) async fn reserve_active_download(
        &self,
        model_name: &str,
    ) -> Result<Arc<ActiveDownload>> {
        let mut active_downloads = self.active_downloads.lock().await;
        if active_downloads.contains_key(model_name) {
            return Err(anyhow!(
                "Download already in progress for model: {}",
                model_name
            ));
        }

        let (completion, _) = watch::channel(false);
        let active_download = Arc::new(ActiveDownload {
            cancellation: CancellationToken::new(),
            completion,
        });

        active_downloads.insert(model_name.to_string(), Arc::clone(&active_download));
        Ok(active_download)
    }

    pub(super) async fn finish_download(
        &self,
        model_name: &str,
        active_download: &Arc<ActiveDownload>,
        file_path: &PathBuf,
        mut result: Result<()>,
    ) -> Result<()> {
        let active_owner_matches = {
            let active_downloads = self.active_downloads.lock().await;
            matches!(
                active_downloads.get(model_name),
                Some(current) if Arc::ptr_eq(current, active_download)
            )
        };
        if !active_owner_matches {
            log::warn!(
                "Download owner for {} was no longer active during finalization",
                model_name
            );
            active_download.completion.send_replace(true);
            return result;
        }

        if result.is_ok() && !active_download.cancellation.is_cancelled() {
            result = self.validate_model_file(file_path).await;

            if result.is_ok() {
                let expected_min_size = WHISPER_MODEL_CATALOG
                    .iter()
                    .find(|model| model.0 == model_name)
                    .map(|model| ((model.2 as f64 * 0.9) as u64) * 1024 * 1024);

                result = match expected_min_size {
                    Some(expected_min_size) => match fs::metadata(file_path).await {
                        Ok(metadata) if metadata.len() >= expected_min_size => Ok(()),
                        Ok(metadata) => Err(anyhow!(
                            "Downloaded model file is too small: {} bytes (expected at least {} bytes)",
                            metadata.len(),
                            expected_min_size
                        )),
                        Err(e) => Err(anyhow!(
                            "Failed to read downloaded model file metadata: {}",
                            e
                        )),
                    },
                    None => Err(anyhow!(
                        "Unsupported model for download validation: {}",
                        model_name
                    )),
                };
            }
        }

        if result.is_err() && !active_download.cancellation.is_cancelled() && file_path.exists() {
            if let Err(e) = fs::remove_file(file_path).await {
                log::warn!("Failed to clean up failed download file: {}", e);
            } else {
                log::info!("Cleaned up failed download file: {}", file_path.display());
            }
        }

        let (released_active_owner, cancellation_won) = {
            let mut active_downloads = self.active_downloads.lock().await;
            match active_downloads.get(model_name) {
                Some(current) if Arc::ptr_eq(current, active_download) => {
                    if active_download.cancellation.is_cancelled() {
                        (false, true)
                    } else {
                        active_downloads.remove(model_name);
                        (true, false)
                    }
                }
                _ => {
                    log::warn!(
                        "Download owner for {} was no longer active during finalization",
                        model_name
                    );
                    (false, active_download.cancellation.is_cancelled())
                }
            }
        };

        if cancellation_won {
            result = Err(DownloadCancelled.into());
            if file_path.exists() {
                if let Err(e) = fs::remove_file(file_path).await {
                    log::warn!("Failed to clean up cancelled download file: {}", e);
                } else {
                    log::info!(
                        "Cleaned up cancelled download file: {}",
                        file_path.display()
                    );
                }
            }

            let removed = {
                let mut active_downloads = self.active_downloads.lock().await;
                #[cfg(test)]
                if let Some(hook) = self.test_hook() {
                    hook.cancellation_first_lock_acquired.notify_one();
                    hook.continue_cancellation.notified().await;
                }

                let mut models = self.available_models.write().await;
                match active_downloads.get(model_name) {
                    Some(current) if Arc::ptr_eq(current, active_download) => {
                        if let Some(model_info) = models.get_mut(model_name) {
                            model_info.status = ModelStatus::Missing;
                        }
                        active_downloads.remove(model_name);
                        true
                    }
                    _ => false,
                }
            };
            if !removed {
                log::warn!(
                    "Download owner for {} was no longer active during cancellation cleanup",
                    model_name
                );
            }
        } else if released_active_owner {
            let mut models = self.available_models.write().await;
            if let Some(model_info) = models.get_mut(model_name) {
                if result.is_ok() {
                    model_info.status = ModelStatus::Available;
                    model_info.path = file_path.clone();
                } else {
                    model_info.status = ModelStatus::Missing;
                }
            }
        }

        active_download.completion.send_replace(true);
        result
    }

    pub async fn download_model(
        &self,
        model_name: &str,
        progress_callback: Option<Box<dyn Fn(u8) + Send>>,
    ) -> Result<()> {
        log::info!("Starting download for model: {}", model_name);

        let pins: serde_json::Value = serde_json::from_str(include_str!("../model-pins.json"))?;
        let pin = &pins["files"][model_name];
        let digest = pin["sha256"]
            .as_str()
            .ok_or_else(|| anyhow!("Unsupported model: {model_name}"))?
            .to_owned();
        let bytes = pin["bytes"]
            .as_u64()
            .ok_or_else(|| anyhow!("Missing model size"))?;
        let revision = pins["revision"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing model revision"))?;
        let filename = pin["file"]
            .as_str()
            .ok_or_else(|| anyhow!("Missing model filename"))?;
        let model_url =
            format!("https://huggingface.co/ggerganov/whisper.cpp/resolve/{revision}/{filename}");
        self.download_model_verified(
            model_name,
            &model_url,
            progress_callback,
            Some((bytes, digest)),
        )
        .await
    }

    #[cfg(test)]
    pub(super) async fn download_model_from_url(
        &self,
        model_name: &str,
        model_url: &str,
        progress_callback: Option<Box<dyn Fn(u8) + Send>>,
    ) -> Result<()> {
        self.download_model_verified(model_name, model_url, progress_callback, None)
            .await
    }

    pub(super) async fn download_model_verified(
        &self,
        model_name: &str,
        model_url: &str,
        progress_callback: Option<Box<dyn Fn(u8) + Send>>,
        expected: Option<(u64, String)>,
    ) -> Result<()> {
        let active_download = self.reserve_active_download(model_name).await?;
        let file_path = self.models_dir.join(format!("ggml-{}.bin", model_name));

        let result = self
            .download_model_with_owner(
                model_name,
                model_url,
                &file_path,
                &active_download,
                progress_callback,
                expected.as_ref(),
            )
            .await;

        self.finish_download(model_name, &active_download, &file_path, result)
            .await
    }

    pub(super) async fn download_model_with_owner(
        &self,
        model_name: &str,
        model_url: &str,
        file_path: &PathBuf,
        active_download: &ActiveDownload,
        progress_callback: Option<Box<dyn Fn(u8) + Send>>,
        expected: Option<&(u64, String)>,
    ) -> Result<()> {
        if active_download.cancellation.is_cancelled() {
            return Err(DownloadCancelled.into());
        }

        if !self.models_dir.exists() {
            fs::create_dir_all(&self.models_dir)
                .await
                .map_err(|e| anyhow!("Failed to create models directory: {}", e))?;
        }

        {
            let mut models = self.available_models.write().await;
            if let Some(model_info) = models.get_mut(model_name) {
                model_info.status = ModelStatus::Downloading { progress: 0 };
            }
        }

        let client = Client::builder()
            .tls_certs_only(crate::security::transport::tls_roots())
            .user_agent(concat!("Overtalk/", env!("CARGO_PKG_VERSION")))
            .no_proxy()
            .https_only(expected.is_some())
            .connect_timeout(Duration::from_secs(15))
            .build()
            .map_err(|e| anyhow!("Failed to create download client: {}", e))?;
        let response = tokio::select! {
            biased;
            _ = active_download.cancellation.cancelled() => return Err(DownloadCancelled.into()),
            response = client.get(model_url).send() => response
                .map_err(|e| anyhow!("Failed to start download: {}", e))?,
        };

        if !response.status().is_success() {
            return Err(anyhow!(
                "Download failed with status: {}",
                response.status()
            ));
        }

        let total_size = response.content_length().unwrap_or(0);
        if expected.is_some_and(|(bytes, _)| total_size != 0 && total_size != *bytes) {
            return Err(anyhow!(
                "Model download size does not match the pinned checkpoint"
            ));
        }
        use sha2::{Digest, Sha256};
        let mut checksum = Sha256::new();
        let mut file = fs::File::create(file_path)
            .await
            .map_err(|e| anyhow!("Failed to create file: {}", e))?;

        use futures_util::StreamExt;
        let mut stream = response.bytes_stream();
        let mut downloaded = 0u64;
        let mut last_progress_report = 0u8;
        let mut last_report_time = std::time::Instant::now();

        if let Some(ref callback) = progress_callback {
            callback(0);
        }

        loop {
            let chunk_result = tokio::select! {
                biased;
                _ = active_download.cancellation.cancelled() => return Err(DownloadCancelled.into()),
                chunk_result = tokio::time::timeout(Duration::from_secs(60), stream.next()) =>
                    chunk_result.map_err(|_| anyhow!("Model download stalled"))?,
            };

            let Some(chunk_result) = chunk_result else {
                break;
            };

            if active_download.cancellation.is_cancelled() {
                return Err(DownloadCancelled.into());
            }

            let chunk = chunk_result.map_err(|e| anyhow!("Failed to read chunk: {}", e))?;
            let limit = expected
                .map(|(bytes, _)| *bytes)
                .unwrap_or(4 * 1024 * 1024 * 1024);
            if downloaded.saturating_add(chunk.len() as u64) > limit {
                return Err(anyhow!("Model download exceeds its size limit"));
            }
            checksum.update(&chunk);
            file.write_all(&chunk)
                .await
                .map_err(|e| anyhow!("Failed to write chunk to file: {}", e))?;

            downloaded += chunk.len() as u64;
            let progress = if total_size > 0 {
                ((downloaded as f64 / total_size as f64) * 100.0) as u8
            } else {
                0
            };

            let time_since_last_report = last_report_time.elapsed().as_secs();
            if progress > last_progress_report || progress == 100 || time_since_last_report >= 2 {
                let mut models = self.available_models.write().await;
                if let Some(model_info) = models.get_mut(model_name) {
                    model_info.status = ModelStatus::Downloading { progress };
                }

                if let Some(ref callback) = progress_callback {
                    callback(progress);
                }

                last_progress_report = progress;
                last_report_time = std::time::Instant::now();
            }
        }

        {
            let mut models = self.available_models.write().await;
            if let Some(model_info) = models.get_mut(model_name) {
                model_info.status = ModelStatus::Downloading { progress: 100 };
            }
        }

        if let Some(ref callback) = progress_callback {
            callback(100);
        }

        file.flush()
            .await
            .map_err(|e| anyhow!("Failed to flush file: {}", e))?;
        if let Some((bytes, digest)) = expected {
            if downloaded != *bytes
                || crate::security::storage::hex(&checksum.finalize()) != *digest
            {
                return Err(anyhow!("Model checkpoint integrity verification failed"));
            }
        }

        if active_download.cancellation.is_cancelled() {
            return Err(DownloadCancelled.into());
        }

        Ok(())
    }

    pub async fn cancel_download(&self, model_name: &str) -> Result<CancelDownloadOutcome> {
        self.cancel_download_with_timeout(model_name, CANCEL_DOWNLOAD_CLEANUP_TIMEOUT)
            .await
    }

    pub(super) async fn cancel_download_with_timeout(
        &self,
        model_name: &str,
        cleanup_timeout: Duration,
    ) -> Result<CancelDownloadOutcome> {
        log::info!("Cancelling download for model: {}", model_name);

        let Some(active_download) = self.active_downloads.lock().await.get(model_name).cloned()
        else {
            return Ok(CancelDownloadOutcome::Cancelled);
        };

        active_download.cancellation.cancel();

        let mut completion = active_download.completion.subscribe();
        if !*completion.borrow() {
            match tokio::time::timeout(cleanup_timeout, completion.changed()).await {
                Ok(Ok(())) => {}
                Ok(Err(_)) => {
                    return Err(anyhow!(
                        "Download worker ended before completing cancellation cleanup"
                    ));
                }
                Err(_) => {
                    return Ok(CancelDownloadOutcome::Pending);
                }
            }
        }

        Ok(CancelDownloadOutcome::Cancelled)
    }
}
