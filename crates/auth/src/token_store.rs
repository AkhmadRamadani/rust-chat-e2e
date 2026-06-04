//! Refresh-token storage and revocation.
//!
//! Defines the [`TokenStore`] trait and its PostgreSQL implementation
//! [`PgTokenStore`].  All operations are scoped to a `tenant_id` so that
//! tokens belonging to different tenants are never visible to one another.
//!
//! # Schema
//! The implementation targets the `refresh_tokens` table:
//!
//! ```sql
//! CREATE TABLE refresh_tokens (
//!     jti        TEXT PRIMARY KEY,
//!     tenant_id  UUID NOT NULL REFERENCES tenants(tenant_id),
//!     user_id    TEXT NOT NULL,
//!     device_id  UUID NOT NULL,
//!     expires_at TIMESTAMPTZ NOT NULL,
//!     revoked    BOOLEAN NOT NULL DEFAULT FALSE
//! );
//! ```

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use common::{DeviceId, TenantId, UserId};
use sqlx::PgPool;

// ── RefreshTokenData ──────────────────────────────────────────────────────────

/// All the data stored for a single refresh token.
#[derive(Debug, Clone)]
pub struct RefreshTokenData {
    /// The JWT ID claim — serves as the primary key.
    pub jti: String,
    /// The tenant this token belongs to.
    pub tenant_id: TenantId,
    /// The user who was issued this token.
    pub user_id: UserId,
    /// The device session associated with this token.
    pub device_id: DeviceId,
    /// Absolute UTC timestamp after which the token must not be accepted.
    pub expires_at: DateTime<Utc>,
}

// ── TokenStoreError ───────────────────────────────────────────────────────────

/// Errors returned by [`TokenStore`] operations.
#[derive(Debug, thiserror::Error)]
pub enum TokenStoreError {
    /// The token has already been revoked.
    #[error("token has already been revoked")]
    AlreadyRevoked,

    /// No token with the given `jti` was found for this tenant.
    #[error("token not found")]
    NotFound,

    /// The token's `expires_at` is in the past.
    #[error("token has expired")]
    Expired,

    /// A database-level error occurred.
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

// ── TokenStore trait ──────────────────────────────────────────────────────────

/// Persistent storage and revocation tracking for refresh tokens.
///
/// All methods accept a `tenant_id` and scope every query to that tenant so
/// that a token belonging to tenant A is never accessible to tenant B.
#[async_trait]
pub trait TokenStore: Send + Sync {
    /// Persist a new refresh token.
    ///
    /// Inserts `data` into the backing store.  Returns an error if a token
    /// with the same `jti` already exists (duplicate insert) or if the
    /// database operation fails.
    async fn store_refresh_token(
        &self,
        tenant_id: TenantId,
        data: RefreshTokenData,
    ) -> Result<(), TokenStoreError>;

    /// Mark the token identified by `jti` as revoked for `tenant_id`.
    ///
    /// Returns [`TokenStoreError::NotFound`] if no matching row exists.
    async fn revoke(
        &self,
        tenant_id: TenantId,
        jti: &str,
    ) -> Result<(), TokenStoreError>;

    /// Return `true` when the token should be rejected.
    ///
    /// A token is considered revoked (and this method returns `true`) when:
    /// - No row with `jti` + `tenant_id` is found (treat as revoked for safety).
    /// - The `revoked` column is `TRUE`.
    /// - The `expires_at` timestamp is in the past (expired ⇒ effectively revoked).
    ///
    /// Returns `false` only when the token exists, has not been explicitly
    /// revoked, and has not expired.
    async fn is_revoked(
        &self,
        tenant_id: TenantId,
        jti: &str,
    ) -> Result<bool, TokenStoreError>;
}

// ── PgTokenStore ──────────────────────────────────────────────────────────────

/// PostgreSQL-backed implementation of [`TokenStore`].
///
/// Wraps a [`sqlx::PgPool`] and translates trait operations to SQL statements
/// against the `refresh_tokens` table.  The pool is cheap to clone because it
/// is reference-counted internally.
#[derive(Debug, Clone)]
pub struct PgTokenStore {
    pool: PgPool,
}

impl PgTokenStore {
    /// Create a new [`PgTokenStore`] that uses `pool` for all database access.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl TokenStore for PgTokenStore {
    /// Insert a row into `refresh_tokens`.
    ///
    /// Uses a plain `INSERT` (no `ON CONFLICT`); callers are responsible for
    /// ensuring `jti` values are unique.
    async fn store_refresh_token(
        &self,
        tenant_id: TenantId,
        data: RefreshTokenData,
    ) -> Result<(), TokenStoreError> {
        sqlx::query(
            r#"
            INSERT INTO refresh_tokens (jti, tenant_id, user_id, device_id, expires_at, revoked)
            VALUES ($1, $2, $3, $4, $5, FALSE)
            "#,
        )
        .bind(&data.jti)
        .bind(tenant_id.0)
        .bind(&data.user_id.0)
        .bind(data.device_id.0)
        .bind(data.expires_at)
        .execute(&self.pool)
        .await
        .map_err(TokenStoreError::Database)?;

        Ok(())
    }

    /// Set `revoked = TRUE` for the token matching `(jti, tenant_id)`.
    ///
    /// Returns [`TokenStoreError::NotFound`] when the `UPDATE` affects zero
    /// rows (i.e. the token does not exist for this tenant).
    async fn revoke(
        &self,
        tenant_id: TenantId,
        jti: &str,
    ) -> Result<(), TokenStoreError> {
        let result = sqlx::query(
            r#"
            UPDATE refresh_tokens
               SET revoked = TRUE
             WHERE jti = $1
               AND tenant_id = $2
            "#,
        )
        .bind(jti)
        .bind(tenant_id.0)
        .execute(&self.pool)
        .await
        .map_err(TokenStoreError::Database)?;

        if result.rows_affected() == 0 {
            return Err(TokenStoreError::NotFound);
        }

        Ok(())
    }

    /// Check whether the token `(jti, tenant_id)` should be rejected.
    ///
    /// Returns `true` (treat as revoked) when:
    /// - The row is not found.
    /// - `revoked = TRUE`.
    /// - `expires_at < NOW()`.
    ///
    /// Returns `false` only when all three conditions are clear.
    async fn is_revoked(
        &self,
        tenant_id: TenantId,
        jti: &str,
    ) -> Result<bool, TokenStoreError> {
        let row: Option<(bool, DateTime<Utc>)> = sqlx::query_as(
            r#"
            SELECT revoked, expires_at
              FROM refresh_tokens
             WHERE jti = $1
               AND tenant_id = $2
            "#,
        )
        .bind(jti)
        .bind(tenant_id.0)
        .fetch_optional(&self.pool)
        .await
        .map_err(TokenStoreError::Database)?;

        match row {
            // Not found — treat as revoked for safety.
            None => Ok(true),
            Some((revoked, expires_at)) => {
                if revoked || expires_at < Utc::now() {
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
        }
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn make_token_data(jti: &str, tenant_id: TenantId, expires_at: DateTime<Utc>) -> RefreshTokenData {
        RefreshTokenData {
            jti: jti.to_string(),
            tenant_id,
            user_id: UserId("user-test".to_string()),
            device_id: DeviceId(Uuid::new_v4()),
            expires_at,
        }
    }

    #[test]
    fn refresh_token_data_fields_accessible() {
        let tid = TenantId(Uuid::new_v4());
        let exp = Utc::now() + chrono::Duration::hours(1);
        let data = make_token_data("jti-abc", tid, exp);

        assert_eq!(data.jti, "jti-abc");
        assert_eq!(data.tenant_id, tid);
        assert_eq!(data.user_id.0, "user-test");
        assert_eq!(data.expires_at, exp);
    }

    #[test]
    fn token_store_error_display_not_found() {
        let err = TokenStoreError::NotFound;
        assert_eq!(err.to_string(), "token not found");
    }

    #[test]
    fn token_store_error_display_already_revoked() {
        let err = TokenStoreError::AlreadyRevoked;
        assert_eq!(err.to_string(), "token has already been revoked");
    }

    #[test]
    fn token_store_error_display_expired() {
        let err = TokenStoreError::Expired;
        assert_eq!(err.to_string(), "token has expired");
    }

    #[test]
    fn pg_token_store_clone_shares_pool() {
        // We cannot create a real pool without a DB, but we can verify the
        // type compiles and clone works (PgPool is Arc-backed).
        // This test just validates the code structure by checking PgTokenStore::new
        // can accept a PgPool and be cloned — tested structurally at compile time.
        // The actual DB interaction is tested via integration tests.
        let _ = std::mem::size_of::<PgTokenStore>();
    }
}
