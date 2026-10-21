//! Browser authorization for the desktop app. Only fixed, official provider
//! endpoints are trusted; bearer tokens never cross their resource audience.
use super::connections::{client, response, secret, Connection};
use crate::state::AppState;
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, LazyLock,
    },
    time::{Duration, Instant},
};
use tauri::State;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::{Mutex, Notify},
};

const GOOGLE_SCOPE: &str = "openid email https://www.googleapis.com/auth/documents https://www.googleapis.com/auth/drive.metadata.readonly https://www.googleapis.com/auth/calendar.readonly";
const ATLASSIAN_SCOPE: &str = "read:me read:account offline_access email read:jira:agent-interface write:jira:agent-interface read:confluence:agent-interface write:confluence:agent-interface";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Provider {
    Linear,
    Atlassian,
    Google,
    Notion,
    Microsoft,
    Slack,
    Github,
}
impl Provider {
    fn parse(s: &str) -> Result<Self, String> {
        match s {
            "linear_mcp" => Ok(Self::Linear),
            "atlassian_mcp" => Ok(Self::Atlassian),
            "google" => Ok(Self::Google),
            "notion_mcp" => Ok(Self::Notion),
            "microsoft" => Ok(Self::Microsoft),
            "slack" => Ok(Self::Slack),
            "github" => Ok(Self::Github),
            _ => Err("Unsupported sign-in provider".into()),
        }
    }
    fn id(self) -> &'static str {
        match self {
            Self::Linear => "linear_mcp",
            Self::Atlassian => "atlassian_mcp",
            Self::Google => "google",
            Self::Notion => "notion_mcp",
            Self::Microsoft => "microsoft",
            Self::Slack => "slack",
            Self::Github => "github",
        }
    }
    fn dynamic(self) -> bool {
        matches!(self, Self::Linear | Self::Atlassian | Self::Notion)
    }
    fn resource(self) -> &'static str {
        match self {
            Self::Linear => "https://mcp.linear.app/mcp",
            Self::Atlassian => "https://mcp.atlassian.com/v2/mcp",
            Self::Google => "https://docs.googleapis.com",
            Self::Notion => "https://mcp.notion.com/mcp",
            Self::Microsoft => "https://graph.microsoft.com",
            Self::Slack => "https://slack.com/api",
            Self::Github => "https://api.github.com",
        }
    }
    fn scope(self) -> &'static str {
        match self {
            Self::Linear => "read write",
            Self::Atlassian => ATLASSIAN_SCOPE,
            Self::Google => GOOGLE_SCOPE,
            Self::Notion => "default",
            Self::Microsoft => {
                "openid profile offline_access User.Read Calendars.Read Files.ReadWrite"
            }
            Self::Slack => "channels:read,groups:read,channels:history,groups:history,chat:write",
            Self::Github => "",
        }
    }
    fn metadata(self) -> &'static str {
        match self{Self::Linear=>"https://mcp.linear.app/.well-known/oauth-authorization-server",Self::Atlassian=>"https://auth.atlassian.com/.well-known/oauth-authorization-server/VCeDsk8ZHncYF1g234fKtc4lNipbBhu3",Self::Notion=>"https://mcp.notion.com/.well-known/oauth-authorization-server",_=>""}
    }
    fn configured_id(self) -> String {
        let (key, built) = match self {
            Self::Google => (
                "OVERTALK_GOOGLE_CLIENT_ID",
                option_env!("OVERTALK_GOOGLE_CLIENT_ID"),
            ),
            Self::Microsoft => (
                "OVERTALK_MICROSOFT_CLIENT_ID",
                option_env!("OVERTALK_MICROSOFT_CLIENT_ID"),
            ),
            Self::Slack => (
                "OVERTALK_SLACK_CLIENT_ID",
                option_env!("OVERTALK_SLACK_CLIENT_ID"),
            ),
            Self::Github => (
                "OVERTALK_GITHUB_CLIENT_ID",
                option_env!("OVERTALK_GITHUB_CLIENT_ID"),
            ),
            _ => return String::new(),
        };
        std::env::var(key)
            .ok()
            .or_else(|| built.map(str::to_owned))
            .unwrap_or_default()
    }
    fn valid_id(self, id: &str) -> bool {
        match self {
            Self::Google => valid_google_id(id),
            Self::Microsoft => uuid::Uuid::parse_str(id).is_ok(),
            Self::Slack | Self::Github => {
                !id.is_empty()
                    && id.len() <= 128
                    && id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            }
            _ => true,
        }
    }
    fn endpoint_allowed(self, value: &str) -> bool {
        let Ok(url) = reqwest::Url::parse(value) else {
            return false;
        };
        url.scheme() == "https"
            && url.username().is_empty()
            && url.password().is_none()
            && url.port().is_none()
            && url.fragment().is_none()
            && match self {
                Self::Linear => url.host_str() == Some("mcp.linear.app"),
                Self::Atlassian => url.host_str() == Some("auth.atlassian.com"),
                Self::Google => matches!(
                    url.host_str(),
                    Some("oauth2.googleapis.com" | "accounts.google.com")
                ),
                Self::Notion => url.host_str() == Some("mcp.notion.com"),
                Self::Microsoft => url.host_str() == Some("login.microsoftonline.com"),
                Self::Slack => url.host_str() == Some("slack.com"),
                Self::Github => url.host_str() == Some("github.com"),
            }
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Credential {
    pub kind: String,
    #[serde(default)]
    pub account_id: String,
    #[serde(default)]
    pub account_label: String,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
    pub client_id: String,
    #[serde(default)]
    pub client_secret: String,
    pub token_endpoint: String,
    pub revocation_endpoint: Option<String>,
    pub resource: String,
    pub scope: String,
}
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum AccountAction {
    Capabilities,
    Start {
        attempt: String,
        provider: String,
        name: String,
        project_id: Option<String>,
        #[serde(default)]
        connection_id: Option<String>,
        #[serde(default)]
        client_id: String,
        #[serde(default)]
        client_secret: String,
    },
    Wait {
        attempt: String,
    },
    Cancel {
        attempt: String,
    },
}
struct Attempt {
    cancelled: AtomicBool,
    waiting: AtomicBool,
    cancel: Notify,
    finished: Notify,
    result: Mutex<Option<Result<Value, String>>>,
    created: Instant,
}
static ATTEMPTS: LazyLock<Mutex<HashMap<String, Arc<Attempt>>>> = LazyLock::new(Default::default);
// Serializes rotating refresh tokens and disconnects, but unrelated accounts do
// not block one another. Weak entries live only while a caller holds or waits
// for the lock. Never detach a lock on disconnect: an existing waiter must
// still serialize with a new caller for that same account.
static LOCKS: LazyLock<Mutex<HashMap<String, std::sync::Weak<Mutex<()>>>>> =
    LazyLock::new(Default::default);
pub(super) async fn account_lock(id: &str) -> Arc<Mutex<()>> {
    let mut locks = LOCKS.lock().await;
    locks.retain(|_, lock| lock.strong_count() > 0);
    if let Some(lock) = locks.get(id).and_then(std::sync::Weak::upgrade) {
        return lock;
    }
    let lock = Arc::new(Mutex::new(()));
    locks.insert(id.into(), Arc::downgrade(&lock));
    lock
}
// Held across draft preparation/publication and credential replacement/removal.
// Separate from token refresh locks, which can be acquired inside an operation.
pub(super) async fn operation_lock(id: &str) -> Arc<Mutex<()>> {
    account_lock(&format!("operation:{id}")).await
}

async fn invalidate_account_drafts(
    db: &mut sqlx::SqliteConnection,
    id: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE outgoing_drafts SET status='cancelled',result='Account reconnected. Prepare and review a new draft before publishing.' WHERE connection_id=? AND status='draft'")
        .bind(id).execute(db).await?;
    Ok(())
}
fn google_client_id() -> String {
    std::env::var("OVERTALK_GOOGLE_CLIENT_ID")
        .ok()
        .or_else(|| option_env!("OVERTALK_GOOGLE_CLIENT_ID").map(str::to_owned))
        .unwrap_or_default()
}
fn valid_google_id(id: &str) -> bool {
    id.ends_with(".apps.googleusercontent.com")
        && id.len() < 250
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-".contains(&b))
}
fn random() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}
fn required(v: &Value, key: &str) -> Result<String, String> {
    v[key]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() < 16384)
        .map(str::to_owned)
        .ok_or_else(|| format!("Sign-in provider omitted {key}. Please reconnect."))
}

#[tauri::command]
pub async fn workspace_account_auth(
    state: State<'_, AppState>,
    action: AccountAction,
) -> Result<Value, String> {
    match action {
        AccountAction::Capabilities => {
            let providers = [
                Provider::Linear,
                Provider::Atlassian,
                Provider::Google,
                Provider::Notion,
                Provider::Microsoft,
                Provider::Slack,
                Provider::Github,
            ]
            .iter()
            .map(
                |p| json!({"id":p.id(),"configured":p.dynamic() || p.valid_id(&p.configured_id())}),
            )
            .collect::<Vec<_>>();
            Ok(json!({
                "google_configured": valid_google_id(&google_client_id()),
                "providers": providers
            }))
        }
        AccountAction::Cancel { attempt } => {
            if let Some(flow) = ATTEMPTS.lock().await.get(&attempt) {
                flow.cancelled.store(true, Ordering::SeqCst);
                flow.cancel.notify_one();
            }
            Ok(Value::Null)
        }
        AccountAction::Wait { attempt } => {
            let flow = ATTEMPTS
                .lock()
                .await
                .get(&attempt)
                .cloned()
                .ok_or("Sign-in expired. Please try again.")?;
            if flow.waiting.swap(true, Ordering::SeqCst) {
                return Err("This sign-in already has a waiting request".into());
            }
            loop {
                let notified = flow.finished.notified();
                if let Some(result) = flow.result.lock().await.take() {
                    ATTEMPTS.lock().await.remove(&attempt);
                    return result;
                }
                notified.await;
            }
        }
        AccountAction::Start {
            attempt,
            provider,
            name,
            project_id,
            connection_id,
            client_id,
            client_secret,
        } => {
            uuid::Uuid::parse_str(&attempt).map_err(|_| "Invalid sign-in attempt")?;
            let provider = Provider::parse(&provider)?;
            if name.trim().is_empty() || name.len() > 120 || client_secret.len() > 500 {
                return Err("Enter a connection name under 120 characters".into());
            }
            // Reauthorization keeps the connection ID so existing reviewed
            // drafts and project links continue to reference the same account.
            let mut saved_client = None;
            if let Some(id) = &connection_id {
                let stored: Option<(String,)> =
                    sqlx::query_as("SELECT provider FROM workspace_connections WHERE id=?")
                        .bind(id)
                        .fetch_optional(state.db_manager.pool())
                        .await
                        .map_err(|_| "Could not find the connection")?;
                if stored.as_ref().map(|r| r.0.as_str()) != Some(provider.id()) {
                    return Err("This connection no longer exists or uses another provider".into());
                }
                if !provider.dynamic() {
                    if let Ok(raw) = secret(id, None).await {
                        if let Ok(token) = serde_json::from_str::<Credential>(&raw) {
                            saved_client = Some((token.client_id, token.client_secret));
                        } else if let Ok(token) =
                            serde_json::from_str::<super::connections::GoogleToken>(&raw)
                        {
                            saved_client = Some((token.client_id, token.client_secret));
                        }
                    }
                }
            }
            let client_secret = if client_secret.is_empty() {
                saved_client
                    .as_ref()
                    .map(|p| p.1.clone())
                    .unwrap_or_default()
            } else {
                client_secret
            };
            let client_id = if client_id.is_empty() {
                saved_client.map(|p| p.0).unwrap_or_default()
            } else {
                client_id
            };
            let client_id = if !provider.dynamic() && client_id.is_empty() {
                provider.configured_id()
            } else {
                client_id
            };
            if !provider.valid_id(&client_id) {
                return Err(format!(
                    "{} sign-in needs Overtalk's registered public client ID. The app publisher \
                     sets this up once; an API key is not required.",
                    provider.id()
                ));
            }
            let flow = Arc::new(Attempt {
                cancelled: AtomicBool::new(false),
                waiting: AtomicBool::new(false),
                cancel: Notify::new(),
                finished: Notify::new(),
                result: Mutex::new(None),
                created: Instant::now(),
            });
            let mut attempts = ATTEMPTS.lock().await;
            attempts.retain(|_, a| a.created.elapsed() < Duration::from_secs(960));
            if attempts.len() >= 4 || attempts.contains_key(&attempt) {
                return Err("Finish or cancel your current sign-in first".into());
            }
            attempts.insert(attempt.clone(), flow.clone());
            drop(attempts);
            let device = if provider == Provider::Github {
                match tokio::select! {
                    biased;
                    _ = flow.cancel.notified() => Err("Sign-in cancelled".into()),
                    r = github_device(&client_id) => r,
                } {
                    Ok(device) => Some(device),
                    Err(error) => {
                        ATTEMPTS.lock().await.remove(&attempt);
                        return Err(error);
                    }
                }
            } else {
                None
            };
            let prompt = device
                .as_ref()
                .map(|d| json!({"user_code":d.user_code,"verification_uri":d.uri}))
                .unwrap_or(Value::Null);
            let pool = state.db_manager.pool().clone();
            tokio::spawn(async move {
                let result = tokio::select! {
                    biased;
                    _ = flow.cancel.notified() => Err("Sign-in cancelled".into()),
                    result = tokio::time::timeout(
                        Duration::from_secs(if provider == Provider::Github { 900 } else { 180 }),
                        async {
                            if let Some(device) = device {
                                github_authorize(&client_id, device).await
                            } else {
                                authorize(provider, &client_id, &client_secret).await
                            }
                        },
                    ) => result.unwrap_or_else(|_| Err("Sign-in timed out. Try again.".into())),
                };
                let result = match result {
                    Ok(credential) if !flow.cancelled.load(Ordering::SeqCst) => {
                        save(
                            &pool,
                            provider,
                            &name,
                            project_id,
                            connection_id,
                            credential,
                            &flow,
                        )
                        .await
                    }
                    Ok(_) => Err("Sign-in cancelled".into()),
                    Err(e) => Err(e),
                };
                *flow.result.lock().await = Some(result);
                flow.finished.notify_one();
            });
            Ok(json!({"attempt":attempt,"device":prompt}))
        }
    }
}
async fn save(
    pool: &sqlx::SqlitePool,
    provider: Provider,
    name: &str,
    project_id: Option<String>,
    connection_id: Option<String>,
    credential: Credential,
    flow: &Attempt,
) -> Result<Value, String> {
    let replacing = connection_id.is_some();
    let id = connection_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let operation = operation_lock(&id).await;
    let _operation = operation.lock().await;
    let lock = account_lock(&id).await;
    let _guard = lock.lock().await;
    let old = if replacing {
        secret(&id, None).await.ok()
    } else {
        None
    };
    if flow.cancelled.load(Ordering::SeqCst) {
        return Err("Sign-in cancelled".into());
    }
    // Keychain unlock can involve user interaction. Never hold a SQLite write
    // transaction while waiting for it: recording and notes must stay writable.
    secret(
        &id,
        Some(serde_json::to_string(&credential).map_err(|_| "Could not encode sign-in")?),
    )
    .await?;
    let result = async {
        let mut tx = pool
            .begin()
            .await
            .map_err(|_| "Could not save the connection".to_owned())?;
        if replacing {
            let result = sqlx::query(
                "UPDATE workspace_connections SET name=?,username=? WHERE id=? AND provider=?",
            )
            .bind(name.trim())
            .bind(&credential.account_label)
            .bind(&id)
            .bind(provider.id())
            .execute(&mut *tx)
            .await
            .map_err(|_| "Could not reconnect the account".to_owned())?;
            if result.rows_affected() != 1 {
                return Err("This connection no longer exists".to_owned());
            }
            // Providers do not all expose a stable account subject. Re-review
            // on every reauthorization rather than guess whether it is the same user.
            invalidate_account_drafts(&mut tx, &id)
                .await
                .map_err(|_| "Could not invalidate previous account approvals".to_owned())?;
        } else {
            sqlx::query(
                "INSERT INTO workspace_connections(id,provider,name,base_url,username,project_id,\
                 created_at) VALUES(?,?,?,?,?,?,?)",
            )
            .bind(&id)
            .bind(provider.id())
            .bind(name.trim())
            .bind(provider.resource())
            .bind(if credential.account_label.is_empty() {
                "Browser sign-in"
            } else {
                &credential.account_label
            })
            .bind(project_id)
            .bind(chrono::Utc::now().to_rfc3339())
            .execute(&mut *tx)
            .await
            .map_err(|_| "Could not save the connection".to_owned())?;
        }
        if flow.cancelled.load(Ordering::SeqCst) {
            return Err("Sign-in cancelled".to_owned());
        }
        tx.commit()
            .await
            .map_err(|_| "Could not save the connection".to_owned())?;
        Ok(json!({"id":id}))
    }
    .await;
    if result.is_err() {
        if let Some(old) = old {
            let _ = secret(&id, Some(old)).await;
        } else {
            let _ = delete_secret(&id).await;
        }
    } else {
        super::connection_mcp::forget(&id).await;
    }
    result
}

async fn authorize(
    provider: Provider,
    configured_id: &str,
    client_secret: &str,
) -> Result<Credential, String> {
    let listener = tokio::net::TcpListener::bind(if provider == Provider::Slack {
        "127.0.0.1:43871"
    } else {
        "127.0.0.1:0"
    })
    .await
    .map_err(|_| "Could not open the local sign-in callback")?;
    let redirect = format!(
        "http://{}:{}/callback",
        if matches!(provider, Provider::Slack | Provider::Microsoft) {
            "localhost"
        } else {
            "127.0.0.1"
        },
        listener
            .local_addr()
            .map_err(|_| "Missing callback address")?
            .port()
    );
    let metadata = if provider.dynamic() {
        response(
            client()?
                .get(provider.metadata())
                .send()
                .await
                .map_err(|_| "Could not reach the sign-in provider")?,
        )
        .await?
    } else {
        match provider {
            Provider::Google => {
                json!({
                    "authorization_endpoint": "https://accounts.google.com/o/oauth2/v2/auth",
                    "token_endpoint": "https://oauth2.googleapis.com/token",
                    "revocation_endpoint": "https://oauth2.googleapis.com/revoke"
                })
            }
            Provider::Microsoft => {
                json!({
                    "authorization_endpoint":
                        "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
                    "token_endpoint": "https://login.microsoftonline.com/common/oauth2/v2.0/token"
                })
            }
            Provider::Slack => {
                json!({
                    "authorization_endpoint": "https://slack.com/oauth/v2/authorize",
                    "token_endpoint": "https://slack.com/api/oauth.v2.access"
                })
            }
            _ => return Err("Unsupported browser flow".into()),
        }
    };
    let auth = required(&metadata, "authorization_endpoint")?;
    let token = required(&metadata, "token_endpoint")?;
    if !provider.endpoint_allowed(&auth) || !provider.endpoint_allowed(&token) {
        return Err("The provider returned an unexpected sign-in endpoint".into());
    }
    if provider.dynamic()
        && !metadata["code_challenge_methods_supported"]
            .as_array()
            .is_some_and(|a| a.iter().any(|v| v == "S256"))
    {
        return Err("The provider does not offer secure PKCE sign-in".into());
    }
    let client_id = if !provider.dynamic() {
        configured_id.to_owned()
    } else {
        let registration = required(&metadata, "registration_endpoint")?;
        if !provider.endpoint_allowed(&registration) {
            return Err("Unexpected client registration endpoint".into());
        }
        let registered = response(
            client()?
                .post(registration)
                .json(&json!({
                    "client_name": "Overtalk",
                    "redirect_uris": [redirect],
                    "grant_types": ["authorization_code", "refresh_token"],
                    "response_types": ["code"],
                    "token_endpoint_auth_method": "none"
                }))
                .send()
                .await
                .map_err(|_| "Could not register Overtalk for browser sign-in")?,
        )
        .await?;
        required(&registered, "client_id")?
    };
    let verifier = random();
    let state = random();
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .encode(Sha256::digest(verifier.as_bytes()));
    let mut url = reqwest::Url::parse(&auth).map_err(|_| "Invalid authorization URL")?;
    url.query_pairs_mut().extend_pairs([
        ("client_id", client_id.as_str()),
        ("redirect_uri", redirect.as_str()),
        ("response_type", "code"),
        (
            if provider == Provider::Slack {
                "user_scope"
            } else {
                "scope"
            },
            provider.scope(),
        ),
        ("state", state.as_str()),
        ("code_challenge", challenge.as_str()),
        ("code_challenge_method", "S256"),
    ]);
    if provider == Provider::Google {
        url.query_pairs_mut()
            .extend_pairs([("access_type", "offline"), ("prompt", "consent")]);
    } else if provider.dynamic() {
        url.query_pairs_mut()
            .append_pair("resource", provider.resource());
    }
    crate::api::open_external_url(url.to_string()).await?;
    let code = callback(&listener, &state, metadata["issuer"].as_str()).await?;
    let mut form = vec![
        ("grant_type", "authorization_code"),
        ("client_id", &client_id),
        ("code", &code),
        ("redirect_uri", &redirect),
        ("code_verifier", &verifier),
    ];
    if !client_secret.is_empty() {
        form.push(("client_secret", client_secret));
    }
    if provider.dynamic() {
        form.push(("resource", provider.resource()));
    }
    let value = response(
        client()?
            .post(&token)
            .header("Accept", "application/json")
            .form(&form)
            .send()
            .await
            .map_err(|_| "Sign-in could not be completed")?,
    )
    .await?;
    let value = token_payload(provider, value)?;
    if value["token_type"]
        .as_str()
        .is_some_and(|v| !v.eq_ignore_ascii_case("bearer"))
    {
        return Err("Unsupported sign-in token type".into());
    }
    let scope = value["scope"]
        .as_str()
        .unwrap_or(provider.scope())
        .to_owned();
    let mut credential = Credential {
        kind: provider.id().into(),
        account_id: String::new(),
        account_label: String::new(),
        access_token: required(&value, "access_token")?,
        refresh_token: value["refresh_token"].as_str().unwrap_or("").into(),
        expires_at: expiry(provider, &value),
        client_id,
        client_secret: client_secret.into(),
        token_endpoint: token,
        revocation_endpoint: metadata["revocation_endpoint"]
            .as_str()
            .filter(|s| provider.endpoint_allowed(s))
            .map(str::to_owned),
        resource: provider.resource().into(),
        scope,
    };
    identify(&mut credential).await?;
    Ok(credential)
}
fn callback_code(request: &str, state: &str, issuer: Option<&str>) -> Result<String, String> {
    let path = request
        .lines()
        .next()
        .and_then(|l| l.strip_prefix("GET "))
        .and_then(|l| l.strip_suffix(" HTTP/1.1"))
        .filter(|p| p.starts_with("/callback?") && !p.starts_with("//"))
        .ok_or("Invalid sign-in callback")?;
    let url = reqwest::Url::parse(&format!("http://127.0.0.1{path}"))
        .map_err(|_| "Invalid callback URL")?;
    let mut params = HashMap::new();
    for (k, v) in url.query_pairs() {
        if params.insert(k.to_string(), v.to_string()).is_some() {
            return Err("Duplicate callback parameter".into());
        }
    }
    if params.get("state").map(String::as_str) != Some(state) || url.fragment().is_some() {
        return Err("Sign-in callback did not match this attempt".into());
    }
    if let Some(actual) = params.get("iss") {
        if Some(actual.as_str()) != issuer {
            return Err("Unexpected sign-in issuer".into());
        }
    }
    if params.contains_key("error") {
        return Err("Sign-in was declined. You can try again when ready.".into());
    }
    params
        .remove("code")
        .filter(|s| !s.is_empty() && s.len() <= 8192)
        .ok_or("The provider returned no authorization code".into())
}
async fn callback(
    listener: &tokio::net::TcpListener,
    state: &str,
    issuer: Option<&str>,
) -> Result<String, String> {
    for _ in 0..24 {
        let (mut stream, _) = listener
            .accept()
            .await
            .map_err(|_| "Could not receive sign-in")?;
        let mut data = Vec::new();
        let read = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let mut chunk = [0u8; 1024];
                let n = stream.read(&mut chunk).await.map_err(|_| ())?;
                if n == 0 {
                    return Err(());
                }
                data.extend_from_slice(&chunk[..n]);
                if data.len() > 8192 {
                    return Err(());
                }
                if data.windows(4).any(|w| w == b"\r\n\r\n") {
                    return Ok(());
                }
            }
        })
        .await;
        let result = if matches!(read, Ok(Ok(()))) {
            callback_code(&String::from_utf8_lossy(&data), state, issuer)
        } else {
            Err("Invalid sign-in callback".into())
        };
        let matched = String::from_utf8_lossy(&data).contains(&format!("state={state}"));
        let (status, body) = if result.is_ok() {
            ("200 OK", "Authorization received. Return to Overtalk.")
        } else {
            (
                "400 Bad Request",
                "Sign-in was not completed. Return to Overtalk to try again.",
            )
        };
        let reply=format!("HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Security-Policy: default-src 'none'\r\nCache-Control: no-store\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",body.len());
        let _ =
            tokio::time::timeout(Duration::from_secs(1), stream.write_all(reply.as_bytes())).await;
        if result.is_ok() || matched {
            return result;
        }
    }
    Err("Too many invalid sign-in callbacks. Please retry.".into())
}
pub(super) async fn access_token(c: &Connection) -> Result<String, String> {
    let lock = account_lock(&c.id).await;
    let _guard = lock.lock().await;
    let raw = secret(&c.id, None).await?;
    let Ok(mut credential) = serde_json::from_str::<Credential>(&raw) else {
        return super::connections::legacy_token(c, raw).await;
    };
    let provider = Provider::parse(&credential.kind)?;
    if credential.kind != c.provider
        || credential.resource != provider.resource()
        || !provider.endpoint_allowed(&credential.token_endpoint)
    {
        return Err("Connection audience changed. Reconnect this account.".into());
    }
    if credential.expires_at <= chrono::Utc::now().timestamp() + 90 {
        if credential.refresh_token.is_empty() {
            return Err("Your sign-in expired. Reconnect this account.".into());
        }
        let mut form = vec![
            ("grant_type", "refresh_token"),
            ("client_id", credential.client_id.as_str()),
            ("refresh_token", credential.refresh_token.as_str()),
        ];
        if !credential.client_secret.is_empty() {
            form.push(("client_secret", &credential.client_secret));
        }
        if provider.dynamic() {
            form.push(("resource", provider.resource()));
        }
        let value = response(
            client()?
                .post(&credential.token_endpoint)
                .header("Accept", "application/json")
                .form(&form)
                .send()
                .await
                .map_err(|_| "Could not refresh sign-in. Check your connection and retry.")?,
        )
        .await?;
        let value = token_payload(provider, value)?;
        credential.access_token = required(&value, "access_token")?;
        if let Some(refresh) = value["refresh_token"].as_str() {
            credential.refresh_token = refresh.into();
        }
        credential.expires_at = expiry(provider, &value);
        identify(&mut credential).await?;
        secret(
            &c.id,
            Some(
                serde_json::to_string(&credential)
                    .map_err(|_| "Could not save refreshed sign-in")?,
            ),
        )
        .await?;
    }
    Ok(credential.access_token)
}
pub(super) async fn delete_secret(id: &str) -> Result<(), String> {
    let id = id.to_owned();
    tokio::task::spawn_blocking(move || {
        match keyring::Entry::new("Overtalk connections", &id)
            .map_err(|_| "Could not open the keychain")?
            .delete_credential()
        {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err("Could not remove credentials from the keychain".into()),
        }
    })
    .await
    .map_err(|_| "Keychain worker failed")?
}
pub(super) async fn disconnect(c: &Connection) -> Result<bool, String> {
    let lock = account_lock(&c.id).await;
    let _guard = lock.lock().await;
    let mut revoked = false;
    if let Ok(raw) = secret(&c.id, None).await {
        if let Ok(credential) = serde_json::from_str::<Credential>(&raw) {
            if let Some(endpoint) = credential.revocation_endpoint {
                if Provider::parse(&credential.kind)?.endpoint_allowed(&endpoint) {
                    let token = if credential.refresh_token.is_empty() {
                        credential.access_token
                    } else {
                        credential.refresh_token
                    };
                    revoked = client()?
                        .post(endpoint)
                        .form(&[("token", token), ("client_id", credential.client_id)])
                        .send()
                        .await
                        .is_ok_and(|r| r.status().is_success());
                }
            }
        }
    }
    delete_secret(&c.id).await?;
    super::connection_mcp::forget(&c.id).await;
    Ok(revoked)
}
fn expiry(provider: Provider, value: &Value) -> i64 {
    match value["expires_in"].as_i64() {
        Some(seconds) => chrono::Utc::now().timestamp() + seconds.clamp(1, 86400 * 180),
        None if matches!(provider, Provider::Slack | Provider::Github) => i64::MAX,
        None => chrono::Utc::now().timestamp() + 3600,
    }
}
fn token_payload(provider: Provider, value: Value) -> Result<Value, String> {
    if value.get("error").is_some() || value["ok"] == false {
        return Err("Sign-in could not be completed or renewed. Check the app registration and sign in again.".into());
    }
    if provider == Provider::Slack && value["authed_user"]["access_token"].is_string() {
        let mut token = value["authed_user"].clone();
        for key in ["refresh_token", "expires_in"] {
            if token[key].is_null() && !value[key].is_null() {
                token[key] = value[key].clone();
            }
        }
        return Ok(token);
    }
    Ok(value)
}
async fn identify(credential: &mut Credential) -> Result<(), String> {
    let (url, method) = match credential.kind.as_str() {
        "github" => ("https://api.github.com/user", reqwest::Method::GET),
        "microsoft" => (
            "https://graph.microsoft.com/v1.0/me?$select=id,displayName,userPrincipalName",
            reqwest::Method::GET,
        ),
        "slack" => ("https://slack.com/api/auth.test", reqwest::Method::POST),
        _ => return Ok(()),
    };
    let value = response(
        client()?
            .request(method, url)
            .bearer_auth(&credential.access_token)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|_| "Could not verify the connected account")?,
    )
    .await?;
    if value["ok"] == false {
        return Err("The connected account could not be verified".into());
    }
    let (id, label) = match credential.kind.as_str() {
        "github" => (
            value["id"].to_string(),
            value["login"]
                .as_str()
                .unwrap_or("GitHub account")
                .to_owned(),
        ),
        "microsoft" => (
            required(&value, "id")?,
            value["userPrincipalName"]
                .as_str()
                .or_else(|| value["displayName"].as_str())
                .unwrap_or("Microsoft account")
                .to_owned(),
        ),
        _ => (
            format!(
                "{}:{}",
                required(&value, "team_id")?,
                required(&value, "user_id")?
            ),
            format!(
                "{} · {}",
                value["team"].as_str().unwrap_or("Slack"),
                value["user"].as_str().unwrap_or("Connected account")
            ),
        ),
    };
    if id == "null" || (!credential.account_id.is_empty() && credential.account_id != id) {
        return Err("The refreshed token belongs to a different account. Sign in again.".into());
    }
    credential.account_id = id;
    credential.account_label = label;
    Ok(())
}
struct DeviceGrant {
    code: String,
    user_code: String,
    uri: String,
    interval: u64,
    expires: Duration,
}
async fn github_device(client_id: &str) -> Result<DeviceGrant, String> {
    let value = response(
        client()?
            .post("https://github.com/login/device/code")
            .header("Accept", "application/json")
            .form(&[("client_id", client_id)])
            .send()
            .await
            .map_err(|_| "Could not start GitHub sign-in")?,
    )
    .await?;
    let uri = required(&value, "verification_uri")?;
    if uri != "https://github.com/login/device" {
        return Err("GitHub returned an unexpected verification address".into());
    }
    let user_code = required(&value, "user_code")?;
    if user_code.len() > 32
        || !user_code
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err("GitHub returned an invalid sign-in code".into());
    }
    Ok(DeviceGrant {
        code: required(&value, "device_code")?,
        user_code,
        uri,
        interval: value["interval"].as_u64().unwrap_or(5).clamp(5, 60),
        expires: Duration::from_secs(value["expires_in"].as_u64().unwrap_or(900).clamp(30, 900)),
    })
}
async fn github_authorize(client_id: &str, mut device: DeviceGrant) -> Result<Credential, String> {
    let deadline = Instant::now() + device.expires;
    while Instant::now() < deadline {
        tokio::time::sleep(Duration::from_secs(device.interval)).await;
        let value = response(
            client()?
                .post("https://github.com/login/oauth/access_token")
                .header("Accept", "application/json")
                .form(&[
                    ("client_id", client_id),
                    ("device_code", device.code.as_str()),
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ])
                .send()
                .await
                .map_err(|_| "GitHub sign-in was interrupted")?,
        )
        .await?;
        match value["error"].as_str() {
            Some("authorization_pending") => continue,
            Some("slow_down") => {
                device.interval = (device.interval + 5).min(60);
                continue;
            }
            Some("access_denied") => return Err("GitHub sign-in was declined".into()),
            Some("expired_token") => break,
            Some(_) => {
                return Err(
                    "GitHub could not complete sign-in. Check the app's device-flow setting."
                        .into(),
                )
            }
            None => {}
        }
        let mut credential = Credential {
            kind: "github".into(),
            account_id: String::new(),
            account_label: String::new(),
            access_token: required(&value, "access_token")?,
            refresh_token: value["refresh_token"].as_str().unwrap_or("").into(),
            expires_at: expiry(Provider::Github, &value),
            client_id: client_id.into(),
            client_secret: String::new(),
            token_endpoint: "https://github.com/login/oauth/access_token".into(),
            revocation_endpoint: None,
            resource: Provider::Github.resource().into(),
            scope: value["scope"].as_str().unwrap_or("").into(),
        };
        identify(&mut credential).await?;
        return Ok(credential);
    }
    Err("GitHub sign-in code expired. Start again for a new code.".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn unused_account_locks_are_reclaimed() {
        let first = uuid::Uuid::new_v4().to_string();
        drop(account_lock(&first).await);
        let _next = account_lock(&uuid::Uuid::new_v4().to_string()).await;
        assert!(!LOCKS.lock().await.contains_key(&first));
    }
    #[tokio::test]
    async fn account_lock_remains_shared_while_callers_wait() {
        let id = uuid::Uuid::new_v4().to_string();
        let first = account_lock(&id).await;
        let guard = first.clone().lock_owned().await;
        let waiting = account_lock(&id).await;
        drop(first);
        let later = account_lock(&id).await;
        assert!(Arc::ptr_eq(&waiting, &later));
        assert!(later.try_lock().is_err());
        drop(guard);
        assert!(later.try_lock().is_ok());
    }
    #[tokio::test]
    async fn reconnect_invalidates_only_pending_approvals_for_that_account() {
        use sqlx::Connection as _;
        let mut db = sqlx::SqliteConnection::connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::raw_sql("CREATE TABLE outgoing_drafts(id TEXT,connection_id TEXT,status TEXT,result TEXT); INSERT INTO outgoing_drafts VALUES('a','one','draft',NULL),('b','one','published','sent'),('c','two','draft',NULL);")
            .execute(&mut db).await.unwrap();
        invalidate_account_drafts(&mut db, "one").await.unwrap();
        let rows: Vec<(String, String)> =
            sqlx::query_as("SELECT id,status FROM outgoing_drafts ORDER BY id")
                .fetch_all(&mut db)
                .await
                .unwrap();
        assert_eq!(
            rows,
            [
                ("a".into(), "cancelled".into()),
                ("b".into(), "published".into()),
                ("c".into(), "draft".into())
            ]
        );
        db.close().await.unwrap();
    }
    #[tokio::test]
    async fn publishing_and_reconnection_share_a_lock_but_can_refresh_tokens() {
        let id = uuid::Uuid::new_v4().to_string();
        let publish = operation_lock(&id).await;
        let _guard = publish.lock().await;
        let reconnect = operation_lock(&id).await;
        assert!(reconnect.try_lock().is_err());
        let refresh = account_lock(&id).await;
        assert!(refresh.try_lock().is_ok());
    }
    #[tokio::test]
    async fn loopback_ignores_a_forged_request_and_accepts_the_real_callback() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move { callback(&listener, "expected-state", None).await });
        let client = reqwest::Client::new();
        assert_eq!(
            client
                .get(format!("http://{addr}/callback?state=wrong&code=bad"))
                .send()
                .await
                .unwrap()
                .status()
                .as_u16(),
            400
        );
        assert_eq!(
            client
                .get(format!(
                    "http://{addr}/callback?state=expected-state&code=correct"
                ))
                .send()
                .await
                .unwrap()
                .status()
                .as_u16(),
            200
        );
        assert_eq!(task.await.unwrap().unwrap(), "correct");
    }
    #[test]
    fn callback_rejects_forgery_duplicates_and_fragments() {
        assert_eq!(
            callback_code(
                "GET /callback?state=abc&code=ok HTTP/1.1\r\n\r\n",
                "abc",
                None
            )
            .unwrap(),
            "ok"
        );
        for path in [
            "/callback?state=x&code=ok",
            "/callback?state=abc&state=abc&code=ok",
            "/callback?state=abc&code=ok#fragment",
            "//evil/callback?state=abc&code=ok",
        ] {
            assert!(callback_code(&format!("GET {path} HTTP/1.1"), "abc", None).is_err());
        }
    }
    #[test]
    fn callback_rejects_denial_wrong_issuer_and_post() {
        assert!(callback_code("POST /callback?state=abc&code=ok HTTP/1.1", "abc", None).is_err());
        assert!(callback_code(
            "GET /callback?state=abc&error=access_denied HTTP/1.1",
            "abc",
            None
        )
        .unwrap_err()
        .contains("declined"));
        assert!(callback_code(
            "GET /callback?state=abc&code=ok&iss=https://evil.test HTTP/1.1",
            "abc",
            Some("https://good.test")
        )
        .is_err());
    }
    #[test]
    fn endpoints_cannot_exfiltrate_credentials() {
        for p in [
            Provider::Linear,
            Provider::Atlassian,
            Provider::Google,
            Provider::Notion,
            Provider::Microsoft,
            Provider::Slack,
            Provider::Github,
        ] {
            for u in [
                "http://mcp.linear.app/token",
                "https://mcp.linear.app.evil.test/token",
                "https://evil.test",
                "https://user@mcp.linear.app/token",
                "https://mcp.linear.app:444/token",
            ] {
                assert!(!p.endpoint_allowed(u));
            }
        }
        assert!(Provider::Linear.endpoint_allowed("https://mcp.linear.app/token"));
        assert!(Provider::Atlassian.endpoint_allowed("https://auth.atlassian.com/oauth/token"));
    }
    #[tokio::test]
    async fn account_migration_preserves_existing_drafts_and_foreign_keys() {
        use sqlx::Connection as _;
        let mut db = sqlx::SqliteConnection::connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::raw_sql("PRAGMA foreign_keys=ON; CREATE TABLE projects(id TEXT PRIMARY KEY);")
            .execute(&mut db)
            .await
            .unwrap();
        let old = include_str!("../../migrations/20261018000000_project_workspace.sql");
        let start = old.find("CREATE TABLE workspace_connections (").unwrap();
        let end = old
            .find("ALTER TABLE workspace_calendar ADD COLUMN participants")
            .unwrap();
        sqlx::raw_sql(&old[start..end])
            .execute(&mut db)
            .await
            .unwrap();
        sqlx::raw_sql(
            "INSERT INTO projects VALUES('project'); \
             INSERT INTO workspace_connections VALUES('old','linear','Team',\
             'https://api.linear.app','user','project','2026-10-17'); \
             INSERT INTO outgoing_drafts VALUES('draft','old','update','ENG-4','Keep title',\
             'Keep body','r7','uncertain','2026-10-17','unconfirmed');",
        )
        .execute(&mut db)
        .await
        .unwrap();
        assert!(sqlx::query(
            "INSERT INTO workspace_connections(id,provider,name,created_at) \
             VALUES('blocked','slack','Slack','2026-10-17')"
        )
        .execute(&mut db)
        .await
        .is_err());
        let mut tx = db.begin().await.unwrap();
        sqlx::raw_sql(include_str!(
            "../../migrations/20261019000000_connection_accounts.sql"
        ))
        .execute(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
        let preserved: (String, String, String, String) = sqlx::query_as(
            "SELECT body,expected_revision,status,result FROM outgoing_drafts WHERE id='draft'",
        )
        .fetch_one(&mut db)
        .await
        .unwrap();
        assert_eq!(
            preserved,
            (
                "Keep body".into(),
                "r7".into(),
                "uncertain".into(),
                "unconfirmed".into()
            )
        );
        for provider in [
            "linear_mcp",
            "atlassian_mcp",
            "notion_mcp",
            "microsoft",
            "slack",
            "github",
        ] {
            sqlx::query(
                "INSERT INTO workspace_connections(id,provider,name,created_at) VALUES(?,?,?,?)",
            )
            .bind(provider)
            .bind(provider)
            .bind(provider)
            .bind("2026-10-17")
            .execute(&mut db)
            .await
            .unwrap();
        }
        assert!(sqlx::query(
            "INSERT INTO outgoing_drafts(id,connection_id,kind,target,title,body,created_at) \
             VALUES('orphan','missing','create','','','','2026-10-17')"
        )
        .execute(&mut db)
        .await
        .is_err());
        let violations = sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&mut db)
            .await
            .unwrap();
        assert!(violations.is_empty());
        sqlx::query("DELETE FROM projects WHERE id='project'")
            .execute(&mut db)
            .await
            .unwrap();
        let project: Option<String> =
            sqlx::query_scalar("SELECT project_id FROM workspace_connections WHERE id='old'")
                .fetch_one(&mut db)
                .await
                .unwrap();
        assert!(project.is_none());
        sqlx::query("DELETE FROM workspace_connections WHERE id='old'")
            .execute(&mut db)
            .await
            .unwrap();
        let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM outgoing_drafts")
            .fetch_one(&mut db)
            .await
            .unwrap();
        assert_eq!(remaining, 0);
    }
    #[test]
    fn provider_identities_and_oauth_audiences_are_distinct() {
        for (provider, allowed) in [
            (Provider::Notion, "https://mcp.notion.com/token"),
            (
                Provider::Microsoft,
                "https://login.microsoftonline.com/common/oauth2/v2.0/token",
            ),
            (Provider::Slack, "https://slack.com/api/oauth.v2.access"),
            (
                Provider::Github,
                "https://github.com/login/oauth/access_token",
            ),
        ] {
            assert!(provider.endpoint_allowed(allowed));
            assert!(!provider.endpoint_allowed(&format!("{allowed}#redirect")));
            assert!(!provider.endpoint_allowed("https://api.github.com/token"));
            assert!(Provider::parse(provider.id()).unwrap() == provider);
        }
        assert!(!Provider::Microsoft.valid_id("https://login.microsoftonline.com/tenant"));
        assert!(Provider::Microsoft.valid_id("00000000-0000-0000-0000-000000000001"));
        assert!(!Provider::Slack.valid_id("id&redirect_uri=https://evil.test"));
        assert!(!Provider::Github.valid_id(""));
    }
    #[test]
    fn slack_uses_the_user_grant_and_rejects_failed_grants() {
        let value = token_payload(Provider::Slack, json!({"ok":true,
            "access_token":"bot-token-must-not-be-used",
            "authed_user":{"access_token":"user-grant","refresh_token":"rotating","expires_in":3600}})).unwrap();
        assert_eq!(value["access_token"], "user-grant");
        assert_eq!(value["refresh_token"], "rotating");
        assert!(
            token_payload(Provider::Slack, json!({"ok":false,"error":"invalid_code"})).is_err()
        );
        assert!(token_payload(Provider::Github, json!({"error":"access_denied"})).is_err());
        assert_eq!(expiry(Provider::Slack, &json!({})), i64::MAX);
        assert!(
            expiry(Provider::Microsoft, &json!({"expires_in":-5}))
                <= chrono::Utc::now().timestamp() + 2
        );
    }
    #[test]
    fn google_configuration_is_a_public_desktop_id() {
        assert!(valid_google_id("123-example.apps.googleusercontent.com"));
        assert!(!valid_google_id(
            "https://example.apps.googleusercontent.com"
        ));
        assert!(!valid_google_id(""));
    }
}
