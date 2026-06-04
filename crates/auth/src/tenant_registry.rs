//! In-memory tenant registry cache.
//!
//! [`TenantRegistry`] wraps an `Arc<RwLock<HashMap<String, TenantConfig>>>`
//! keyed by `oidc_issuer`.  It is loaded once at startup from the `tenants`
//! table and mutated by admin operations (create, OIDC issuer update,
//! deactivation) so that per-request `resolve_by_issuer` calls never touch
//! the database.

use std::collections::HashMap;
use std::sync::Arc;

use common::{TenantConfig, TenantId};
use sqlx::{FromRow, PgPool, Row};
use tokio::sync::RwLock;
use uuid::Uuid;

// ── Internal row struct ───────────────────────────────────────────────────────

/// Raw row returned when querying the `tenants` table.
#[derive(Debug)]
struct TenantRow {
    tenant_id: Uuid,
    name: String,
    oidc_issuer: String,
    active: bool,
}

impl<'r> FromRow<'r, sqlx::postgres::PgRow> for TenantRow {
    fn from_row(row: &'r sqlx::postgres::PgRow) -> Result<Self, sqlx::Error> {
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

// ── TenantRegistry ────────────────────────────────────────────────────────────

/// Shared in-memory cache of tenant configurations, keyed by OIDC issuer URL.
///
/// # Concurrency
/// The inner `HashMap` is protected by a `tokio::sync::RwLock` so that reads
/// (the hot path during token validation) can proceed concurrently.  Writes
/// occur only on admin mutations and are expected to be infrequent.
///
/// The struct itself wraps the lock in an `Arc` so it can be cheaply cloned
/// and shared across Axum state, Tower middleware layers, and background tasks.
#[derive(Debug, Clone)]
pub struct TenantRegistry {
    inner: Arc<RwLock<HashMap<String, TenantConfig>>>,
}

impl Default for TenantRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl TenantRegistry {
    /// Create a new, empty registry.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Bulk-load all active and inactive tenants from the `tenants` table.
    ///
    /// This is intended to be called **once at startup** before the server
    /// begins accepting requests.  Any existing entries in the registry are
    /// replaced.
    ///
    /// Uses `sqlx::query_as` (the function, not the `!` macro) so that no
    /// live database connection or prepared query cache is required at compile
    /// time.
    pub async fn load_all(&self, db: &PgPool) {
        let rows: Vec<TenantRow> = sqlx::query_as(
            r#"
            SELECT tenant_id, name, oidc_issuer, active
            FROM   tenants
            "#,
        )
        .fetch_all(db)
        .await
        .unwrap_or_default();

        let mut map = self.inner.write().await;
        map.clear();
        for row in rows {
            let config = TenantConfig::from(row);
            map.insert(config.oidc_issuer.clone(), config);
        }
    }

    /// Look up a tenant by its OIDC `iss` claim value.
    ///
    /// Returns a cloned [`TenantConfig`] if found, or `None` when no tenant
    /// matches the issuer.  This is an O(1) in-memory lookup — it does **not**
    /// hit the database.
    pub async fn resolve_by_issuer(&self, iss: &str) -> Option<TenantConfig> {
        let map = self.inner.read().await;
        map.get(iss).cloned()
    }

    /// Remove the entry for the given OIDC issuer from the cache.
    ///
    /// Called after an admin operation that changes or deactivates a tenant's
    /// OIDC configuration so that the stale entry is not served to subsequent
    /// authentication requests.
    pub async fn invalidate(&self, oidc_issuer: &str) {
        let mut map = self.inner.write().await;
        map.remove(oidc_issuer);
    }

    /// Insert or update a tenant entry in the cache.
    ///
    /// Called after an admin operation creates a new tenant or updates an
    /// existing one so that the registry reflects the latest configuration
    /// without requiring a full `load_all` reload.
    pub async fn upsert(&self, config: TenantConfig) {
        let mut map = self.inner.write().await;
        map.insert(config.oidc_issuer.clone(), config);
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config(issuer: &str, active: bool) -> TenantConfig {
        TenantConfig {
            tenant_id: TenantId(Uuid::new_v4()),
            name: format!("Tenant for {issuer}"),
            oidc_issuer: issuer.to_string(),
            active,
        }
    }

    #[tokio::test]
    async fn new_registry_is_empty() {
        let registry = TenantRegistry::new();
        assert!(registry.resolve_by_issuer("https://example.com").await.is_none());
    }

    #[tokio::test]
    async fn upsert_then_resolve() {
        let registry = TenantRegistry::new();
        let config = make_config("https://idp.example.com", true);
        let tenant_id = config.tenant_id;

        registry.upsert(config).await;

        let found = registry
            .resolve_by_issuer("https://idp.example.com")
            .await
            .expect("should find tenant after upsert");
        assert_eq!(found.tenant_id, tenant_id);
        assert_eq!(found.oidc_issuer, "https://idp.example.com");
        assert!(found.active);
    }

    #[tokio::test]
    async fn resolve_unknown_issuer_returns_none() {
        let registry = TenantRegistry::new();
        let config = make_config("https://idp.example.com", true);
        registry.upsert(config).await;

        assert!(registry
            .resolve_by_issuer("https://other.example.com")
            .await
            .is_none());
    }

    #[tokio::test]
    async fn invalidate_removes_entry() {
        let registry = TenantRegistry::new();
        let config = make_config("https://idp.example.com", true);
        registry.upsert(config).await;

        // Confirm it is there first.
        assert!(registry
            .resolve_by_issuer("https://idp.example.com")
            .await
            .is_some());

        registry.invalidate("https://idp.example.com").await;

        // Now it should be gone.
        assert!(registry
            .resolve_by_issuer("https://idp.example.com")
            .await
            .is_none());
    }

    #[tokio::test]
    async fn invalidate_nonexistent_key_is_a_noop() {
        let registry = TenantRegistry::new();
        // Should not panic on missing key.
        registry.invalidate("https://nobody.example.com").await;
        assert!(registry
            .resolve_by_issuer("https://nobody.example.com")
            .await
            .is_none());
    }

    #[tokio::test]
    async fn upsert_overwrites_existing_entry() {
        let registry = TenantRegistry::new();
        let original = make_config("https://idp.example.com", true);
        let original_id = original.tenant_id;
        registry.upsert(original).await;

        // Upsert a different config at the same issuer URL.
        let updated = TenantConfig {
            tenant_id: original_id,
            name: "Updated Name".to_string(),
            oidc_issuer: "https://idp.example.com".to_string(),
            active: false,
        };
        registry.upsert(updated).await;

        let found = registry
            .resolve_by_issuer("https://idp.example.com")
            .await
            .expect("entry should still exist after overwrite");
        assert_eq!(found.name, "Updated Name");
        assert!(!found.active);
    }

    #[tokio::test]
    async fn multiple_tenants_are_isolated() {
        let registry = TenantRegistry::new();
        let a = make_config("https://a.example.com", true);
        let b = make_config("https://b.example.com", false);
        let a_id = a.tenant_id;
        let b_id = b.tenant_id;

        registry.upsert(a).await;
        registry.upsert(b).await;

        let found_a = registry
            .resolve_by_issuer("https://a.example.com")
            .await
            .unwrap();
        let found_b = registry
            .resolve_by_issuer("https://b.example.com")
            .await
            .unwrap();

        assert_eq!(found_a.tenant_id, a_id);
        assert_eq!(found_b.tenant_id, b_id);
        assert!(found_a.active);
        assert!(!found_b.active);
    }

    #[tokio::test]
    async fn clone_shares_same_inner_state() {
        let registry = TenantRegistry::new();
        let clone = registry.clone();

        let config = make_config("https://shared.example.com", true);
        registry.upsert(config).await;

        // The clone should see the entry too because they share the Arc.
        assert!(clone
            .resolve_by_issuer("https://shared.example.com")
            .await
            .is_some());
    }

    #[tokio::test]
    async fn default_creates_empty_registry() {
        let registry = TenantRegistry::default();
        assert!(registry
            .resolve_by_issuer("https://anything.example.com")
            .await
            .is_none());
    }
}
