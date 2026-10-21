use super::*;

use crate::config::test_config as disabled_config;

/// Requests built here carry a complete body, like the gateway's replies.
type Body = Full<Bytes>;

async fn bytes(response: Reply) -> Bytes {
    response.into_body().collect().await.unwrap().to_bytes()
}

#[tokio::test]
async fn missing_approval_fails_closed_without_disclosing_server_credentials() {
    let app = App::new(disabled_config(), Ledger::memory().await.unwrap()).unwrap();
    for route in ["/v1/accounts", "/v1/checkouts", "/v1/chat/completions"] {
        let response = app
            .handle(
                Request::post(route).body(Body::from("{}")).unwrap(),
                "127.0.0.1".parse().unwrap(),
            )
            .await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = bytes(response).await;
        let text = std::str::from_utf8(&body).unwrap();
        assert!(!text.contains("test-private"));
    }
}

#[tokio::test]
async fn an_account_credential_does_not_grant_credit_or_access_to_others() {
    let mut config = disabled_config();
    config.approved = true;
    let app = App::new(config, Ledger::memory().await.unwrap()).unwrap();
    let created = app
        .handle(
            Request::post("/v1/accounts").body(Body::default()).unwrap(),
            "127.0.0.1".parse().unwrap(),
        )
        .await;
    assert_eq!(created.status(), StatusCode::CREATED);
    assert_eq!(created.headers()["cache-control"], "no-store");
    let json: Value = serde_json::from_slice(&bytes(created).await).unwrap();
    let token = json["account_token"].as_str().unwrap();
    let request = Request::get("/v1/account")
        .header("authorization", format!("Bearer {token}"))
        .body(Body::default())
        .unwrap();
    let response = app.handle(request, "127.0.0.1".parse().unwrap()).await;
    let balance: Value = serde_json::from_slice(&bytes(response).await).unwrap();
    assert_eq!(balance["balance"]["available_micro_usd"], 0);
    let unauthorized = app
        .handle(
            Request::get("/v1/account").body(Body::default()).unwrap(),
            "127.0.0.1".parse().unwrap(),
        )
        .await;
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
}

#[test]
fn money_rounds_up_without_floating_point_and_request_limits_include_reasoning() {
    assert_eq!(cost(20_000, 5_000, 250_000, 800_000).unwrap(), 9_000);
    assert_eq!(cost(1, 0, 1, 0).unwrap(), 1);
    let valid = json!({"model":MODEL,"messages":[{"role":"user","content":"notes"}],"max_completion_tokens":1024});
    assert!(serde_json::from_value::<ChatInput>(valid.clone())
        .unwrap()
        .validate()
        .is_ok());
    for (field, value) in [
        ("model", json!("glm-5.3")),
        ("max_completion_tokens", json!(1_000_000)),
        ("stream", json!(true)),
        ("reasoning_effort", json!("none")),
    ] {
        let mut bad = valid.clone();
        bad[field] = value;
        assert!(serde_json::from_value::<ChatInput>(bad)
            .unwrap()
            .validate()
            .is_err());
    }
    let mut salt = valid;
    salt["cache_salt"] = json!("steal-another-cache");
    assert!(serde_json::from_value::<ChatInput>(salt).is_err());
}

async fn mock_inference(
    usage: bool,
) -> (
    url::Url,
    tokio::task::JoinHandle<()>,
    Arc<std::sync::atomic::AtomicUsize>,
) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = calls.clone();
    let handler = move |req: Request<Incoming>, _| {
        let count = count.clone();
        async move {
            assert_eq!(
                req.headers()["authorization"],
                "Bearer test-private-provider-key"
            );
            let body = req.into_body().collect().await.unwrap().to_bytes();
            let value: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(value["model"], MODEL);
            assert_eq!(value["cache_salt"].as_str().unwrap().len(), 43);
            count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let mut value = json!({"choices":[{"message":{"role":"assistant","content":"Test notes"},"finish_reason":"stop"}]});
            if usage {
                value["usage"] =
                    json!({"prompt_tokens":20,"completion_tokens":10,"total_tokens":30});
            }
            Response::new(Body::from(value.to_string()))
        }
    };
    listener.set_nonblocking(true).unwrap();
    let listener = TcpListener::from_std(listener).unwrap();
    let task = tokio::spawn(serve(listener, std::future::pending(), handler));
    (
        format!("http://{address}/v1/").parse().unwrap(),
        task,
        calls,
    )
}

async fn funded_inference(
    usage: bool,
) -> (
    App,
    String,
    tokio::task::JoinHandle<()>,
    Arc<std::sync::atomic::AtomicUsize>,
) {
    let (proxy, task, calls) = mock_inference(usage).await;
    let mut config = disabled_config();
    config.approved = true;
    config.proxy = proxy;
    let db = Ledger::memory().await.unwrap();
    let token = crate::secret().unwrap();
    let account = db.create_account(&token, crate::now()).await.unwrap();
    db.grant(crate::ledger::Grant {
        source: "cs_purchase",
        account: &account,
        cycle: None,
        payment: "pi_purchase",
        units: 2_000_000,
        paid_cents: 1000,
        refunded_cents: 0,
        disputed: false,
        starts: 0,
        expires: None,
    })
    .await
    .unwrap();
    (App::new(config, db).unwrap(), token, task, calls)
}

fn inference_request(token: &str, id: &str) -> Request<Body> {
    Request::post("/v1/chat/completions").header("authorization",format!("Bearer {token}")).header("idempotency-key",id)
        .body(Body::from(json!({"model":MODEL,"messages":[{"role":"user","content":"Synthetic transcript"}],"max_completion_tokens":1024}).to_string())).unwrap()
}

#[tokio::test]
async fn the_gateway_spends_only_verified_usage_and_never_forwards_customer_credentials() {
    let (app, token, task, calls) = funded_inference(true).await;
    let response = app
        .handle(
            inference_request(&token, "request-1"),
            "127.0.0.1".parse().unwrap(),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let account = app.db.authenticate(&token).await.unwrap();
    let balance = app.db.balance(&account, crate::now()).await.unwrap();
    assert_eq!(
        balance.available_micro_usd,
        2_000_000 - cost(20, 10, 250_000, 800_000).unwrap()
    );
    assert_eq!(balance.held_micro_usd, 0);
    assert_eq!(
        app.handle(
            inference_request(&token, "request-1"),
            "127.0.0.1".parse().unwrap()
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    task.abort();
}

#[tokio::test]
async fn unknown_usage_remains_held_and_unauthenticated_calls_never_reach_the_provider() {
    let (app, token, task, calls) = funded_inference(false).await;
    assert_eq!(
        app.handle(
            inference_request("invalid", "bad"),
            "127.0.0.1".parse().unwrap()
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    let response = app
        .handle(
            inference_request(&token, "request-1"),
            "127.0.0.1".parse().unwrap(),
        )
        .await;
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let account = app.db.authenticate(&token).await.unwrap();
    assert!(
        app.db
            .balance(&account, crate::now())
            .await
            .unwrap()
            .held_micro_usd
            > 0
    );
    assert_eq!(
        app.handle(
            inference_request(&token, "request-1"),
            "127.0.0.1".parse().unwrap()
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    task.abort();
}

#[tokio::test]
async fn the_end_to_end_route_gives_subscribers_only_their_own_key_and_never_takes_text() {
    let mut config = disabled_config();
    config.approved = true;
    config.route = crate::config::Route::EndToEnd;
    let app = App::new(config, Ledger::memory().await.unwrap()).unwrap();
    let ip: IpAddr = "127.0.0.1".parse().unwrap();
    let mut tokens = Vec::new();
    for _ in 0..2 {
        let created = app
            .handle(
                Request::post("/v1/accounts").body(Body::default()).unwrap(),
                ip,
            )
            .await;
        let json: Value = serde_json::from_slice(&bytes(created).await).unwrap();
        tokens.push(json["account_token"].as_str().unwrap().to_owned());
    }
    let call = |method: Method, path: &str, token: &str| {
        Request::builder()
            .method(method)
            .uri(path)
            .header("authorization", format!("Bearer {token}"))
            .body(Body::from("{}"))
            .unwrap()
    };
    let read = |response: Reply| async move {
        let status = response.status();
        let json: Value = serde_json::from_slice(&bytes(response).await).unwrap();
        (status, json)
    };
    // Without a paid period there is no key, and the account says so.
    let (status, account) = read(
        app.handle(call(Method::GET, "/v1/account", &tokens[0]), ip)
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        (account["route"].as_str(), account["entitled"].as_bool()),
        (Some("end-to-end"), Some(false))
    );
    let (status, _) = read(
        app.handle(call(Method::POST, "/v1/summaries/key", &tokens[0]), ip)
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    // A paid period but an empty pool: try later, without leaking anything.
    let first = app.db.authenticate(&tokens[0]).await.unwrap();
    app.db
        .entitle(&first, "INV-7", 1500, 0, crate::now() + 86400)
        .await
        .unwrap();
    let (status, _) = read(
        app.handle(call(Method::POST, "/v1/summaries/key", &tokens[0]), ip)
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    app.db
        .import_key(
            "pm-a",
            "pm_key_aaaaaaaaaaaaaaaa",
            crate::now() + 40 * 86400,
            crate::now(),
        )
        .await
        .unwrap();
    app.db
        .import_key(
            "pm-b",
            "pm_key_bbbbbbbbbbbbbbbb",
            crate::now() + 30 * 86400,
            crate::now(),
        )
        .await
        .unwrap();
    let (status, key) = read(
        app.handle(call(Method::POST, "/v1/summaries/key", &tokens[0]), ip)
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // Both keys outlive the one-day period; the shorter one overhangs it least.
    assert_eq!(key["key"], "pm_key_bbbbbbbbbbbbbbbb");
    assert_eq!(key["monthly_hours"], 200);
    // The other, unpaid account still gets nothing, not even the free key.
    let (status, other) = read(
        app.handle(call(Method::POST, "/v1/summaries/key", &tokens[1]), ip)
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(!other.to_string().contains("pm_key_"));
    // Meeting text is refused outright on this route.
    let (status, _) = read(
        app.handle(call(Method::POST, "/v1/chat/completions", &tokens[0]), ip)
            .await,
    )
    .await;
    assert_eq!(status, StatusCode::GONE);
    let (_, health) = read(
        app.handle(Request::get("/health").body(Body::default()).unwrap(), ip)
            .await,
    )
    .await;
    assert_eq!(health["route"], "end-to-end");
}
