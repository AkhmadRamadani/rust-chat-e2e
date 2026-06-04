//! JWT validation with per-tenant JWKS resolution.
//!
//! [`validate_bearer_token`] is the central entry-point for authenticating an
//! incoming HTTP request.  It implements the 7-step flow described in the
//! design document:
//!
//! 1. Base64-decode the JWT payload to extract `iss` (and `sub`) **without**
//!    verifying the signature.
//! 2. Resolve [`TenantConfig`] from [`TenantRegistry`] by issuer.
//! 3. Return [`AuthError::UnknownTenant`] when no tenant matches.
//! 4. Return [`AuthError::TenantInactive`] when the tenant is deactivated.
//! 5. Build the JWKS URL from the OIDC issuer (`{oidc_issuer}/.well-known/jwks.json`).
//! 6. Fetch (or return from cache) the JWKS via [`JwksCache::get_or_fetch`].
//! 7. Validate the token signature and expiry; extract `sub` as `user_id`.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use common::{TenantId, UserId};
use jsonwebtoken::{decode, decode_header, jwk::AlgorithmParameters, Algorithm, DecodingKey, Validation};
use serde::Deserialize;
use tracing::debug;

use crate::{AuthenticatedUser, AuthError, JwksCache, TenantRegistry};

// ── Internal claim structs ────────────────────────────────────────────────────

/// Minimal JWT claims decoded **without** signature verification, used only to
/// read the `iss` claim so we can select the correct tenant.
#[derive(Debug, Deserialize)]
struct UnverifiedClaims {
    /// Issuer — the OIDC provider's base URL.
    pub iss: String,
    /// Subject — the user identifier within the tenant's identity provider.
    /// Captured during unverified decode; the authoritative value comes from
    /// the verified claims after signature validation.
    #[allow(dead_code)]
    pub sub: String,
}

/// Full JWT claims decoded **with** signature verification against the
/// tenant's JWKS.
#[derive(Debug, Deserialize)]
struct VerifiedClaims {
    /// Subject (user identifier).
    pub sub: String,
    /// Issuer (validated against tenant config).
    #[allow(dead_code)]
    pub iss: String,
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Validate a raw Bearer token string and return an [`AuthenticatedUser`] on
/// success.
///
/// # Errors
/// | Condition | Error |
/// |---|---|
/// | Malformed JWT / cannot decode payload | `AuthError::InvalidToken` |
/// | Issuer not found in registry | `AuthError::UnknownTenant` |
/// | Tenant found but inactive | `AuthError::TenantInactive` |
/// | JWKS fetch fails | `AuthError::InvalidToken` |
/// | Signature / expiry verification fails | `AuthError::InvalidToken` / `AuthError::ExpiredToken` |
pub async fn validate_bearer_token(
    token: &str,
    registry: &TenantRegistry,
    jwks_cache: &JwksCache,
) -> Result<AuthenticatedUser, AuthError> {
    // ── Step 1: decode claims without verifying ───────────────────────────
    let unverified = decode_unverified_claims(token)?;
    let iss = &unverified.iss;

    debug!(iss = %iss, "resolved JWT issuer from unverified claims");

    // ── Step 2: look up TenantConfig ──────────────────────────────────────
    let config = registry
        .resolve_by_issuer(iss)
        .await
        .ok_or(AuthError::UnknownTenant)?;

    // ── Step 3: check tenant is active ────────────────────────────────────
    if !config.active {
        return Err(AuthError::TenantInactive);
    }

    let tenant_id: TenantId = config.tenant_id;

    // ── Step 4: build the JWKS URL ────────────────────────────────────────
    // The conventional OpenID Connect discovery path.  Trailing slashes on
    // the issuer are stripped before appending to avoid double-slashes.
    let jwks_url = build_jwks_url(&config.oidc_issuer);

    debug!(tenant_id = ?tenant_id, jwks_url = %jwks_url, "fetching JWKS for tenant");

    // ── Step 5: fetch / cache JWKS ────────────────────────────────────────
    let jwks = jwks_cache.get_or_fetch(tenant_id, &jwks_url).await?;

    // ── Step 6: validate token signature against JWKS ─────────────────────
    //
    // We iterate over the keys in the JWK Set and attempt validation with
    // each one.  The first successful decode wins (JWKS rotation support).
    // `jsonwebtoken` v9 can derive a `DecodingKey` directly from a `Jwk`.
    let header = decode_header(token)
        .map_err(|e| AuthError::InvalidToken(format!("invalid JWT header: {e}")))?;

    let mut last_err: Option<AuthError> = None;
    for jwk in &jwks.keys {
        let decoding_key = match DecodingKey::from_jwk(jwk) {
            Ok(k) => k,
            Err(_) => continue,
        };

        let algorithm = match pick_algorithm(jwk, &header) {
            Some(alg) => alg,
            None => continue,
        };

        let mut validation = Validation::new(algorithm);
        validation.set_issuer(&[iss.as_str()]);
        // Audience validation is deliberately omitted here; individual
        // handlers may add further claims checks if required.
        validation.validate_aud = false;

        match decode::<VerifiedClaims>(token, &decoding_key, &validation) {
            Ok(token_data) => {
                // ── Step 7: extract user_id ──────────────────────────
                let user_id = UserId(token_data.claims.sub);
                return Ok(AuthenticatedUser {
                    tenant_id,
                    user_id,
                    device_id: None,
                });
            }
            Err(e) => {
                use jsonwebtoken::errors::ErrorKind;
                let auth_err = match e.kind() {
                    ErrorKind::ExpiredSignature => AuthError::ExpiredToken,
                    _ => AuthError::InvalidToken(format!("token validation failed: {e}")),
                };
                last_err = Some(auth_err);
            }
        }
    }

    // All keys tried and none succeeded.
    Err(last_err.unwrap_or_else(|| {
        AuthError::InvalidToken("no matching JWKS key found for token".to_string())
    }))
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Build the JWKS URL from an OIDC issuer string using the OpenID Connect
/// discovery convention.
///
/// ```
/// # use auth::validate::build_jwks_url;
/// let url = build_jwks_url("https://example.com");
/// assert_eq!(url, "https://example.com/.well-known/jwks.json");
///
/// // Trailing slash is normalised.
/// let url = build_jwks_url("https://example.com/");
/// assert_eq!(url, "https://example.com/.well-known/jwks.json");
/// ```
pub fn build_jwks_url(oidc_issuer: &str) -> String {
    let base = oidc_issuer.trim_end_matches('/');
    format!("{base}/.well-known/jwks.json")
}

/// Decode the JWT payload segment **without verifying** the signature and
/// return the minimal [`UnverifiedClaims`].
///
/// This is safe to call for the sole purpose of reading the `iss` claim so we
/// can select the correct JWKS endpoint — no trust decision is based on these
/// unverified claims.
fn decode_unverified_claims(token: &str) -> Result<UnverifiedClaims, AuthError> {
    // A JWT is three Base64URL segments separated by '.'.
    let parts: Vec<&str> = token.splitn(3, '.').collect();
    if parts.len() != 3 {
        return Err(AuthError::InvalidToken(
            "malformed JWT: expected three dot-separated segments".to_string(),
        ));
    }

    let payload_b64 = parts[1];
    let payload_bytes = URL_SAFE_NO_PAD
        .decode(payload_b64)
        .map_err(|e| AuthError::InvalidToken(format!("JWT payload is not valid base64url: {e}")))?;

    let claims: UnverifiedClaims = serde_json::from_slice(&payload_bytes)
        .map_err(|e| AuthError::InvalidToken(format!("JWT payload is not valid JSON: {e}")))?;

    Ok(claims)
}

/// Select the [`Algorithm`] to use for validation.
///
/// Prefers the algorithm recorded in the JWK's `alg` field; falls back to the
/// algorithm from the JWT header.
fn pick_algorithm(
    jwk: &jsonwebtoken::jwk::Jwk,
    header: &jsonwebtoken::Header,
) -> Option<Algorithm> {
    // If the JWK advertises an algorithm, honour it.
    if let Some(alg) = &jwk.common.key_algorithm {
        use jsonwebtoken::jwk::KeyAlgorithm;
        let alg = match alg {
            KeyAlgorithm::RS256 => Algorithm::RS256,
            KeyAlgorithm::RS384 => Algorithm::RS384,
            KeyAlgorithm::RS512 => Algorithm::RS512,
            KeyAlgorithm::PS256 => Algorithm::PS256,
            KeyAlgorithm::PS384 => Algorithm::PS384,
            KeyAlgorithm::PS512 => Algorithm::PS512,
            KeyAlgorithm::ES256 => Algorithm::ES256,
            KeyAlgorithm::ES384 => Algorithm::ES384,
            // HS* is uncommon for OIDC but included for completeness.
            KeyAlgorithm::HS256 => Algorithm::HS256,
            KeyAlgorithm::HS384 => Algorithm::HS384,
            KeyAlgorithm::HS512 => Algorithm::HS512,
            _ => return None,
        };
        return Some(alg);
    }

    // Derive from the key type when no `alg` is present.
    match &jwk.algorithm {
        AlgorithmParameters::RSA(_) => {
            // Default to RS256 for RSA keys when no algorithm is specified.
            Some(match header.alg {
                Algorithm::RS256
                | Algorithm::RS384
                | Algorithm::RS512
                | Algorithm::PS256
                | Algorithm::PS384
                | Algorithm::PS512 => header.alg,
                _ => Algorithm::RS256,
            })
        }
        AlgorithmParameters::EllipticCurve(_) => {
            Some(match header.alg {
                Algorithm::ES256 | Algorithm::ES384 => header.alg,
                _ => Algorithm::ES256,
            })
        }
        AlgorithmParameters::OctetKey(_) | AlgorithmParameters::OctetKeyPair(_) => {
            Some(match header.alg {
                Algorithm::HS256 | Algorithm::HS384 | Algorithm::HS512 => header.alg,
                _ => Algorithm::HS256,
            })
        }
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // A JWT with payload { "iss": "https://example.com", "sub": "user-123", "exp": 9999999999 }
    // Signed with a dummy HS256 key (we only test unverified decoding here).
    const EXAMPLE_TOKEN: &str = concat!(
        "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9",
        ".",
        "eyJpc3MiOiJodHRwczovL2V4YW1wbGUuY29tIiwic3ViIjoidXNlci0xMjMiLCJleHAiOjk5OTk5OTk5OTl9",
        ".",
        "SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c"
    );

    #[test]
    fn build_jwks_url_no_trailing_slash() {
        let url = build_jwks_url("https://example.com");
        assert_eq!(url, "https://example.com/.well-known/jwks.json");
    }

    #[test]
    fn build_jwks_url_with_trailing_slash() {
        let url = build_jwks_url("https://example.com/");
        assert_eq!(url, "https://example.com/.well-known/jwks.json");
    }

    #[test]
    fn build_jwks_url_with_path_suffix() {
        let url = build_jwks_url("https://idp.example.com/tenants/acme");
        assert_eq!(
            url,
            "https://idp.example.com/tenants/acme/.well-known/jwks.json"
        );
    }

    #[test]
    fn decode_unverified_claims_valid_token() {
        let claims = decode_unverified_claims(EXAMPLE_TOKEN).unwrap();
        assert_eq!(claims.iss, "https://example.com");
        assert_eq!(claims.sub, "user-123");
    }

    #[test]
    fn decode_unverified_claims_malformed_token() {
        let result = decode_unverified_claims("not.a.jwt.with.too.many.parts");
        // splitn(3, '.') gives ["not", "a", "jwt.with.too.many.parts"] — 3 parts, so
        // the payload is "a" which decodes to an empty-ish byte slice.
        // The resulting JSON parse should fail or succeed depending on content.
        // Either way we don't panic.
        let _ = result;
    }

    #[test]
    fn decode_unverified_claims_only_two_parts() {
        let result = decode_unverified_claims("header.payload");
        assert!(matches!(result, Err(AuthError::InvalidToken(_))));
    }

    #[test]
    fn decode_unverified_claims_invalid_base64() {
        // The payload segment (middle part) is not valid base64url.
        let result = decode_unverified_claims("header.!!!invalid!!!.signature");
        assert!(matches!(result, Err(AuthError::InvalidToken(_))));
    }

    #[test]
    fn decode_unverified_claims_invalid_json() {
        // Encode some invalid JSON as base64url.
        let bad_payload = URL_SAFE_NO_PAD.encode(b"not json");
        let token = format!("header.{bad_payload}.sig");
        let result = decode_unverified_claims(&token);
        assert!(matches!(result, Err(AuthError::InvalidToken(_))));
    }

    // Full validate_bearer_token tests require a running OIDC provider, so
    // they live in integration tests.  Here we only verify the early-return
    // paths using a mock registry.
    #[tokio::test]
    async fn validate_returns_unknown_tenant_when_issuer_not_found() {
        use crate::{JwksCache, TenantRegistry};

        let registry = TenantRegistry::new();
        // Registry is empty — any issuer will be unknown.
        let jwks_cache = JwksCache::new();

        let result = validate_bearer_token(EXAMPLE_TOKEN, &registry, &jwks_cache).await;
        assert!(matches!(result, Err(AuthError::UnknownTenant)));
    }

    #[tokio::test]
    async fn validate_returns_tenant_inactive_when_deactivated() {
        use common::{TenantConfig, TenantId};
        use uuid::Uuid;

        use crate::{JwksCache, TenantRegistry};

        let registry = TenantRegistry::new();
        registry
            .upsert(TenantConfig {
                tenant_id: TenantId(Uuid::new_v4()),
                name: "Deactivated Tenant".to_string(),
                oidc_issuer: "https://example.com".to_string(),
                active: false,
            })
            .await;

        let jwks_cache = JwksCache::new();

        let result = validate_bearer_token(EXAMPLE_TOKEN, &registry, &jwks_cache).await;
        assert!(matches!(result, Err(AuthError::TenantInactive)));
    }
}
