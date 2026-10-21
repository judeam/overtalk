use super::*;
use serde_json::json;
use std::{cell::Cell, task::Poll};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::oneshot,
    time::{timeout, Duration},
};

#[test]
fn flash_defaults_to_greedy_sampling_without_changing_other_models() {
    for model in ["glm-5.3-flash", "glm-flash-latest"] {
        let body = build_openai_compat_chat_body(
            &LLMProvider::Privatemode,
            model,
            "s",
            "u",
            None,
            None,
            None,
        );
        assert_eq!(body["temperature"], json!(0.0));
        let explicit = build_openai_compat_chat_body(
            &LLMProvider::Privatemode,
            model,
            "s",
            "u",
            None,
            Some(0.4),
            None,
        );
        assert_eq!(explicit["temperature"], json!(0.4_f32));
    }
    for provider in [
        LLMProvider::Privatemode,
        LLMProvider::BuiltInAI,
        LLMProvider::Ollama,
    ] {
        for model in ["glm-5.3", "gpt-oss-120b", "qwen3.5:4b"] {
            let body = build_openai_compat_chat_body(&provider, model, "s", "u", None, None, None);
            assert!(body.get("temperature").is_none());
        }
    }
}

async fn read_http_request(stream: &mut tokio::net::TcpStream) -> Vec<u8> {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 1024];

    loop {
        let bytes_read = stream.read(&mut buffer).await.unwrap();
        assert_ne!(bytes_read, 0, "connection closed before completing request");
        request.extend_from_slice(&buffer[..bytes_read]);

        let Some(headers_end) = request
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .map(|position| position + 4)
        else {
            continue;
        };
        let content_length = std::str::from_utf8(&request[..headers_end])
            .unwrap()
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().unwrap())
            })
            .expect("request should have Content-Length");
        if request.len() >= headers_end + content_length {
            return request;
        }
    }
}

fn request_json(request: &[u8]) -> serde_json::Value {
    let headers_end = request
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|position| position + 4)
        .expect("request should contain complete headers");
    serde_json::from_slice(&request[headers_end..]).expect("request body should be valid JSON")
}

async fn synthetic_reply(
    status: &str,
    body: String,
    chunked: bool,
) -> Result<LlmCompletion, String> {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let status = status.to_owned();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        read_http_request(&mut stream).await;
        let response = if chunked {
            format!("HTTP/1.1 {status}\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n{body}\r\n0\r\n\r\n", body.len())
        } else {
            format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
        };
        // Early rejection can close the connection before the entire reply is sent.
        let _ = stream.write_all(response.as_bytes()).await;
    });
    let result = generate_summary(crate::summary::llm_client::SummaryRequest {
        provider: &LLMProvider::CustomOpenAI,
        model_name: "synthetic",
        api_key: "synthetic-key",
        system_prompt: "system",
        user_prompt: "synthetic transcript",
        ollama_endpoint: None,
        custom_openai_endpoint: Some(&format!("http://{address}")),
        max_tokens: None,
        temperature: None,
        top_p: None,
        app_data_dir: None,
        cancellation_token: None,
    })
    .await;
    server.await.unwrap();
    result
}

#[tokio::test]
async fn provider_errors_do_not_echo_private_response_content() {
    for (status, body) in [
        ("400 Bad Request", "private-transcript-and-key"),
        ("200 OK", r#"{"choices":"private-transcript-and-key"}"#),
    ] {
        let error = synthetic_reply(status, body.into(), false)
            .await
            .unwrap_err();
        assert!(
            !error.contains("private-transcript-and-key"),
            "response body escaped: {error}"
        );
    }
}

#[tokio::test]
async fn summary_response_budget_includes_chunked_and_ignored_fields() {
    for chunked in [false, true] {
        let body = format!(
            r#"{{"ignored":"{}","choices":[{{"message":{{"content":"Notes"}}}}]}}"#,
            "x".repeat(8 * 1024 * 1024)
        );
        let result = synthetic_reply("200 OK", body, chunked).await;
        assert!(
            result.is_err(),
            "accepted oversized provider response (chunked={chunked})"
        );
    }
}

#[tokio::test]
async fn privatemode_uses_local_proxy_contract_and_hides_reasoning() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let request = read_http_request(&mut stream).await;
        let headers = String::from_utf8_lossy(&request).to_lowercase();
        assert!(headers.starts_with("post /v1/chat/completions "));
        assert!(headers.contains("authorization: bearer placeholder"));
        let body = request_json(&request);
        assert_eq!(body["model"], "glm-5.3");
        assert_eq!(body["reasoning_effort"], "low");
        assert_eq!(body["max_completion_tokens"], 1234);
        assert!(body.get("max_tokens").is_none());
        let body = r###"{"choices":[{"message":{"content":"## Notes\nA decision.","reasoning":"private thoughts"},"finish_reason":"stop"}]}"###;
        stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
    });
    let result = generate_summary(crate::summary::llm_client::SummaryRequest {
        provider: &LLMProvider::Privatemode,
        model_name: "glm-5.3",
        api_key: "",
        system_prompt: "system",
        user_prompt: "synthetic transcript",
        ollama_endpoint: Some(&format!("http://{address}/v1")),
        custom_openai_endpoint: None,
        max_tokens: Some(1234),
        temperature: None,
        top_p: None,
        app_data_dir: None,
        cancellation_token: None,
    })
    .await
    .unwrap();
    server.await.unwrap();
    assert_eq!(result.content, "## Notes\nA decision.");
    assert!(result.reasoning_stripped);
}

#[tokio::test]
async fn source_audit_uses_high_reasoning_with_a_bounded_output() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let request = read_http_request(&mut stream).await;
        let body = request_json(&request);
        assert_eq!(body["reasoning_effort"], "high");
        assert_eq!(body["max_completion_tokens"], 4096);
        let response =
            r#"{"choices":[{"message":{"content":"{\"edits\":[]}"},"finish_reason":"stop"}]}"#;
        stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).as_bytes()).await.unwrap();
    });
    let result = generate_source_audit(crate::summary::llm_client::SummaryRequest {
        provider: &LLMProvider::Privatemode,
        model_name: "glm-5.3-flash",
        api_key: "",
        system_prompt: "audit",
        user_prompt: "synthetic transcript",
        ollama_endpoint: Some(&format!("http://{address}/v1")),
        custom_openai_endpoint: None,
        max_tokens: Some(4096),
        temperature: Some(0.0),
        top_p: None,
        app_data_dir: None,
        cancellation_token: None,
    })
    .await
    .unwrap();
    assert_eq!(result.content, r#"{"edits":[]}"#);
    server.await.unwrap();
}

#[tokio::test]
async fn privatemode_blocks_redirects_and_truncated_completions() {
    for (status, body, extra, expected) in [
        (
            "307 Temporary Redirect",
            "secret echoed by proxy",
            "Location: https://example.com/steal\r\n",
            "redirect",
        ),
        (
            "200 OK",
            r#"{"choices":[{"message":{"content":"partial"},"finish_reason":"length"}]}"#,
            "",
            "output limit",
        ),
        (
            "401 Unauthorized",
            "secret echoed by proxy",
            "",
            "authentication",
        ),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            read_http_request(&mut stream).await;
            stream.write_all(format!("HTTP/1.1 {status}\r\n{extra}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
        });
        let error = generate_summary(crate::summary::llm_client::SummaryRequest {
            provider: &LLMProvider::Privatemode,
            model_name: "gpt-oss-120b",
            api_key: "synthetic-key",
            system_prompt: "system",
            user_prompt: "user",
            ollama_endpoint: Some(&format!("http://{address}")),
            custom_openai_endpoint: None,
            max_tokens: None,
            temperature: None,
            top_p: None,
            app_data_dir: None,
            cancellation_token: None,
        })
        .await
        .unwrap_err();
        server.await.unwrap();
        assert!(error.contains(expected), "{error}");
        assert!(!error.contains("secret echoed"));
    }
}

#[test]
fn only_ollama_disables_reasoning_and_custom_sampling_is_preserved() {
    for provider in [
        LLMProvider::OpenAI,
        LLMProvider::Groq,
        LLMProvider::OpenRouter,
        LLMProvider::CustomOpenAI,
    ] {
        assert!(
            build_openai_compat_chat_body(&provider, "model", "sys", "user", None, None, None)
                .get("reasoning_effort")
                .is_none()
        );
    }
    let ollama = build_openai_compat_chat_body(
        &LLMProvider::Ollama,
        "model",
        "sys",
        "user",
        None,
        None,
        None,
    );
    assert_eq!(ollama["reasoning_effort"], "none");
    let custom = build_openai_compat_chat_body(
        &LLMProvider::CustomOpenAI,
        "model",
        "sys",
        "user",
        Some(12),
        Some(0.3),
        Some(0.8),
    );
    assert_eq!(custom["max_tokens"], 12);
    assert_eq!(custom["temperature"].as_f64(), Some(0.3_f32 as f64));
    assert_eq!(custom["top_p"].as_f64(), Some(0.8_f32 as f64));
}

#[test]
fn compatible_reasoning_is_separate_from_visible_content() {
    for message in [
        json!({"reasoning_content": "private"}),
        json!({"content": null, "reasoning_content": "private"}),
    ] {
        let response: ChatResponse = serde_json::from_value(json!({
            "choices": [{"message": message}]
        }))
        .unwrap();
        assert_eq!(
            response.completion().unwrap(),
            LlmCompletion {
                content: String::new(),
                reasoning_stripped: true,
            }
        );
    }
}

#[test]
fn legacy_ollama_rejection_requires_compatible_error() {
    assert!(ollama_rejects_reasoning_effort(
        reqwest::StatusCode::BAD_REQUEST,
        r#"{"error":{"param":"reasoning_effort"}}"#,
    ));
    assert!(ollama_rejects_reasoning_effort(
        reqwest::StatusCode::UNPROCESSABLE_ENTITY,
        r#"{"message":"Unsupported parameter 'ReAsOnInG_EfFoRt'."}"#,
    ));
    assert!(ollama_rejects_reasoning_effort(
        reqwest::StatusCode::BAD_REQUEST,
        r#"{"error":"think value \"none\" is not supported for this model"}"#,
    ));
    assert!(!ollama_rejects_reasoning_effort(
        reqwest::StatusCode::BAD_REQUEST,
        r#"{"message":"invalid model"}"#,
    ));
    assert!(!ollama_rejects_reasoning_effort(
        reqwest::StatusCode::UNAUTHORIZED,
        r#"{"error":{"param":"reasoning_effort"}}"#,
    ));
    assert!(!ollama_rejects_reasoning_effort(
        reqwest::StatusCode::INTERNAL_SERVER_ERROR,
        r#"{"error":"think value \"none\" is not supported"}"#,
    ));
}

#[tokio::test]
async fn cancellation_prevents_compatibility_io_from_being_polled() {
    let cancellation_token = CancellationToken::new();
    cancellation_token.cancel();
    let polled = Cell::new(false);

    let result = await_or_cancel(
        std::future::poll_fn(|_| {
            polled.set(true);
            Poll::<()>::Pending
        }),
        Some(&cancellation_token),
    )
    .await;

    assert_eq!(result, Err("Summary generation was cancelled".to_string()));
    assert!(!polled.get());
}

#[tokio::test]
async fn cancellation_during_failed_legacy_ollama_retry_body_returns_promptly() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (retry_headers_sent, mut retry_headers_ready) = oneshot::channel();
    let (release_retry_body, retry_body_released) = oneshot::channel();

    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let request = read_http_request(&mut stream).await;
        let body = request_json(&request);
        assert_eq!(body["reasoning_effort"], "none");

        let rejection_body = br#"{"error":{"param":"reasoning_effort"}}"#;
        let rejection_headers = format!(
            "HTTP/1.1 400 Bad Request\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            rejection_body.len()
        );
        stream
            .write_all(rejection_headers.as_bytes())
            .await
            .unwrap();
        stream.write_all(rejection_body).await.unwrap();
        stream.flush().await.unwrap();
        drop(stream);

        let (mut stream, _) = listener.accept().await.unwrap();
        let request = read_http_request(&mut stream).await;
        let body = request_json(&request);
        assert!(body.get("reasoning_effort").is_none());

        stream
                .write_all(
                    b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 4\r\nConnection: close\r\n\r\n",
                )
                .await
                .unwrap();
        stream.flush().await.unwrap();
        let _ = retry_headers_sent.send(());
        let _ = retry_body_released.await;
        let _ = stream.write_all(b"fail").await;
    });

    let cancellation_token = CancellationToken::new();
    let endpoint = format!("http://{address}");
    let generation = generate_summary(crate::summary::llm_client::SummaryRequest {
        provider: &LLMProvider::Ollama,
        model_name: "model",
        api_key: "",
        system_prompt: "system",
        user_prompt: "user",
        ollama_endpoint: Some(&endpoint),
        custom_openai_endpoint: None,
        max_tokens: None,
        temperature: None,
        top_p: None,
        app_data_dir: None,
        cancellation_token: Some(&cancellation_token),
    });
    tokio::pin!(generation);

    tokio::select! {
        biased;
        result = &mut generation => panic!("generation ended before retry headers: {result:?}"),
        _ = &mut retry_headers_ready => {}
    }

    cancellation_token.cancel();
    let completion = timeout(Duration::from_secs(1), &mut generation).await;
    let _ = release_retry_body.send(());
    let released_completion = if completion.is_err() {
        Some(timeout(Duration::from_secs(1), &mut generation).await)
    } else {
        None
    };
    server.await.unwrap();

    let result = completion.unwrap_or_else(|_| {
        released_completion
            .expect("generation should be awaited after releasing the retry body")
            .expect("generation should finish after releasing the retry body")
    });
    assert_eq!(result, Err("Summary generation was cancelled".to_string()));
}

#[tokio::test]
async fn cancellation_during_successful_legacy_ollama_retry_body_returns_promptly() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (retry_headers_sent, mut retry_headers_ready) = oneshot::channel();
    let (release_retry_body, retry_body_released) = oneshot::channel();

    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let request = read_http_request(&mut stream).await;
        let body = request_json(&request);
        assert_eq!(body["reasoning_effort"], "none");

        let rejection_body = br#"{"error":{"param":"reasoning_effort"}}"#;
        let rejection_headers = format!(
            "HTTP/1.1 400 Bad Request\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            rejection_body.len()
        );
        stream
            .write_all(rejection_headers.as_bytes())
            .await
            .unwrap();
        stream.write_all(rejection_body).await.unwrap();
        stream.flush().await.unwrap();
        drop(stream);

        let (mut stream, _) = listener.accept().await.unwrap();
        let request = read_http_request(&mut stream).await;
        let body = request_json(&request);
        assert!(body.get("reasoning_effort").is_none());

        let completion_body = br#"{"choices":[{"message":{"content":"Meeting summary."}}]}"#;
        let completion_headers = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            completion_body.len()
        );
        stream
            .write_all(completion_headers.as_bytes())
            .await
            .unwrap();
        stream.flush().await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        let _ = retry_headers_sent.send(());
        let _ = retry_body_released.await;
        let _ = stream.write_all(completion_body).await;
    });

    let cancellation_token = CancellationToken::new();
    let endpoint = format!("http://{address}");
    let generation = generate_summary(crate::summary::llm_client::SummaryRequest {
        provider: &LLMProvider::Ollama,
        model_name: "model",
        api_key: "",
        system_prompt: "system",
        user_prompt: "user",
        ollama_endpoint: Some(&endpoint),
        custom_openai_endpoint: None,
        max_tokens: None,
        temperature: None,
        top_p: None,
        app_data_dir: None,
        cancellation_token: Some(&cancellation_token),
    });
    tokio::pin!(generation);

    tokio::select! {
        biased;
        result = &mut generation => panic!("generation ended before retry headers: {result:?}"),
        _ = &mut retry_headers_ready => {}
    }

    cancellation_token.cancel();
    let completion = timeout(Duration::from_secs(1), &mut generation).await;
    let _ = release_retry_body.send(());
    if completion.is_err() {
        let _ = timeout(Duration::from_secs(1), &mut generation).await;
    }
    server.await.unwrap();

    let result =
        completion.expect("generation should observe cancellation before retry body release");
    assert_eq!(result, Err("Summary generation was cancelled".to_string()));
}

#[tokio::test]
async fn legacy_ollama_retry_without_reasoning_effort_returns_completion() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();

    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let request = read_http_request(&mut stream).await;
        let body = request_json(&request);
        assert_eq!(body["reasoning_effort"], "none");

        let rejection_body = br#"{"error":{"param":"reasoning_effort"}}"#;
        let rejection_headers = format!(
            "HTTP/1.1 400 Bad Request\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            rejection_body.len()
        );
        stream
            .write_all(rejection_headers.as_bytes())
            .await
            .unwrap();
        stream.write_all(rejection_body).await.unwrap();
        stream.flush().await.unwrap();
        drop(stream);

        let (mut stream, _) = listener.accept().await.unwrap();
        let request = read_http_request(&mut stream).await;
        let body = request_json(&request);
        assert!(body.get("reasoning_effort").is_none());

        let completion_body = br#"{"choices":[{"message":{"content":"Meeting summary."}}]}"#;
        let completion_headers = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            completion_body.len()
        );
        stream
            .write_all(completion_headers.as_bytes())
            .await
            .unwrap();
        stream.write_all(completion_body).await.unwrap();
        stream.flush().await.unwrap();
    });

    let cancellation_token = CancellationToken::new();
    let endpoint = format!("http://{address}");
    let completion = timeout(
        Duration::from_secs(1),
        generate_summary(crate::summary::llm_client::SummaryRequest {
            provider: &LLMProvider::Ollama,
            model_name: "model",
            api_key: "",
            system_prompt: "system",
            user_prompt: "user",
            ollama_endpoint: Some(&endpoint),
            custom_openai_endpoint: None,
            max_tokens: None,
            temperature: None,
            top_p: None,
            app_data_dir: None,
            cancellation_token: Some(&cancellation_token),
        }),
    )
    .await;
    server.await.unwrap();

    assert_eq!(
        completion
            .expect("generation should finish after the compatibility retry")
            .unwrap(),
        LlmCompletion {
            content: "Meeting summary.".to_string(),
            reasoning_stripped: false,
        }
    );
}

#[test]
fn claude_response_uses_exact_block_types() {
    let response: ClaudeChatResponse = serde_json::from_value(json!({
        "content": [
            {"type": "analysis", "text": "not visible"},
            {"type": "thinking", "thinking": "private"},
            {"type": "text", "text": "Meeting summary."}
        ]
    }))
    .unwrap();
    assert_eq!(
        response.completion(),
        Some(LlmCompletion {
            content: "Meeting summary.".to_string(),
            reasoning_stripped: true,
        })
    );
}
