async fn response_canary(response: reqwest::Response) {
    // ruleid: unbounded-provider-response
    let _ = response.json::<serde_json::Value>();
    // ruleid: unbounded-provider-response
    let _ = response.text();
    // ok: unbounded-provider-response
    let _ = crate::security::transport::response_text(response, 4096).await;
}
fn transport_canary(client: reqwest::ClientBuilder) {
    // ruleid: disabled-tls-validation
    let _ = client.danger_accept_invalid_certs(true);
    // ok: disabled-tls-validation
    let _ = client.danger_accept_invalid_certs(false);
}
