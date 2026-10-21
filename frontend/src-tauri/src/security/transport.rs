use reqwest::Url;

pub fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        _ => false,
    }
}

/// Unencrypted HTTP is limited to a numeric loopback address or exact localhost.
/// Intranet names, link-local addresses and localhost-lookalikes are not loopback.
pub fn endpoint(input: &str) -> Result<Url, String> {
    let mut url = Url::parse(input.trim()).map_err(|_| "Enter a valid provider URL")?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.host_str().is_none()
    {
        return Err("Provider URLs cannot contain credentials, queries or fragments".into());
    }
    let local = match url.host() {
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        Some(url::Host::Domain("localhost")) => {
            url.set_host(Some("127.0.0.1"))
                .map_err(|_| "Invalid local URL")?;
            true
        }
        _ => false,
    };
    if url.scheme() != "https" && !(url.scheme() == "http" && local) {
        return Err("Remote providers require HTTPS. HTTP is allowed only on this device.".into());
    }
    Ok(url)
}

/// Do not forward a prompt to a redirect destination or a system HTTP proxy.
pub fn client() -> Result<reqwest::Client, String> {
    static CLIENT: std::sync::OnceLock<Result<reqwest::Client, String>> =
        std::sync::OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(std::time::Duration::from_secs(15))
                .build()
                .map_err(|_| "Could not initialize the secure provider connection".into())
        })
        .clone()
}

pub async fn response_text(
    mut response: reqwest::Response,
    limit: usize,
) -> Result<String, String> {
    if response
        .content_length()
        .is_some_and(|size| size > limit as u64)
    {
        return Err("Provider response exceeds the size limit".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "Provider response interrupted")?
    {
        if chunk.len() > limit.saturating_sub(bytes.len()) {
            return Err("Provider response exceeds the size limit".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    String::from_utf8(bytes).map_err(|_| "Provider returned invalid text".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn custom_api_key_headers_are_not_forwarded_to_redirects() {
        use tokio::{
            io::{AsyncReadExt, AsyncWriteExt},
            net::TcpListener,
        };
        let destination = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let source = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = source.local_addr().unwrap();
        let target = destination.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = source.accept().await.unwrap();
            let mut request = [0u8; 4096];
            let count = stream.read(&mut request).await.unwrap();
            assert!(
                String::from_utf8_lossy(&request[..count]).contains("x-api-key: synthetic-only")
            );
            stream.write_all(format!("HTTP/1.1 302 Found\r\nLocation: http://{target}/stolen\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
        });
        let response = client()
            .unwrap()
            .get(format!("http://{address}/models"))
            .header("x-api-key", "synthetic-only")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::FOUND);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), destination.accept())
                .await
                .is_err()
        );
        server.await.unwrap();
    }
    #[tokio::test]
    async fn oversized_provider_responses_are_rejected() {
        use tokio::{io::AsyncWriteExt, net::TcpListener};
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            crate::test_support::read_http_headers(&mut stream).await;
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\n0123456789",
                )
                .await
                .unwrap();
        });
        let response = client()
            .unwrap()
            .get(format!("http://{address}/models"))
            .send()
            .await
            .unwrap();
        assert!(response_text(response, 4).await.is_err());
        server.await.unwrap();
    }
    #[test]
    fn provider_transport_rejects_remote_cleartext_and_credential_urls() {
        for url in [
            "http://example.org/v1",
            "http://192.168.1.4:8000",
            "http://localhost.evil/v1",
            "http://127.0.0.1.evil/v1",
            "https://user:secret@example.org",
            "https://api.example.org/?key=secret",
            "file:///tmp/model",
            "http://169.254.169.254",
            "https://api.example.org/#key",
        ] {
            assert!(endpoint(url).is_err(), "accepted {url}");
        }
        for url in [
            "http://127.0.0.1:11434",
            "http://[::1]:8080/v1",
            "https://api.example.org/v1",
        ] {
            assert!(endpoint(url).is_ok());
        }
        assert_eq!(
            endpoint("http://localhost:8000").unwrap().host_str(),
            Some("127.0.0.1")
        );
    }
}
