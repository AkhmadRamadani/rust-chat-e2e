//! Auth Tower middleware layer for tenant OIDC bearer-token validation.
//!
//! [`AuthLayer`] wraps any inner service and validates every incoming request
//! against a tenant-scoped OIDC bearer token.  On success it injects
//! [`AuthenticatedUser`] into [`http::Extensions`] so downstream Axum handlers
//! can extract it via `Extension<AuthenticatedUser>`.
//!
//! # Response codes
//!
//! | Condition | HTTP | `WWW-Authenticate` |
//! |---|---|---|
//! | `Authorization` header absent | 401 | `Bearer` |
//! | Token present but signature/claims invalid | 401 | `Bearer` |
//! | Token expired | 401 | `Bearer error="invalid_token"` |
//! | Unknown tenant (`iss` not in registry) | 401 | `Bearer error="unknown_tenant"` |
//! | Tenant inactive | 403 | — |

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::body::Body;
use axum::extract::FromRequestParts;
use axum::http::{Request, Response, StatusCode};
use axum::response::IntoResponse;
use tower::{Layer, Service};

use crate::{AuthError, AuthenticatedUser, JwksCache, TenantRegistry};
use crate::validate::validate_bearer_token;

// ── AuthLayer ─────────────────────────────────────────────────────────────────

/// Tower [`Layer`] that wraps an inner service with [`AuthService`].
///
/// Both `registry` and `jwks_cache` are stored behind `Arc` so the layer can
/// be cloned cheaply and the allocations are shared across all worker threads.
#[derive(Debug, Clone)]
pub struct AuthLayer {
    registry: Arc<TenantRegistry>,
    jwks_cache: Arc<JwksCache>,
}

impl AuthLayer {
    /// Create a new layer from shared registry and JWKS-cache handles.
    pub fn new(registry: Arc<TenantRegistry>, jwks_cache: Arc<JwksCache>) -> Self {
        Self {
            registry,
            jwks_cache,
        }
    }
}

impl<S> Layer<S> for AuthLayer {
    type Service = AuthService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        AuthService {
            inner,
            registry: Arc::clone(&self.registry),
            jwks_cache: Arc::clone(&self.jwks_cache),
        }
    }
}

// ── AuthService ───────────────────────────────────────────────────────────────

/// Tower [`Service`] that enforces tenant OIDC bearer-token validation.
#[derive(Debug, Clone)]
pub struct AuthService<S> {
    inner: S,
    registry: Arc<TenantRegistry>,
    jwks_cache: Arc<JwksCache>,
}

impl<S> Service<Request<Body>> for AuthService<S>
where
    S: Service<Request<Body>, Response = Response<Body>> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = Response<Body>;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<Body>) -> Self::Future {
        // ── Extract bearer token from Authorization header ──────────────────
        let token = match extract_bearer(req.headers()) {
            Some(t) if !t.is_empty() => t,
            _ => {
                // Missing or empty token → 401 with plain Bearer challenge.
                let resp = unauthorized_bearer(
                    None,
                    common::error_codes::MISSING_TOKEN,
                    "No Authorization header was present on the request.",
                );
                return Box::pin(async move { Ok(resp) });
            }
        };

        // Clone the shared state into the async block.
        let registry = Arc::clone(&self.registry);
        let jwks_cache = Arc::clone(&self.jwks_cache);

        // We need to call the inner service — clone before moving into the future
        // so the original `self` is not partially moved.
        let mut inner = self.inner.clone();

        Box::pin(async move {
            match validate_bearer_token(&token, &registry, &jwks_cache).await {
                Ok(user) => {
                    // Inject the authenticated user into request extensions.
                    req.extensions_mut().insert(user);
                    inner.call(req).await
                }
                Err(AuthError::MissingToken) => Ok(unauthorized_bearer(
                    None,
                    common::error_codes::MISSING_TOKEN,
                    "No Authorization header was present on the request.",
                )),
                Err(AuthError::InvalidToken(_)) => Ok(unauthorized_bearer(
                    None,
                    common::error_codes::INVALID_TOKEN,
                    "The provided token is invalid or could not be verified.",
                )),
                Err(AuthError::ExpiredToken) => Ok(unauthorized_bearer(
                    Some("error=\"invalid_token\""),
                    common::error_codes::INVALID_TOKEN,
                    "The provided token has expired.",
                )),
                Err(AuthError::UnknownTenant) => Ok(unauthorized_bearer(
                    Some("error=\"unknown_tenant\""),
                    common::error_codes::UNKNOWN_TENANT,
                    "The issuer in this token is not a registered tenant.",
                )),
                Err(AuthError::TenantInactive) => Ok(forbidden_tenant_inactive()),
            }
        })
    }
}

// ── Axum extractor ────────────────────────────────────────────────────────────

/// Axum extractor for the authenticated caller.
///
/// Downstream handlers can declare `Extension(user): Extension<AuthenticatedUser>`
/// (or `AuthenticatedUser` directly if this `FromRequestParts` impl is used)
/// to access the validated identity injected by [`AuthService`].
///
/// # Errors
/// Returns HTTP 401 if the auth middleware was not applied to this route or if
/// the extension is otherwise absent from the request.
#[axum::async_trait]
impl<S> FromRequestParts<S> for AuthenticatedUser
where
    S: Send + Sync,
{
    type Rejection = Response<Body>;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<AuthenticatedUser>()
            .cloned()
            .ok_or_else(|| {
                unauthorized_bearer(
                    None,
                    common::error_codes::MISSING_TOKEN,
                    "No Authorization header was present on the request.",
                )
            })
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Extract the bearer token string from an `Authorization` header.
///
/// Returns `None` when the header is absent or does not begin with `Bearer `.
fn extract_bearer(headers: &axum::http::HeaderMap) -> Option<String> {
    let value = headers.get(axum::http::header::AUTHORIZATION)?;
    let s = value.to_str().ok()?;
    let token = s.strip_prefix("Bearer ")?;
    Some(token.to_owned())
}

/// Build an HTTP 401 response with a `WWW-Authenticate: Bearer` header.
///
/// When `extra` is `Some("error=\"invalid_token\"")` (for example) the header
/// value becomes `Bearer error="invalid_token"` as required by RFC 6750.
///
/// `error_code` must be one of the constants in [`common::error_codes`].
fn unauthorized_bearer(extra: Option<&str>, error_code: &str, message: &str) -> Response<Body> {
    use common::{ApiError, error_codes as ec};
    use uuid::Uuid;

    let www_auth = match extra {
        Some(params) => format!("Bearer {params}"),
        None => "Bearer".to_string(),
    };

    let body = ApiError {
        error_code: error_code.to_string(),
        message: message.to_string(),
        request_id: Uuid::new_v4(),
    };

    let json = serde_json::to_vec(&body).unwrap_or_else(|_| {
        format!(r#"{{"error_code":"{}","message":"{}"}}"#, ec::INVALID_TOKEN, message)
            .into_bytes()
    });

    Response::builder()
        .status(StatusCode::UNAUTHORIZED)
        .header(axum::http::header::WWW_AUTHENTICATE, www_auth)
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(Body::from(json))
        .unwrap_or_else(|_| StatusCode::UNAUTHORIZED.into_response())
}

/// Build an HTTP 403 response for an inactive tenant.
fn forbidden_tenant_inactive() -> Response<Body> {
    use common::{ApiError, error_codes as ec};
    use uuid::Uuid;

    let body = ApiError {
        error_code: ec::TENANT_INACTIVE.to_string(),
        message: "This tenant has been deactivated.".to_string(),
        request_id: Uuid::new_v4(),
    };

    let json = serde_json::to_vec(&body).unwrap_or_else(|_| {
        br#"{"error_code":"tenant_inactive","message":"This tenant has been deactivated."}"#
            .to_vec()
    });

    Response::builder()
        .status(StatusCode::FORBIDDEN)
        .header(axum::http::header::CONTENT_TYPE, "application/json")
        .body(Body::from(json))
        .unwrap_or_else(|_| StatusCode::FORBIDDEN.into_response())
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::header::AUTHORIZATION;
    use axum::http::HeaderMap;

    // ── extract_bearer ────────────────────────────────────────────────────────

    #[test]
    fn extract_bearer_present() {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, "Bearer my-token".parse().unwrap());
        assert_eq!(extract_bearer(&headers), Some("my-token".to_string()));
    }

    #[test]
    fn extract_bearer_absent() {
        let headers = HeaderMap::new();
        assert_eq!(extract_bearer(&headers), None);
    }

    #[test]
    fn extract_bearer_wrong_scheme() {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, "Basic dXNlcjpwYXNz".parse().unwrap());
        assert_eq!(extract_bearer(&headers), None);
    }

    #[test]
    fn extract_bearer_empty_token() {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, "Bearer ".parse().unwrap());
        // strip_prefix("Bearer ") on "Bearer " yields "" (empty string)
        assert_eq!(extract_bearer(&headers), Some(String::new()));
    }

    // ── Response helpers ──────────────────────────────────────────────────────

    #[test]
    fn unauthorized_bearer_no_extra_has_plain_www_auth() {
        let resp = unauthorized_bearer(None, common::error_codes::MISSING_TOKEN, "test");
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let www = resp
            .headers()
            .get(axum::http::header::WWW_AUTHENTICATE)
            .unwrap();
        assert_eq!(www, "Bearer");
    }

    #[test]
    fn unauthorized_bearer_with_invalid_token_param() {
        let resp = unauthorized_bearer(
            Some("error=\"invalid_token\""),
            common::error_codes::INVALID_TOKEN,
            "expired",
        );
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let www = resp
            .headers()
            .get(axum::http::header::WWW_AUTHENTICATE)
            .unwrap();
        assert_eq!(www, "Bearer error=\"invalid_token\"");
    }

    #[test]
    fn forbidden_tenant_inactive_is_403() {
        let resp = forbidden_tenant_inactive();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    // ── Integration: AuthService rejects missing token ────────────────────────

    #[tokio::test]
    async fn auth_service_returns_401_for_missing_token() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        // Build a trivial inner service that always returns 200.
        let inner = tower::service_fn(|_req: Request<Body>| async move {
            Ok::<_, std::convert::Infallible>(
                Response::builder()
                    .status(StatusCode::OK)
                    .body(Body::empty())
                    .unwrap(),
            )
        });

        let registry = Arc::new(TenantRegistry::new());
        let jwks_cache = Arc::new(JwksCache::new());
        let layer = AuthLayer::new(registry, jwks_cache);
        let mut svc = layer.layer(inner);

        let req = Request::builder()
            .uri("/some/path")
            .body(Body::empty())
            .unwrap();

        let resp = svc.ready().await.unwrap().call(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            resp.headers()
                .get(axum::http::header::WWW_AUTHENTICATE)
                .unwrap(),
            "Bearer"
        );
    }

    #[tokio::test]
    async fn auth_service_returns_401_for_invalid_token() {
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        let inner = tower::service_fn(|_req: Request<Body>| async move {
            Ok::<_, std::convert::Infallible>(
                Response::builder()
                    .status(StatusCode::OK)
                    .body(Body::empty())
                    .unwrap(),
            )
        });

        let registry = Arc::new(TenantRegistry::new());
        let jwks_cache = Arc::new(JwksCache::new());
        let layer = AuthLayer::new(registry, jwks_cache);
        let mut svc = layer.layer(inner);

        // Provide a syntactically valid-looking bearer token for an unknown tenant.
        // validate_bearer_token will return UnknownTenant → 401.
        let req = Request::builder()
            .uri("/some/path")
            .header(
                AUTHORIZATION,
                // A minimal 3-part JWT with iss=https://unknown.example.com
                // Header: {"alg":"HS256","typ":"JWT"}
                // Payload: {"iss":"https://unknown.example.com","sub":"user-1","exp":9999999999}
                "Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.\
                 eyJpc3MiOiJodHRwczovL3Vua25vd24uZXhhbXBsZS5jb20iLCJzdWIiOiJ1c2VyLTEiLCJleHAiOjk5OTk5OTk5OTl9.\
                 signature",
            )
            .body(Body::empty())
            .unwrap();

        let resp = svc.ready().await.unwrap().call(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}
