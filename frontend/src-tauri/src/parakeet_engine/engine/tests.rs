use super::*;
use crossbeam::queue::SegQueue;
use tempfile::tempdir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
const TEST_MODEL_NAME: &str = "parakeet-test";
const SMALL_ARTIFACTS: &[ArtifactSpec] = &[
    ArtifactSpec {
        filename: "encoder.bin",
        exact_bytes: 4,
        sha256: None,
    },
    ArtifactSpec {
        filename: "decoder.bin",
        exact_bytes: 3,
        sha256: None,
    },
    ArtifactSpec {
        filename: "nemo.bin",
        exact_bytes: 2,
        sha256: None,
    },
    ArtifactSpec {
        filename: "vocab.txt",
        exact_bytes: 1,
        sha256: None,
    },
];
const SMALL_MODEL_SPECS: &[ModelSpec] = &[ModelSpec {
    name: TEST_MODEL_NAME,
    size_mb: 1,
    quantization: QuantizationType::Int8,
    speed: "test",
    description: "test model",
    source_base_url: "",
    artifacts: SMALL_ARTIFACTS,
}];
const HASHED_ARTIFACTS: &[ArtifactSpec] = &[ArtifactSpec {
    filename: "model.bin",
    exact_bytes: 4,
    sha256: Some("e12e115acf4552b2568b55e93cbd39394c4ef81c82447fafc997882a02d23677"),
}];

struct ExpectedResponse {
    filename: &'static str,
    range: Option<&'static str>,
    status: &'static str,
    content_length: Option<u64>,
    content_range: Option<&'static str>,
    body: &'static [u8],
    release_after_body: Option<oneshot::Receiver<()>>,
}

fn response(
    filename: &'static str,
    range: Option<&'static str>,
    status: &'static str,
    body: &'static [u8],
    content_range: Option<&'static str>,
) -> ExpectedResponse {
    ExpectedResponse {
        filename,
        range,
        status,
        content_length: Some(body.len() as u64),
        content_range,
        body,
        release_after_body: None,
    }
}

async fn read_request(socket: &mut tokio::net::TcpStream) -> String {
    let mut request = Vec::new();
    let mut buffer = [0u8; 1024];
    loop {
        let read = socket.read(&mut buffer).await.expect("read request");
        assert_ne!(read, 0, "request ended before its headers");
        request.extend_from_slice(&buffer[..read]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            return String::from_utf8(request).expect("request is valid UTF-8");
        }
    }
}

async fn serve_requests(
    expected_responses: Vec<ExpectedResponse>,
) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind loopback test server");
    let address = listener.local_addr().expect("get loopback address");
    let server = tokio::spawn(async move {
        for expected in expected_responses {
            let (mut socket, _) = listener.accept().await.expect("accept test request");
            let request = read_request(&mut socket).await;
            assert!(
                request.starts_with(&format!("GET /{} HTTP/", expected.filename)),
                "unexpected request path: {request}"
            );
            let requested_range = request.lines().find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("range").then_some(value.trim())
            });
            assert_eq!(requested_range, expected.range);

            let mut headers = format!("HTTP/1.1 {}\r\nConnection: close\r\n", expected.status);
            if let Some(content_length) = expected.content_length {
                headers.push_str(&format!("Content-Length: {content_length}\r\n"));
            }
            if let Some(content_range) = expected.content_range {
                headers.push_str(&format!("Content-Range: {content_range}\r\n"));
            }
            headers.push_str("\r\n");
            socket
                .write_all(headers.as_bytes())
                .await
                .expect("write response headers");
            socket
                .write_all(expected.body)
                .await
                .expect("write response body");
            if let Some(release_after_body) = expected.release_after_body {
                release_after_body.await.expect("release partial response");
            }
        }
    });

    (format!("http://{address}"), server)
}

async fn test_engine() -> (tempfile::TempDir, Arc<ParakeetEngine>, PathBuf) {
    let temp_dir = tempdir().expect("create temporary models directory");
    let engine = Arc::new(
        ParakeetEngine::new_with_models_dir(Some(temp_dir.path().to_path_buf()))
            .expect("create Parakeet engine"),
    );
    let model_dir = engine.models_dir.join(TEST_MODEL_NAME);
    engine.available_models.write().await.insert(
        TEST_MODEL_NAME.to_string(),
        ModelInfo {
            name: TEST_MODEL_NAME.to_string(),
            path: model_dir.clone(),
            size_mb: 1,
            quantization: QuantizationType::Int8,
            speed: "test".to_string(),
            status: ModelStatus::Missing,
            description: "test model".to_string(),
        },
    );
    (temp_dir, engine, model_dir)
}

async fn test_model_status(engine: &ParakeetEngine) -> ModelStatus {
    engine
        .available_models
        .read()
        .await
        .get(TEST_MODEL_NAME)
        .expect("test model remains registered")
        .status
        .clone()
}

#[tokio::test]
async fn directory_validation_requires_exact_artifact_sizes() {
    let temp_dir = tempdir().expect("create temporary model directory");
    for artifact in SMALL_ARTIFACTS {
        fs::write(
            temp_dir.path().join(artifact.filename),
            vec![0; artifact.exact_bytes as usize],
        )
        .await
        .expect("seed exact artifact");
    }
    assert!(ParakeetEngine::validate_model_directory(temp_dir.path(), SMALL_ARTIFACTS).is_ok());

    fs::remove_file(temp_dir.path().join("vocab.txt"))
        .await
        .expect("remove required artifact");
    assert!(ParakeetEngine::validate_model_directory(temp_dir.path(), SMALL_ARTIFACTS).is_err());

    fs::write(temp_dir.path().join("vocab.txt"), [])
        .await
        .expect("restore undersized artifact");
    fs::write(temp_dir.path().join("encoder.bin"), [0; 3])
        .await
        .expect("seed one-byte-short artifact");
    assert!(ParakeetEngine::validate_model_directory(temp_dir.path(), SMALL_ARTIFACTS).is_err());

    fs::write(temp_dir.path().join("encoder.bin"), [0; 5])
        .await
        .expect("seed one-byte-oversized artifact");
    assert!(ParakeetEngine::validate_model_directory(temp_dir.path(), SMALL_ARTIFACTS).is_err());
}

#[tokio::test]
async fn sha256_mismatch_removes_staged_bytes_without_publishing_success() {
    let (_temp_dir, engine, model_dir) = test_engine().await;
    let events = Arc::new(SegQueue::new());
    let callback_events = Arc::clone(&events);
    let (base_url, server) =
        serve_requests(vec![response("model.bin", None, "200 OK", b"WXYZ", None)]).await;
    let error = engine
        .download_model_detailed_from_source(
            TEST_MODEL_NAME,
            &model_dir,
            &base_url,
            HASHED_ARTIFACTS,
            Some(Box::new(move |progress| {
                callback_events.push(progress);
            })),
        )
        .await
        .expect_err("same-length corrupted download must fail integrity verification");
    server.await.expect("join wrong-hash server");
    assert!(error.to_string().contains("SHA-256 mismatch"));
    assert!(!fs::try_exists(model_dir.join("model.bin")).await.unwrap());
    assert!(!fs::try_exists(model_dir.join("model.bin.part"))
        .await
        .unwrap());
    assert!(std::iter::from_fn(|| events.pop()).all(|progress| progress.percent < 100));
    assert!(matches!(
        test_model_status(&engine).await,
        ModelStatus::Missing
    ));
}

#[tokio::test]
async fn sha256_resume_verifies_retained_prefix_and_streamed_suffix() {
    for (prefix, succeeds) in [(b"AB", true), (b"ZZ", false)] {
        let (_temp_dir, engine, model_dir) = test_engine().await;
        fs::create_dir_all(&model_dir).await.unwrap();
        // A pre-upgrade partial is moved to staging and remains resumable.
        fs::write(model_dir.join("model.bin"), prefix)
            .await
            .unwrap();
        let (base_url, server) = serve_requests(vec![response(
            "model.bin",
            Some("bytes=2-"),
            "206 Partial Content",
            b"CD",
            Some("bytes 2-3/4"),
        )])
        .await;
        let result = engine
            .download_model_detailed_from_source(
                TEST_MODEL_NAME,
                &model_dir,
                &base_url,
                HASHED_ARTIFACTS,
                None,
            )
            .await;
        server.await.expect("join resume-integrity server");
        assert!(!fs::try_exists(model_dir.join("model.bin.part"))
            .await
            .unwrap());
        if succeeds {
            result.expect("correct prefix plus suffix matches the pinned hash");
            assert_eq!(
                fs::read(model_dir.join("model.bin")).await.unwrap(),
                b"ABCD"
            );
            assert!(matches!(
                test_model_status(&engine).await,
                ModelStatus::Available
            ));
        } else {
            assert!(result.unwrap_err().to_string().contains("SHA-256 mismatch"));
            assert!(!fs::try_exists(model_dir.join("model.bin")).await.unwrap());
            assert!(matches!(
                test_model_status(&engine).await,
                ModelStatus::Missing
            ));
        }
    }
}

#[tokio::test]
async fn sha256_retry_rejects_corrupt_same_length_completed_cache() {
    let (_temp_dir, engine, model_dir) = test_engine().await;
    fs::create_dir_all(&model_dir).await.unwrap();
    fs::write(model_dir.join("model.bin"), b"WXYZ")
        .await
        .unwrap();
    let error = engine
        .download_model_detailed_from_source(
            TEST_MODEL_NAME,
            &model_dir,
            "http://127.0.0.1:1",
            HASHED_ARTIFACTS,
            None,
        )
        .await
        .expect_err("explicit retry verifies a full cache before skipping it");
    assert!(error.to_string().contains("SHA-256 mismatch"));
    assert!(!fs::try_exists(model_dir.join("model.bin")).await.unwrap());
    assert!(matches!(
        test_model_status(&engine).await,
        ModelStatus::Missing
    ));
}

#[tokio::test]
async fn sha256_cancelled_complete_stage_is_verified_before_retry_publish() {
    let (_temp_dir, engine, model_dir) = test_engine().await;
    let owner = engine
        .reserve_active_download(TEST_MODEL_NAME)
        .await
        .unwrap();
    let cancel = owner.cancellation.clone();
    let (base_url, server) =
        serve_requests(vec![response("model.bin", None, "200 OK", b"ABCD", None)]).await;
    let error = engine
        .download_artifacts(
            TEST_MODEL_NAME,
            &model_dir,
            &base_url,
            HASHED_ARTIFACTS,
            &owner,
            Some(Box::new(move |_| {
                cancel.cancel();
            })),
        )
        .await
        .err()
        .expect("cancel before final integrity verification");
    server.await.expect("join cancellation-integrity server");
    assert!(is_download_cancelled(&error));
    assert!(!fs::try_exists(model_dir.join("model.bin")).await.unwrap());
    assert_eq!(
        fs::read(model_dir.join("model.bin.part")).await.unwrap(),
        b"ABCD"
    );
    engine
        .finish_download(
            TEST_MODEL_NAME,
            &model_dir,
            HASHED_ARTIFACTS,
            &owner,
            Err(error),
            None,
        )
        .await
        .expect_err("complete cancellation cleanup");
    assert!(ParakeetEngine::validate_model_directory(&model_dir, HASHED_ARTIFACTS).is_err());

    // The complete staged cache is verified/published without a new request.
    engine
        .download_model_detailed_from_source(
            TEST_MODEL_NAME,
            &model_dir,
            "http://127.0.0.1:1",
            HASHED_ARTIFACTS,
            None,
        )
        .await
        .expect("retry verifies the completed staged bytes");
    assert_eq!(
        fs::read(model_dir.join("model.bin")).await.unwrap(),
        b"ABCD"
    );
    assert!(!fs::try_exists(model_dir.join("model.bin.part"))
        .await
        .unwrap());
    assert!(matches!(
        test_model_status(&engine).await,
        ModelStatus::Available
    ));
}

#[tokio::test]
async fn loading_releases_available_models_and_serializes_unload() {
    let (_temp_dir, engine, _model_dir) = test_engine().await;
    engine
        .available_models
        .write()
        .await
        .get_mut(TEST_MODEL_NAME)
        .expect("test model remains registered")
        .status = ModelStatus::Available;
    *engine.current_model_name.write().await = Some("previous-test-model".to_string());

    let hook = Arc::new(ModelLifecycleTestHook {
        load_started: tokio::sync::Notify::new(),
        continue_load: tokio::sync::Notify::new(),
        unload_attempted: tokio::sync::Notify::new(),
    });
    *engine.model_lifecycle_test_hook.lock().await = Some(Arc::clone(&hook));

    let load_engine = Arc::clone(&engine);
    let load = tokio::spawn(async move { load_engine.load_model(TEST_MODEL_NAME).await });
    tokio::time::timeout(Duration::from_secs(1), hook.load_started.notified())
        .await
        .expect("model load must reach its blocking task");

    {
        let mut models =
            tokio::time::timeout(Duration::from_secs(1), engine.available_models.write())
                .await
                .expect("model cache must remain writable during native loading");
        models
            .get_mut(TEST_MODEL_NAME)
            .expect("test model remains registered")
            .description = "updated while native load is paused".to_string();
    }

    let (unloaded_tx, mut unloaded_rx) = oneshot::channel();
    let unload_engine = Arc::clone(&engine);
    let unload = tokio::spawn(async move {
        let unloaded = unload_engine.unload_model().await;
        unloaded_tx
            .send(unloaded)
            .expect("unload receiver remains connected");
    });
    tokio::time::timeout(Duration::from_secs(1), hook.unload_attempted.notified())
        .await
        .expect("unload must reach the lifecycle boundary");
    assert!(matches!(
        unloaded_rx.try_recv(),
        Err(oneshot::error::TryRecvError::Empty)
    ));

    hook.continue_load.notify_one();
    tokio::time::timeout(Duration::from_secs(1), load)
        .await
        .expect("model load must finish after release")
        .expect("join model load task")
        .expect_err("empty model directory must fail loading");
    assert!(!tokio::time::timeout(Duration::from_secs(1), unloaded_rx)
        .await
        .expect("unload must finish after model load")
        .expect("unload sender remains connected"));
    tokio::time::timeout(Duration::from_secs(1), unload)
        .await
        .expect("unload task must join")
        .expect("join unload task");

    assert_eq!(
        engine
            .available_models
            .read()
            .await
            .get(TEST_MODEL_NAME)
            .expect("test model remains registered")
            .description,
        "updated while native load is paused"
    );
    assert!(engine.current_model.read().await.is_none());
    assert!(engine.current_model_name.read().await.is_none());
    *engine.model_lifecycle_test_hook.lock().await = None;
}

#[tokio::test]
async fn completed_sibling_survives_403_then_retry_resumes_partial() {
    let (_temp_dir, engine, model_dir) = test_engine().await;
    fs::create_dir_all(&model_dir)
        .await
        .expect("create model directory");
    fs::write(model_dir.join("encoder.bin"), b"ABCD")
        .await
        .expect("seed completed sibling");
    fs::write(model_dir.join("decoder.bin"), b"X")
        .await
        .expect("seed resumable artifact");

    let (base_url, server) = serve_requests(vec![response(
        "decoder.bin",
        Some("bytes=1-"),
        "403 Forbidden",
        b"",
        None,
    )])
    .await;
    let error = engine
        .download_model_detailed_from_source(
            TEST_MODEL_NAME,
            &model_dir,
            &base_url,
            SMALL_ARTIFACTS,
            None,
        )
        .await
        .expect_err("403 must fail without destructive cleanup");
    server.await.expect("join 403 server");
    assert!(error.to_string().contains("403"));
    assert_eq!(
        fs::read(model_dir.join("encoder.bin")).await.unwrap(),
        b"ABCD"
    );
    assert_eq!(fs::read(model_dir.join("decoder.bin")).await.unwrap(), b"X");
    assert!(!engine
        .active_downloads
        .lock()
        .await
        .downloads
        .contains_key(TEST_MODEL_NAME));

    let (base_url, server) = serve_requests(vec![
        response(
            "decoder.bin",
            Some("bytes=1-"),
            "206 Partial Content",
            b"YZ",
            Some("bytes 1-2/3"),
        ),
        response("nemo.bin", None, "200 OK", b"NO", None),
        response("vocab.txt", None, "200 OK", b"V", None),
    ])
    .await;
    engine
        .download_model_detailed_from_source(
            TEST_MODEL_NAME,
            &model_dir,
            &base_url,
            SMALL_ARTIFACTS,
            None,
        )
        .await
        .expect("retry resumes only the partial artifact");
    server.await.expect("join retry server");

    assert_eq!(
        fs::read(model_dir.join("encoder.bin")).await.unwrap(),
        b"ABCD"
    );
    assert_eq!(
        fs::read(model_dir.join("decoder.bin")).await.unwrap(),
        b"XYZ"
    );
    assert!(matches!(
        test_model_status(&engine).await,
        ModelStatus::Available
    ));
    assert!(!engine
        .active_downloads
        .lock()
        .await
        .downloads
        .contains_key(TEST_MODEL_NAME));
}

#[tokio::test]
async fn cancelled_near_complete_artifact_is_resumed_on_retry() {
    const ARTIFACTS: &[ArtifactSpec] = &[ArtifactSpec {
        filename: "near.bin",
        exact_bytes: 100,
        sha256: None,
    }];
    const PREFIX: &[u8] = &[b'A'; 99];

    let (_temp_dir, engine, model_dir) = test_engine().await;
    let (release_tx, release_rx) = oneshot::channel();
    let (progress_tx, progress_rx) = oneshot::channel();
    let progress_tx = Arc::new(std::sync::Mutex::new(Some(progress_tx)));
    let (base_url, server) = serve_requests(vec![ExpectedResponse {
        filename: "near.bin",
        range: None,
        status: "200 OK",
        content_length: Some(100),
        content_range: None,
        body: PREFIX,
        release_after_body: Some(release_rx),
    }])
    .await;

    let download_engine = Arc::clone(&engine);
    let download_dir = model_dir.clone();
    let download = tokio::spawn(async move {
        download_engine
            .download_model_detailed_from_source(
                TEST_MODEL_NAME,
                &download_dir,
                &base_url,
                ARTIFACTS,
                Some(Box::new(move |progress| {
                    if progress.downloaded_bytes == 99 {
                        if let Some(sender) =
                            progress_tx.lock().expect("lock progress sender").take()
                        {
                            let _ = sender.send(());
                        }
                    }
                })),
            )
            .await
    });

    tokio::time::timeout(Duration::from_secs(5), progress_rx)
        .await
        .expect("receive near-complete progress")
        .expect("progress sender remains connected");
    assert_eq!(
        engine
            .cancel_download_with_timeout(TEST_MODEL_NAME, Duration::from_secs(5))
            .await
            .expect("cancel near-complete download"),
        CancelDownloadOutcome::Cancelled
    );
    release_tx.send(()).expect("release partial response");
    let error = download
        .await
        .expect("join cancelled download")
        .expect_err("cancelled download must not succeed");
    server.await.expect("join partial-response server");

    assert!(is_download_cancelled(&error));
    assert_eq!(
        fs::metadata(model_dir.join("near.bin"))
            .await
            .unwrap()
            .len(),
        99
    );
    assert!(matches!(
        test_model_status(&engine).await,
        ModelStatus::Missing
    ));

    let (base_url, server) = serve_requests(vec![response(
        "near.bin",
        Some("bytes=99-"),
        "206 Partial Content",
        b"B",
        Some("bytes 99-99/100"),
    )])
    .await;
    engine
        .download_model_detailed_from_source(
            TEST_MODEL_NAME,
            &model_dir,
            &base_url,
            ARTIFACTS,
            None,
        )
        .await
        .expect("retry resumes the cancelled near-complete artifact");
    server.await.expect("join retry server");

    assert_eq!(
        fs::metadata(model_dir.join("near.bin"))
            .await
            .unwrap()
            .len(),
        100
    );
    assert!(matches!(
        test_model_status(&engine).await,
        ModelStatus::Available
    ));
}

#[tokio::test]
async fn range_ignored_replaces_partial_with_honest_progress() {
    const ARTIFACTS: &[ArtifactSpec] = &[ArtifactSpec {
        filename: "model.bin",
        exact_bytes: 4,
        sha256: None,
    }];
    let (_temp_dir, engine, model_dir) = test_engine().await;
    fs::create_dir_all(&model_dir)
        .await
        .expect("create model directory");
    fs::write(model_dir.join("model.bin"), b"zz")
        .await
        .expect("seed partial artifact");
    let events = Arc::new(SegQueue::new());
    let callback_events = Arc::clone(&events);

    let (base_url, server) = serve_requests(vec![response(
        "model.bin",
        Some("bytes=2-"),
        "200 OK",
        b"ABCD",
        None,
    )])
    .await;
    engine
        .download_model_detailed_from_source(
            TEST_MODEL_NAME,
            &model_dir,
            &base_url,
            ARTIFACTS,
            Some(Box::new(move |progress| {
                callback_events.push(progress);
            })),
        )
        .await
        .expect("range-ignored response replaces the partial artifact");
    server.await.expect("join range-ignored server");

    let events: Vec<_> = std::iter::from_fn(|| events.pop()).collect();
    assert_eq!(
        fs::read(model_dir.join("model.bin")).await.unwrap(),
        b"ABCD"
    );
    assert!(events
        .iter()
        .all(|progress| progress.downloaded_bytes <= progress.total_bytes));
    assert_eq!(events.last().expect("final event").percent, 100);
    assert!(events[..events.len() - 1]
        .iter()
        .all(|progress| progress.percent < 100));
}

#[tokio::test]
async fn range_416_retries_fresh_with_honest_progress() {
    const ARTIFACTS: &[ArtifactSpec] = &[ArtifactSpec {
        filename: "model.bin",
        exact_bytes: 4,
        sha256: None,
    }];
    let (_temp_dir, engine, model_dir) = test_engine().await;
    fs::create_dir_all(&model_dir)
        .await
        .expect("create model directory");
    fs::write(model_dir.join("model.bin"), b"zz")
        .await
        .expect("seed partial artifact");
    let events = Arc::new(SegQueue::new());
    let callback_events = Arc::clone(&events);

    let (base_url, server) = serve_requests(vec![
        response(
            "model.bin",
            Some("bytes=2-"),
            "416 Range Not Satisfiable",
            b"",
            Some("bytes */4"),
        ),
        response("model.bin", None, "200 OK", b"ABCD", None),
    ])
    .await;
    engine
        .download_model_detailed_from_source(
            TEST_MODEL_NAME,
            &model_dir,
            &base_url,
            ARTIFACTS,
            Some(Box::new(move |progress| {
                callback_events.push(progress);
            })),
        )
        .await
        .expect("416 should retry without Range");
    server.await.expect("join 416 server");

    assert_eq!(
        fs::read(model_dir.join("model.bin")).await.unwrap(),
        b"ABCD"
    );
    assert!(std::iter::from_fn(|| events.pop())
        .all(|progress| progress.downloaded_bytes <= progress.total_bytes));
}

#[tokio::test]
async fn invalid_or_short_response_never_publishes_available() {
    const ARTIFACTS: &[ArtifactSpec] = &[
        ArtifactSpec {
            filename: "complete.bin",
            exact_bytes: 1,
            sha256: None,
        },
        ArtifactSpec {
            filename: "target.bin",
            exact_bytes: 4,
            sha256: None,
        },
    ];
    let cases = vec![
        (
            "malformed",
            ExpectedResponse {
                filename: "target.bin",
                range: Some("bytes=1-"),
                status: "206 Partial Content",
                content_length: Some(3),
                content_range: Some("bytes invalid"),
                body: b"XYZ",
                release_after_body: None,
            },
        ),
        (
            "wrong-start",
            ExpectedResponse {
                filename: "target.bin",
                range: Some("bytes=1-"),
                status: "206 Partial Content",
                content_length: Some(4),
                content_range: Some("bytes 0-3/4"),
                body: b"ABCD",
                release_after_body: None,
            },
        ),
        (
            "wrong-total",
            ExpectedResponse {
                filename: "target.bin",
                range: Some("bytes=1-"),
                status: "206 Partial Content",
                content_length: Some(3),
                content_range: Some("bytes 1-3/5"),
                body: b"XYZ",
                release_after_body: None,
            },
        ),
        (
            "short",
            ExpectedResponse {
                filename: "target.bin",
                range: Some("bytes=1-"),
                status: "200 OK",
                content_length: Some(4),
                content_range: None,
                body: b"ABC",
                release_after_body: None,
            },
        ),
        (
            "overlong",
            ExpectedResponse {
                filename: "target.bin",
                range: Some("bytes=1-"),
                status: "200 OK",
                content_length: Some(5),
                content_range: None,
                body: b"ABCDE",
                release_after_body: None,
            },
        ),
    ];

    for (case_name, invalid_response) in cases {
        let (_temp_dir, engine, model_dir) = test_engine().await;
        fs::create_dir_all(&model_dir)
            .await
            .expect("create model directory");
        fs::write(model_dir.join("complete.bin"), b"C")
            .await
            .expect("seed completed sibling");
        fs::write(model_dir.join("target.bin"), b"Z")
            .await
            .expect("seed retained prefix");

        let (base_url, server) = serve_requests(vec![invalid_response]).await;
        assert!(
            engine
                .download_model_detailed_from_source(
                    TEST_MODEL_NAME,
                    &model_dir,
                    &base_url,
                    ARTIFACTS,
                    None,
                )
                .await
                .is_err(),
            "{case_name} response must fail"
        );
        server.await.expect("join invalid-response server");
        assert_eq!(
            fs::read(model_dir.join("complete.bin")).await.unwrap(),
            b"C"
        );
        assert!(!matches!(
            test_model_status(&engine).await,
            ModelStatus::Available
        ));
        assert!(!engine
            .active_downloads
            .lock()
            .await
            .downloads
            .contains_key(TEST_MODEL_NAME));
    }
}

#[tokio::test]
async fn pending_cancellation_keeps_owner_and_blocks_retry() {
    let (_temp_dir, engine, model_dir) = test_engine().await;
    fs::create_dir_all(&model_dir)
        .await
        .expect("create model directory");
    fs::write(model_dir.join("encoder.bin"), b"AB")
        .await
        .expect("seed resumable prefix");
    let owner = engine
        .reserve_active_download(TEST_MODEL_NAME)
        .await
        .expect("reserve initial owner");

    assert_eq!(
        engine
            .cancel_download_with_timeout(TEST_MODEL_NAME, Duration::from_millis(1))
            .await
            .expect("request cancellation"),
        CancelDownloadOutcome::Pending
    );
    assert!(engine
        .reserve_active_download(TEST_MODEL_NAME)
        .await
        .is_err());

    let error = engine
        .finish_download(
            TEST_MODEL_NAME,
            &model_dir,
            SMALL_ARTIFACTS,
            &owner,
            Err(DownloadCancelled.into()),
            None,
        )
        .await
        .expect_err("cancelled owner must finish as cancellation");
    assert!(is_download_cancelled(&error));
    assert_eq!(
        fs::read(model_dir.join("encoder.bin")).await.unwrap(),
        b"AB"
    );
    assert!(!engine
        .active_downloads
        .lock()
        .await
        .downloads
        .contains_key(TEST_MODEL_NAME));
    assert!(engine
        .reserve_active_download(TEST_MODEL_NAME)
        .await
        .is_ok());
}

#[tokio::test]
async fn cancellation_wins_before_terminal_commit() {
    const ARTIFACTS: &[ArtifactSpec] = &[ArtifactSpec {
        filename: "model.bin",
        exact_bytes: 4,
        sha256: None,
    }];
    let (_temp_dir, engine, model_dir) = test_engine().await;
    let hook = Arc::new(DownloadStateTestHook {
        finalization_ready: tokio::sync::Notify::new(),
        continue_finalization: tokio::sync::Notify::new(),
        discovery_scanned: tokio::sync::Notify::new(),
        continue_discovery: tokio::sync::Notify::new(),
    });
    *engine.download_state_test_hook.lock().await = Some(Arc::clone(&hook));
    let events = Arc::new(SegQueue::new());
    let callback_events = Arc::clone(&events);

    let (base_url, server) =
        serve_requests(vec![response("model.bin", None, "200 OK", b"ABCD", None)]).await;
    let download_engine = Arc::clone(&engine);
    let download_dir = model_dir.clone();
    let download = tokio::spawn(async move {
        download_engine
            .download_model_detailed_from_source(
                TEST_MODEL_NAME,
                &download_dir,
                &base_url,
                ARTIFACTS,
                Some(Box::new(move |progress| {
                    callback_events.push(progress);
                })),
            )
            .await
    });

    hook.finalization_ready.notified().await;
    assert_eq!(
        engine
            .cancel_download_with_timeout(TEST_MODEL_NAME, Duration::from_millis(1))
            .await
            .expect("request cancellation while finalization is paused"),
        CancelDownloadOutcome::Pending
    );
    hook.continue_finalization.notify_one();

    let error = download
        .await
        .expect("join download task")
        .expect_err("cancellation must win before terminal commit");
    server.await.expect("join cancellation server");
    assert!(is_download_cancelled(&error));
    assert!(std::iter::from_fn(|| events.pop()).all(|progress| progress.percent < 100));
    assert!(matches!(
        test_model_status(&engine).await,
        ModelStatus::Missing
    ));
    assert!(!engine
        .active_downloads
        .lock()
        .await
        .downloads
        .contains_key(TEST_MODEL_NAME));
}

#[tokio::test]
async fn discovery_retries_when_download_finalizes_after_disk_scan() {
    let (_temp_dir, engine, model_dir) = test_engine().await;
    let hook = Arc::new(DownloadStateTestHook {
        finalization_ready: tokio::sync::Notify::new(),
        continue_finalization: tokio::sync::Notify::new(),
        discovery_scanned: tokio::sync::Notify::new(),
        continue_discovery: tokio::sync::Notify::new(),
    });
    *engine.download_state_test_hook.lock().await = Some(Arc::clone(&hook));

    let discovery_engine = Arc::clone(&engine);
    let discovery = tokio::spawn(async move {
        discovery_engine
            .discover_models_from_specs(SMALL_MODEL_SPECS)
            .await
    });
    tokio::time::timeout(Duration::from_secs(1), hook.discovery_scanned.notified())
        .await
        .expect("discovery must finish its first disk scan");

    fs::create_dir_all(&model_dir)
        .await
        .expect("create model directory");
    for artifact in SMALL_ARTIFACTS {
        fs::write(
            model_dir.join(artifact.filename),
            vec![0; artifact.exact_bytes as usize],
        )
        .await
        .expect("seed exact artifact");
    }

    let owner = engine
        .reserve_active_download(TEST_MODEL_NAME)
        .await
        .expect("reserve download owner");
    let finalization_engine = Arc::clone(&engine);
    let finalization_dir = model_dir.clone();
    let finalization_owner = Arc::clone(&owner);
    let finalization = tokio::spawn(async move {
        finalization_engine
            .finish_download(
                TEST_MODEL_NAME,
                &finalization_dir,
                SMALL_ARTIFACTS,
                &finalization_owner,
                Ok(DownloadProgress::new(10, 10, 0.0)),
                None,
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(1), hook.finalization_ready.notified())
        .await
        .expect("finalization must reach its commit barrier");
    hook.continue_finalization.notify_one();
    tokio::time::timeout(Duration::from_secs(1), finalization)
        .await
        .expect("finalization must complete")
        .expect("join finalization task")
        .expect("successful download must finalize");

    *engine.download_state_test_hook.lock().await = None;
    hook.continue_discovery.notify_one();
    let discovered = tokio::time::timeout(Duration::from_secs(1), discovery)
        .await
        .expect("discovery must complete")
        .expect("join discovery task")
        .expect("discovery must succeed");

    let discovered_model = discovered
        .iter()
        .find(|model| model.name == TEST_MODEL_NAME)
        .expect("test model must be discovered");
    assert!(matches!(discovered_model.status, ModelStatus::Available));
    assert!(matches!(
        test_model_status(&engine).await,
        ModelStatus::Available
    ));
    assert!(!engine
        .active_downloads
        .lock()
        .await
        .downloads
        .contains_key(TEST_MODEL_NAME));
}
