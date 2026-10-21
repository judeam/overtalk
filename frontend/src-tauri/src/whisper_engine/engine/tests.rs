use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::time::{timeout, Duration};

#[test]
fn downloadable_whisper_catalog_has_exact_size_and_digest_pins() {
    let pins: serde_json::Value = serde_json::from_str(include_str!("../model-pins.json")).unwrap();
    assert_eq!(pins["revision"].as_str().unwrap().len(), 40);
    for model in WHISPER_MODEL_CATALOG {
        let pin = &pins["files"][model.0];
        assert_eq!(pin["file"], format!("ggml-{}.bin", model.0));
        assert!(pin["bytes"].as_u64().unwrap() > 1_000_000);
        let hash = pin["sha256"].as_str().unwrap();
        assert_eq!(hash.len(), 64);
        assert!(hash.bytes().all(|b| b.is_ascii_hexdigit()));
    }
}

fn tiny_model(models: &[ModelInfo]) -> &ModelInfo {
    models
        .iter()
        .find(|model| model.name == "tiny")
        .expect("tiny should be present in the Whisper model catalog")
}

#[tokio::test]
async fn discover_models_keeps_active_downloads_downloading_before_a_file_exists() {
    let dir = tempfile::tempdir().unwrap();
    let engine = WhisperEngine::new_with_models_dir(Some(dir.path().to_path_buf())).unwrap();

    let _active_download = engine.reserve_active_download("tiny").await.unwrap();
    engine.available_models.write().await.insert(
        "tiny".to_string(),
        ModelInfo {
            name: "tiny".to_string(),
            path: dir.path().join("ggml-tiny.bin"),
            size_mb: 75,
            accuracy: "Basic".to_string(),
            speed: "Fast".to_string(),
            status: ModelStatus::Downloading { progress: 42 },
            description: "test model".to_string(),
        },
    );

    let absent = engine.discover_models().await.unwrap();
    assert!(matches!(
        tiny_model(&absent).status,
        ModelStatus::Downloading { progress: 42 }
    ));

    std::fs::write(dir.path().join("ggml-tiny.bin"), b"").unwrap();
    let zero_byte = engine.discover_models().await.unwrap();
    assert!(matches!(
        tiny_model(&zero_byte).status,
        ModelStatus::Downloading { progress: 42 }
    ));

    std::fs::write(dir.path().join("ggml-tiny.bin"), [0_u8; 32]).unwrap();
    let partial = engine.discover_models().await.unwrap();
    assert!(matches!(
        tiny_model(&partial).status,
        ModelStatus::Downloading { progress: 42 }
    ));
}

#[tokio::test]
async fn discover_models_uses_disk_state_after_a_download_is_no_longer_active() {
    let dir = tempfile::tempdir().unwrap();
    let engine = WhisperEngine::new_with_models_dir(Some(dir.path().to_path_buf())).unwrap();

    let model_path = dir.path().join("ggml-tiny.bin");
    std::fs::write(&model_path, b"").unwrap();
    let missing_models = engine.discover_models().await.unwrap();
    assert!(matches!(
        tiny_model(&missing_models).status,
        ModelStatus::Missing
    ));

    std::fs::write(&model_path, vec![0_u8; 2 * 1024 * 1024]).unwrap();
    let corrupted_models = engine.discover_models().await.unwrap();

    assert!(matches!(
        tiny_model(&corrupted_models).status,
        ModelStatus::Corrupted { .. }
    ));
}

#[tokio::test]
async fn finish_download_publishes_available_after_releasing_active_owner() {
    let dir = tempfile::tempdir().unwrap();
    let engine = WhisperEngine::new_with_models_dir(Some(dir.path().to_path_buf())).unwrap();
    let model_path = dir.path().join("ggml-tiny.bin");

    std::fs::write(&model_path, b"ggml\0\0\0\0").unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&model_path)
        .unwrap()
        .set_len(68 * 1024 * 1024)
        .unwrap();

    engine.discover_models().await.unwrap();
    let active_download = engine.reserve_active_download("tiny").await.unwrap();
    let active_models = engine.discover_models().await.unwrap();
    assert!(matches!(
        tiny_model(&active_models).status,
        ModelStatus::Downloading { progress: 0 }
    ));

    engine
        .finish_download("tiny", &active_download, &model_path, Ok(()))
        .await
        .unwrap();

    assert!(!engine.active_downloads.lock().await.contains_key("tiny"));
    assert!(matches!(
        engine
            .available_models
            .read()
            .await
            .get("tiny")
            .expect("tiny should remain cached")
            .status,
        ModelStatus::Available
    ));

    let rediscovered_models = engine.discover_models().await.unwrap();
    assert!(matches!(
        tiny_model(&rediscovered_models).status,
        ModelStatus::Available
    ));
}

#[tokio::test]
async fn finish_download_rejects_invalid_header_and_cleans_file() {
    let dir = tempfile::tempdir().unwrap();
    let engine = WhisperEngine::new_with_models_dir(Some(dir.path().to_path_buf())).unwrap();
    let model_path = dir.path().join("ggml-tiny.bin");
    std::fs::write(&model_path, b"not-a-model").unwrap();

    engine.discover_models().await.unwrap();
    let active_download = engine.reserve_active_download("tiny").await.unwrap();
    let error = engine
        .finish_download("tiny", &active_download, &model_path, Ok(()))
        .await
        .unwrap_err();

    assert!(error
        .to_string()
        .contains("Invalid model file: missing GGML/GGUF magic number"));
    assert!(!engine.active_downloads.lock().await.contains_key("tiny"));
    assert!(!model_path.exists());
    assert!(matches!(
        engine
            .available_models
            .read()
            .await
            .get("tiny")
            .expect("tiny should remain cached")
            .status,
        ModelStatus::Missing
    ));
}

#[tokio::test]
async fn unsupported_download_does_not_leave_an_active_transfer() {
    let dir = tempfile::tempdir().unwrap();
    let engine = WhisperEngine::new_with_models_dir(Some(dir.path().to_path_buf())).unwrap();

    assert!(engine.download_model("not-a-model", None).await.is_err());
    assert!(!engine
        .active_downloads
        .lock()
        .await
        .contains_key("not-a-model"));
}

#[tokio::test]
async fn duplicate_download_reservation_is_rejected_while_the_first_owner_is_active() {
    let dir = tempfile::tempdir().unwrap();
    let engine = WhisperEngine::new_with_models_dir(Some(dir.path().to_path_buf())).unwrap();

    let _first_owner = engine.reserve_active_download("tiny").await.unwrap();
    assert!(engine.reserve_active_download("tiny").await.is_err());
}

#[tokio::test]
async fn cancelled_finalization_holds_active_ownership_before_discovery() {
    let dir = tempfile::tempdir().unwrap();
    let engine =
        Arc::new(WhisperEngine::new_with_models_dir(Some(dir.path().to_path_buf())).unwrap());
    engine.discover_models().await.unwrap();

    let active_download = engine.reserve_active_download("tiny").await.unwrap();
    active_download.cancellation.cancel();

    let hook = Arc::new(DownloadStateTestHook {
        cancellation_first_lock_acquired: tokio::sync::Notify::new(),
        continue_cancellation: tokio::sync::Notify::new(),
        discovery_active_lock_acquired: tokio::sync::Notify::new(),
        continue_discovery: tokio::sync::Notify::new(),
    });
    *engine.download_state_test_hook.lock().unwrap() = Some(Arc::clone(&hook));

    let finalization_engine = Arc::clone(&engine);
    let finalization_owner = Arc::clone(&active_download);
    let model_path = dir.path().join("ggml-tiny.bin");
    let finalization = tokio::spawn(async move {
        finalization_engine
            .finish_download(
                "tiny",
                &finalization_owner,
                &model_path,
                Err(DownloadCancelled.into()),
            )
            .await
    });

    timeout(
        Duration::from_secs(1),
        hook.cancellation_first_lock_acquired.notified(),
    )
    .await
    .expect("cancellation finalization should reach its first lock");

    let discovery_engine = Arc::clone(&engine);
    let discovery = tokio::spawn(async move { discovery_engine.discover_models().await });

    assert!(
        timeout(
            Duration::from_millis(100),
            hook.discovery_active_lock_acquired.notified(),
        )
        .await
        .is_err(),
        "discovery must wait for cancellation to release active ownership"
    );

    hook.continue_cancellation.notify_one();
    let finalization_result = timeout(Duration::from_secs(1), finalization)
        .await
        .expect("cancelled finalization should complete")
        .unwrap()
        .unwrap_err();
    assert!(is_download_cancelled(&finalization_result));

    timeout(
        Duration::from_secs(1),
        hook.discovery_active_lock_acquired.notified(),
    )
    .await
    .expect("discovery should acquire active ownership after cancellation finalizes");
    hook.continue_discovery.notify_one();

    let models = timeout(Duration::from_secs(1), discovery)
        .await
        .expect("discovery should complete")
        .unwrap()
        .unwrap();
    assert!(matches!(tiny_model(&models).status, ModelStatus::Missing));
    assert!(!engine.active_downloads.lock().await.contains_key("tiny"));
}

#[tokio::test]
async fn cancelling_an_unresponsive_download_keeps_ownership_reserved() {
    let dir = tempfile::tempdir().unwrap();
    let engine = WhisperEngine::new_with_models_dir(Some(dir.path().to_path_buf())).unwrap();
    let active_download = engine.reserve_active_download("tiny").await.unwrap();

    let outcome = engine
        .cancel_download_with_timeout("tiny", Duration::from_millis(1))
        .await
        .expect("an unresponsive download should report cancellation pending");

    assert_eq!(outcome, CancelDownloadOutcome::Pending);
    assert!(active_download.cancellation.is_cancelled());
    assert!(matches!(
        engine.active_downloads.lock().await.get("tiny"),
        Some(current) if Arc::ptr_eq(current, &active_download)
    ));
    assert!(engine.reserve_active_download("tiny").await.is_err());
}

#[tokio::test]
async fn late_cancellation_preserves_a_completed_model_state() {
    let dir = tempfile::tempdir().unwrap();
    let engine = WhisperEngine::new_with_models_dir(Some(dir.path().to_path_buf())).unwrap();
    let model_path = dir.path().join("ggml-tiny.bin");

    std::fs::write(&model_path, b"ggml\0\0\0\0").unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&model_path)
        .unwrap()
        .set_len(68 * 1024 * 1024)
        .unwrap();

    engine.discover_models().await.unwrap();
    let active_download = engine.reserve_active_download("tiny").await.unwrap();
    engine
        .finish_download("tiny", &active_download, &model_path, Ok(()))
        .await
        .unwrap();

    assert_eq!(
        engine
            .cancel_download_with_timeout("tiny", Duration::from_millis(1))
            .await
            .unwrap(),
        CancelDownloadOutcome::Cancelled
    );
    assert!(matches!(
        tiny_model(&engine.discover_models().await.unwrap()).status,
        ModelStatus::Available
    ));
}

async fn stalled_http_server() -> (
    String,
    oneshot::Receiver<()>,
    oneshot::Sender<()>,
    tokio::task::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (headers_sent, headers_ready) = oneshot::channel();
    let (release_body, body_released) = oneshot::channel();

    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        crate::test_support::read_http_headers(&mut socket).await;
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        socket.flush().await.unwrap();
        headers_sent.send(()).unwrap();
        let _ = body_released.await;
        let _ = socket.write_all(b"test").await;
    });

    (
        format!("http://{address}/model.bin"),
        headers_ready,
        release_body,
        server,
    )
}

async fn response_http_server(
    content_length: usize,
    body: &'static [u8],
) -> (
    String,
    oneshot::Receiver<Vec<u8>>,
    tokio::task::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (request_sent, request_received) = oneshot::channel();

    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buffer = [0_u8; 1024];
        loop {
            let bytes_read = socket.read(&mut buffer).await.unwrap();
            if bytes_read == 0 {
                break;
            }

            request.extend_from_slice(&buffer[..bytes_read]);
            if request.ends_with(b"\r\n\r\n") {
                break;
            }
        }
        request_sent.send(request).unwrap();

        let headers = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {content_length}\r\nConnection: close\r\n\r\n"
        );
        socket.write_all(headers.as_bytes()).await.unwrap();
        socket.write_all(body).await.unwrap();
        socket.flush().await.unwrap();
    });

    (
        format!("http://{address}/model.bin"),
        request_received,
        server,
    )
}

#[tokio::test]
async fn downloaded_model_sends_user_agent_and_rejects_undersized_valid_header() {
    let dir = tempfile::tempdir().unwrap();
    let engine = WhisperEngine::new_with_models_dir(Some(dir.path().to_path_buf())).unwrap();
    engine.discover_models().await.unwrap();
    let (url, request, server) = response_http_server(8, b"ggml\0\0\0\0").await;

    let error = engine
        .download_model_from_url("tiny", &url, None)
        .await
        .unwrap_err();
    let request = String::from_utf8(request.await.unwrap()).unwrap();
    server.await.unwrap();

    assert!(error.to_string().contains("too small"));
    assert!(request.to_ascii_lowercase().contains(&format!(
        "user-agent: overtalk/{}",
        env!("CARGO_PKG_VERSION")
    )));
    assert!(!engine.active_downloads.lock().await.contains_key("tiny"));
    assert!(!dir.path().join("ggml-tiny.bin").exists());
    assert!(matches!(
        engine
            .available_models
            .read()
            .await
            .get("tiny")
            .expect("tiny should remain cached")
            .status,
        ModelStatus::Missing
    ));
}

#[tokio::test]
async fn cancelling_a_stalled_download_waits_for_cleanup_before_retry_can_start() {
    let dir = tempfile::tempdir().unwrap();
    let engine =
        Arc::new(WhisperEngine::new_with_models_dir(Some(dir.path().to_path_buf())).unwrap());
    let (url_a, headers_a, release_a, server_a) = stalled_http_server().await;

    let download_a_engine = Arc::clone(&engine);
    let download_a = tokio::spawn(async move {
        download_a_engine
            .download_model_from_url("tiny", &url_a, None)
            .await
    });

    timeout(Duration::from_secs(1), headers_a)
        .await
        .expect("first request should receive response headers")
        .unwrap();

    assert!(engine.active_downloads.lock().await.contains_key("tiny"));
    let status_while_active = engine.discover_models().await.unwrap();
    assert!(matches!(
        tiny_model(&status_while_active).status,
        ModelStatus::Downloading { progress: 0 }
    ));

    timeout(Duration::from_secs(1), engine.cancel_download("tiny"))
        .await
        .expect("cancellation should not wait for a stalled response body")
        .unwrap();

    let download_a_result = timeout(Duration::from_secs(1), download_a)
        .await
        .expect("cancelled worker should finish promptly")
        .unwrap();
    assert!(is_download_cancelled(&download_a_result.unwrap_err()));
    assert!(!engine.active_downloads.lock().await.contains_key("tiny"));
    assert!(!dir.path().join("ggml-tiny.bin").exists());

    let (url_b, headers_b, release_b, server_b) = stalled_http_server().await;
    let download_b_engine = Arc::clone(&engine);
    let download_b = tokio::spawn(async move {
        download_b_engine
            .download_model_from_url("tiny", &url_b, None)
            .await
    });
    timeout(Duration::from_secs(1), headers_b)
        .await
        .expect("retry should reserve the released model slot")
        .unwrap();
    assert!(engine.active_downloads.lock().await.contains_key("tiny"));

    timeout(Duration::from_secs(1), engine.cancel_download("tiny"))
        .await
        .expect("retry cancellation should also complete promptly")
        .unwrap();
    assert!(is_download_cancelled(
        &timeout(Duration::from_secs(1), download_b)
            .await
            .expect("retry worker should finish promptly")
            .unwrap()
            .unwrap_err()
    ));

    let _ = release_a.send(());
    let _ = release_b.send(());
    let _ = server_a.await;
    let _ = server_b.await;
}

#[tokio::test]
async fn incomplete_response_cleans_partial_file_and_cache() {
    let dir = tempfile::tempdir().unwrap();
    let engine = WhisperEngine::new_with_models_dir(Some(dir.path().to_path_buf())).unwrap();
    let model_path = dir.path().join("ggml-tiny.bin");
    engine.discover_models().await.unwrap();
    let (url, _request, server) = response_http_server(8, b"ggml").await;

    let error = engine
        .download_model_from_url("tiny", &url, None)
        .await
        .unwrap_err();
    server.await.unwrap();

    assert!(error.to_string().contains("Failed to read chunk"));
    assert!(!engine.active_downloads.lock().await.contains_key("tiny"));
    assert!(!model_path.exists());
    assert!(matches!(
        engine
            .available_models
            .read()
            .await
            .get("tiny")
            .expect("tiny should remain cached")
            .status,
        ModelStatus::Missing
    ));

    let models = engine.discover_models().await.unwrap();
    assert!(matches!(tiny_model(&models).status, ModelStatus::Missing));
}

#[tokio::test]
async fn terminal_download_error_releases_the_active_transfer() {
    let dir = tempfile::tempdir().unwrap();
    let engine = WhisperEngine::new_with_models_dir(Some(dir.path().to_path_buf())).unwrap();
    let model_path = dir.path().join("ggml-tiny.bin");
    std::fs::write(&model_path, vec![0_u8; 2 * 1024 * 1024]).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();

    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        crate::test_support::read_http_headers(&mut socket).await;
        socket
            .write_all(b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n")
            .await
            .unwrap();
    });

    assert!(engine
        .download_model_from_url("tiny", &format!("http://{address}/model.bin"), None)
        .await
        .is_err());
    server.await.unwrap();

    assert!(!engine.active_downloads.lock().await.contains_key("tiny"));
    assert!(!model_path.exists());
    let models = engine.discover_models().await.unwrap();
    assert!(matches!(tiny_model(&models).status, ModelStatus::Missing));
}
