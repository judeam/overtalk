//! The desktop knows only its own scoped Overtalk credential and, during a paid
//! subscription period, its own Privatemode key. Stripe secrets and the pool of
//! Privatemode keys belong exclusively to the hosted gateway, which never
//! receives meeting text.
use reqwest::Url;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::LazyLock;

static ACCOUNT_WRITE: LazyLock<tokio::sync::Mutex<()>> =
    LazyLock::new(|| tokio::sync::Mutex::new(()));

pub fn endpoint() -> Result<Url, String> {
    let value = std::env::var("OVERTALK_CLOUD_URL")
        .ok()
        .or_else(|| option_env!("OVERTALK_CLOUD_URL").map(str::to_owned))
        .filter(|v| !v.is_empty())
        .ok_or("Overtalk cloud plans are not available in this build")?;
    normalize_endpoint(&value)
}

fn normalize_endpoint(value: &str) -> Result<Url, String> {
    let mut url = Url::parse(value).map_err(|_| "Invalid Overtalk cloud address")?;
    let loopback = url.host_str().is_some_and(|host| {
        host.parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
    });
    if (url.scheme() != "https" && !(url.scheme() == "http" && loopback))
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/")
    {
        return Err("Overtalk cloud requires an HTTPS origin".into());
    }
    url.set_path("/");
    Ok(url)
}

pub async fn credential(value: Option<String>) -> Result<Option<String>, String> {
    let host = endpoint()?.as_str().to_owned();
    tokio::task::spawn_blocking(move || {
        let entry = keyring::Entry::new("Overtalk cloud account", &host)
            .map_err(|_| "The OS keychain is unavailable")?;
        if let Some(value) = value {
            if value.len() != 43
                || !value
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
            {
                return Err("Invalid Overtalk account credential".into());
            }
            entry
                .set_password(&value)
                .map_err(|_| "Unlock the OS keychain to save your account")?;
            Ok(None)
        } else {
            match entry.get_password() {
                Ok(value) => Ok(Some(value)),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(_) => Err("Unlock the OS keychain to use your account".into()),
            }
        }
    })
    .await
    .map_err(|_| "Account keychain request failed".to_owned())?
}

async fn call(path: &str, body: Option<Value>, token: Option<&str>) -> Result<Value, String> {
    let url = endpoint()?
        .join(path)
        .map_err(|_| "Invalid cloud operation")?;
    crate::security::privacy::guard_process_destination(url.as_str())?;
    let client = crate::summary::privatemode::client()?;
    let mut request = if let Some(body) = body {
        client.post(url).json(&body)
    } else {
        client.get(url)
    };
    if let Some(token) = token {
        request = request.bearer_auth(token);
    }
    let response = request
        .timeout(std::time::Duration::from_secs(25))
        .send()
        .await
        .map_err(|_| "Could not reach Overtalk cloud")?;
    let status = response.status();
    let text = crate::security::transport::response_text(response, 256 * 1024).await?;
    if !status.is_success() {
        return Err(match status.as_u16() {
            401 => "Your cloud account credential was rejected",
            402 => "Your cloud credits are exhausted",
            403 => "Private AI summaries need an active subscription",
            410 => "Update Overtalk to use private AI summaries",
            503 => "Overtalk cloud plans are not available yet",
            _ => "The cloud operation could not be completed",
        }
        .into());
    }
    serde_json::from_str(&text).map_err(|_| "Invalid cloud response".into())
}

async fn summary_entry(host: String, value: Option<String>) -> Result<Option<String>, String> {
    tokio::task::spawn_blocking(move || {
        let entry = keyring::Entry::new("Overtalk private summaries", &host)
            .map_err(|_| "The OS keychain is unavailable")?;
        match value {
            Some(value) => entry
                .set_password(&value)
                .map(|_| None)
                .map_err(|_| "Unlock the OS keychain to save your summary key".to_owned()),
            None => match entry.get_password() {
                Ok(value) => Ok(Some(value)),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(_) => Err("Unlock the OS keychain to use private AI summaries".into()),
            },
        }
    })
    .await
    .map_err(|_| "Summary keychain request failed".to_owned())?
}

/// This subscription's Privatemode key, kept in the OS keychain and fetched
/// again a day before it expires or after Privatemode rejects it.
pub async fn summaries_key() -> Result<String, String> {
    let host = endpoint()?.as_str().to_owned();
    if let Some(cached) = summary_entry(host.clone(), None).await? {
        if let Ok(value) = serde_json::from_str::<Value>(&cached) {
            let fresh = value["expires"]
                .as_i64()
                .is_some_and(|end| end - chrono::Utc::now().timestamp() > 86_400);
            if let (true, Some(key)) = (fresh, value["key"].as_str()) {
                return Ok(key.to_owned());
            }
        }
    }
    let token = credential(None)
        .await?
        .ok_or("Connect your Overtalk account in Settings to use private AI summaries")?;
    let result = call("v1/summaries/key", Some(json!({})), Some(&token)).await?;
    let key = result["key"]
        .as_str()
        .filter(|k| (16..=512).contains(&k.len()) && !k.contains(char::is_whitespace))
        .ok_or("Invalid summary key response")?
        .to_owned();
    let expires = result["expires"]
        .as_i64()
        .ok_or("Invalid summary key response")?;
    summary_entry(
        host,
        Some(json!({"key": key, "expires": expires}).to_string()),
    )
    .await?;
    Ok(key)
}

pub async fn forget_summaries_key() {
    if let Ok(host) = endpoint() {
        let host = host.as_str().to_owned();
        let _ = tokio::task::spawn_blocking(move || {
            if let Ok(entry) = keyring::Entry::new("Overtalk private summaries", &host) {
                let _ = entry.delete_credential();
            }
        })
        .await;
    }
    crate::summary::privatemode_runtime::shutdown().await;
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum CloudAction {
    Status,
    Restore { account_token: String },
    Checkout { plan: CloudPlan },
    Billing,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CloudPlan {
    SummariesOnce,
    SummariesMonthly,
    SummariesAnnual,
    Credits,
}

fn checkout_url(value: Value) -> Result<Value, String> {
    let url = Url::parse(
        value["url"]
            .as_str()
            .ok_or("Billing did not return a link")?,
    )
    .map_err(|_| "Invalid billing link")?;
    if url.scheme() != "https"
        || !matches!(
            url.host_str(),
            Some("checkout.stripe.com" | "billing.stripe.com")
        )
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("Untrusted billing link was blocked".into());
    }
    Ok(json!({"url":url.as_str()}))
}

#[tauri::command]
pub async fn cloud_account(action: CloudAction) -> Result<Value, String> {
    if matches!(action, CloudAction::Status) {
        if endpoint().is_err() {
            return Ok(json!({"configured":false,"connected":false}));
        }
        if call("health", None, None).await?["ready"] != true {
            return Ok(json!({"configured":false,"connected":false}));
        }
        let Some(token) = credential(None).await? else {
            return Ok(json!({"configured":true,"connected":false}));
        };
        let mut result = call("v1/account", None, Some(&token)).await?;
        result["configured"] = true.into();
        result["connected"] = true.into();
        return Ok(result);
    }
    let _guard = ACCOUNT_WRITE.lock().await;
    if let CloudAction::Restore { ref account_token } = action {
        let mut result = call("v1/account", None, Some(account_token)).await?;
        credential(Some(account_token.clone())).await?;
        // A different account has a different key.
        forget_summaries_key().await;
        result["configured"] = true.into();
        result["connected"] = true.into();
        return Ok(result);
    }
    let token = match credential(None).await? {
        Some(token) => token,
        None => {
            let result = call("v1/accounts", Some(json!({})), None).await?;
            let token = result["account_token"]
                .as_str()
                .ok_or("Cloud account could not be created")?
                .to_owned();
            credential(Some(token.clone())).await?;
            token
        }
    };
    match action {
        CloudAction::Checkout { plan } => {
            let plan = match plan {
                CloudPlan::SummariesOnce => "summaries_once",
                CloudPlan::SummariesMonthly => "summaries_monthly",
                CloudPlan::SummariesAnnual => "summaries_annual",
                CloudPlan::Credits => "credits",
            };
            checkout_url(
                call(
                    "v1/checkouts",
                    Some(json!({"plan":plan,"idempotency_key":uuid::Uuid::new_v4().to_string()})),
                    Some(&token),
                )
                .await?,
            )
        }
        CloudAction::Billing => {
            checkout_url(call("v1/billing", Some(json!({})), Some(&token)).await?)
        }
        CloudAction::Status => unreachable!(),
        CloudAction::Restore { .. } => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_cloud_origin_and_billing_links_do_not_accept_redirect_targets_or_secrets() {
        assert!(normalize_endpoint("https://cloud.example/").is_ok());
        assert!(normalize_endpoint("http://127.0.0.1:8095/").is_ok());
        for url in [
            "http://cloud.example/",
            "https://key@cloud.example/",
            "https://cloud.example/?key=x",
            "https://cloud.example/path",
        ] {
            assert!(normalize_endpoint(url).is_err());
        }
        assert!(checkout_url(json!({"url":"https://checkout.stripe.com/c/pay/test"})).is_ok());
        for url in [
            "https://checkout.stripe.com.evil.test/",
            "http://checkout.stripe.com/",
            "https://secret@billing.stripe.com/",
        ] {
            assert!(checkout_url(json!({"url":url})).is_err());
        }
    }
}
