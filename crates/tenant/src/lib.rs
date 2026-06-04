//! `crates/tenant` — Tenant registry: repository trait and PostgreSQL implementation.
//!
//! This crate owns all persistence logic for the `tenants` table and provides
//! the [`TenantRepository`] async trait plus its Postgres implementation
//! [`PgTenantRepository`].
//!
//! # Design notes
//! - All `sqlx` queries use the non-macro `sqlx::query_as` / `sqlx::query`
//!   functions with explicit `FromRow` derivations to avoid requiring a live
//!   database or a prepared query cache at compile time.
//! - `active_wt_sessions` in [`TenantUsage`] is always `0` because active
//!   WebTransport sessions are tracked in-memory by `crates/realtime`, not in
//!   the database.

use async_trait::async_trait;
use common::{TenantConfig, TenantId};
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgRow;
use sqlx::{FromRow, PgPool, Row};
use uuid::Uuid;

// ── Error type ────────────────────────────────────────────────────────────────

/// Errors that can be returned by [`TenantRepository`] operations.
#[derive(Debug, thiserror::Error)]
pub enum TenantRepositoryError {
    /// The OIDC issuer string already exists in the `tenants` table.
    #[error("OIDC issuer already registered: {0}")]
    DuplicateIssuer(String),

    /// The requested tenant was not found (used for update/deactivate operations).
    #[error("Tenant not found: {0:?}")]
    NotFound(TenantId),

    /// A PostgreSQL error that does not map to a more specific variant.
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
}

// ── TenantUsage ───────────────────────────────────────────────────────────────

/// Aggregated usage metrics for a single tenant.
///
/// Returned by [`TenantRepository::get_usage`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TenantUsage {
    /// Total number of distinct users registered under this tenant.
    pub user_count: i64,
    /// Total number of registered devices under this tenant.
    pub device_count: i64,
    /// Number of messages stored in the last 30 days for this tenant.
    pub message_count_30d: i64,
    /// Number of active WebTransport sessions.
    ///
    /// Always `0` — active sessions are tracked in-memory by `crates/realtime`
    /// and are not persisted to the database.
    pub active_wt_sessions: i64,
}

// ── TenantRepository trait ────────────────────────────────────────────────────

/// Async repository trait for tenant CRUD operations.
///
/// All mutating methods that receive a `tenant_id` will return
/// [`TenantRepositoryError::NotFound`] when no matching row exists.
#[async_trait]
pub trait TenantRepository: Send + Sync {
    /// Create a new tenant record in the database and return its full
    /// configuration.
    ///
    /// # Errors
    /// Returns [`TenantRepositoryError::DuplicateIssuer`] when the
    /// `oidc_issuer` is already registered.
    async fn create_tenant(
        &self,
        name: &str,
        oidc_issuer: &str,
    ) -> Result<TenantConfig, TenantRepositoryError>;

    /// Look up a tenant by its OIDC `iss` claim value.
    ///
    /// Returns `None` when no matching row exists.
    async fn get_by_issuer(
        &self,
        iss: &str,
    ) -> Result<Option<TenantConfig>, TenantRepositoryError>;

    /// Look up a tenant by its UUID.
    ///
    /// Returns `None` when no matching row exists.
    async fn get_by_id(
        &self,
        tenant_id: TenantId,
    ) -> Result<Option<TenantConfig>, TenantRepositoryError>;

    /// Deactivate a tenant (set `active = FALSE`).
    ///
    /// Subsequent JWT validations for this tenant's issuer will return HTTP 403.
    ///
    /// # Errors
    /// Returns [`TenantRepositoryError::NotFound`] when the tenant does not
    /// exist.
    async fn deactivate_tenant(
        &self,
        tenant_id: TenantId,
    ) -> Result<(), TenantRepositoryError>;

    /// Replace the OIDC issuer for a tenant.
    ///
    /// The caller must also invalidate the `TenantRegistry` cache and any
    /// cached JWKS entries after this succeeds.
    ///
    /// # Errors
    /// Returns [`TenantRepositoryError::NotFound`] when the tenant does not
    /// exist.  Returns [`TenantRepositoryError::DuplicateIssuer`] when
    /// `new_issuer` is already registered to a different tenant.
    async fn update_oidc_issuer(
        &self,
        tenant_id: TenantId,
        new_issuer: &str,
    ) -> Result<(), TenantRepositoryError>;

    /// Return aggregated usage metrics for a tenant.
    ///
    /// # Errors
    /// Returns [`TenantRepositoryError::NotFound`] when the tenant does not
    /// exist.
    async fn get_usage(
        &self,
        tenant_id: TenantId,
    ) -> Result<TenantUsage, TenantRepositoryError>;
}

// ── Internal row struct for manual FromRow mapping ────────────────────────────

/// Raw row returned by queries against the `tenants` table.
#[derive(Debug)]
struct TenantRow {
    tenant_id: Uuid,
    name: String,
    oidc_issuer: String,
    active: bool,
}

impl<'r> FromRow<'r, PgRow> for TenantRow {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        Ok(TenantRow {
            tenant_id: row.try_get("tenant_id")?,
            name: row.try_get("name")?,
            oidc_issuer: row.try_get("oidc_issuer")?,
            active: row.try_get("active")?,
        })
    }
}

impl From<TenantRow> for TenantConfig {
    fn from(row: TenantRow) -> Self {
        TenantConfig {
            tenant_id: TenantId(row.tenant_id),
            name: row.name,
            oidc_issuer: row.oidc_issuer,
            active: row.active,
        }
    }
}

// ── Helper: map unique-constraint DB errors ───────────────────────────────────

fn map_unique_violation(e: sqlx::Error, issuer: &str) -> TenantRepositoryError {
    if let sqlx::Error::Database(ref db_err) = e {
        if db_err.code().as_deref() == Some("23505") {
            return TenantRepositoryError::DuplicateIssuer(issuer.to_owned());
        }
    }
    TenantRepositoryError::Database(e)
}

// ── PgTenantRepository ────────────────────────────────────────────────────────

/// PostgreSQL-backed implementation of [`TenantRepository`].
///
/// Wraps a [`PgPool`] and issues plain `sqlx::query_as` (function, not macro)
/// queries so that no live database connection is required at compile time.
#[derive(Debug, Clone)]
pub struct PgTenantRepository {
    pool: PgPool,
}

impl PgTenantRepository {
    /// Create a new repository wrapping the provided connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl TenantRepository for PgTenantRepository {
    // ── create_tenant ──────────────────────────────────────────────────────

    async fn create_tenant(
        &self,
        name: &str,
        oidc_issuer: &str,
    ) -> Result<TenantConfig, TenantRepositoryError> {
        let row: TenantRow = sqlx::query_as(
            r#"
            INSERT INTO tenants (name, oidc_issuer)
            VALUES ($1, $2)
            RETURNING tenant_id, name, oidc_issuer, active
            "#,
        )
        .bind(name)
        .bind(oidc_issuer)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| map_unique_violation(e, oidc_issuer))?;

        Ok(TenantConfig::from(row))
    }

    // ── get_by_issuer ──────────────────────────────────────────────────────

    async fn get_by_issuer(
        &self,
        iss: &str,
    ) -> Result<Option<TenantConfig>, TenantRepositoryError> {
        let maybe_row: Option<TenantRow> = sqlx::query_as(
            r#"
            SELECT tenant_id, name, oidc_issuer, active
            FROM   tenants
            WHERE  oidc_issuer = $1
            "#,
        )
        .bind(iss)
        .fetch_optional(&self.pool)
        .await?;

        Ok(maybe_row.map(TenantConfig::from))
    }

    // ── get_by_id ──────────────────────────────────────────────────────────

    async fn get_by_id(
        &self,
        tenant_id: TenantId,
    ) -> Result<Option<TenantConfig>, TenantRepositoryError> {
        let maybe_row: Option<TenantRow> = sqlx::query_as(
            r#"
            SELECT tenant_id, name, oidc_issuer, active
            FROM   tenants
            WHERE  tenant_id = $1
            "#,
        )
        .bind(tenant_id.0)
        .fetch_optional(&self.pool)
        .await?;

        Ok(maybe_row.map(TenantConfig::from))
    }

    // ── deactivate_tenant ──────────────────────────────────────────────────

    async fn deactivate_tenant(
        &self,
        tenant_id: TenantId,
    ) -> Result<(), TenantRepositoryError> {
        let result = sqlx::query(
            r#"
            UPDATE tenants
            SET    active = FALSE
            WHERE  tenant_id = $1
            "#,
        )
        .bind(tenant_id.0)
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(TenantRepositoryError::NotFound(tenant_id));
        }

        Ok(())
    }

    // ── update_oidc_issuer ─────────────────────────────────────────────────

    async fn update_oidc_issuer(
        &self,
        tenant_id: TenantId,
        new_issuer: &str,
    ) -> Result<(), TenantRepositoryError> {
        let result = sqlx::query(
            r#"
            UPDATE tenants
            SET    oidc_issuer = $2
            WHERE  tenant_id = $1
            "#,
        )
        .bind(tenant_id.0)
        .bind(new_issuer)
        .execute(&self.pool)
        .await
        .map_err(|e| map_unique_violation(e, new_issuer))?;

        if result.rows_affected() == 0 {
            return Err(TenantRepositoryError::NotFound(tenant_id));
        }

        Ok(())
    }

    // ── get_usage ──────────────────────────────────────────────────────────

    async fn get_usage(
        &self,
        tenant_id: TenantId,
    ) -> Result<TenantUsage, TenantRepositoryError> {
        // Verify the tenant exists first so we can return a specific error.
        let exists: bool = sqlx::query_scalar(
            r#"SELECT EXISTS(SELECT 1 FROM tenants WHERE tenant_id = $1)"#,
        )
        .bind(tenant_id.0)
        .fetch_one(&self.pool)
        .await?;

        if !exists {
            return Err(TenantRepositoryError::NotFound(tenant_id));
        }

        // Count distinct users.
        let user_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE tenant_id = $1")
                .bind(tenant_id.0)
                .fetch_one(&self.pool)
                .await?;

        // Count registered devices.
        let device_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM devices WHERE tenant_id = $1")
                .bind(tenant_id.0)
                .fetch_one(&self.pool)
                .await?;

        // Count messages in the last 30 days.
        // `server_ts` is stored as Unix epoch milliseconds (BIGINT).
        let message_count_30d: i64 = sqlx::query_scalar(
            r#"
            SELECT COUNT(*)
            FROM   message_envelopes
            WHERE  tenant_id = $1
              AND  server_ts >= (EXTRACT(EPOCH FROM NOW() - INTERVAL '30 days') * 1000)::BIGINT
            "#,
        )
        .bind(tenant_id.0)
        .fetch_one(&self.pool)
        .await?;

        Ok(TenantUsage {
            user_count,
            device_count,
            message_count_30d,
            // Active WebTransport sessions are tracked in-memory by
            // crates/realtime; the database has no record of them.
            active_wt_sessions: 0,
        })
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tenant_usage_active_wt_sessions_always_zero() {
        let usage = TenantUsage {
            user_count: 10,
            device_count: 20,
            message_count_30d: 100,
            active_wt_sessions: 0,
        };
        assert_eq!(usage.active_wt_sessions, 0);
    }

    #[test]
    fn tenant_usage_serde_roundtrip() {
        let usage = TenantUsage {
            user_count: 5,
            device_count: 12,
            message_count_30d: 300,
            active_wt_sessions: 0,
        };
        let json = serde_json::to_string(&usage).unwrap();
        let decoded: TenantUsage = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.user_count, usage.user_count);
        assert_eq!(decoded.device_count, usage.device_count);
        assert_eq!(decoded.message_count_30d, usage.message_count_30d);
        assert_eq!(decoded.active_wt_sessions, 0);
    }

    #[test]
    fn tenant_row_converts_to_tenant_config() {
        let id = Uuid::new_v4();
        let row = TenantRow {
            tenant_id: id,
            name: "Acme Corp".to_string(),
            oidc_issuer: "https://acme.example.com".to_string(),
            active: true,
        };
        let config = TenantConfig::from(row);
        assert_eq!(config.tenant_id.0, id);
        assert_eq!(config.name, "Acme Corp");
        assert_eq!(config.oidc_issuer, "https://acme.example.com");
        assert!(config.active);
    }

    #[test]
    fn tenant_row_inactive_converts_correctly() {
        let id = Uuid::new_v4();
        let row = TenantRow {
            tenant_id: id,
            name: "Inactive Corp".to_string(),
            oidc_issuer: "https://inactive.example.com".to_string(),
            active: false,
        };
        let config = TenantConfig::from(row);
        assert!(!config.active);
    }

    #[test]
    fn tenant_repository_error_display() {
        let err = TenantRepositoryError::DuplicateIssuer("https://foo.example.com".to_string());
        assert!(err.to_string().contains("https://foo.example.com"));

        let id = TenantId(Uuid::new_v4());
        let err2 = TenantRepositoryError::NotFound(id);
        assert!(err2.to_string().to_lowercase().contains("not found"));
    }

    #[test]
    fn tenant_usage_json_shape() {
        let usage = TenantUsage {
            user_count: 1,
            device_count: 2,
            message_count_30d: 3,
            active_wt_sessions: 0,
        };
        let json = serde_json::to_value(&usage).unwrap();
        assert!(json.get("user_count").is_some());
        assert!(json.get("device_count").is_some());
        assert!(json.get("message_count_30d").is_some());
        assert!(json.get("active_wt_sessions").is_some());
    }
}
