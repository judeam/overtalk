//! Shared bounded protocol helpers for local mock servers; never production code.
use tokio::io::{AsyncRead, AsyncReadExt};

/// Wait for a complete HTTP header block, not an arbitrary first TCP packet.
pub(crate) async fn read_http_headers(stream: &mut (impl AsyncRead + Unpin)) {
    let mut headers = Vec::new();
    while !headers.ends_with(b"\r\n\r\n") {
        assert!(
            headers.len() < 16 * 1024,
            "Mock request headers exceeded their limit"
        );
        headers.push(
            stream
                .read_u8()
                .await
                .expect("Mock request ended before headers"),
        );
    }
}
