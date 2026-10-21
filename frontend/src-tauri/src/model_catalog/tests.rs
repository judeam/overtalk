use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Clone, Debug, Deserialize, PartialEq)]
struct Model {
    id: String,
}
fn fallback() -> Vec<Model> {
    vec![Model {
        id: "fallback".into(),
    }]
}
fn accept(model: &Model) -> bool {
    model.id.starts_with("chat-")
}

async fn server(
    responses: Vec<(&'static str, String, &'static str)>,
) -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/models", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        for (status, body, credential) in responses {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                stream.read_exact(&mut byte).await.unwrap();
                request.push(byte[0]);
                assert!(request.len() < 16_384);
            }
            let request = String::from_utf8(request).unwrap().to_lowercase();
            assert!(request.contains(credential));
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        }
    });
    (endpoint, task)
}

#[tokio::test]
async fn cache_is_scoped_to_normalized_credential_and_expires() {
    let (url, task) = server(vec![
        (
            "200 OK",
            r#"{"data":[{"id":"chat-first"},{"id":"embedding"}]}"#.into(),
            "authorization: bearer first",
        ),
        (
            "200 OK",
            r#"{"data":[{"id":"chat-second"}]}"#.into(),
            "authorization: bearer second",
        ),
        (
            "200 OK",
            r#"{"data":[{"id":"chat-refreshed"}]}"#.into(),
            "authorization: bearer second",
        ),
    ])
    .await;
    let catalog = ModelCatalog::new("test", &url, CatalogAuth::Bearer);
    assert_eq!(
        catalog
            .get(Some(" first ".into()), fallback, accept)
            .await
            .unwrap()[0]
            .id,
        "chat-first"
    );
    assert_eq!(
        catalog
            .get(Some("first".into()), fallback, accept)
            .await
            .unwrap()[0]
            .id,
        "chat-first"
    );
    assert_eq!(
        catalog.get(None, fallback, accept).await.unwrap(),
        fallback()
    );
    assert_eq!(
        catalog
            .get(Some("second".into()), fallback, accept)
            .await
            .unwrap()[0]
            .id,
        "chat-second"
    );
    catalog.cache.write().unwrap().as_mut().unwrap().fetched_at = Instant::now() - CACHE_TTL;
    assert_eq!(
        catalog
            .get(Some("second".into()), fallback, accept)
            .await
            .unwrap()[0]
            .id,
        "chat-refreshed"
    );
    task.await.unwrap();
}

#[tokio::test]
async fn failures_and_empty_filtered_catalogs_do_not_poison_cache() {
    let (url, task) = server(vec![
        ("401 Unauthorized", "{}".into(), "x-api-key: first"),
        (
            "200 OK",
            "invalid JSON".into(),
            "anthropic-version: 2023-06-01",
        ),
        (
            "200 OK",
            r#"{"data":[{"id":"embedding"}]}"#.into(),
            "x-api-key: first",
        ),
        (
            "200 OK",
            r#"{"data":[{"id":"chat-recovered"}]}"#.into(),
            "x-api-key: first",
        ),
    ])
    .await;
    let catalog = ModelCatalog::new("test", &url, CatalogAuth::Anthropic);
    for _ in 0..3 {
        assert_eq!(
            catalog
                .get(Some("first".into()), fallback, accept)
                .await
                .unwrap(),
            fallback()
        );
    }
    assert_eq!(
        catalog
            .get(Some("first".into()), fallback, accept)
            .await
            .unwrap()[0]
            .id,
        "chat-recovered"
    );
    task.await.unwrap();
}

#[tokio::test]
async fn oversized_response_is_rejected_without_caching() {
    let (url, task) = server(vec![(
        "200 OK",
        " ".repeat(2 * 1024 * 1024 + 1),
        "authorization: bearer first",
    )])
    .await;
    let catalog = ModelCatalog::new("test", &url, CatalogAuth::Bearer);
    let result = catalog.get(Some("first".into()), fallback, accept).await;
    assert_eq!(
        result.unwrap_err(),
        "Provider response exceeds the size limit"
    );
    // The client may close while the oversized fixture is still being written.
    task.abort();
    assert!(catalog.cache.read().unwrap().is_none());
}

#[test]
fn public_provider_dtos_ignore_unneeded_api_metadata() {
    let openai: crate::openai::client::OpenAIModel =
        serde_json::from_str(r#"{"id":"gpt-test","object":"model","owned_by":"organization"}"#)
            .unwrap();
    assert_eq!(openai.id, "gpt-test");
    let groq: crate::groq::client::GroqModel =
        serde_json::from_str(r#"{"id":"chat-test"}"#).unwrap();
    assert!(groq.owned_by.is_none());
    let anthropic: crate::anthropic::client::AnthropicModel = serde_json::from_str(
        r#"{"id":"claude-test","display_name":"Claude","created_at":"2026-10-19"}"#,
    )
    .unwrap();
    assert_eq!(anthropic.display_name.as_deref(), Some("Claude"));
}
