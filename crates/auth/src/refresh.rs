//! `POST /auth/refresh` — access-token refresh handler.
//!
//! # Flow
//! 1. Parse the JSON body to extract the `refresh_token` string.
//! 2. Decode the refresh token (HS256, `REFRESH_TOKEN_SECRET` env var) to
//!    extract `jti`, `tenant_id`, `sub` (user_id), and `device_id`.
//! 3. Call [`TokenStore::is_revoked`] scoped to the decoded `tenant_id`.
//!    - If the token is revoked / expired / not found → HTTP 401.
//! 4. Revoke the consumed refresh token via [`TokenStore::revoke`].
//! 5. Issue a new access token (lifetime 3600 s) and a new refresh token
//!    (lifetime 30 days) using HS256.
//! 6. Store the new refresh token via [`TokenStore::store_refresh_token`].
//! 7. Return HTTP 200 with `{ access_token, refresh_token }`.
//!
//! Both tokens are signed with a shared secret loaded from the
//! `ACCESS_TOKEN_SECRET` / `REFRESH_TOKEN_SECRET` environment variables.
//! If those variables are absent the handler falls back to a hard-coded
//! development secret and logs a warning.

use std::sync::Arc;

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::Utc;
use common::{DeviceId, TenantId, UserId};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use tracing::warn;
use uuid::Uuid;

use crate::token_store::{RefreshTokenData, TokenStore, TokenStoreError};

// ── Constants ─────────────────────────────────────────────────────────────────

/// Lifetime of a newly-issued access token in seconds (1 hour).
const ACCESS_TOKEN_LIFETIME_SECS: i64 = 3600;

/// Lifetime of a newly-issued refresh token in seconds (30 days).
const REFRESH_TOKEN_LIFETIME_SECS: i64 = 30 * 24 * 3600;

/// Development-only fallback secret (never used in production when env vars
/// are properly set).
const DEV_SECRET: &str = "dev-secret-do-not-use-in-production";

// ── Request / response types ──────────────────────────────────────────────────

/// JSON body accepted by `POST /auth/refresh`.
#[derive(Debug, Deserialize)]
pub struct RefreshRequest {
    pub refresh_token: String,
}

/// Successful JSON response from `POST /auth/refresh`.
#[derive(Debug, Serialize)]
pub struct TokenPair {
    pub access_token: String,
    pub refresh_token: String,
}

// ── JWT claim structs ─────────────────────────────────────────────────────────

/// Claims embedded in a refresh token.
#[derive(Debug, Serialize, Deserialize)]
struct RefreshClaims {
    /// JWT ID — uniquely identifies this token in the revocation store.
    pub jti: String,
    /// Tenant identifier (UUID string).
    pub tenant_id: String,
    /// User identifier (OIDC `sub`).
    pub sub: String,
    /// Device identifier (UUID string), if known.
    pub device_id: Option<String>,
    /// Expiry (Unix timestamp seconds).
    pub exp: i64,
    /// Issued-at (Unix timestamp seconds).
    pub iat: i64,
}

/// Claims embedded in an access token.
#[derive(Debug, Serialize, Deserialize)]
struct AccessClaims {
    /// User identifier.
    pub sub: String,
    /// Tenant identifier.
    pub tenant_id: String,
    /// Device identifier, if known.
    pub device_id: Option<String>,
    /// Expiry.
    pub exp: i64,
    /// Issued-at.
    pub iat: i64,
}

// ── Axum handler state ────────────────────────────────────────────────────────

/// Shared state required by the refresh handler.
///
/// Wrap this in an `Arc` and register it as Axum `State`.
#[derive(Clone)]
pub struct RefreshState {
    pub token_store: Arc<dyn TokenStore>,
}

// ── Handler ───────────────────────────────────────────────────────────────────

/// Axum handler for `POST /auth/refresh`.
///
/// Validates the incoming refresh token, issues a new [`TokenPair`], revokes
/// the consumed token, and stores the new refresh token.
pub async fn refresh_access_token(
    State(state): State<Arc<RefreshState>>,
    Json(body): Json<RefreshRequest>,
) -> Response {
    match handle_refresh(&state, &body.refresh_token).await {
        Ok(pair) => (StatusCode::OK, Json(pair)).into_response(),
        Err(RefreshError::Unauthorized(msg)) => {
            let body = serde_json::json!({
                "error_code": "invalid_token",
                "message": msg,
            });
            (StatusCode::UNAUTHORIZED, Json(body)).into_response()
        }
        Err(RefreshError::Internal(msg)) => {
            let body = serde_json::json!({
                "error_code": "internal_error",
                "message": msg,
            });
            (StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response()
        }
    }
}

// ── Internal error type ───────────────────────────────────────────────────────

#[derive(Debug)]
enum RefreshError {
    Unauthorized(String),
    Internal(String),
}

// ── Core logic ────────────────────────────────────────────────────────────────

async fn handle_refresh(
    state: &RefreshState,
    raw_refresh_token: &str,
) -> Result<TokenPair, RefreshError> {
    // ── Step 1: decode the refresh token ─────────────────────────────────
    let refresh_secret = refresh_secret();
    let decoding_key = DecodingKey::from_secret(refresh_secret.as_bytes());

    let mut validation = Validation::new(Algorithm::HS256);
    // We check expiry ourselves via the store, but jsonwebtoken also checks it.
    // Allow some clock skew by keeping the default leeway (0 s is fine here).
    validation.validate_aud = false;

    let token_data = decode::<RefreshClaims>(raw_refresh_token, &decoding_key, &validation)
        .map_err(|e| {
            RefreshError::Unauthorized(format!("refresh token decode failed: {e}"))
        })?;

    let claims = token_data.claims;

    // ── Step 2: parse tenant_id / device_id ──────────────────────────────
    let tenant_uuid = Uuid::parse_str(&claims.tenant_id)
        .map_err(|_| RefreshError::Unauthorized("invalid tenant_id in token".to_string()))?;
    let tenant_id = TenantId(tenant_uuid);

    let device_id: Option<DeviceId> = claims
        .device_id
        .as_deref()
        .map(|s| Uuid::parse_str(s).map(DeviceId))
        .transpose()
        .map_err(|_| RefreshError::Unauthorized("invalid device_id in token".to_string()))?;

    let user_id = UserId(claims.sub.clone());

    // ── Step 3: check revocation in the store (tenant-scoped) ─────────────
    let is_revoked = state
        .token_store
        .is_revoked(tenant_id, &claims.jti)
        .await
        .map_err(|e| RefreshError::Internal(format!("token store error: {e}")))?;

    if is_revoked {
        return Err(RefreshError::Unauthorized(
            "refresh token is revoked, expired, or not found".to_string(),
        ));
    }

    // ── Step 4: revoke the consumed refresh token ─────────────────────────
    state
        .token_store
        .revoke(tenant_id, &claims.jti)
        .await
        .map_err(|e| match e {
            TokenStoreError::NotFound => {
                RefreshError::Unauthorized("refresh token not found".to_string())
            }
            other => RefreshError::Internal(format!("revoke failed: {other}")),
        })?;

    // ── Step 5: issue new access token (3600 s) ───────────────────────────
    let now = Utc::now().timestamp();

    let access_claims = AccessClaims {
        sub: claims.sub.clone(),
        tenant_id: claims.tenant_id.clone(),
        device_id: claims.device_id.clone(),
        exp: now + ACCESS_TOKEN_LIFETIME_SECS,
        iat: now,
    };

    let access_secret = access_secret();
    let encoding_key = EncodingKey::from_secret(access_secret.as_bytes());
    let access_token = encode(&Header::new(Algorithm::HS256), &access_claims, &encoding_key)
        .map_err(|e| RefreshError::Internal(format!("access token encode failed: {e}")))?;

    // ── Step 6: issue new refresh token (30 days) ─────────────────────────
    let new_jti = Uuid::new_v4().to_string();
    let refresh_expires_at = now + REFRESH_TOKEN_LIFETIME_SECS;

    let new_refresh_claims = RefreshClaims {
        jti: new_jti.clone(),
        tenant_id: claims.tenant_id.clone(),
        sub: claims.sub.clone(),
        device_id: claims.device_id.clone(),
        exp: refresh_expires_at,
        iat: now,
    };

    let refresh_encoding_key = EncodingKey::from_secret(refresh_secret.as_bytes());
    let new_refresh_token = encode(
        &Header::new(Algorithm::HS256),
        &new_refresh_claims,
        &refresh_encoding_key,
    )
    .map_err(|e| RefreshError::Internal(format!("refresh token encode failed: {e}")))?;

    // ── Step 7: store the new refresh token ───────────────────────────────
    let expires_at = chrono::DateTime::<Utc>::from_timestamp(refresh_expires_at, 0)
        .unwrap_or_else(|| Utc::now() + chrono::Duration::seconds(REFRESH_TOKEN_LIFETIME_SECS));

    // device_id is required by RefreshTokenData; default to a new UUID when
    // the token does not carry one (shouldn't happen in normal flow).
    let effective_device_id = device_id.unwrap_or_else(|| DeviceId(Uuid::new_v4()));

    let new_token_data = RefreshTokenData {
        jti: new_jti,
        tenant_id,
        user_id,
        device_id: effective_device_id,
        expires_at,
    };

    state
        .token_store
        .store_refresh_token(tenant_id, new_token_data)
        .await
        .map_err(|e| RefreshError::Internal(format!("store_refresh_token failed: {e}")))?;

    Ok(TokenPair {
        access_token,
        refresh_token: new_refresh_token,
    })
}

// ── Secret helpers ────────────────────────────────────────────────────────────

fn access_secret() -> String {
    match std::env::var("ACCESS_TOKEN_SECRET") {
        Ok(s) if !s.is_empty() => s,
        _ => {
            warn!(
                "ACCESS_TOKEN_SECRET env var not set; using dev fallback — \
                 DO NOT use this in production"
            );
            DEV_SECRET.to_string()
        }
    }
}

fn refresh_secret() -> String {
    match std::env::var("REFRESH_TOKEN_SECRET") {
        Ok(s) if !s.is_empty() => s,
        _ => {
            warn!(
                "REFRESH_TOKEN_SECRET env var not set; using dev fallback — \
                 DO NOT use this in production"
            );
            DEV_SECRET.to_string()
        }
    }
}

// ── Public helpers for issuing an initial token pair ─────────────────────────

/// Issue a brand-new [`TokenPair`] and persist the refresh token.
///
/// This helper is intended to be called by an initial login / OIDC callback
/// handler (not yet wired) so that the token format stays consistent.
///
/// Returns `None` only when JWT encoding unexpectedly fails; callers should
/// treat that as an internal error.
pub async fn issue_token_pair(
    token_store: &dyn TokenStore,
    tenant_id: TenantId,
    user_id: &UserId,
    device_id: Option<DeviceId>,
) -> Result<TokenPair, String> {
    let now = Utc::now().timestamp();
    let jti = Uuid::new_v4().to_string();

    // ── Access token ──────────────────────────────────────────────────────
    let access_claims = AccessClaims {
        sub: user_id.0.clone(),
        tenant_id: tenant_id.0.to_string(),
        device_id: device_id.map(|d| d.0.to_string()),
        exp: now + ACCESS_TOKEN_LIFETIME_SECS,
        iat: now,
    };

    let access_secret = access_secret();
    let enc_key = EncodingKey::from_secret(access_secret.as_bytes());
    let access_token = encode(&Header::new(Algorithm::HS256), &access_claims, &enc_key)
        .map_err(|e| format!("access token encode: {e}"))?;

    // ── Refresh token ─────────────────────────────────────────────────────
    let refresh_expires_secs = now + REFRESH_TOKEN_LIFETIME_SECS;
    let refresh_claims = RefreshClaims {
        jti: jti.clone(),
        tenant_id: tenant_id.0.to_string(),
        sub: user_id.0.clone(),
        device_id: device_id.map(|d| d.0.to_string()),
        exp: refresh_expires_secs,
        iat: now,
    };

    let refresh_secret = refresh_secret();
    let ref_enc_key = EncodingKey::from_secret(refresh_secret.as_bytes());
    let refresh_token = encode(&Header::new(Algorithm::HS256), &refresh_claims, &ref_enc_key)
        .map_err(|e| format!("refresh token encode: {e}"))?;

    // ── Persist the refresh token ─────────────────────────────────────────
    let expires_at =
        chrono::DateTime::<Utc>::from_timestamp(refresh_expires_secs, 0).unwrap_or_else(|| {
            Utc::now() + chrono::Duration::seconds(REFRESH_TOKEN_LIFETIME_SECS)
        });

    let effective_device_id = device_id.unwrap_or_else(|| DeviceId(Uuid::new_v4()));

    let token_data = RefreshTokenData {
        jti,
        tenant_id,
        user_id: user_id.clone(),
        device_id: effective_device_id,
        expires_at,
    };

    token_store
        .store_refresh_token(tenant_id, token_data)
        .await
        .map_err(|e| format!("store_refresh_token: {e}"))?;

    Ok(TokenPair {
        access_token,
        refresh_token,
    })
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token_store::{RefreshTokenData, TokenStore, TokenStoreError};
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::Mutex;

    // ── In-memory mock token store ────────────────────────────────────────

    #[derive(Default)]
    struct MockTokenStore {
        tokens: Mutex<HashMap<String, (RefreshTokenData, bool /* revoked */)>>,
    }

    #[async_trait]
    impl TokenStore for MockTokenStore {
        async fn store_refresh_token(
            &self,
            _tenant_id: TenantId,
            data: RefreshTokenData,
        ) -> Result<(), TokenStoreError> {
            self.tokens
                .lock()
                .unwrap()
                .insert(data.jti.clone(), (data, false));
            Ok(())
        }

        async fn revoke(
            &self,
            _tenant_id: TenantId,
            jti: &str,
        ) -> Result<(), TokenStoreError> {
            let mut map = self.tokens.lock().unwrap();
            match map.get_mut(jti) {
                Some(entry) => {
                    entry.1 = true;
                    Ok(())
                }
                None => Err(TokenStoreError::NotFound),
            }
        }

        async fn is_revoked(
            &self,
            _tenant_id: TenantId,
            jti: &str,
        ) -> Result<bool, TokenStoreError> {
            let map = self.tokens.lock().unwrap();
            match map.get(jti) {
                None => Ok(true), // not found → treat as revoked
                Some((data, revoked)) => {
                    if *revoked || data.expires_at < Utc::now() {
                        Ok(true)
                    } else {
                        Ok(false)
                    }
                }
            }
        }
    }

    fn make_store() -> Arc<MockTokenStore> {
        Arc::new(MockTokenStore::default())
    }

    fn test_tenant_id() -> TenantId {
        TenantId(Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap())
    }

    fn test_user() -> UserId {
        UserId("user-abc".to_string())
    }

    fn test_device() -> DeviceId {
        DeviceId(Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap())
    }

    /// Set the env vars to a known secret for deterministic tests.
    fn set_test_secrets() {
        std::env::set_var("ACCESS_TOKEN_SECRET", "test-access-secret");
        std::env::set_var("REFRESH_TOKEN_SECRET", "test-refresh-secret");
    }

    // ── Tests ─────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn issue_then_refresh_succeeds() {
        set_test_secrets();

        let store = make_store();
        let tenant_id = test_tenant_id();
        let user_id = test_user();
        let device_id = test_device();

        // Issue an initial token pair.
        let pair = issue_token_pair(store.as_ref(), tenant_id, &user_id, Some(device_id))
            .await
            .expect("issue_token_pair should succeed");

        // Use the refresh token to obtain a new pair.
        let state = Arc::new(RefreshState {
            token_store: store.clone(),
        });
        let result = handle_refresh(&state, &pair.refresh_token).await;
        assert!(result.is_ok(), "refresh should succeed: {result:?}");
    }

    #[tokio::test]
    async fn refresh_revokes_old_token() {
        set_test_secrets();

        let store = make_store();
        let tenant_id = test_tenant_id();
        let user_id = test_user();
        let device_id = test_device();

        let pair = issue_token_pair(store.as_ref(), tenant_id, &user_id, Some(device_id))
            .await
            .unwrap();

        let state = Arc::new(RefreshState {
            token_store: store.clone(),
        });

        // First refresh — should succeed.
        let _new_pair = handle_refresh(&state, &pair.refresh_token)
            .await
            .expect("first refresh should succeed");

        // Second use of the same token — should fail (already revoked).
        let second = handle_refresh(&state, &pair.refresh_token).await;
        assert!(
            matches!(second, Err(RefreshError::Unauthorized(_))),
            "second use of the same refresh token must be rejected"
        );
    }

    #[tokio::test]
    async fn refresh_with_invalid_token_returns_unauthorized() {
        set_test_secrets();

        let store = make_store();
        let state = Arc::new(RefreshState {
            token_store: store.clone(),
        });

        let result = handle_refresh(&state, "not.a.valid.jwt").await;
        assert!(
            matches!(result, Err(RefreshError::Unauthorized(_))),
            "invalid JWT should be rejected with Unauthorized"
        );
    }

    #[tokio::test]
    async fn refresh_with_unknown_token_returns_unauthorized() {
        set_test_secrets();

        // Build a syntactically valid refresh token that is NOT in the store.
        let now = Utc::now().timestamp();
        let claims = RefreshClaims {
            jti: Uuid::new_v4().to_string(),
            tenant_id: test_tenant_id().0.to_string(),
            sub: "ghost-user".to_string(),
            device_id: None,
            exp: now + 3600,
            iat: now,
        };
        let key = EncodingKey::from_secret(b"test-refresh-secret");
        let token = encode(&Header::new(Algorithm::HS256), &claims, &key).unwrap();

        let store = make_store(); // empty — no tokens registered
        let state = Arc::new(RefreshState {
            token_store: store.clone(),
        });

        let result = handle_refresh(&state, &token).await;
        assert!(
            matches!(result, Err(RefreshError::Unauthorized(_))),
            "token not in store should be rejected"
        );
    }

    #[tokio::test]
    async fn new_access_token_has_correct_sub() {
        set_test_secrets();

        let store = make_store();
        let tenant_id = test_tenant_id();
        let user_id = UserId("alice".to_string());
        let device_id = test_device();

        let pair = issue_token_pair(store.as_ref(), tenant_id, &user_id, Some(device_id))
            .await
            .unwrap();

        let state = Arc::new(RefreshState {
            token_store: store.clone(),
        });
        let new_pair = handle_refresh(&state, &pair.refresh_token)
            .await
            .expect("refresh should succeed");

        // Decode the new access token and verify the `sub` claim.
        let dec_key = DecodingKey::from_secret(b"test-access-secret");
        let mut val = Validation::new(Algorithm::HS256);
        val.validate_aud = false;
        let decoded = decode::<AccessClaims>(&new_pair.access_token, &dec_key, &val)
            .expect("new access token must be decodable");

        assert_eq!(decoded.claims.sub, "alice");
    }
}
