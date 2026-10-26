use std::{future::Future, hash::Hash};

use moka::future::Cache;
use tracing::error;

use super::error::ApplicationError;

/// Returns the cached value for `key`, or runs `fetch` once (coalescing concurrent callers) and caches the result.
pub async fn get_or_fetch<K, V, F>(
    cache: &Cache<K, V>,
    key: K,
    fetch: F,
    description: &str,
) -> Result<V, ApplicationError>
where
    K: Hash + Eq + Send + Sync + 'static,
    V: Clone + Send + Sync + 'static,
    F: Future<Output = anyhow::Result<V>>,
{
    cache.try_get_with(key, fetch).await.map_err(|err| {
        error!(error = ?err, "Failed to fetch {description}");
        ApplicationError::BadGateway(format!("Failed to fetch {description}"))
    })
}
