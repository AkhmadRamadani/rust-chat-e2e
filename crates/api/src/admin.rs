//! Admin API handlers for tenant lifecycle management.
//!
//! All handlers are mounted under `/admin/...` and protected by the
//! [`AdminAuthLayer`][crate::admin_auth::AdminAuthLayer] Tower middleware which
//! checks `Authorization: Bearer <ADMIN_TOKEN>`.
//!
//! # Route table
//!
//! | Method | Path | Handler |
//! |--------|------|---------|
//! | `GET`  | `/admin/tenants` | [`list_tenants`] |
//! | `POST` | `/admin/tenants` | [`create_tenant`] |
//! | `DELETE` | `/admin/tenants/{tenant_id}` | [`deactivate_tenant`] |
//! | `PUT` | `/admin/tenants/{tenant_id}/oidc` | [`update_oidc_issuer`] |
//! | `GET` | `/admin/tenants/{tenant_id}/usage` | [`get_tenant_usage`] |

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use auth::{JwksCache, TenantRegistry};
use common::TenantId;
use sqlx::PgPool;
use tenant::{TenantRepositoryError, TenantUsage};

// ── Shared app state ──────────────────────────────────────────────────────────

/// Axum state shared across all admin handlers.
///
/// Holds references to the tenant repository, the in-memory tenant registry,
/// and the JWKS cache so that admin mutations can keep all three in sync.
#[derive(Clone)]
pub struct AdminState {
    /// Persistent tenant storage (PostgreSQL).
    pub repo: Arc<dyn tenant::TenantRepository>,
    /// In-memory cache of tenant configurations, keyed by OIDC issuer.
    pub registry: TenantRegistry,
    /// Per-tenant JWKS cache, invalidated on OIDC issuer updates.
    pub jwks_cache: JwksCache,
    /// Raw connection pool used by handlers that issue custom SQL (e.g. list_tenants).
    pub pool: PgPool,
}

impl std::fmt::Debug for AdminState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdminState")
            .field("registry", &self.registry)
            .field("jwks_cache", &self.jwks_cache)
            .finish_non_exhaustive()
    }
}

// ── Request / Response types ──────────────────────────────────────────────────

/// Request body for `POST /admin/tenants`.
#[derive(Debug, Deserialize)]
pub struct CreateTenantRequest {
    /// Human-readable name for the tenant organisation.
    pub name: String,
    /// OIDC `iss` claim URL for this tenant's identity provider.
    pub oidc_issuer: String,
}

/// Successful response body for `POST /admin/tenants`.
#[derive(Debug, Serialize)]
pub struct CreateTenantResponse {
    /// The newly assigned tenant UUID.
    pub tenant_id: Uuid,
}

/// Request body for `PUT /admin/tenants/{tenant_id}/oidc`.
#[derive(Debug, Deserialize)]
pub struct UpdateIssuerRequest {
    /// The new OIDC `iss` claim URL to associate with this tenant.
    pub oidc_issuer: String,
}

/// A single item in the `GET /admin/tenants` response array.
///
/// Returned for each provisioned tenant: active or inactive.
#[derive(Debug, Serialize)]
pub struct TenantListItem {
    /// The tenant's UUID.
    pub tenant_id: Uuid,
    /// Human-readable name of the tenant organisation.
    pub name: String,
    /// OIDC `iss` claim URL for this tenant's identity provider.
    pub oidc_issuer: String,
    /// Whether the tenant is currently active.
    pub active: bool,
}

// ── Error type ────────────────────────────────────────────────────────────────

/// Errors that can be returned by admin handlers.
#[derive(Debug)]
pub enum AdminError {
    /// The requested tenant was not found.
    NotFound(#[allow(dead_code)] TenantId),
    /// The supplied OIDC issuer is already registered to another tenant.
    DuplicateIssuer(String),
    /// An unexpected database error occurred.
    Database(String),
}

impl IntoResponse for AdminError {
    fn into_response(self) -> Response {
        match self {
            AdminError::NotFound(_) => {
                (StatusCode::NOT_FOUND, "Tenant not found").into_response()
            }
            AdminError::DuplicateIssuer(iss) => (
                StatusCode::CONFLICT,
                format!("OIDC issuer already registered: {iss}"),
            )
                .into_response(),
            AdminError::Database(msg) => {
                tracing::error!("Admin handler database error: {msg}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Internal storage error",
                )
                    .into_response()
            }
        }
    }
}

impl From<TenantRepositoryError> for AdminError {
    fn from(err: TenantRepositoryError) -> Self {
        match err {
            TenantRepositoryError::NotFound(id) => AdminError::NotFound(id),
            TenantRepositoryError::DuplicateIssuer(iss) => AdminError::DuplicateIssuer(iss),
            TenantRepositoryError::Database(e) => AdminError::Database(e.to_string()),
        }
    }
}

// ── GET /admin/tenants ────────────────────────────────────────────────────────

/// List all provisioned tenants.
///
/// Returns every row in the `tenants` table (active and inactive) ordered by
/// creation time.  The response shape matches [`TenantListItem`].
///
/// # Errors
/// - HTTP 500 on unexpected database errors.
///
/// Requirements: 10.1
pub async fn list_tenants(
    State(state): State<AdminState>,
) -> Result<Json<Vec<TenantListItem>>, AdminError> {
    let rows = sqlx::query_as::<_, (uuid::Uuid, String, String, bool)>(
        r#"
        SELECT tenant_id, name, oidc_issuer, active
        FROM   tenants
        ORDER  BY created_at
        "#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| AdminError::Database(e.to_string()))?;

    let items = rows
        .into_iter()
        .map(|(tenant_id, name, oidc_issuer, active)| TenantListItem {
            tenant_id,
            name,
            oidc_issuer,
            active,
        })
        .collect();

    Ok(Json(items))
}

// ── POST /admin/tenants ───────────────────────────────────────────────────────

/// Create a new tenant.
///
/// Persists the tenant to the database, upserts it into the in-memory
/// `TenantRegistry`, and returns HTTP 201 with the new `tenant_id`.
///
/// # Errors
/// - HTTP 409 when the `oidc_issuer` is already registered.
/// - HTTP 500 on unexpected database errors.
pub async fn create_tenant(
    State(state): State<AdminState>,
    Json(body): Json<CreateTenantRequest>,
) -> Result<(StatusCode, Json<CreateTenantResponse>), AdminError> {
    let config = state
        .repo
        .create_tenant(&body.name, &body.oidc_issuer)
        .await?;

    // Keep the in-memory registry in sync.
    state.registry.upsert(config.clone()).await;

    Ok((
        StatusCode::CREATED,
        Json(CreateTenantResponse {
            tenant_id: config.tenant_id.0,
        }),
    ))
}

// ── DELETE /admin/tenants/{tenant_id} ─────────────────────────────────────────

/// Deactivate a tenant.
///
/// Marks the tenant as inactive in the database and removes it from the
/// in-memory `TenantRegistry` so that subsequent token validations for that
/// tenant's issuer fail immediately with HTTP 403.
///
/// # Errors
/// - HTTP 404 when the tenant does not exist.
/// - HTTP 500 on unexpected database errors.
pub async fn deactivate_tenant(
    State(state): State<AdminState>,
    Path(tenant_id): Path<Uuid>,
) -> Result<StatusCode, AdminError> {
    let id = TenantId(tenant_id);

    // Fetch the current config first so we can invalidate by issuer.
    let config = state
        .repo
        .get_by_id(id)
        .await
        .map_err(AdminError::from)?
        .ok_or(AdminError::NotFound(id))?;

    state.repo.deactivate_tenant(id).await?;

    // Remove from the registry so future auth lookups fail with 403
    // (tenant_inactive) rather than serving stale data.
    state.registry.invalidate(&config.oidc_issuer).await;

    Ok(StatusCode::NO_CONTENT)
}

// ── PUT /admin/tenants/{tenant_id}/oidc ───────────────────────────────────────

/// Update the OIDC issuer for a tenant.
///
/// Persists the new issuer to the database, invalidates the stale entry from
/// the `TenantRegistry`, upserts the updated config, and evicts the old JWKS
/// cache entry so the next authentication fetches fresh keys.
///
/// # Errors
/// - HTTP 404 when the tenant does not exist.
/// - HTTP 409 when the new issuer is already registered to another tenant.
/// - HTTP 500 on unexpected database errors.
pub async fn update_oidc_issuer(
    State(state): State<AdminState>,
    Path(tenant_id): Path<Uuid>,
    Json(body): Json<UpdateIssuerRequest>,
) -> Result<StatusCode, AdminError> {
    let id = TenantId(tenant_id);

    // Fetch the current config to get the old issuer for cache invalidation.
    let old_config = state
        .repo
        .get_by_id(id)
        .await
        .map_err(AdminError::from)?
        .ok_or(AdminError::NotFound(id))?;

    state
        .repo
        .update_oidc_issuer(id, &body.oidc_issuer)
        .await?;

    // Invalidate stale registry entry for the old issuer.
    state.registry.invalidate(&old_config.oidc_issuer).await;

    // Evict stale JWKS cache entry (keyed by tenant_id + old issuer-derived URL).
    // The JWKS URL is conventionally `{iss}/.well-known/jwks.json`; evict the
    // old URL so the next token validation fetches fresh keys from the new
    // issuer's endpoint.
    let old_jwks_url = format!("{}/.well-known/jwks.json", old_config.oidc_issuer);
    state.jwks_cache.invalidate(id, &old_jwks_url).await;

    // Upsert the updated config into the registry.
    let updated_config = common::TenantConfig {
        tenant_id: id,
        name: old_config.name,
        oidc_issuer: body.oidc_issuer,
        active: old_config.active,
    };
    state.registry.upsert(updated_config).await;

    Ok(StatusCode::NO_CONTENT)
}

// ── GET /admin/tenants/{tenant_id}/usage ──────────────────────────────────────

/// Return usage metrics for a tenant.
///
/// Queries the database for current user count, device count,
/// message count (last 30 days), and active WebTransport session count.
///
/// # Errors
/// - HTTP 404 when the tenant does not exist.
/// - HTTP 500 on unexpected database errors.
pub async fn get_tenant_usage(
    State(state): State<AdminState>,
    Path(tenant_id): Path<Uuid>,
) -> Result<Json<TenantUsage>, AdminError> {
    let id = TenantId(tenant_id);
    let usage = state.repo.get_usage(id).await?;
    Ok(Json(usage))
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_tenant_response_serialises() {
        let resp = CreateTenantResponse {
            tenant_id: Uuid::new_v4(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("tenant_id"));
    }

    #[test]
    fn admin_error_from_tenant_repo_not_found() {
        let id = TenantId(Uuid::new_v4());
        let err = AdminError::from(TenantRepositoryError::NotFound(id));
        assert!(matches!(err, AdminError::NotFound(_)));
    }

    #[test]
    fn admin_error_from_tenant_repo_duplicate_issuer() {
        let err = AdminError::from(TenantRepositoryError::DuplicateIssuer(
            "https://dup.example.com".to_string(),
        ));
        assert!(matches!(err, AdminError::DuplicateIssuer(_)));
    }
}
