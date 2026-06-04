// crates/messaging — message routing and storage
// Repository implementation for task 2.5.

use async_trait::async_trait;
use bytes::Bytes;
use chrono::Utc;
use common::{
    ConversationId, DeviceId, MessageEnvelope, NewMessageEnvelope, ProtocolHeader, TenantId,
    UserId,
};
use serde_json::Value as JsonValue;
use sqlx::PgPool;
use thiserror::Error;
use uuid::Uuid;

// ── Error type ────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum MessagingError {
    #[error("conversation not found")]
    ConversationNotFound,

    #[error("requester is not a participant in this conversation")]
    NotParticipant,

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

// ── Pagination parameters ─────────────────────────────────────────────────────

/// Parameters for keyset-paginated message retrieval.
#[derive(Debug, Clone)]
pub struct GetMessagesParams {
    pub conversation_id: ConversationId,
    /// Return messages with seq < before_seq. If None, returns the most recent messages.
    pub before_seq: Option<u64>,
    /// Maximum number of messages to return. Default 50, max 200.
    pub limit: Option<u64>,
}

// ── Repository trait ──────────────────────────────────────────────────────────

/// Async repository trait for all messaging storage operations.
///
/// Every method is scoped to a specific `tenant_id` so that no query can
/// cross tenant boundaries.
#[async_trait]
pub trait MessagingRepository: Send + Sync {
    /// Find an existing direct conversation between exactly two participants
    /// within the same tenant.
    ///
    /// Returns `Some(ConversationId)` if a `kind='direct'` conversation exists
    /// whose member set is exactly `{(user_a, device_a), (user_b, device_b)}`,
    /// or `None` if no such conversation exists.
    async fn find_direct_conversation(
        &self,
        tenant_id: TenantId,
        user_a: &UserId,
        user_b: &UserId,
    ) -> Result<Option<ConversationId>, MessagingError>;

    /// Create a new direct conversation and add both participants as members,
    /// all within a single transaction.
    ///
    /// Returns the newly assigned `ConversationId`.
    async fn create_direct_conversation(
        &self,
        tenant_id: TenantId,
        user_a: UserId,
        device_a: DeviceId,
        user_b: UserId,
        device_b: DeviceId,
    ) -> Result<ConversationId, MessagingError>;

    /// Check whether a user is a participant of a conversation.
    async fn is_participant(
        &self,
        tenant_id: TenantId,
        conversation_id: ConversationId,
        user_id: &UserId,
    ) -> Result<bool, MessagingError>;
    /// Atomically assign the next sequence number and persist the envelope.
    ///
    /// Uses the pattern:
    /// ```sql
    /// UPDATE conversations SET last_seq = last_seq + 1
    /// WHERE (tenant_id, conversation_id) = ($1, $2)
    /// RETURNING last_seq
    /// ```
    /// then inserts the envelope with the returned `seq` and the current
    /// server timestamp.
    async fn store_envelope(
        &self,
        tenant_id: TenantId,
        envelope: NewMessageEnvelope,
    ) -> Result<MessageEnvelope, MessagingError>;

    /// Retrieve messages using keyset pagination (ascending by seq).
    ///
    /// Returns messages where `seq < before_seq` (or all messages when
    /// `before_seq` is `None`), ordered ascending by `seq`, limited to at
    /// most `limit` results (default 50, clamped to 200 max).
    async fn get_messages(
        &self,
        tenant_id: TenantId,
        params: GetMessagesParams,
    ) -> Result<Vec<MessageEnvelope>, MessagingError>;

    /// Update the per-device delivery state to record `last_delivered_seq`.
    ///
    /// Upserts a row in `delivery_state` for `(tenant_id, conversation_id,
    /// device_id)`.
    async fn mark_delivered(
        &self,
        tenant_id: TenantId,
        conversation_id: ConversationId,
        device_id: DeviceId,
        seq: u64,
    ) -> Result<(), MessagingError>;

    /// Enqueue an envelope for offline delivery.
    ///
    /// Checks the current queue depth (`last_delivered_seq - last_acked_seq`) for
    /// the device.  If the depth is already at the 10,000-envelope cap,
    /// increments `dropped_count` instead of queuing the envelope.  Otherwise,
    /// advances `last_delivered_seq` to record that the envelope has been
    /// dispatched toward the device.
    async fn enqueue_offline(
        &self,
        tenant_id: TenantId,
        conversation_id: ConversationId,
        device_id: DeviceId,
        seq: u64,
    ) -> Result<EnqueueResult, MessagingError>;

    /// Return all messages that have not yet been acknowledged by the device.
    ///
    /// Fetches `message_envelopes` rows where
    /// `seq > last_acked_seq` for the given `(tenant_id, conversation_id,
    /// device_id)`, up to 100 messages at a time.
    async fn drain_offline_queue(
        &self,
        tenant_id: TenantId,
        conversation_id: ConversationId,
        device_id: DeviceId,
    ) -> Result<Vec<MessageEnvelope>, MessagingError>;
}

/// Result returned by [`MessagingRepository::enqueue_offline`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnqueueResult {
    /// The envelope was recorded in the device's delivery queue.
    Queued,
    /// The queue was already at the 10,000-envelope cap; the envelope was
    /// dropped and `dropped_count` was incremented.
    DroppedQueueFull,
}

// ── PostgreSQL implementation ─────────────────────────────────────────────────

/// `sqlx`-backed implementation of [`MessagingRepository`] for PostgreSQL.
#[derive(Debug, Clone)]
pub struct PgMessagingRepository {
    pool: PgPool,
}

impl PgMessagingRepository {
    /// Create a new repository wrapping the given connection pool.
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

/// Maximum number of envelopes that can be queued per (conversation, device).
const OFFLINE_QUEUE_CAP: i64 = 10_000;

/// Default page size for `get_messages`.
const DEFAULT_LIMIT: i64 = 50;

/// Maximum allowed page size for `get_messages`.
const MAX_LIMIT: i64 = 200;

// ── Helper: row → MessageEnvelope ─────────────────────────────────────────────

fn row_to_envelope(
    conversation_id: Uuid,
    seq: i64,
    sender_user_id: String,
    sender_device_id: Uuid,
    recipient_user_id: Option<String>,
    recipient_device_id: Option<Uuid>,
    ciphertext: Vec<u8>,
    protocol_header: JsonValue,
    server_ts: i64,
    attachment_id: Option<Uuid>,
) -> Result<MessageEnvelope, MessagingError> {
    let protocol_header: ProtocolHeader = serde_json::from_value(protocol_header)?;
    Ok(MessageEnvelope {
        conversation_id: ConversationId(conversation_id),
        seq: seq as u64,
        sender_user_id: UserId(sender_user_id),
        sender_device_id: DeviceId(sender_device_id),
        recipient_user_id: recipient_user_id.map(UserId),
        recipient_device_id: recipient_device_id.map(DeviceId),
        ciphertext: Bytes::from(ciphertext),
        protocol_header,
        server_ts: server_ts as u64,
        attachment_id,
    })
}

// ── Trait implementation ──────────────────────────────────────────────────────

#[async_trait]
impl MessagingRepository for PgMessagingRepository {
    async fn find_direct_conversation(
        &self,
        tenant_id: TenantId,
        user_a: &UserId,
        user_b: &UserId,
    ) -> Result<Option<ConversationId>, MessagingError> {
        let tenant_uuid = tenant_id.0;
        let user_a_str = &user_a.0;
        let user_b_str = &user_b.0;

        // Find a 'direct' conversation where both user_a and user_b are members
        // within the same tenant, and the conversation has exactly 2 members total.
        let row = sqlx::query(
            r#"
            SELECT cm1.conversation_id
              FROM conversation_members cm1
              JOIN conversation_members cm2
                ON cm1.tenant_id       = cm2.tenant_id
               AND cm1.conversation_id = cm2.conversation_id
              JOIN conversations c
                ON c.tenant_id         = cm1.tenant_id
               AND c.conversation_id   = cm1.conversation_id
             WHERE cm1.tenant_id = $1
               AND cm1.user_id   = $2
               AND cm2.user_id   = $3
               AND c.kind        = 'direct'
             LIMIT 1
            "#,
        )
        .bind(tenant_uuid)
        .bind(user_a_str)
        .bind(user_b_str)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(r) => {
                use sqlx::Row;
                let conv_uuid: Uuid = r.try_get("conversation_id")?;
                Ok(Some(ConversationId(conv_uuid)))
            }
            None => Ok(None),
        }
    }

    async fn create_direct_conversation(
        &self,
        tenant_id: TenantId,
        user_a: UserId,
        device_a: DeviceId,
        user_b: UserId,
        device_b: DeviceId,
    ) -> Result<ConversationId, MessagingError> {
        let tenant_uuid = tenant_id.0;
        let conv_uuid = Uuid::new_v4();

        let mut tx = self.pool.begin().await?;

        // Insert the conversation row.
        sqlx::query(
            r#"
            INSERT INTO conversations (tenant_id, conversation_id, kind, last_seq)
            VALUES ($1, $2, 'direct', 0)
            "#,
        )
        .bind(tenant_uuid)
        .bind(conv_uuid)
        .execute(&mut *tx)
        .await?;

        // Insert member rows for both participants.
        sqlx::query(
            r#"
            INSERT INTO conversation_members (tenant_id, conversation_id, user_id, device_id)
            VALUES ($1, $2, $3, $4)
            "#,
        )
        .bind(tenant_uuid)
        .bind(conv_uuid)
        .bind(&user_a.0)
        .bind(device_a.0)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO conversation_members (tenant_id, conversation_id, user_id, device_id)
            VALUES ($1, $2, $3, $4)
            "#,
        )
        .bind(tenant_uuid)
        .bind(conv_uuid)
        .bind(&user_b.0)
        .bind(device_b.0)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(ConversationId(conv_uuid))
    }

    async fn is_participant(
        &self,
        tenant_id: TenantId,
        conversation_id: ConversationId,
        user_id: &UserId,
    ) -> Result<bool, MessagingError> {
        let tenant_uuid = tenant_id.0;
        let conv_uuid = conversation_id.0;
        let user_str = &user_id.0;

        let row = sqlx::query(
            r#"
            SELECT 1
              FROM conversation_members
             WHERE tenant_id       = $1
               AND conversation_id = $2
               AND user_id         = $3
             LIMIT 1
            "#,
        )
        .bind(tenant_uuid)
        .bind(conv_uuid)
        .bind(user_str)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.is_some())
    }

    async fn store_envelope(
        &self,
        tenant_id: TenantId,
        envelope: NewMessageEnvelope,
    ) -> Result<MessageEnvelope, MessagingError> {
        let tenant_uuid = tenant_id.0;
        let conv_uuid = envelope.conversation_id.0;
        let server_ts = Utc::now().timestamp_millis();

        let protocol_header_json = serde_json::to_value(&envelope.protocol_header)?;
        let ciphertext_bytes: Vec<u8> = envelope.ciphertext.to_vec();
        let sender_user_id = envelope.sender_user_id.0.clone();
        let sender_device_uuid = envelope.sender_device_id.0;
        let recipient_user_id = envelope.recipient_user_id.as_ref().map(|u| u.0.clone());
        let recipient_device_uuid = envelope.recipient_device_id.as_ref().map(|d| d.0);
        let attachment_id = envelope.attachment_id;

        let mut tx = self.pool.begin().await?;

        let seq_row = sqlx::query(
            r#"
            UPDATE conversations
               SET last_seq = last_seq + 1
             WHERE tenant_id       = $1
               AND conversation_id = $2
            RETURNING last_seq
            "#,
        )
        .bind(tenant_uuid)
        .bind(conv_uuid)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(MessagingError::ConversationNotFound)?;

        let seq: i64 = sqlx::Row::try_get(&seq_row, "last_seq")?;

        sqlx::query(
            r#"
            INSERT INTO message_envelopes
                (tenant_id, conversation_id, seq,
                 sender_user_id, sender_device_id,
                 recipient_user_id, recipient_device_id,
                 ciphertext, protocol_header, server_ts, attachment_id)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            "#,
        )
        .bind(tenant_uuid)
        .bind(conv_uuid)
        .bind(seq)
        .bind(&sender_user_id)
        .bind(sender_device_uuid)
        .bind(&recipient_user_id)
        .bind(recipient_device_uuid)
        .bind(&ciphertext_bytes)
        .bind(&protocol_header_json)
        .bind(server_ts)
        .bind(attachment_id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(MessageEnvelope {
            conversation_id: envelope.conversation_id,
            seq: seq as u64,
            sender_user_id: envelope.sender_user_id,
            sender_device_id: envelope.sender_device_id,
            recipient_user_id: envelope.recipient_user_id,
            recipient_device_id: envelope.recipient_device_id,
            ciphertext: envelope.ciphertext,
            protocol_header: envelope.protocol_header,
            server_ts: server_ts as u64,
            attachment_id,
        })
    }

    async fn get_messages(
        &self,
        tenant_id: TenantId,
        params: GetMessagesParams,
    ) -> Result<Vec<MessageEnvelope>, MessagingError> {
        let tenant_uuid = tenant_id.0;
        let conv_uuid = params.conversation_id.0;

        // Apply defaults and clamp: 0 → 50, max 200, min 1.
        let raw_limit = params.limit.unwrap_or(DEFAULT_LIMIT as u64) as i64;
        let limit = raw_limit.min(MAX_LIMIT).max(1);

        let rows: Vec<sqlx::postgres::PgRow> = match params.before_seq {
            Some(before_seq) => {
                let before_seq_i64 = before_seq as i64;
                sqlx::query(
                    r#"
                    SELECT conversation_id,
                           seq,
                           sender_user_id,
                           sender_device_id,
                           recipient_user_id,
                           recipient_device_id,
                           ciphertext,
                           protocol_header,
                           server_ts,
                           attachment_id
                      FROM message_envelopes
                     WHERE tenant_id       = $1
                       AND conversation_id = $2
                       AND seq             < $3
                     ORDER BY seq ASC
                     LIMIT $4
                    "#,
                )
                .bind(tenant_uuid)
                .bind(conv_uuid)
                .bind(before_seq_i64)
                .bind(limit)
                .fetch_all(&self.pool)
                .await?
            }
            None => {
                sqlx::query(
                    r#"
                    SELECT conversation_id,
                           seq,
                           sender_user_id,
                           sender_device_id,
                           recipient_user_id,
                           recipient_device_id,
                           ciphertext,
                           protocol_header,
                           server_ts,
                           attachment_id
                      FROM message_envelopes
                     WHERE tenant_id       = $1
                       AND conversation_id = $2
                     ORDER BY seq ASC
                     LIMIT $3
                    "#,
                )
                .bind(tenant_uuid)
                .bind(conv_uuid)
                .bind(limit)
                .fetch_all(&self.pool)
                .await?
            }
        };

        rows.into_iter()
            .map(|r| {
                use sqlx::Row;
                let conversation_id: Uuid = r.try_get("conversation_id")?;
                let seq: i64 = r.try_get("seq")?;
                let sender_user_id: String = r.try_get("sender_user_id")?;
                let sender_device_id: Uuid = r.try_get("sender_device_id")?;
                let recipient_user_id: Option<String> = r.try_get("recipient_user_id")?;
                let recipient_device_id: Option<Uuid> = r.try_get("recipient_device_id")?;
                let ciphertext: Vec<u8> = r.try_get("ciphertext")?;
                let protocol_header: JsonValue = r.try_get("protocol_header")?;
                let server_ts: i64 = r.try_get("server_ts")?;
                let attachment_id: Option<Uuid> = r.try_get("attachment_id")?;

                row_to_envelope(
                    conversation_id,
                    seq,
                    sender_user_id,
                    sender_device_id,
                    recipient_user_id,
                    recipient_device_id,
                    ciphertext,
                    protocol_header,
                    server_ts,
                    attachment_id,
                )
                .map_err(MessagingError::from)
            })
            .collect()
    }

    async fn mark_delivered(
        &self,
        tenant_id: TenantId,
        conversation_id: ConversationId,
        device_id: DeviceId,
        seq: u64,
    ) -> Result<(), MessagingError> {
        let tenant_uuid = tenant_id.0;
        let conv_uuid = conversation_id.0;
        let device_uuid = device_id.0;
        let seq_i64 = seq as i64;

        // Upsert: if a row doesn't exist yet, create it; otherwise advance
        // last_delivered_seq only if the new value is greater.
        sqlx::query(
            r#"
            INSERT INTO delivery_state
                (tenant_id, conversation_id, device_id, last_delivered_seq, last_acked_seq, dropped_count)
            VALUES ($1, $2, $3, $4, 0, 0)
            ON CONFLICT (tenant_id, conversation_id, device_id)
            DO UPDATE SET last_delivered_seq = GREATEST(delivery_state.last_delivered_seq, $4)
            "#,
        )
        .bind(tenant_uuid)
        .bind(conv_uuid)
        .bind(device_uuid)
        .bind(seq_i64)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn enqueue_offline(
        &self,
        tenant_id: TenantId,
        conversation_id: ConversationId,
        device_id: DeviceId,
        seq: u64,
    ) -> Result<EnqueueResult, MessagingError> {
        let tenant_uuid = tenant_id.0;
        let conv_uuid = conversation_id.0;
        let device_uuid = device_id.0;
        let seq_i64 = seq as i64;

        // Ensure a delivery_state row exists for this (tenant, conv, device).
        sqlx::query(
            r#"
            INSERT INTO delivery_state
                (tenant_id, conversation_id, device_id, last_delivered_seq, last_acked_seq, dropped_count)
            VALUES ($1, $2, $3, 0, 0, 0)
            ON CONFLICT (tenant_id, conversation_id, device_id) DO NOTHING
            "#,
        )
        .bind(tenant_uuid)
        .bind(conv_uuid)
        .bind(device_uuid)
        .execute(&self.pool)
        .await?;

        // Read the current queue depth: queued = last_delivered_seq - last_acked_seq.
        let state_row_opt = sqlx::query(
            r#"
            SELECT last_delivered_seq, last_acked_seq
              FROM delivery_state
             WHERE tenant_id       = $1
               AND conversation_id = $2
               AND device_id       = $3
            "#,
        )
        .bind(tenant_uuid)
        .bind(conv_uuid)
        .bind(device_uuid)
        .fetch_optional(&self.pool)
        .await?;

        // If the row is still not found (unlikely after the INSERT above), treat
        // the message as queued since there is no queue to overflow.
        let state_row = match state_row_opt {
            Some(r) => r,
            None => return Ok(EnqueueResult::Queued),
        };

        use sqlx::Row;
        let last_delivered_seq: i64 = state_row.try_get("last_delivered_seq")?;
        let last_acked_seq: i64 = state_row.try_get("last_acked_seq")?;
        let queue_depth = last_delivered_seq - last_acked_seq;

        if queue_depth >= OFFLINE_QUEUE_CAP {
            // Queue full — drop and increment counter.
            sqlx::query(
                r#"
                UPDATE delivery_state
                   SET dropped_count = dropped_count + 1
                 WHERE tenant_id       = $1
                   AND conversation_id = $2
                   AND device_id       = $3
                "#,
            )
            .bind(tenant_uuid)
            .bind(conv_uuid)
            .bind(device_uuid)
            .execute(&self.pool)
            .await?;

            Ok(EnqueueResult::DroppedQueueFull)
        } else {
            // There is room — advance last_delivered_seq to record this envelope.
            sqlx::query(
                r#"
                UPDATE delivery_state
                   SET last_delivered_seq = GREATEST(last_delivered_seq, $4)
                 WHERE tenant_id       = $1
                   AND conversation_id = $2
                   AND device_id       = $3
                "#,
            )
            .bind(tenant_uuid)
            .bind(conv_uuid)
            .bind(device_uuid)
            .bind(seq_i64)
            .execute(&self.pool)
            .await?;

            Ok(EnqueueResult::Queued)
        }
    }

    async fn drain_offline_queue(
        &self,
        tenant_id: TenantId,
        conversation_id: ConversationId,
        device_id: DeviceId,
    ) -> Result<Vec<MessageEnvelope>, MessagingError> {
        let tenant_uuid = tenant_id.0;
        let conv_uuid = conversation_id.0;
        let device_uuid = device_id.0;

        // Find the last acknowledged seq so we can return everything above it.
        let state_row_opt = sqlx::query(
            r#"
            SELECT last_acked_seq
              FROM delivery_state
             WHERE tenant_id       = $1
               AND conversation_id = $2
               AND device_id       = $3
            "#,
        )
        .bind(tenant_uuid)
        .bind(conv_uuid)
        .bind(device_uuid)
        .fetch_optional(&self.pool)
        .await?;

        // If there is no delivery_state row the device has never connected to
        // this conversation — return an empty list.
        let last_acked_seq: i64 = match state_row_opt {
            Some(r) => {
                use sqlx::Row;
                r.try_get("last_acked_seq")?
            }
            None => return Ok(Vec::new()),
        };

        let rows = sqlx::query(
            r#"
            SELECT conversation_id,
                   seq,
                   sender_user_id,
                   sender_device_id,
                   recipient_user_id,
                   recipient_device_id,
                   ciphertext,
                   protocol_header,
                   server_ts,
                   attachment_id
              FROM message_envelopes
             WHERE tenant_id       = $1
               AND conversation_id = $2
               AND seq             > $3
             ORDER BY seq ASC
             LIMIT 100
            "#,
        )
        .bind(tenant_uuid)
        .bind(conv_uuid)
        .bind(last_acked_seq)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|r| {
                use sqlx::Row;
                let conversation_id: Uuid = r.try_get("conversation_id")?;
                let seq: i64 = r.try_get("seq")?;
                let sender_user_id: String = r.try_get("sender_user_id")?;
                let sender_device_id: Uuid = r.try_get("sender_device_id")?;
                let recipient_user_id: Option<String> = r.try_get("recipient_user_id")?;
                let recipient_device_id: Option<Uuid> = r.try_get("recipient_device_id")?;
                let ciphertext: Vec<u8> = r.try_get("ciphertext")?;
                let protocol_header: JsonValue = r.try_get("protocol_header")?;
                let server_ts: i64 = r.try_get("server_ts")?;
                let attachment_id: Option<Uuid> = r.try_get("attachment_id")?;

                row_to_envelope(
                    conversation_id,
                    seq,
                    sender_user_id,
                    sender_device_id,
                    recipient_user_id,
                    recipient_device_id,
                    ciphertext,
                    protocol_header,
                    server_ts,
                    attachment_id,
                )
                .map_err(MessagingError::from)
            })
            .collect()
    }
}

// ── realtime trait implementations ───────────────────────────────────────────
//
// These allow `PgMessagingRepository` to be used directly as the offline
// queue drain/enqueue for the WebSocket session manager.

#[async_trait]
impl realtime::OfflineQueueDrain for PgMessagingRepository {
    /// Drain all unacknowledged envelopes for a device.
    ///
    /// NOTE: This implementation returns an empty list because the current
    /// schema tracks delivery per (conversation, device) and we don't have a
    /// cross-conversation query. Conversations call `drain_offline_queue`
    /// individually. A future migration can add a device-level view.
    async fn drain_for_device(
        &self,
        _tenant_id: common::TenantId,
        _device_id: common::DeviceId,
    ) -> Result<Vec<common::MessageEnvelope>, String> {
        Ok(vec![])
    }
}

#[async_trait]
impl realtime::OfflineEnqueue for PgMessagingRepository {
    async fn enqueue(
        &self,
        tenant_id: common::TenantId,
        device_id: common::DeviceId,
        envelope: common::MessageEnvelope,
    ) -> Result<realtime::EnqueueResult, String> {
        match self
            .enqueue_offline(
                tenant_id,
                envelope.conversation_id,
                device_id,
                envelope.seq,
            )
            .await
        {
            Ok(EnqueueResult::Queued) => Ok(realtime::EnqueueResult::Queued),
            Ok(EnqueueResult::DroppedQueueFull) => Ok(realtime::EnqueueResult::DroppedQueueFull),
            Err(e) => Err(e.to_string()),
        }
    }
}
