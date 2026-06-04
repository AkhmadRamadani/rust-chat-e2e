//! `AdminAuth` Tower middleware layer.
//!
//! Checks that an incoming request carries an `Authorization: Bearer <token>`
//! header whose token value matches the `ADMIN_TOKEN` environment variable.
//! Requests without a valid admin token receive HTTP 401.
//!
//! The middleware is applied only to the `/admin/...` sub-router so that
//! regular tenant user routes are not affected.

use std::env;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::body::Body;
use axum::http::{Request, Response, StatusCode};
use axum::response::IntoResponse;
use tower::{Layer, Service};

// ── AdminAuthLayer ────────────────────────────────────────────────────────────

/// [`Layer`] that wraps an inner service with [`AdminAuthService`].
///
/// Reads `ADMIN_TOKEN` once at construction time so the env-var lookup is not
/// repeated on every request.  Returns HTTP 500 if the variable is not set.
#[derive(Debug, Clone)]
pub struct AdminAuthLayer {
    /// The expected admin token, loaded from `ADMIN_TOKEN` at construction.
    token: Arc<Option<String>>,
}

impl AdminAuthLayer {
    /// Create a new layer, reading `ADMIN_TOKEN` from the environment.
    ///
    /// If the environment variable is absent the layer stores `None`; every
    /// request will then receive HTTP 500 with a descriptive message so that
    /// misconfiguration is obvious.
    pub fn from_env() -> Self {
        let token = env::var("ADMIN_TOKEN").ok();
        Self {
            token: Arc::new(token),
        }
    }

    /// Create a layer with an explicit token value (useful in tests).
    #[cfg(test)]
    #[allow(dead_code)]
    pub fn with_token(token: impl Into<String>) -> Self {
        Self {
            token: Arc::new(Some(token.into())),
        }
    }
}

impl<S> Layer<S> for AdminAuthLayer {
    type Service = AdminAuthService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        AdminAuthService {
            inner,
            token: Arc::clone(&self.token),
        }
    }
}

// ── AdminAuthService ──────────────────────────────────────────────────────────

/// Tower [`Service`] that enforces the admin bearer-token check.
#[derive(Debug, Clone)]
pub struct AdminAuthService<S> {
    inner: S,
    token: Arc<Option<String>>,
}

impl<S> Service<Request<Body>> for AdminAuthService<S>
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

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        // ── Retrieve the expected admin token ─────────────────────────────
        let expected = match self.token.as_ref() {
            Some(t) => t.clone(),
            None => {
                // ADMIN_TOKEN was not set — misconfiguration, return 500.
                let resp = (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "ADMIN_TOKEN environment variable is not set",
                )
                    .into_response();
                return Box::pin(async move { Ok(resp) });
            }
        };

        // ── Extract bearer token from Authorization header ─────────────────
        let presented = extract_bearer(req.headers());

        match presented {
            None => {
                let resp = (StatusCode::UNAUTHORIZED, "Missing admin bearer token").into_response();
                Box::pin(async move { Ok(resp) })
            }
            Some(tok) if tok != expected => {
                let resp = (StatusCode::UNAUTHORIZED, "Invalid admin bearer token").into_response();
                Box::pin(async move { Ok(resp) })
            }
            Some(_) => {
                // Token matches — forward to the inner service.
                let fut = self.inner.call(req);
                Box::pin(async move { fut.await })
            }
        }
    }
}

// ── Helper ────────────────────────────────────────────────────────────────────

/// Extract the bearer token string from an `Authorization` header, if present
/// and well-formed.
///
/// Returns `None` when the header is absent or does not start with `Bearer `.
fn extract_bearer(headers: &axum::http::HeaderMap) -> Option<String> {
    let value = headers.get(axum::http::header::AUTHORIZATION)?;
    let s = value.to_str().ok()?;
    let token = s.strip_prefix("Bearer ")?;
    Some(token.to_owned())
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::header::AUTHORIZATION;
    use axum::http::HeaderMap;

    #[test]
    fn extract_bearer_present() {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, "Bearer my-secret-token".parse().unwrap());
        assert_eq!(
            extract_bearer(&headers),
            Some("my-secret-token".to_string())
        );
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
        // strip_prefix("Bearer ") on "Bearer " yields ""
        assert_eq!(extract_bearer(&headers), Some(String::new()));
    }
}
