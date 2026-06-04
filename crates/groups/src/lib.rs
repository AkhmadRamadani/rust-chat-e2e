//! crates/groups — group conversation management
//!
//! Provides the [`GroupRepository`] trait and its PostgreSQL implementation
//! [`PgGroupRepository`].  All methods are scoped to a specific tenant via
//! `tenant_id`.

use async_trait::async_trait;
use common::{ConversationId, ConversationMember, DeviceId, TenantId, UserId};
use sqlx::PgPool;
use thiserror::Error;
use uuid::Uuid;

// ── Error type ────────────────────────────────────────────────────────────────

/// Errors that can be returned by [`GroupRepository`] implementations.
#[derive(Debug, Error)]
pub enum GroupError {
    /// The requested conversation does not exist (within the given tenant).
    #[error("group not found")]
    NotFound,

    /// The user is not a member of the requested conversation.
    #[error("user is not a member")]
    NotMember,

    /// An underlying database error occurred.
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
}

// ── Repository trait ──────────────────────────────────────────────────────────

/// Async repository for group conversation operations.
///
/// All operations are scoped to `tenant_id` to enforce multi-tenant isolation.
#[async_trait]
pub trait GroupRepository: Send + Sync {
    /// Create a new group conversation and add the `initial_members` in a
    /// single atomic transaction.
    ///
    /// Returns the newly assigned [`ConversationId`] and the final member list
    /// (which equals `initial_members`).
    async fn create_group(
        &self,
        tenant_id: TenantId,
        initial_members: Vec<ConversationMember>,
    ) -> Result<(ConversationId, Vec<ConversationMember>), GroupError>;

    /// Add a single (user_id, device_id) pair to an existing group.
    async fn add_member(
        &self,
        tenant_id: TenantId,
        conversation_id: ConversationId,
        user_id: UserId,
        device_id: DeviceId,
    ) -> Result<(), GroupError>;

    /// Remove all device rows for `user_id` from a group conversation.
    async fn remove_member(
        &self,
        tenant_id: TenantId,
        conversation_id: ConversationId,
        user_id: UserId,
    ) -> Result<(), GroupError>;

    /// Return `true` when `user_id` has at least one device row in the group.
    async fn is_member(
        &self,
        tenant_id: TenantId,
        conversation_id: ConversationId,
        user_id: UserId,
    ) -> Result<bool, GroupError>;

    /// Fetch all current (user_id, device_id) pairs for a group conversation.
    async fn get_members(
        &self,
        tenant_id: TenantId,
        conversation_id: ConversationId,
    ) -> Result<Vec<ConversationMember>, GroupError>;

    /// Bulk-insert one [`sender_key_distributions`] row per recipient.
    ///
    /// Each recipient tuple is `(recipient_user_id, recipient_device_id,
    /// encrypted_skdm)`.
    async fn store_skdm(
        &self,
        tenant_id: TenantId,
        conversation_id: ConversationId,
        sender_user_id: UserId,
        sender_device_id: DeviceId,
        recipients: Vec<(UserId, DeviceId, Vec<u8>)>,
    ) -> Result<(), GroupError>;
}

// ── PostgreSQL implementation ─────────────────────────────────────────────────

/// [`GroupRepository`] backed by a `sqlx` [`PgPool`].
#[derive(Clone)]
pub struct PgGroupRepository {
    pool: PgPool,
}

impl PgGroupRepository {
    /// Create a new [`PgGroupRepository`] wrapping `pool`.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl GroupRepository for PgGroupRepository {
    /// Create a group conversation and add `initial_members` atomically.
    ///
    /// Steps performed inside a single transaction:
    /// 1. `INSERT INTO conversations` with `kind = 'group'`.
    /// 2. `INSERT INTO conversation_members` one row per `ConversationMember`.
    ///
    /// Returns the new [`ConversationId`] and the same `initial_members` list.
    async fn create_group(
        &self,
        tenant_id: TenantId,
        initial_members: Vec<ConversationMember>,
    ) -> Result<(ConversationId, Vec<ConversationMember>), GroupError> {
        let mut tx = self.pool.begin().await?;

        // 1. Insert the conversations row.
        let conversation_id: Uuid = sqlx::query_scalar(
            r#"
            INSERT INTO conversations (tenant_id, kind)
            VALUES ($1, 'group')
            RETURNING conversation_id
            "#,
        )
        .bind(tenant_id.0)
        .fetch_one(&mut *tx)
        .await?;

        // 2. Bulk-insert all initial members.
        for member in &initial_members {
            sqlx::query(
                r#"
                INSERT INTO conversation_members (tenant_id, conversation_id, user_id, device_id)
                VALUES ($1, $2, $3, $4)
                "#,
            )
            .bind(tenant_id.0)
            .bind(conversation_id)
            .bind(&member.user_id.0)
            .bind(member.device_id.0)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;

        let conv_id = ConversationId(conversation_id);
        Ok((conv_id, initial_members))
    }

    /// Add a single `(user_id, device_id)` pair to an existing group.
    async fn add_member(
        &self,
        tenant_id: TenantId,
        conversation_id: ConversationId,
        user_id: UserId,
        device_id: DeviceId,
    ) -> Result<(), GroupError> {
        sqlx::query(
            r#"
            INSERT INTO conversation_members (tenant_id, conversation_id, user_id, device_id)
            VALUES ($1, $2, $3, $4)
            "#,
        )
        .bind(tenant_id.0)
        .bind(conversation_id.0)
        .bind(&user_id.0)
        .bind(device_id.0)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Remove all device rows for `user_id` from a group conversation.
    async fn remove_member(
        &self,
        tenant_id: TenantId,
        conversation_id: ConversationId,
        user_id: UserId,
    ) -> Result<(), GroupError> {
        sqlx::query(
            r#"
            DELETE FROM conversation_members
            WHERE tenant_id = $1
              AND conversation_id = $2
              AND user_id = $3
            "#,
        )
        .bind(tenant_id.0)
        .bind(conversation_id.0)
        .bind(&user_id.0)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Return `true` when `user_id` has at least one row in `conversation_members`.
    async fn is_member(
        &self,
        tenant_id: TenantId,
        conversation_id: ConversationId,
        user_id: UserId,
    ) -> Result<bool, GroupError> {
        let exists: bool = sqlx::query_scalar(
            r#"
            SELECT EXISTS (
                SELECT 1
                FROM conversation_members
                WHERE tenant_id = $1
                  AND conversation_id = $2
                  AND user_id = $3
            )
            "#,
        )
        .bind(tenant_id.0)
        .bind(conversation_id.0)
        .bind(&user_id.0)
        .fetch_one(&self.pool)
        .await?;

        Ok(exists)
    }

    /// Fetch all current `(user_id, device_id)` pairs for a group conversation.
    async fn get_members(
        &self,
        tenant_id: TenantId,
        conversation_id: ConversationId,
    ) -> Result<Vec<ConversationMember>, GroupError> {
        let rows = sqlx::query_as::<_, (String, Uuid)>(
            r#"
            SELECT user_id, device_id
            FROM conversation_members
            WHERE tenant_id = $1
              AND conversation_id = $2
            "#,
        )
        .bind(tenant_id.0)
        .bind(conversation_id.0)
        .fetch_all(&self.pool)
        .await?;

        let members = rows
            .into_iter()
            .map(|(user_id, device_id)| ConversationMember {
                user_id: UserId(user_id),
                device_id: DeviceId(device_id),
            })
            .collect();

        Ok(members)
    }

    /// Bulk-insert one `sender_key_distributions` row per recipient.
    async fn store_skdm(
        &self,
        tenant_id: TenantId,
        conversation_id: ConversationId,
        sender_user_id: UserId,
        sender_device_id: DeviceId,
        recipients: Vec<(UserId, DeviceId, Vec<u8>)>,
    ) -> Result<(), GroupError> {
        let mut tx = self.pool.begin().await?;

        for (recipient_user_id, recipient_device_id, encrypted_skdm) in &recipients {
            sqlx::query(
                r#"
                INSERT INTO sender_key_distributions
                    (tenant_id, conversation_id, sender_user_id, sender_device_id,
                     recipient_user_id, recipient_device_id, encrypted_skdm)
                VALUES ($1, $2, $3, $4, $5, $6, $7)
                "#,
            )
            .bind(tenant_id.0)
            .bind(conversation_id.0)
            .bind(&sender_user_id.0)
            .bind(sender_device_id.0)
            .bind(&recipient_user_id.0)
            .bind(recipient_device_id.0)
            .bind(encrypted_skdm.as_slice())
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify that GroupError variants produce non-empty Display strings.
    #[test]
    fn error_display_not_empty() {
        let err_not_found = GroupError::NotFound;
        let err_not_member = GroupError::NotMember;
        assert!(!err_not_found.to_string().is_empty());
        assert!(!err_not_member.to_string().is_empty());
    }

    /// Verify that GroupError::NotFound is distinct from GroupError::NotMember.
    #[test]
    fn error_variants_are_distinct() {
        let not_found = GroupError::NotFound.to_string();
        let not_member = GroupError::NotMember.to_string();
        assert_ne!(not_found, not_member);
    }
}
