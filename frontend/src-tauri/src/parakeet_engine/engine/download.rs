//! Download ownership, verification, progress and cancellation.
use super::{
    find_model_spec, is_download_cancelled, parse_content_range, ActiveDownload, ArtifactSpec,
    CancelDownloadOutcome, ContentRange, DownloadCancelled, DownloadProgress, ModelStatus,
    ParakeetEngine, CANCEL_DOWNLOAD_CLEANUP_TIMEOUT,
};
use anyhow::{anyhow, Result};
use ring::digest;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::fs;
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::sync::watch;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

impl ParakeetEngine {
    /// Download a Parakeet model from HuggingFace (backward-compatible wrapper).
    pub async fn download_model(
        &self,
        model_name: &str,
        progress_callback: Option<Box<dyn Fn(u8) + Send>>,
    ) -> Result<()> {
        let detailed_callback = progress_callback.map(|callback| {
            Box::new(move |progress: DownloadProgress| callback(progress.percent))
                as Box<dyn Fn(DownloadProgress) + Send>
        });
        self.download_model_detailed(model_name, detailed_callback)
            .await
    }

    /// Download a catalogued Parakeet model with detailed progress.
    pub async fn download_model_detailed(
        &self,
        model_name: &str,
        progress_callback: Option<Box<dyn Fn(DownloadProgress) + Send>>,
    ) -> Result<()> {
        let model_info = self
            .available_models
            .read()
            .await
            .get(model_name)
            .cloned()
            .ok_or_else(|| anyhow!("Model {} not found", model_name))?;
        let spec = find_model_spec(model_name)
            .ok_or_else(|| anyhow!("Unsupported Parakeet model: {}", model_name))?;

        self.download_model_detailed_from_source(
            model_name,
            &model_info.path,
            spec.source_base_url,
            spec.artifacts,
            progress_callback,
        )
        .await
    }

    pub(super) async fn reserve_active_download(
        &self,
        model_name: &str,
    ) -> Result<Arc<ActiveDownload>> {
        let mut active_downloads = self.active_downloads.lock().await;
        if active_downloads.downloads.contains_key(model_name) {
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
        active_downloads
            .downloads
            .insert(model_name.to_string(), Arc::clone(&active_download));
        active_downloads.revision = active_downloads.revision.wrapping_add(1);
        Ok(active_download)
    }

    pub(super) async fn set_downloading_status(&self, model_name: &str, progress: u8) {
        let mut models = self.available_models.write().await;
        if let Some(model) = models.get_mut(model_name) {
            model.status = ModelStatus::Downloading { progress };
        }
    }

    pub(super) fn in_flight_progress(
        downloaded_bytes: u64,
        total_bytes: u64,
        speed_mbps: f64,
    ) -> DownloadProgress {
        let mut progress = DownloadProgress::new(downloaded_bytes, total_bytes, speed_mbps);
        progress.percent = progress.percent.min(99);
        progress
    }

    pub(super) async fn send_download_request(
        &self,
        client: &reqwest::Client,
        file_url: &str,
        range_start: Option<u64>,
        active_download: &ActiveDownload,
    ) -> Result<reqwest::Response> {
        let mut request = client.get(file_url);
        if let Some(range_start) = range_start {
            request = request.header(reqwest::header::RANGE, format!("bytes={range_start}-"));
        }

        tokio::select! {
            biased;
            _ = active_download.cancellation.cancelled() => Err(DownloadCancelled.into()),
            response = request.send() => response
                .map_err(|error| anyhow!("Failed to start download for {}: {}", file_url, error)),
        }
    }

    pub(super) fn declared_content_length(response: &reqwest::Response) -> Result<Option<u64>> {
        response
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .map(|value| {
                value
                    .to_str()
                    .map_err(|error| anyhow!("Invalid Content-Length header encoding: {}", error))?
                    .parse()
                    .map_err(|error| anyhow!("Invalid Content-Length header: {}", error))
            })
            .transpose()
    }

    pub(super) fn validate_full_response(
        response: &reqwest::Response,
        exact_bytes: u64,
    ) -> Result<()> {
        if response.status() != reqwest::StatusCode::OK {
            return Err(anyhow!(
                "Expected full 200 response, received {}",
                response.status()
            ));
        }
        if let Some(content_length) = Self::declared_content_length(response)? {
            if content_length != exact_bytes {
                return Err(anyhow!(
                    "Full response declared {} bytes, expected {}",
                    content_length,
                    exact_bytes
                ));
            }
        }
        Ok(())
    }

    pub(super) fn validate_partial_response(
        response: &reqwest::Response,
        expected_start: u64,
        exact_bytes: u64,
    ) -> Result<()> {
        if response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
            return Err(anyhow!(
                "Expected partial 206 response, received {}",
                response.status()
            ));
        }
        let content_range = response
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .ok_or_else(|| anyhow!("Partial response is missing Content-Range"))?;
        let ContentRange::Range { start, end, total } = parse_content_range(content_range)? else {
            return Err(anyhow!("Partial response has an unsatisfied Content-Range"));
        };
        if start != expected_start || end != exact_bytes - 1 || total != exact_bytes {
            return Err(anyhow!(
                "Partial response range {}-{} / {} does not match {}-{} / {}",
                start,
                end,
                total,
                expected_start,
                exact_bytes - 1,
                exact_bytes
            ));
        }
        let expected_length = end
            .checked_sub(start)
            .and_then(|length| length.checked_add(1))
            .ok_or_else(|| anyhow!("Partial response range length overflow"))?;
        if let Some(content_length) = Self::declared_content_length(response)? {
            if content_length != expected_length {
                return Err(anyhow!(
                    "Partial response declared {} bytes, expected {}",
                    content_length,
                    expected_length
                ));
            }
        }
        Ok(())
    }

    pub(super) fn validate_unsatisfied_response(
        response: &reqwest::Response,
        exact_bytes: u64,
    ) -> Result<()> {
        if response.status() != reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
            return Err(anyhow!(
                "Expected range-not-satisfiable 416 response, received {}",
                response.status()
            ));
        }
        let content_range = response
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .ok_or_else(|| anyhow!("416 response is missing Content-Range"))?;
        match parse_content_range(content_range)? {
            ContentRange::Unsatisfied { total } if total == exact_bytes => Ok(()),
            ContentRange::Unsatisfied { total } => Err(anyhow!(
                "416 response reports {} total bytes, expected {}",
                total,
                exact_bytes
            )),
            ContentRange::Range { .. } => {
                Err(anyhow!("416 response has a satisfied Content-Range"))
            }
        }
    }

    pub(super) async fn download_model_detailed_from_source(
        &self,
        model_name: &str,
        model_dir: &Path,
        base_url: &str,
        artifacts: &[ArtifactSpec],
        progress_callback: Option<Box<dyn Fn(DownloadProgress) + Send>>,
    ) -> Result<()> {
        let active_download = self.reserve_active_download(model_name).await?;
        self.set_downloading_status(model_name, 0).await;

        let result = self
            .download_artifacts(
                model_name,
                model_dir,
                base_url,
                artifacts,
                &active_download,
                progress_callback,
            )
            .await;
        let (result, progress_callback) = match result {
            Ok((progress, progress_callback)) => (Ok(progress), progress_callback),
            Err(error) => (Err(error), None),
        };

        self.finish_download(
            model_name,
            model_dir,
            artifacts,
            &active_download,
            result,
            progress_callback,
        )
        .await
    }

    pub(super) async fn download_artifacts(
        &self,
        model_name: &str,
        model_dir: &Path,
        base_url: &str,
        artifacts: &[ArtifactSpec],
        active_download: &ActiveDownload,
        progress_callback: Option<Box<dyn Fn(DownloadProgress) + Send>>,
    ) -> Result<(
        DownloadProgress,
        Option<Box<dyn Fn(DownloadProgress) + Send>>,
    )> {
        if active_download.cancellation.is_cancelled() {
            return Err(DownloadCancelled.into());
        }
        fs::create_dir_all(model_dir)
            .await
            .map_err(|error| anyhow!("Failed to create model directory: {}", error))?;

        let client = reqwest::Client::builder()
            .tls_certs_only(crate::security::transport::tls_roots())
            .tcp_nodelay(true)
            .pool_max_idle_per_host(1)
            .timeout(Duration::from_secs(3600))
            .connect_timeout(Duration::from_secs(30))
            .build()
            .map_err(|error| anyhow!("Failed to create HTTP client: {}", error))?;
        let total_bytes: u64 = artifacts.iter().map(|artifact| artifact.exact_bytes).sum();
        let download_started = Instant::now();
        let mut confirmed_bytes = 0u64;
        let mut streamed_bytes = 0u64;
        let mut bytes_since_report = 0u64;
        let mut last_report = Instant::now();
        let mut last_percent = 0u8;

        for artifact in artifacts {
            if active_download.cancellation.is_cancelled() {
                return Err(DownloadCancelled.into());
            }

            let final_path = model_dir.join(artifact.filename);
            let (final_exists, final_bytes) = match fs::metadata(&final_path).await {
                Ok(metadata) => (true, metadata.len()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => (false, 0),
                Err(error) => {
                    return Err(anyhow!(
                        "Failed to read {} metadata: {}",
                        artifact.filename,
                        error
                    ));
                }
            };

            // Keep v3 partials invisible to cheap model discovery until their
            // content digest passes. Migrate legacy partials so retries resume.
            let file_path = if artifact.sha256.is_some() && final_bytes != artifact.exact_bytes {
                let partial_path = model_dir.join(format!("{}.part", artifact.filename));
                let partial_exists = fs::try_exists(&partial_path).await.map_err(|error| {
                    anyhow!("Failed to inspect partial {}: {}", artifact.filename, error)
                })?;
                if final_exists {
                    if partial_exists {
                        fs::remove_file(&final_path).await.map_err(|error| {
                            anyhow!(
                                "Failed to remove incomplete {}: {}",
                                artifact.filename,
                                error
                            )
                        })?;
                    } else {
                        fs::rename(&final_path, &partial_path)
                            .await
                            .map_err(|error| {
                                anyhow!("Failed to stage partial {}: {}", artifact.filename, error)
                            })?;
                    }
                }
                partial_path
            } else {
                final_path.clone()
            };
            let local_bytes = if file_path == final_path {
                final_bytes
            } else {
                match fs::metadata(&file_path).await {
                    Ok(metadata) => metadata.len(),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
                    Err(error) => {
                        return Err(anyhow!(
                            "Failed to inspect staged {}: {}",
                            artifact.filename,
                            error
                        ))
                    }
                }
            };

            if local_bytes == artifact.exact_bytes {
                let mut context = artifact
                    .sha256
                    .map(|_| digest::Context::new(&digest::SHA256));
                if let Some(context) = context.as_mut() {
                    Self::hash_local_artifact(
                        &file_path,
                        local_bytes,
                        context,
                        &active_download.cancellation,
                    )
                    .await?;
                }
                Self::verify_artifact_digest(&file_path, artifact, context).await?;
                if active_download.cancellation.is_cancelled() {
                    return Err(DownloadCancelled.into());
                }
                if file_path != final_path {
                    fs::rename(&file_path, &final_path).await.map_err(|error| {
                        anyhow!(
                            "Failed to publish verified {}: {}",
                            artifact.filename,
                            error
                        )
                    })?;
                }
                confirmed_bytes = confirmed_bytes
                    .checked_add(artifact.exact_bytes)
                    .ok_or_else(|| {
                        anyhow!("Progress overflow while skipping {}", artifact.filename)
                    })?;
                let progress = Self::in_flight_progress(confirmed_bytes, total_bytes, 0.0);
                if let Some(callback) = &progress_callback {
                    callback(progress.clone());
                }
                self.set_downloading_status(model_name, progress.percent)
                    .await;
                last_percent = progress.percent;
                last_report = Instant::now();
                continue;
            }

            let file_url = format!("{}/{}", base_url.trim_end_matches('/'), artifact.filename);
            let range_start =
                (local_bytes > 0 && local_bytes < artifact.exact_bytes).then_some(local_bytes);
            let response = self
                .send_download_request(&client, &file_url, range_start, active_download)
                .await?;

            let (response, mut artifact_bytes, append) = match range_start {
                Some(range_start) => match response.status() {
                    reqwest::StatusCode::PARTIAL_CONTENT => {
                        Self::validate_partial_response(
                            &response,
                            range_start,
                            artifact.exact_bytes,
                        )?;
                        confirmed_bytes =
                            confirmed_bytes.checked_add(range_start).ok_or_else(|| {
                                anyhow!("Progress overflow while resuming {}", artifact.filename)
                            })?;
                        let progress = Self::in_flight_progress(confirmed_bytes, total_bytes, 0.0);
                        if let Some(callback) = &progress_callback {
                            callback(progress.clone());
                        }
                        self.set_downloading_status(model_name, progress.percent)
                            .await;
                        last_percent = progress.percent;
                        last_report = Instant::now();
                        (response, range_start, true)
                    }
                    reqwest::StatusCode::OK => {
                        Self::validate_full_response(&response, artifact.exact_bytes)?;
                        (response, 0, false)
                    }
                    reqwest::StatusCode::RANGE_NOT_SATISFIABLE => {
                        Self::validate_unsatisfied_response(&response, artifact.exact_bytes)?;
                        let retry = self
                            .send_download_request(&client, &file_url, None, active_download)
                            .await?;
                        Self::validate_full_response(&retry, artifact.exact_bytes)?;
                        (retry, 0, false)
                    }
                    status => {
                        return Err(anyhow!(
                            "Download failed for {} with status {}",
                            artifact.filename,
                            status
                        ));
                    }
                },
                None => match response.status() {
                    reqwest::StatusCode::OK => {
                        Self::validate_full_response(&response, artifact.exact_bytes)?;
                        (response, 0, false)
                    }
                    reqwest::StatusCode::PARTIAL_CONTENT => {
                        Self::validate_partial_response(&response, 0, artifact.exact_bytes)?;
                        (response, 0, false)
                    }
                    status => {
                        return Err(anyhow!(
                            "Download failed for {} with status {}",
                            artifact.filename,
                            status
                        ));
                    }
                },
            };

            if active_download.cancellation.is_cancelled() {
                return Err(DownloadCancelled.into());
            }
            let mut context = artifact
                .sha256
                .map(|_| digest::Context::new(&digest::SHA256));
            if append {
                if let Some(context) = context.as_mut() {
                    Self::hash_local_artifact(
                        &file_path,
                        artifact_bytes,
                        context,
                        &active_download.cancellation,
                    )
                    .await?;
                }
            }
            let file = if append {
                fs::OpenOptions::new()
                    .append(true)
                    .open(&file_path)
                    .await
                    .map_err(|error| {
                        anyhow!("Failed to open {} for resume: {}", artifact.filename, error)
                    })?
            } else {
                fs::OpenOptions::new()
                    .create(true)
                    .truncate(true)
                    .write(true)
                    .open(&file_path)
                    .await
                    .map_err(|error| {
                        anyhow!("Failed to replace {}: {}", artifact.filename, error)
                    })?
            };
            let mut writer = BufWriter::with_capacity(8 * 1024 * 1024, file);
            use futures_util::StreamExt;
            let mut stream = response.bytes_stream();

            loop {
                let next_chunk = tokio::select! {
                    biased;
                    _ = active_download.cancellation.cancelled() => {
                        writer.flush().await.map_err(|error| {
                            anyhow!("Failed to preserve {} during cancellation: {}", artifact.filename, error)
                        })?;
                        return Err(DownloadCancelled.into());
                    }
                    chunk = timeout(Duration::from_secs(30), stream.next()) => chunk,
                };
                let chunk = match next_chunk {
                    Err(_) => {
                        writer.flush().await.map_err(|error| {
                            anyhow!(
                                "Failed to preserve {} after timeout: {}",
                                artifact.filename,
                                error
                            )
                        })?;
                        return Err(anyhow!(
                            "Download timeout for {}: no data received for 30 seconds",
                            artifact.filename
                        ));
                    }
                    Ok(None) => break,
                    Ok(Some(Err(error))) => {
                        writer.flush().await.map_err(|flush_error| {
                            anyhow!(
                                "Failed to preserve {} after stream error: {}",
                                artifact.filename,
                                flush_error
                            )
                        })?;
                        return Err(anyhow!(
                            "Download stream failed for {}: {}",
                            artifact.filename,
                            error
                        ));
                    }
                    Ok(Some(Ok(chunk))) => chunk,
                };

                let chunk_bytes = chunk.len() as u64;
                let next_artifact_bytes = artifact_bytes
                    .checked_add(chunk_bytes)
                    .ok_or_else(|| anyhow!("{} size overflow", artifact.filename))?;
                if next_artifact_bytes > artifact.exact_bytes {
                    writer.flush().await.map_err(|error| {
                        anyhow!(
                            "Failed to preserve {} after overlong response: {}",
                            artifact.filename,
                            error
                        )
                    })?;
                    return Err(anyhow!(
                        "{} response exceeds its exact {} byte size",
                        artifact.filename,
                        artifact.exact_bytes
                    ));
                }
                let next_confirmed_bytes =
                    confirmed_bytes.checked_add(chunk_bytes).ok_or_else(|| {
                        anyhow!("Progress overflow while downloading {}", artifact.filename)
                    })?;
                if next_confirmed_bytes > total_bytes {
                    return Err(anyhow!("Download progress exceeds the catalog total"));
                }

                writer
                    .write_all(&chunk)
                    .await
                    .map_err(|error| anyhow!("Failed to write {}: {}", artifact.filename, error))?;
                if let Some(context) = context.as_mut() {
                    context.update(&chunk);
                }
                artifact_bytes = next_artifact_bytes;
                confirmed_bytes = next_confirmed_bytes;
                streamed_bytes = streamed_bytes
                    .checked_add(chunk_bytes)
                    .ok_or_else(|| anyhow!("Streamed byte count overflow"))?;
                bytes_since_report = bytes_since_report
                    .checked_add(chunk_bytes)
                    .ok_or_else(|| anyhow!("Progress byte count overflow"))?;

                let progress = Self::in_flight_progress(confirmed_bytes, total_bytes, 0.0);
                let elapsed = last_report.elapsed();
                if progress.percent > last_percent
                    || elapsed >= Duration::from_millis(500)
                    || artifact_bytes == artifact.exact_bytes
                {
                    let speed_mbps = if elapsed.as_secs_f64() > 0.0 {
                        bytes_since_report as f64 / (1024.0 * 1024.0) / elapsed.as_secs_f64()
                    } else {
                        0.0
                    };
                    if let Some(callback) = &progress_callback {
                        callback(Self::in_flight_progress(
                            confirmed_bytes,
                            total_bytes,
                            speed_mbps,
                        ));
                    }
                    self.set_downloading_status(model_name, progress.percent)
                        .await;
                    last_percent = progress.percent;
                    last_report = Instant::now();
                    bytes_since_report = 0;
                }
            }

            writer
                .flush()
                .await
                .map_err(|error| anyhow!("Failed to flush {}: {}", artifact.filename, error))?;
            drop(writer);

            if active_download.cancellation.is_cancelled() {
                return Err(DownloadCancelled.into());
            }
            let stored_bytes = fs::metadata(&file_path)
                .await
                .map_err(|error| {
                    anyhow!(
                        "Failed to read {} after download: {}",
                        artifact.filename,
                        error
                    )
                })?
                .len();
            if stored_bytes != artifact.exact_bytes {
                return Err(anyhow!(
                    "{} stored {} bytes, expected exactly {} bytes",
                    artifact.filename,
                    stored_bytes,
                    artifact.exact_bytes
                ));
            }
            Self::verify_artifact_digest(&file_path, artifact, context).await?;
            if active_download.cancellation.is_cancelled() {
                return Err(DownloadCancelled.into());
            }
            if file_path != final_path {
                fs::rename(&file_path, &final_path).await.map_err(|error| {
                    anyhow!(
                        "Failed to publish verified {}: {}",
                        artifact.filename,
                        error
                    )
                })?;
            }
        }

        if confirmed_bytes != total_bytes {
            return Err(anyhow!(
                "Download confirmed {} bytes, expected {} bytes",
                confirmed_bytes,
                total_bytes
            ));
        }
        let elapsed = download_started.elapsed().as_secs_f64();
        let speed_mbps = if elapsed > 0.0 {
            streamed_bytes as f64 / (1024.0 * 1024.0) / elapsed
        } else {
            0.0
        };
        Ok((
            DownloadProgress::new(total_bytes, total_bytes, speed_mbps),
            progress_callback,
        ))
    }

    pub(super) async fn finish_download(
        &self,
        model_name: &str,
        model_dir: &Path,
        artifacts: &[ArtifactSpec],
        active_download: &Arc<ActiveDownload>,
        mut result: Result<DownloadProgress>,
        progress_callback: Option<Box<dyn Fn(DownloadProgress) + Send>>,
    ) -> Result<()> {
        if result.is_ok() && !active_download.cancellation.is_cancelled() {
            let final_progress = result.expect("successful transfer must carry final progress");
            result = Self::validate_model_directory(model_dir, artifacts).map(|()| final_progress);
        }

        #[cfg(test)]
        if let Some(hook) = self.test_hook().await {
            hook.finalization_ready.notify_one();
            hook.continue_finalization.notified().await;
        }

        let mut active_downloads = self.active_downloads.lock().await;
        let owner_matches = matches!(
            active_downloads.downloads.get(model_name),
            Some(owner) if Arc::ptr_eq(owner, active_download)
        );
        if !owner_matches {
            drop(active_downloads);
            active_download.completion.send_replace(true);
            return result.map(|_| ());
        }

        let cancellation_won = active_download.cancellation.is_cancelled()
            || result.as_ref().err().is_some_and(is_download_cancelled);
        let mut models = self.available_models.write().await;
        active_downloads.downloads.remove(model_name);
        if let Some(model) = models.get_mut(model_name) {
            if cancellation_won || result.is_err() {
                model.status = ModelStatus::Missing;
            } else {
                model.status = ModelStatus::Available;
                model.path = model_dir.to_path_buf();
            }
        }
        active_downloads.revision = active_downloads.revision.wrapping_add(1);
        drop(models);
        drop(active_downloads);
        active_download.completion.send_replace(true);

        if cancellation_won {
            return Err(DownloadCancelled.into());
        }
        let final_progress = result?;
        if let Some(callback) = progress_callback {
            callback(final_progress);
        }
        log::info!("Download completed for Parakeet model: {}", model_name);
        Ok(())
    }

    /// Cancel an ongoing model download.
    pub async fn cancel_download(&self, model_name: &str) -> Result<CancelDownloadOutcome> {
        self.cancel_download_with_timeout(model_name, CANCEL_DOWNLOAD_CLEANUP_TIMEOUT)
            .await
    }

    pub(super) async fn cancel_download_with_timeout(
        &self,
        model_name: &str,
        cleanup_timeout: Duration,
    ) -> Result<CancelDownloadOutcome> {
        let active_download = {
            let active_downloads = self.active_downloads.lock().await;
            let Some(active_download) = active_downloads.downloads.get(model_name).cloned() else {
                return Ok(CancelDownloadOutcome::Cancelled);
            };
            active_download.cancellation.cancel();
            active_download
        };

        let mut completion = active_download.completion.subscribe();
        if !*completion.borrow() {
            match timeout(cleanup_timeout, completion.changed()).await {
                Ok(Ok(())) => {}
                Ok(Err(_)) => {
                    return Err(anyhow!(
                        "Download worker ended before completing cancellation cleanup"
                    ));
                }
                Err(_) => return Ok(CancelDownloadOutcome::Pending),
            }
        }

        Ok(CancelDownloadOutcome::Cancelled)
    }
}
