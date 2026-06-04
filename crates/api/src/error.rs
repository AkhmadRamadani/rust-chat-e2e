//! Shared error-to-response conversions for all API handlers.
//!
//! This module documents the complete error mapping table and provides shared
//! utilities that can be used by any handler module.
//!
//! ## Where each `IntoResponse` impl lives
//!
//! - [`auth::AuthError`] — implemented in `crates/auth/src/lib.rs` (must live
//!   there to satisfy Rust's orphan rule; `AuthError` is not defined in this
//!   crate).
//! - [`kds::KdsHandlerError`] — implemented in `crates/api/src/kds.rs`.
//! - [`ConversationHandlerError`] — implemented in
//!   `crates/api/src/conversations.rs`.
//! - [`GroupHandlerError`] — implemented in `crates/api/src/groups.rs`.
//!
//! ## Error mapping table (Requirements 11.1, 11.5)
//!
//! | Error | HTTP | `error_code` | `WWW-Authenticate` |
//! |---|---|---|---|
//! | `AuthError::MissingToken` | 401 | `missing_token` | `Bearer` |
//! | `AuthError::InvalidToken` | 401 | `invalid_token` | `Bearer` |
//! | `AuthError::ExpiredToken` | 401 | `invalid_token` | `Bearer error="invalid_token"` |
//! | `AuthError::UnknownTenant` | 401 | `unknown_tenant` | `Bearer error="unknown_tenant"` |
//! | `AuthError::TenantInactive` | 403 | `tenant_inactive` | — |
//! | `KdsError::InvalidSignature` | 422 | `invalid_signed_prekey_signature` | — |
//! | `KdsError::DeviceLimitReached` | 409 | `device_limit_reached` | — |
//! | `KdsError::Database(_)` | 503 | `storage_unavailable` | — |
//! | `MessagingError::NotParticipant` | 403 | `forbidden` | — |
//! | `MessagingError::Database(_)` | 503 | `storage_unavailable` | — |
//! | `GroupError::NotMember` | 403 | `not_a_member` | — |
//! | `GroupError::Database(_)` | 503 | `storage_unavailable` | — |
//! | Any panic | 500 | `internal_error` | — |
//! | Redis / any storage failure | 503 | `storage_unavailable` | — |

use axum::http::StatusCode;
use axum::Json;
use uuid::Uuid;

use common::{error_codes, ApiError};

// ── Storage error helper ──────────────────────────────────────────────────────

/// Build a `503 Service Unavailable` [`ApiError`] response for any storage
/// failure (PostgreSQL or Redis).
///
/// The `source` string is logged at `ERROR` level but is never included in the
/// response body to avoid leaking internal details to callers.
///
/// # Requirements: 11.1, 11.5
#[allow(dead_code)]
pub fn storage_error_response(source: &str) -> (StatusCode, Json<ApiError>) {
    tracing::error!(storage_error = source, "storage layer failure");
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(ApiError {
            error_code: error_codes::STORAGE_UNAVAILABLE.to_string(),
            message: "A storage error occurred; please retry.".to_string(),
            request_id: Uuid::new_v4(),
        }),
    )
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use auth::AuthError;
    use axum::response::IntoResponse;

    /// Verify the `IntoResponse` impl for `AuthError` lives in the `auth`
    /// crate and returns the correct status codes and error codes.
    #[test]
    fn auth_missing_token_yields_401() {
        let resp = AuthError::MissingToken.into_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn auth_invalid_token_yields_401() {
        let resp = AuthError::InvalidToken("bad sig".into()).into_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn auth_expired_token_yields_401_with_invalid_token_www_auth() {
        let resp = AuthError::ExpiredToken.into_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let www = resp
            .headers()
            .get(axum::http::header::WWW_AUTHENTICATE)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(
            www.contains("invalid_token"),
            "WWW-Authenticate must contain 'invalid_token', got: {www}"
        );
    }

    #[test]
    fn auth_unknown_tenant_yields_401_with_unknown_tenant_www_auth() {
        let resp = AuthError::UnknownTenant.into_response();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let www = resp
            .headers()
            .get(axum::http::header::WWW_AUTHENTICATE)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(
            www.contains("unknown_tenant"),
            "WWW-Authenticate must contain 'unknown_tenant', got: {www}"
        );
    }

    #[test]
    fn auth_tenant_inactive_yields_403() {
        let resp = AuthError::TenantInactive.into_response();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn auth_error_response_body_has_request_id() {
        use axum::body::to_bytes;

        let resp = AuthError::MissingToken.into_response();
        let (_, body) = resp.into_parts();
        let bytes = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(to_bytes(body, usize::MAX))
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        // error_code must be "missing_token"
        assert_eq!(value["error_code"], error_codes::MISSING_TOKEN);
        // request_id must be present and a valid UUID
        let rid = value["request_id"].as_str().unwrap();
        assert!(
            Uuid::parse_str(rid).is_ok(),
            "request_id must be a valid UUID, got: {rid}"
        );
    }

    #[test]
    fn auth_unknown_tenant_body_has_unknown_tenant_code() {
        use axum::body::to_bytes;

        let resp = AuthError::UnknownTenant.into_response();
        let (_, body) = resp.into_parts();
        let bytes = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(to_bytes(body, usize::MAX))
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["error_code"], error_codes::UNKNOWN_TENANT);
    }

    #[test]
    fn auth_tenant_inactive_body_has_tenant_inactive_code() {
        use axum::body::to_bytes;

        let resp = AuthError::TenantInactive.into_response();
        let (_, body) = resp.into_parts();
        let bytes = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(to_bytes(body, usize::MAX))
            .unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["error_code"], error_codes::TENANT_INACTIVE);
    }

    #[test]
    fn storage_error_response_returns_503() {
        let (status, _body) = storage_error_response("sqlx error: connection refused");
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    }
}
