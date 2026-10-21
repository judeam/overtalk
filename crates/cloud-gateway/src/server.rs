use crate::{
    config::{Config, Route, MODEL, STRIPE_VERSION, SUMMARY_HOURS},
    ledger::{Error as LedgerError, Ledger},
    stripe::Stripe,
};
use anyhow::{ensure, Context};
use hyper::{body::HttpBody, Body, Method, Request, Response, StatusCode};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use std::{
    collections::HashMap,
    net::IpAddr,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, Semaphore};

pub struct App {
    pub config: Arc<Config>,
    pub db: Ledger,
    pub stripe: Stripe,
    client: reqwest::Client,
    requests: Semaphore,
    anonymous: Mutex<HashMap<String, (Instant, u32)>>,
}

impl App {
    pub fn new(config: Config, db: Ledger) -> anyhow::Result<Self> {
        let config = Arc::new(config);
        Ok(Self {
            stripe: Stripe::new(config.clone())?,
            config,
            db,
            client: reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(300))
                .build()?,
            requests: Semaphore::new(32),
            anonymous: Mutex::new(HashMap::new()),
        })
    }

    async fn allow(&self, key: String, limit: u32) -> bool {
        let now = Instant::now();
        let mut map = self.anonymous.lock().await;
        map.retain(|_, v| now.duration_since(v.0) < Duration::from_secs(300));
        if map.len() >= 10_000 && !map.contains_key(&key) {
            return false;
        }
        let entry = map.entry(key).or_insert((now, 0));
        entry.1 += 1;
        entry.1 <= limit
    }

    pub async fn handle(&self, mut request: Request<Body>, ip: IpAddr) -> Response<Body> {
        let Some(_permit) = self.requests.try_acquire().ok() else {
            return reply(
                StatusCode::TOO_MANY_REQUESTS,
                json!({"error":"Service busy"}),
            );
        };
        let path = request.uri().path().to_owned();
        let method = request.method().clone();
        if method == Method::GET && path == "/health" {
            return reply(
                StatusCode::OK,
                json!({"ready":self.config.approved,"model":MODEL,"route":self.route_name()}),
            );
        }
        if method == Method::GET
            && matches!(path.as_str(), "/checkout/complete" | "/checkout/cancelled")
        {
            return reply(
                StatusCode::OK,
                json!({
                    "message": "Return to Overtalk and refresh your cloud credits. Payment is \
                                confirmed only by the billing service."
                }),
            );
        }
        if !self.config.approved {
            return reply(
                StatusCode::SERVICE_UNAVAILABLE,
                json!({"error":"Bundled cloud plans are not available yet"}),
            );
        }
        let body = match tokio::time::timeout(
            Duration::from_secs(10),
            bounded_body(request.body_mut(), 512 * 1024),
        )
        .await
        {
            Ok(Ok(body)) => body,
            _ => {
                return reply(
                    StatusCode::PAYLOAD_TOO_LARGE,
                    json!({"error":"Invalid or oversized request"}),
                )
            }
        };
        if method == Method::POST && path == "/stripe/webhook" {
            let signature = request
                .headers()
                .get("stripe-signature")
                .and_then(|s| s.to_str().ok())
                .unwrap_or_default();
            if !crate::stripe::verify_signature(
                &body,
                signature,
                &self.config.webhook_secret,
                crate::now(),
            ) {
                return reply(
                    StatusCode::BAD_REQUEST,
                    json!({"error":"Invalid signature"}),
                );
            }
            let event: Value = match serde_json::from_slice(&body) {
                Ok(v) => v,
                Err(_) => return reply(StatusCode::BAD_REQUEST, json!({"error":"Invalid event"})),
            };
            if event["livemode"].as_bool() != Some(self.config.live)
                || event["api_version"] != STRIPE_VERSION
            {
                return reply(
                    StatusCode::BAD_REQUEST,
                    json!({"error":"Incorrect billing environment or API version"}),
                );
            }
            let Ok(id) = crate::stripe::id(&event) else {
                return reply(StatusCode::BAD_REQUEST, json!({"error":"Invalid event ID"}));
            };
            if !id.starts_with("evt_") {
                return reply(StatusCode::BAD_REQUEST, json!({"error":"Invalid event ID"}));
            }
            // Acknowledge only after durable insertion; no card, email or raw
            // event payload is retained. The worker retrieves signed events.
            return match sqlx::query("INSERT INTO events(id) VALUES(?) ON CONFLICT(id) DO NOTHING")
                .bind(id)
                .execute(&self.db.0)
                .await
            {
                Ok(_) => reply(StatusCode::OK, json!({"received":true})),
                Err(_) => reply(
                    StatusCode::SERVICE_UNAVAILABLE,
                    json!({"error":"Retry delivery"}),
                ),
            };
        }
        if method == Method::POST && path == "/v1/accounts" {
            if !self.allow(format!("ip:{ip}"), 20).await {
                return reply(
                    StatusCode::TOO_MANY_REQUESTS,
                    json!({"error":"Try again later"}),
                );
            }
            let result = async {
                let token = crate::secret()?;
                self.db.create_account(&token, crate::now()).await?;
                Ok::<_, anyhow::Error>(token)
            }
            .await;
            return match result {
                Ok(token) => reply(StatusCode::CREATED, json!({"account_token":token})),
                Err(_) => reply(
                    StatusCode::SERVICE_UNAVAILABLE,
                    json!({"error":"Account creation unavailable"}),
                ),
            };
        }
        let token = request
            .headers()
            .get("authorization")
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "))
            .unwrap_or_default();
        let account = match self.db.authenticate(token).await {
            Ok(account) => account,
            Err(error) => return ledger_error(error),
        };
        if !self.allow(format!("account:{account}"), 300).await {
            return reply(
                StatusCode::TOO_MANY_REQUESTS,
                json!({"error":"Please wait before making more requests"}),
            );
        }
        if path == "/v1/summaries/key" && !self.allow(format!("key:{account}"), 10).await {
            return reply(
                StatusCode::TOO_MANY_REQUESTS,
                json!({"error":"Please wait before asking for your summary key again"}),
            );
        }
        if path == "/v1/checkouts" && !self.allow(format!("checkout:{account}"), 5).await {
            return reply(
                StatusCode::TOO_MANY_REQUESTS,
                json!({"error":"Please wait before opening checkout again"}),
            );
        }
        match (method, path.as_str()) {
            (Method::GET, "/v1/account") if self.config.route == Route::EndToEnd => {
                match self.db.entitled(&account, crate::now()).await {
                    Ok(entitled) => reply(
                        StatusCode::OK,
                        json!({
                            "route": "end-to-end",
                            "entitled": entitled,
                            "model": MODEL,
                            "monthly_hours": SUMMARY_HOURS,
                            "annual": !self.config.annual_price.is_empty(),
                            "privacy": "Your computer encrypts each summary for Privatemode with \
                                        your own key. Overtalk never receives meeting text."
                        }),
                    ),
                    Err(error) => ledger_error(error),
                }
            }
            (Method::GET, "/v1/account") => match self.db.balance(&account, crate::now()).await {
                Ok(balance) => reply(
                    StatusCode::OK,
                    json!({
                        "route": "gateway",
                        "balance": balance,
                        "model": MODEL,
                        "input_micro_usd_per_million": self.config.input_rate,
                        "output_micro_usd_per_million": self.config.output_rate,
                        "plans": {
                            "summaries_once": "One $2 starter grant; no automatic refill",
                            "summaries_monthly":
                                "$2 per paid month; unused monthly credits expire",
                            "credits": "One $2 credit pack; no automatic refill"
                        },
                        "privacy": "Overtalk processes requests before forwarding them to \
                                    Privatemode. Direct local/BYOK processing remains available."
                    }),
                ),
                Err(error) => ledger_error(error),
            },
            (Method::POST, "/v1/summaries/key") if self.config.route == Route::EndToEnd => {
                self.summary_key(&account).await
            }
            (Method::GET, "/v1/models") => reply(
                StatusCode::OK,
                json!({
                    "object": "list",
                    "data": [{"id": MODEL, "object": "model", "owned_by": "Overtalk"}]
                }),
            ),
            (Method::POST, "/v1/checkouts") => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Input {
                    plan: String,
                    idempotency_key: String,
                }
                let Ok(input) = serde_json::from_slice::<Input>(&body) else {
                    return reply(
                        StatusCode::BAD_REQUEST,
                        json!({"error":"Invalid checkout request"}),
                    );
                };
                match self
                    .stripe
                    .checkout(&self.db, &account, &input.plan, &input.idempotency_key)
                    .await
                {
                    Ok(url) => reply(StatusCode::OK, json!({"url":url})),
                    Err(_) => reply(
                        StatusCode::BAD_REQUEST,
                        json!({
                            "error": "Checkout unavailable. An existing subscription can be \
                                      managed in billing."
                        }),
                    ),
                }
            }
            (Method::POST, "/v1/billing") => match self.stripe.portal(&self.db, &account).await {
                Ok(url) => reply(StatusCode::OK, json!({"url":url})),
                Err(_) => reply(
                    StatusCode::BAD_REQUEST,
                    json!({"error":"Billing management is unavailable"}),
                ),
            },
            (Method::POST, "/v1/chat/completions") if self.config.route == Route::EndToEnd => {
                reply(
                    StatusCode::GONE,
                    json!({
                        "error": "Summaries go from your computer directly to Privatemode. Update \
                                  Overtalk."
                    }),
                )
            }
            (Method::POST, "/v1/chat/completions") => {
                let id = request
                    .headers()
                    .get("idempotency-key")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or_default();
                self.chat(&account, id, &body).await
            }
            _ => reply(StatusCode::NOT_FOUND, json!({"error":"Unknown operation"})),
        }
    }

    fn route_name(&self) -> &'static str {
        match self.config.route {
            Route::EndToEnd => "end-to-end",
            Route::Gateway => "gateway",
        }
    }

    /// The subscriber's own Privatemode key. It leaves this server only to the
    /// account it is assigned to, and only during a paid subscription period.
    async fn summary_key(&self, account: &str) -> Response<Body> {
        let now = crate::now();
        match self.db.entitled(account, now).await {
            Ok(true) => {}
            Ok(false) => {
                return reply(
                    StatusCode::FORBIDDEN,
                    json!({"error":"Private AI summaries need an active subscription"}),
                )
            }
            Err(error) => return ledger_error(error),
        }
        match self.db.summary_key(account, now).await {
            Ok(Some(key)) => reply(
                StatusCode::OK,
                json!({"key":key.key,"expires":key.expires,"model":MODEL,"monthly_hours":SUMMARY_HOURS}),
            ),
            Ok(None) => {
                eprintln!("No free Privatemode summary key: import more with `keys import`");
                reply(
                    StatusCode::SERVICE_UNAVAILABLE,
                    json!({"error":"Your summary key is not ready yet. Try again later; local processing remains available."}),
                )
            }
            Err(error) => ledger_error(error),
        }
    }

    async fn chat(&self, account: &str, idempotency: &str, body: &[u8]) -> Response<Body> {
        let input: ChatInput = match serde_json::from_slice(body) {
            Ok(v) => v,
            Err(_) => {
                return reply(
                    StatusCode::BAD_REQUEST,
                    json!({"error":"Invalid notes request"}),
                )
            }
        };
        let (mut data, input_bound) = match input.validate() {
            Ok(v) => v,
            Err(_) => {
                return reply(
                    StatusCode::BAD_REQUEST,
                    json!({"error":"Unsupported model, content or generation limit"}),
                )
            }
        };
        let output_bound = data["max_completion_tokens"].as_u64().unwrap_or_default();
        let reserved = cost(
            input_bound,
            output_bound,
            self.config.input_rate,
            self.config.output_rate,
        );
        let Ok(reserved) = reserved else {
            return reply(
                StatusCode::BAD_REQUEST,
                json!({"error":"Request exceeds its credit limit"}),
            );
        };
        // Per-account cache isolation must not be selected by the caller.
        data["cache_salt"] = match self.db.cache_salt(account).await {
            Ok(salt) => salt.into(),
            Err(error) => return ledger_error(error),
        };
        let request = match self
            .client
            .post(
                self.config
                    .proxy
                    .join("chat/completions")
                    .expect("fixed relative URL"),
            )
            .bearer_auth(&self.config.proxy_key)
            .json(&data)
            .build()
        {
            Ok(request) => request,
            Err(_) => {
                return reply(
                    StatusCode::SERVICE_UNAVAILABLE,
                    json!({"error":"Provider configuration unavailable"}),
                )
            }
        };
        let reservation = match self
            .db
            .reserve(account, idempotency, reserved, crate::now())
            .await
        {
            Ok(id) => id,
            Err(error) => return ledger_error(error),
        };
        let received = async {
            let mut response = self.client.execute(request).await?;
            let success = response.status().is_success();
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await? {
                ensure!(
                    bytes.len() + chunk.len() <= 2 * 1024 * 1024,
                    "Oversized model response"
                );
                bytes.extend_from_slice(&chunk);
            }
            let value: Value = serde_json::from_slice(&bytes)?;
            let usage = &value["usage"];
            let prompt = usage["prompt_tokens"]
                .as_u64()
                .context("Usage unavailable")?;
            let completion = usage["completion_tokens"]
                .as_u64()
                .context("Usage unavailable")?;
            ensure!(
                prompt <= input_bound && completion <= output_bound,
                "Provider exceeded the reservation bounds"
            );
            let cached = usage["prompt_tokens_details"]["cached_tokens"]
                .as_u64()
                .unwrap_or(0);
            ensure!(cached <= prompt, "Invalid cache usage");
            let charged = cost(
                prompt - cached,
                completion,
                self.config.input_rate,
                self.config.output_rate,
            )? + cost(cached, 0, self.config.cached_rate, 0)?;
            Ok::<_, anyhow::Error>((success, value, charged))
        }
        .await;
        match received {
            Ok((success, value, charged)) => {
                if self.db.settle(&reservation, charged).await.is_err() {
                    return unknown_usage(&reservation);
                }
                if success {
                    reply(StatusCode::OK, value)
                } else {
                    reply(
                        StatusCode::BAD_GATEWAY,
                        json!({"error":"Provider could not finish this request","request_id":reservation}),
                    )
                }
            }
            // Dispatch happened: timeout, cancellation or missing usage is not
            // proof of zero cost. Hold the bounded reservation for reconciliation.
            Err(_) => unknown_usage(&reservation),
        }
    }

    pub async fn billing_worker(self: Arc<Self>) {
        loop {
            tokio::time::sleep(Duration::from_secs(2)).await;
            if !self.config.approved {
                continue;
            }
            let row=sqlx::query("SELECT id,attempts FROM events WHERE state='pending' AND next_attempt<=? ORDER BY rowid LIMIT 1")
                .bind(crate::now()).fetch_optional(&self.db.0).await;
            let Ok(Some(row)) = row else {
                continue;
            };
            let id: String = row.get("id");
            let attempts: i64 = row.get("attempts");
            if self.stripe.process_event(&self.db, &id).await.is_ok() {
                let _ = sqlx::query("UPDATE events SET state='done' WHERE id=?")
                    .bind(id)
                    .execute(&self.db.0)
                    .await;
            } else {
                let attempts = attempts + 1;
                let state = if attempts >= 12 { "review" } else { "pending" };
                let retry = crate::now() + (1_i64 << attempts.min(11)).min(3600);
                let _ =
                    sqlx::query("UPDATE events SET state=?,attempts=?,next_attempt=? WHERE id=?")
                        .bind(state)
                        .bind(attempts)
                        .bind(retry)
                        .bind(id)
                        .execute(&self.db.0)
                        .await;
                eprintln!("Billing event deferred; attempt {attempts}. Inspect the durable inbox.");
            }
        }
    }
}

fn unknown_usage(reservation: &str) -> Response<Body> {
    reply(
        StatusCode::BAD_GATEWAY,
        json!({"error":"Final usage could not be confirmed. Credits are held for review; do not automatically resubmit.","request_id":reservation,"credits_held":true}),
    )
}

fn ledger_error(error: LedgerError) -> Response<Body> {
    let code = match error {
        LedgerError::Unauthorized => StatusCode::UNAUTHORIZED,
        LedgerError::NoCredits => StatusCode::PAYMENT_REQUIRED,
        LedgerError::Suspended => StatusCode::FORBIDDEN,
        LedgerError::Duplicate => StatusCode::CONFLICT,
        LedgerError::Invalid => StatusCode::BAD_REQUEST,
        LedgerError::Storage(_) => StatusCode::SERVICE_UNAVAILABLE,
    };
    reply(code, json!({"error":error.to_string()}))
}

fn reply(status: StatusCode, value: Value) -> Response<Body> {
    Response::builder()
        .status(status)
        .header("Content-Type", "application/json")
        .header("Cache-Control", "no-store")
        .header("X-Content-Type-Options", "nosniff")
        .header(
            "Content-Security-Policy",
            "default-src 'none'; frame-ancestors 'none'",
        )
        .body(Body::from(value.to_string()))
        .expect("fixed response headers")
}

async fn bounded_body(body: &mut Body, max: usize) -> anyhow::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    while let Some(chunk) = body.data().await {
        let chunk = chunk?;
        ensure!(bytes.len() + chunk.len() <= max, "Request is too large");
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChatInput {
    model: String,
    messages: Vec<ChatMessage>,
    max_completion_tokens: u64,
    #[serde(default)]
    reasoning_effort: Option<String>,
    #[serde(default)]
    temperature: Option<f64>,
    #[serde(default)]
    top_p: Option<f64>,
    #[serde(default)]
    stream: bool,
}
#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct ChatMessage {
    role: String,
    content: String,
}

impl ChatInput {
    fn validate(self) -> anyhow::Result<(Value, u64)> {
        ensure!(
            self.model == MODEL
                && (1..=16_384).contains(&self.max_completion_tokens)
                && !self.stream,
            "Unsupported generation"
        );
        ensure!(
            !self.messages.is_empty() && self.messages.len() <= 128,
            "Invalid message count"
        );
        let mut tokens = 1024_u64;
        for message in &self.messages {
            ensure!(
                matches!(message.role.as_str(), "system" | "user" | "assistant"),
                "Invalid role"
            );
            // UTF-8 bytes provide a conservative bound for this text-only
            // byte-token model; template overhead is reserved separately.
            tokens += message.content.len() as u64 + message.role.len() as u64 + 128;
        }
        ensure!(
            tokens + self.max_completion_tokens <= 1_000_000,
            "Context too large"
        );
        let effort = self.reasoning_effort.as_deref().unwrap_or("low");
        ensure!(
            matches!(effort, "low" | "high" | "max"),
            "Invalid reasoning effort"
        );
        ensure!(
            self.temperature.is_none_or(|t| (0.0..=2.0).contains(&t))
                && self.top_p.is_none_or(|p| (0.0..=1.0).contains(&p)),
            "Invalid sampling"
        );
        let mut value = json!({"model":MODEL,"messages":self.messages,"max_completion_tokens":self.max_completion_tokens,"reasoning_effort":effort,"stream":false});
        if let Some(t) = self.temperature {
            value["temperature"] = t.into();
        }
        if let Some(p) = self.top_p {
            value["top_p"] = p.into();
        }
        Ok((value, tokens))
    }
}

pub fn cost(input: u64, output: u64, input_rate: u64, output_rate: u64) -> anyhow::Result<i64> {
    let micro = (u128::from(input) * u128::from(input_rate)
        + u128::from(output) * u128::from(output_rate))
    .div_ceil(1_000_000);
    Ok(i64::try_from(micro)?)
}

#[cfg(test)]
mod tests;
