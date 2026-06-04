// crates/auth — auth middleware and token validation

pub mod jwks_cache;
pub mod middleware;
pub mod refresh;
pub mod tenant_registry;
pub mod token_store;
pub mod validate;

pub use jwks_cache::JwksCache;
pub use middleware::{AuthLayer, AuthService};
pub use refresh::{issue_token_pair, refresh_access_token, RefreshRequest, RefreshState, TokenPair};
pub use tenant_registry::TenantRegistry;
pub use token_store::{PgTokenStore, RefreshTokenData, TokenStore, TokenStoreError};
pub use validate::validate_bearer_token;

use common::{DeviceId, TenantId, UserId};

// ── AuthError ─────────────────────────────────────────────────────────────────

/// Errors that can occur during bearer-token validation.
///
/// Each variant maps directly to an HTTP response:
///
/// | Variant | HTTP status | `error_code` |
/// |---|---|---|
/// | `MissingToken` | 401 | `missing_token` |
/// | `InvalidToken` | 401 | `invalid_token` |
/// | `ExpiredToken` | 401 | `invalid_token` |
/// | `UnknownTenant` | 401 | `unknown_tenant` |
/// | `TenantInactive` | 403 | `tenant_inactive` |
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    /// No `Authorization: Bearer` header was present on the request.
    #[error("missing token")]
    MissingToken,

    /// The JWT failed signature verification, was malformed, or contained
    /// invalid claims.  The inner string carries a human-readable diagnostic.
    #[error("invalid token: {0}")]
    InvalidToken(String),

    /// The JWT's `exp` claim is in the past.
    #[error("token has expired")]
    ExpiredToken,

    /// The JWT's `iss` claim does not match any tenant in the registry.
    #[error("unknown tenant")]
    UnknownTenant,

    /// The tenant was found in the registry but its `active` flag is `false`.
    #[error("tenant is inactive")]
    TenantInactive,
}

// ── AuthenticatedUser ─────────────────────────────────────────────────────────

/// Represents an authenticated caller extracted from a validated JWT.
///
/// Injected into `http::Extensions` by the auth middleware so that downstream
/// handlers can access the caller's identity without re-parsing the token.
#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    /// The tenant this token belongs to, resolved from the JWT `iss` claim.
    pub tenant_id: TenantId,
    /// The user identifier extracted from the JWT `sub` claim.
    pub user_id: UserId,
    /// The device identifier associated with this session, if known.
    pub device_id: Option<DeviceId>,
}


// ── IntoResponse for AuthError ───────────────────────────────────────────────

/// [`axum::response::IntoResponse`] for [`AuthError`].
///
/// Converts authentication errors into properly formatted HTTP responses with
/// correct error codes, messages, and `WWW-Authenticate` headers.
///
/// # Error mapping
///
/// | Variant | HTTP | `error_code` | `WWW-Authenticate` |
/// |---|---|---|---|
/// | `MissingToken` | 401 | `missing_token` | `Bearer` |
/// | `InvalidToken` | 401 | `invalid_token` | `Bearer` |
/// | `ExpiredToken` | 401 | `invalid_token` | `Bearer error="invalid_token"` |
/// | `UnknownTenant` | 401 | `unknown_tenant` | `Bearer error="unknown_tenant"` |
/// | `TenantInactive` | 403 | `tenant_inactive` | — |
impl axum::response::IntoResponse for AuthError {
    fn into_response(self) -> axum::response::Response {
        use axum::body::Body;
        use axum::http::{Response, StatusCode};
        use common::{error_codes, ApiError};
        use uuid::Uuid;

        let request_id = Uuid::new_v4();

        match self {
            AuthError::MissingToken => {
                let body = ApiError {
                    error_code: error_codes::MISSING_TOKEN.to_string(),
                    message: "No Authorization header was present on the request.".to_string(),
                    request_id,
                };
                let json = serde_json::to_vec(&body)
                    .unwrap_or_else(|_| br#"{"error_code":"missing_token"}"#.to_vec());

                Response::builder()
                    .status(StatusCode::UNAUTHORIZED)
                    .header(axum::http::header::WWW_AUTHENTICATE, "Bearer")
                    .header(axum::http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(json))
                    .unwrap_or_else(|_| Response::new(Body::empty()))
            }

            AuthError::InvalidToken(_) => {
                let body = ApiError {
                    error_code: error_codes::INVALID_TOKEN.to_string(),
                    message: "The provided token is invalid or could not be verified.".to_string(),
                    request_id,
                };
                let json = serde_json::to_vec(&body)
                    .unwrap_or_else(|_| br#"{"error_code":"invalid_token"}"#.to_vec());

                Response::builder()
                    .status(StatusCode::UNAUTHORIZED)
                    .header(axum::http::header::WWW_AUTHENTICATE, "Bearer")
                    .header(axum::http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(json))
                    .unwrap_or_else(|_| Response::new(Body::empty()))
            }

            AuthError::ExpiredToken => {
                let body = ApiError {
                    error_code: error_codes::INVALID_TOKEN.to_string(),
                    message: "The provided token has expired.".to_string(),
                    request_id,
                };
                let json = serde_json::to_vec(&body)
                    .unwrap_or_else(|_| br#"{"error_code":"invalid_token"}"#.to_vec());

                Response::builder()
                    .status(StatusCode::UNAUTHORIZED)
                    .header(
                        axum::http::header::WWW_AUTHENTICATE,
                        "Bearer error=\"invalid_token\"",
                    )
                    .header(axum::http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(json))
                    .unwrap_or_else(|_| Response::new(Body::empty()))
            }

            AuthError::UnknownTenant => {
                let body = ApiError {
                    error_code: error_codes::UNKNOWN_TENANT.to_string(),
                    message: "The issuer in this token is not a registered tenant.".to_string(),
                    request_id,
                };
                let json = serde_json::to_vec(&body)
                    .unwrap_or_else(|_| br#"{"error_code":"unknown_tenant"}"#.to_vec());

                Response::builder()
                    .status(StatusCode::UNAUTHORIZED)
                    .header(
                        axum::http::header::WWW_AUTHENTICATE,
                        "Bearer error=\"unknown_tenant\"",
                    )
                    .header(axum::http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(json))
                    .unwrap_or_else(|_| Response::new(Body::empty()))
            }

            AuthError::TenantInactive => {
                let body = ApiError {
                    error_code: error_codes::TENANT_INACTIVE.to_string(),
                    message: "This tenant has been deactivated.".to_string(),
                    request_id,
                };
                let json = serde_json::to_vec(&body)
                    .unwrap_or_else(|_| br#"{"error_code":"tenant_inactive"}"#.to_vec());

                Response::builder()
                    .status(StatusCode::FORBIDDEN)
                    .header(axum::http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(json))
                    .unwrap_or_else(|_| Response::new(Body::empty()))
            }
        }
    }
}
