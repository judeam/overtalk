use anyhow::{bail, Context, Result};
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
};
use url::Url;

pub const STRIPE_VERSION: &str = "2026-09-30.endive";
pub const MODEL: &str = "glm-5.3-flash";
/// The monthly allowance advertised for private summaries. Each Privatemode key's monthly token
/// limits must cover it for dense and multilingual meetings (see README).
pub const SUMMARY_HOURS: u32 = 200;

/// How private AI summaries reach Privatemode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// Each subscriber's desktop runs Privatemode's proxy with its own key from
    /// this server; meeting text never passes through here.
    EndToEnd,
    /// Legacy: this server forwards requests through its own proxy and can read them.
    Gateway,
}

pub struct Config {
    pub bind: SocketAddr,
    pub database: PathBuf,
    pub public_url: Url,
    pub approved: bool,
    pub live: bool,
    pub stripe_key: String,
    pub webhook_secret: String,
    pub lifetime_price: String,
    pub monthly_price: String,
    /// Optional; empty when no annual price is offered.
    pub annual_price: String,
    pub topup_price: String,
    /// The ISO 4217 code, lowercase, that every Stripe price and payment must use.
    pub currency: String,
    pub route: Route,
    pub proxy: Url,
    pub proxy_key: String,
    // The customer rate card, in USD microcredits per million tokens; it is not
    // a live provider invoice.
    pub input_rate: u64,
    pub output_rate: u64,
    pub cached_rate: u64,
}

/// Private summaries are sold in euros by default; the legacy gateway route meters its credits
/// in USD, so it accepts only USD payments.
fn currency(route: Route, configured: Option<&str>) -> Result<String> {
    let currency = configured
        .unwrap_or(if route == Route::EndToEnd {
            "eur"
        } else {
            "usd"
        })
        .to_owned();
    if currency.len() != 3 || !currency.bytes().all(|c| c.is_ascii_lowercase()) {
        bail!("OVERTALK_STRIPE_CURRENCY must be a lowercase ISO 4217 code, such as eur");
    }
    if route == Route::Gateway && currency != "usd" {
        bail!("The gateway route meters credits in USD; use usd prices");
    }
    Ok(currency)
}

fn value(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_owned())
}

fn secret_file(name: &str) -> Result<String> {
    let path =
        PathBuf::from(std::env::var(name).with_context(|| format!("Set {name} to a secret file"))?);
    let secret = private_file(&path)?.trim().to_owned();
    if secret.is_empty() || secret.len() > 4096 || secret.chars().any(char::is_control) {
        bail!("Invalid secret file");
    }
    Ok(secret)
}

/// A regular file only its owner can read: secrets and imported key lists.
pub fn private_file(path: &Path) -> Result<String> {
    let meta = std::fs::symlink_metadata(path)?;
    if !meta.file_type().is_file() {
        bail!("Secret must be a regular file");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o077 != 0 {
            bail!("Secret files must be readable only by their owner");
        }
    }
    Ok(std::fs::read_to_string(path)?)
}

pub fn public_origin(value: &str) -> Result<Url> {
    let mut url = Url::parse(value)?;
    let local = url.host_str().is_some_and(|host| {
        host.parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
    });
    if (url.scheme() != "https" && !(url.scheme() == "http" && local))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/")
    {
        bail!("Use an HTTPS origin, without credentials, query or path");
    }
    url.set_path("/");
    Ok(url)
}

impl Config {
    pub fn load() -> Result<Self> {
        let approved = value("OVERTALK_CLOUD_ENABLED", "false") == "true"
            && !value("OVERTALK_PRIVATEMODE_APPROVAL_REFERENCE", "")
                .trim()
                .is_empty();
        let live = value("OVERTALK_STRIPE_LIVE", "false") == "true";
        let bind: SocketAddr = value("OVERTALK_CLOUD_BIND", "127.0.0.1:8095").parse()?;
        if !bind.ip().is_loopback() {
            bail!("Bind the gateway to loopback behind an HTTPS reverse proxy");
        }
        let public_url =
            public_origin(&value("OVERTALK_CLOUD_PUBLIC_URL", "http://127.0.0.1:8095"))?;
        if approved && live && public_url.scheme() != "https" {
            bail!("Live billing requires HTTPS");
        }
        let stripe_key = if approved {
            secret_file("OVERTALK_STRIPE_KEY_FILE")?
        } else {
            String::new()
        };
        if approved && !stripe_key.starts_with(if live { "sk_live_" } else { "sk_test_" }) {
            bail!("Stripe key does not match the configured test/live mode");
        }
        let webhook_secret = if approved {
            secret_file("OVERTALK_STRIPE_WEBHOOK_FILE")?
        } else {
            String::new()
        };
        if approved && !webhook_secret.starts_with("whsec_") {
            bail!("Invalid Stripe webhook secret");
        }
        let proxy = Url::parse(&value(
            "OVERTALK_PRIVATEMODE_PROXY",
            "http://127.0.0.1:8080/v1/",
        ))?;
        let local = proxy.host_str().is_some_and(|host| {
            host.parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
        });
        if !local
            || !matches!(proxy.scheme(), "http" | "https")
            || proxy.username() != ""
            || proxy.password().is_some()
            || proxy.query().is_some()
            || proxy.fragment().is_some()
            || proxy.path() != "/v1/"
        {
            bail!("The provider proxy must be a loopback /v1/ URL");
        }
        let route = match value("OVERTALK_CLOUD_ROUTE", "end-to-end").as_str() {
            "end-to-end" => Route::EndToEnd,
            "gateway" => Route::Gateway,
            _ => bail!("OVERTALK_CLOUD_ROUTE must be end-to-end or gateway"),
        };
        // The end-to-end route sells subscriptions only; one-time plans and
        // credit top-ups belong to the gateway route.
        let price = |key, optional: bool| -> Result<String> {
            let result = value(key, "");
            if approved
                && !(optional && result.is_empty())
                && (!result.starts_with("price_") || result.len() > 120)
            {
                bail!("Configure each Stripe price ID");
            }
            Ok(result)
        };
        let subscriptions_only = route == Route::EndToEnd;
        Ok(Self {
            bind,
            database: Path::new(&value(
                "OVERTALK_CLOUD_DATABASE",
                "cloud-state/credits.sqlite",
            ))
            .to_owned(),
            public_url,
            approved,
            live,
            stripe_key,
            webhook_secret,
            lifetime_price: price("OVERTALK_STRIPE_LIFETIME_PRICE", subscriptions_only)?,
            monthly_price: price("OVERTALK_STRIPE_MONTHLY_PRICE", false)?,
            annual_price: {
                let annual = value("OVERTALK_STRIPE_ANNUAL_PRICE", "");
                if !annual.is_empty() && (!annual.starts_with("price_") || annual.len() > 120) {
                    bail!("Invalid annual Stripe price ID");
                }
                annual
            },
            topup_price: price("OVERTALK_STRIPE_TOPUP_PRICE", subscriptions_only)?,
            currency: currency(
                route,
                std::env::var("OVERTALK_STRIPE_CURRENCY").ok().as_deref(),
            )?,
            route,
            proxy,
            proxy_key: if approved && std::env::var_os("OVERTALK_PRIVATEMODE_KEY_FILE").is_some() {
                secret_file("OVERTALK_PRIVATEMODE_KEY_FILE")?
            } else {
                "placeholder".into()
            },
            input_rate: 250_000,
            output_rate: 800_000,
            cached_rate: 60_000,
        })
    }

    pub fn price(&self, plan: &str) -> Option<&str> {
        match plan {
            "summaries_once" => Some(self.lifetime_price.as_str()),
            "summaries_monthly" => Some(self.monthly_price.as_str()),
            "summaries_annual" => Some(self.annual_price.as_str()),
            "credits" => Some(self.topup_price.as_str()),
            _ => None,
        }
        .filter(|price| !price.is_empty())
    }
}

#[cfg(test)]
pub(crate) fn test_config() -> Config {
    Config {
        bind: "127.0.0.1:0".parse().unwrap(),
        database: "unused".into(),
        public_url: "https://cloud.example/".parse().unwrap(),
        approved: false,
        live: false,
        stripe_key: "test-private-stripe-key".into(),
        webhook_secret: "test-private-webhook-key".into(),
        lifetime_price: "price_once".into(),
        monthly_price: "price_month".into(),
        annual_price: "price_year".into(),
        topup_price: "price_topup".into(),
        currency: "usd".into(),
        route: Route::Gateway,
        proxy: "http://127.0.0.1:8080/v1/".parse().unwrap(),
        proxy_key: "test-private-provider-key".into(),
        input_rate: 250_000,
        output_rate: 800_000,
        cached_rate: 60_000,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summaries_sell_in_euros_and_the_gateway_route_only_in_dollars() {
        assert_eq!(currency(Route::EndToEnd, None).unwrap(), "eur");
        assert_eq!(currency(Route::EndToEnd, Some("chf")).unwrap(), "chf");
        assert_eq!(currency(Route::Gateway, None).unwrap(), "usd");
        assert!(currency(Route::Gateway, Some("eur")).is_err());
        for bad in ["EUR", "euro", "", "e1r"] {
            assert!(currency(Route::EndToEnd, Some(bad)).is_err(), "{bad}");
        }
    }
}
