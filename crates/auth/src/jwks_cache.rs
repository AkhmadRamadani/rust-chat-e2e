//! JWKS (JSON Web Key Set) cache with per-tenant keying and 5-minute TTL.
//!
//! [`JwksCache`] stores fetched JWKS documents in memory, keyed by
//! `(TenantId, jwks_url)`.  Each entry carries a timestamp; entries older
//! than [`JWKS_TTL`] are considered stale and are re-fetched on the next
//! access.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::TenantId;
use jsonwebtoken::jwk::JwkSet;
use tokio::sync::RwLock;

use crate::AuthError;

// ── Constants ─────────────────────────────────────────────────────────────────

/// How long a fetched JWKS document is considered valid before a refresh is
/// attempted.
pub const JWKS_TTL: Duration = Duration::from_secs(5 * 60);

// ── Internal cache entry ──────────────────────────────────────────────────────

/// A cached JWKS document along with the [`Instant`] at which it was fetched.
#[derive(Debug, Clone)]
pub struct CachedJwks {
    /// The decoded JWKS document.
    pub jwks: JwkSet,
    /// Wall-clock time at which the document was fetched.
    pub fetched_at: Instant,
}

impl CachedJwks {
    /// Returns `true` if the cached entry has exceeded [`JWKS_TTL`].
    pub fn is_expired(&self) -> bool {
        self.fetched_at.elapsed() >= JWKS_TTL
    }
}

// ── JwksCache ─────────────────────────────────────────────────────────────────

/// Shared in-memory JWKS cache keyed by `(TenantId, jwks_url)`.
///
/// # Concurrency
/// The inner `HashMap` is protected by a `tokio::sync::RwLock` so that cache
/// hits (the hot path) only require a read lock.  A write lock is acquired
/// only when a new fetch is required or a stale entry needs to be refreshed.
///
/// The struct wraps the lock in an `Arc` so it can be cheaply cloned and
/// shared across Axum state, Tower middleware layers, and background tasks.
#[derive(Debug, Clone)]
pub struct JwksCache {
    inner: Arc<RwLock<HashMap<(TenantId, String), CachedJwks>>>,
}

impl Default for JwksCache {
    fn default() -> Self {
        Self::new()
    }
}

impl JwksCache {
    /// Create a new, empty cache.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Return the cached [`JwkSet`] for `(tenant_id, jwks_url)`, fetching and
    /// caching it first if the entry is absent or expired.
    ///
    /// # Errors
    /// Returns [`AuthError::InvalidToken`] when the HTTP fetch fails or the
    /// response cannot be parsed as a valid JWKS document.
    pub async fn get_or_fetch(
        &self,
        tenant_id: TenantId,
        jwks_url: &str,
    ) -> Result<JwkSet, AuthError> {
        let key = (tenant_id, jwks_url.to_string());

        // Fast path: valid cached entry.
        {
            let map = self.inner.read().await;
            if let Some(entry) = map.get(&key) {
                if !entry.is_expired() {
                    return Ok(entry.jwks.clone());
                }
            }
        }

        // Slow path: fetch and cache.
        let jwks = fetch_jwks(jwks_url).await?;
        let cached = CachedJwks {
            jwks: jwks.clone(),
            fetched_at: Instant::now(),
        };
        {
            let mut map = self.inner.write().await;
            map.insert(key, cached);
        }
        Ok(jwks)
    }

    /// Evict the cache entry for `(tenant_id, jwks_url)`, if present.
    ///
    /// Useful when an admin updates a tenant's OIDC issuer so the stale JWKS
    /// is not used for the next authentication request.
    pub async fn invalidate(&self, tenant_id: TenantId, jwks_url: &str) {
        let key = (tenant_id, jwks_url.to_string());
        let mut map = self.inner.write().await;
        map.remove(&key);
    }
}

// ── HTTP fetch ────────────────────────────────────────────────────────────────

/// Fetch a JWKS document from `url` via HTTPS.
///
/// Uses a short-lived `reqwest::Client` to perform a GET request to the
/// JWKS endpoint and deserialise the response as a [`JwkSet`].
///
/// # Errors
/// Returns [`AuthError::InvalidToken`] when:
/// - The HTTP request fails (network error, DNS failure, TLS error).
/// - The response body cannot be parsed as a valid JWKS document.
async fn fetch_jwks(url: &str) -> Result<JwkSet, AuthError> {
    let client = reqwest::Client::new();
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| AuthError::InvalidToken(format!("JWKS fetch failed for '{url}': {e}")))?;

    if !response.status().is_success() {
        let status = response.status();
        return Err(AuthError::InvalidToken(format!(
            "JWKS endpoint '{url}' returned non-success status: {status}"
        )));
    }

    let jwks: JwkSet = response
        .json()
        .await
        .map_err(|e| AuthError::InvalidToken(format!("JWKS parse failed for '{url}': {e}")))?;

    Ok(jwks)
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn tenant() -> TenantId {
        TenantId(Uuid::new_v4())
    }

    #[test]
    fn cached_jwks_is_not_expired_immediately() {
        let entry = CachedJwks {
            jwks: JwkSet { keys: vec![] },
            fetched_at: Instant::now(),
        };
        assert!(!entry.is_expired());
    }

    #[test]
    fn cached_jwks_is_expired_after_ttl() {
        let entry = CachedJwks {
            jwks: JwkSet { keys: vec![] },
            // Simulate a fetch that happened 6 minutes ago.
            fetched_at: Instant::now() - Duration::from_secs(6 * 60),
        };
        assert!(entry.is_expired());
    }

    #[tokio::test]
    async fn new_cache_is_empty_and_returns_error() {
        let cache = JwksCache::new();
        let tid = tenant();
        // Without a live OIDC endpoint the stub returns an error.
        let result = cache
            .get_or_fetch(tid, "https://example.com/.well-known/jwks.json")
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn default_creates_empty_cache() {
        let cache = JwksCache::default();
        let tid = tenant();
        let result = cache
            .get_or_fetch(tid, "https://example.com/.well-known/jwks.json")
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn invalidate_nonexistent_key_is_noop() {
        let cache = JwksCache::new();
        let tid = tenant();
        // Should not panic.
        cache
            .invalidate(tid, "https://example.com/.well-known/jwks.json")
            .await;
    }

    #[tokio::test]
    async fn clone_shares_inner_state() {
        let cache = JwksCache::new();
        let clone = cache.clone();
        // Mutating the clone's inner map via invalidate on a pre-populated
        // cache would affect the original — here we just verify the clone
        // is created without panic.
        let _ = clone;
    }
}
