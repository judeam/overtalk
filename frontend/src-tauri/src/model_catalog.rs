//! Credential-scoped discovery for the cloud providers' model catalogs.
//! Provider modules own their public DTOs, fallback lists and chat-model filters.
use serde::de::DeserializeOwned;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::sync::RwLock;
use std::time::{Duration, Instant};

const CACHE_TTL: Duration = Duration::from_secs(300);

pub(crate) enum CatalogAuth {
    Bearer,
    Anthropic,
}

struct CachedModels<M> {
    credential_hash: [u8; 32],
    fetched_at: Instant,
    models: Vec<M>,
}

pub(crate) struct ModelCatalog<'a, M> {
    provider: &'a str,
    endpoint: &'a str,
    auth: CatalogAuth,
    cache: RwLock<Option<CachedModels<M>>>,
}

impl<'a, M: Clone + DeserializeOwned> ModelCatalog<'a, M> {
    pub(crate) const fn new(provider: &'a str, endpoint: &'a str, auth: CatalogAuth) -> Self {
        Self {
            provider,
            endpoint,
            auth,
            cache: RwLock::new(None),
        }
    }

    pub(crate) async fn get(
        &self,
        api_key: Option<String>,
        fallback: impl Fn() -> Vec<M>,
        accept: impl Fn(&M) -> bool,
    ) -> Result<Vec<M>, String> {
        let Some(key) = api_key
            .as_deref()
            .map(str::trim)
            .filter(|key| !key.is_empty())
        else {
            return Ok(fallback());
        };
        let credential_hash = Sha256::digest(key.as_bytes()).into();
        // The lock is released before network I/O. Cache at most one catalog and
        // retain only a fingerprint, never the credential itself.
        {
            let cache = self.cache.read().map_err(|e| e.to_string())?;
            if let Some(entry) = cache.as_ref().filter(|entry| {
                entry.credential_hash == credential_hash && entry.fetched_at.elapsed() < CACHE_TTL
            }) {
                return Ok(entry.models.clone());
            }
        }

        crate::security::privacy::guard_process_destination(self.endpoint)?;
        let request = crate::security::transport::client()?.get(self.endpoint);
        let request = match self.auth {
            CatalogAuth::Bearer => request.bearer_auth(key),
            CatalogAuth::Anthropic => request
                .header("x-api-key", key)
                .header("anthropic-version", "2023-06-01"),
        };
        let response = match request.timeout(Duration::from_secs(5)).send().await {
            Ok(response) if response.status().is_success() => response,
            Ok(response) => {
                log::warn!(
                    "{} model catalog returned {}; using fallback",
                    self.provider,
                    response.status()
                );
                return Ok(fallback());
            }
            Err(_) => {
                log::warn!(
                    "{} model catalog unavailable; using fallback",
                    self.provider
                );
                return Ok(fallback());
            }
        };
        #[derive(Deserialize)]
        struct Response<M> {
            data: Vec<M>,
        }

        let body = crate::security::transport::response_text(response, 2 * 1024 * 1024).await?;
        let models: Vec<M> = match serde_json::from_str::<Response<M>>(&body) {
            Ok(response) => response.data.into_iter().filter(accept).collect(),
            Err(_) => {
                log::warn!(
                    "{} model catalog was invalid; using fallback",
                    self.provider
                );
                return Ok(fallback());
            }
        };
        if models.is_empty() {
            return Ok(fallback());
        }
        *self.cache.write().map_err(|e| e.to_string())? = Some(CachedModels {
            credential_hash,
            fetched_at: Instant::now(),
            models: models.clone(),
        });
        Ok(models)
    }
}

#[cfg(test)]
mod tests;
