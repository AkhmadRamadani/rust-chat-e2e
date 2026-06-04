//! Conversation REST handlers — 1:1 session establishment via X3DH and
//! Double Ratchet messaging.
//!
//! # Route table
//!
//! | Method | Path | Handler |
//! |--------|------|---------|
//! | `POST` | `/conversations` | [`create_conversation`] |
//! | `POST` | `/conversations/{conversationID}/messages` | [`send_message`] |
//! | `GET`  | `/conversations/{conversationID}/messages` | [`get_messages`] |

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use auth::AuthenticatedUser;
use common::{
    error_codes, ApiError, ConversationId, DeviceId, MessageEnvelope, NewMessageEnvelope,
    RtEvent, UserId,
};
use messaging::{GetMessagesParams, MessagingError, MessagingRepository};
use realtime::WebTransportManager;

// ── Shared handler state ──────────────────────────────────────────────────────

/// Axum state shared across all conversation handlers.
#[derive(Clone)]
pub struct ConversationState {
    /// Messaging repository (PostgreSQL).
    pub repo: Arc<dyn MessagingRepository>,
    /// WebTransport manager for delivering real-time events (fan-out).
    pub wt_manager: Arc<dyn WebTransportManager>,
}

impl std::fmt::Debug for ConversationState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConversationState").finish_non_exhaustive()
    }
}

// ── Request / Response types ──────────────────────────────────────────────────

/// Request body for `POST /conversations`.
///
/// Carries the two participant identities plus the initial X3DH
/// `MessageEnvelope` (ciphertext + protocol header).
#[derive(Debug, Deserialize)]
pub struct CreateConversationRequest {
    /// The user ID of the other participant (the recipient).
    ///
    /// The sender's user ID and device ID are taken from the auth token.
    pub recipient_user_id: String,
    /// The device ID of the recipient.
    pub recipient_device_id: Uuid,
    /// The initial message envelope containing the X3DH header fields and
    /// encrypted payload.  Must be a `ProtocolHeader::X3dhInit` variant.
    pub envelope: NewMessageEnvelope,
}

/// Response body returned on both HTTP 200 (existing) and HTTP 201 (created).
#[derive(Debug, Serialize)]
pub struct CreateConversationResponse {
    /// The conversation UUID (pre-existing or newly created).
    pub conversation_id: Uuid,
}

// ── Error type ────────────────────────────────────────────────────────────────

/// Conversation-handler errors mapped to structured `ApiError` responses.
#[derive(Debug)]
pub enum ConversationHandlerError {
    /// HTTP 400 — invalid request (e.g. trying to start a conversation with
    /// yourself, missing required fields).
    BadRequest(String),
    /// HTTP 403 — requester is not a participant in this conversation.
    Forbidden,
    /// HTTP 503 — storage layer error.
    Storage(String),
}

impl IntoResponse for ConversationHandlerError {
    fn into_response(self) -> Response {
        match self {
            ConversationHandlerError::BadRequest(msg) => (
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    error_code: error_codes::BAD_REQUEST.to_string(),
                    message: msg,
                    request_id: Uuid::new_v4(),
                }),
            )
                .into_response(),
            ConversationHandlerError::Forbidden => (
                StatusCode::FORBIDDEN,
                Json(ApiError {
                    error_code: error_codes::NOT_A_MEMBER.to_string(),
                    message: "You are not a participant in this conversation.".to_string(),
                    request_id: Uuid::new_v4(),
                }),
            )
                .into_response(),
            ConversationHandlerError::Storage(msg) => {
                tracing::error!("Conversation storage error: {msg}");
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(ApiError {
                        error_code: error_codes::STORAGE_UNAVAILABLE.to_string(),
                        message: "A storage error occurred; please retry.".to_string(),
                        request_id: Uuid::new_v4(),
                    }),
                )
                    .into_response()
            }
        }
    }
}

impl From<MessagingError> for ConversationHandlerError {
    fn from(err: MessagingError) -> Self {
        match err {
            MessagingError::ConversationNotFound => {
                ConversationHandlerError::Storage("Conversation not found.".to_string())
            }
            MessagingError::NotParticipant => ConversationHandlerError::Forbidden,
            MessagingError::Database(e) => ConversationHandlerError::Storage(e.to_string()),
            MessagingError::Serialization(e) => ConversationHandlerError::Storage(e.to_string()),
        }
    }
}

// ── POST /conversations ───────────────────────────────────────────────────────

/// Create or retrieve a 1:1 direct conversation.
///
/// # Steps
/// 1. Validate the request (sender ≠ recipient).
/// 2. Check whether a direct conversation already exists between the two
///    participants within the same tenant (by querying `conversation_members`).
///    If found, return HTTP 200 with the existing `conversation_id`.
/// 3. Otherwise, within a single DB transaction:
///    a. Insert a new `conversations` row (`kind='direct'`).
///    b. Insert two `conversation_members` rows (one per participant).
/// 4. Store the initial X3DH `MessageEnvelope` via
///    `MessagingRepository::store_envelope`.
/// 5. Return HTTP 201 with the new `conversation_id`.
///
/// # Requirements: 5.1, 5.2, 5.3, 5.4, 5.5
pub async fn create_conversation(
    State(state): State<ConversationState>,
    auth_user: AuthenticatedUser,
    Json(body): Json<CreateConversationRequest>,
) -> Result<Response, ConversationHandlerError> {
    let tenant_id = auth_user.tenant_id;
    let sender_user_id = auth_user.user_id.clone();
    // Use device_id from the envelope since OIDC tokens don't carry a device_id claim.
    let sender_device_id = auth_user.device_id
        .unwrap_or(body.envelope.sender_device_id);

    let recipient_user_id = UserId(body.recipient_user_id.clone());
    let recipient_device_id = DeviceId(body.recipient_device_id);

    // Validate: sender and recipient must be different users.
    if sender_user_id == recipient_user_id {
        return Err(ConversationHandlerError::BadRequest(
            "Cannot create a conversation with yourself.".to_string(),
        ));
    }

    // Step 2: Check for an existing direct conversation between the two participants.
    let existing = state
        .repo
        .find_direct_conversation(tenant_id, &sender_user_id, &recipient_user_id)
        .await
        .map_err(ConversationHandlerError::from)?;

    if let Some(existing_id) = existing {
        // Conversation already exists — return HTTP 200 with the existing ID.
        let resp = Json(CreateConversationResponse {
            conversation_id: existing_id.0,
        });
        return Ok((StatusCode::OK, resp).into_response());
    }

    // Step 3: Create a new conversation + member rows in a transaction.
    let conversation_id: ConversationId = state
        .repo
        .create_direct_conversation(
            tenant_id,
            sender_user_id.clone(),
            sender_device_id,
            recipient_user_id.clone(),
            recipient_device_id,
        )
        .await
        .map_err(ConversationHandlerError::from)?;

    // Step 4: Store the initial X3DH MessageEnvelope.
    //
    // The envelope supplied in the request body must reference the newly
    // created conversation_id.  We override the conversation_id field to
    // ensure consistency regardless of what the client supplied.
    let new_envelope = NewMessageEnvelope {
        conversation_id,
        sender_user_id: sender_user_id.clone(),
        sender_device_id,
        recipient_user_id: Some(recipient_user_id),
        recipient_device_id: Some(recipient_device_id),
        ciphertext: body.envelope.ciphertext,
        protocol_header: body.envelope.protocol_header,
        attachment_id: body.envelope.attachment_id,
    };

    state
        .repo
        .store_envelope(tenant_id, new_envelope)
        .await
        .map_err(ConversationHandlerError::from)?;

    // Step 5: Return HTTP 201 with the new conversation_id.
    let resp = Json(CreateConversationResponse {
        conversation_id: conversation_id.0,
    });
    Ok((StatusCode::CREATED, resp).into_response())
}

// ── POST /conversations/{conversationID}/messages ─────────────────────────────

/// Request body for `POST /conversations/{conversationID}/messages`.
///
/// Carries an opaque Double Ratchet (or other) `NewMessageEnvelope`
/// (ciphertext + protocol header + recipient fields).
#[derive(Debug, Deserialize)]
pub struct SendMessageRequest {
    /// The message envelope to store and deliver.
    pub envelope: NewMessageEnvelope,
}

/// Response body for a successfully stored message.
#[derive(Debug, Serialize)]
pub struct SendMessageResponse {
    /// Server-assigned sequence number for the stored envelope.
    pub seq: u64,
    /// Server receipt timestamp in Unix epoch milliseconds.
    pub server_ts: u64,
}

/// Store and fan-out a message in an existing 1:1 conversation.
///
/// # Steps
/// 1. Verify the requester is a participant of the conversation within the
///    same tenant; return HTTP 403 otherwise.
/// 2. Store the `MessageEnvelope` via `MessagingRepository::store_envelope`.
/// 3. Fan out to the recipient device(s) via `WebTransportManager::deliver`.
///
/// # Requirements: 6.1, 6.2, 6.3, 6.6
pub async fn send_message(
    State(state): State<ConversationState>,
    auth_user: AuthenticatedUser,
    Path(conversation_id): Path<Uuid>,
    Json(body): Json<SendMessageRequest>,
) -> Result<(StatusCode, Json<SendMessageResponse>), ConversationHandlerError> {
    let tenant_id = auth_user.tenant_id;
    let sender_user_id = auth_user.user_id.clone();
    // Use device_id from the envelope since OIDC tokens don't carry a device_id claim.
    let sender_device_id = auth_user.device_id
        .unwrap_or(body.envelope.sender_device_id);
    let conv_id = ConversationId(conversation_id);

    // Step 1: Verify the requester is a participant.
    let is_participant = state
        .repo
        .is_participant(tenant_id, conv_id, &sender_user_id)
        .await
        .map_err(ConversationHandlerError::from)?;

    if !is_participant {
        return Err(ConversationHandlerError::Forbidden);
    }

    // Step 2: Build and store the envelope, overriding the conversation_id to
    // ensure it matches the path parameter.
    let new_envelope = NewMessageEnvelope {
        conversation_id: conv_id,
        sender_user_id: sender_user_id.clone(),
        sender_device_id,
        recipient_user_id: body.envelope.recipient_user_id.clone(),
        recipient_device_id: body.envelope.recipient_device_id,
        ciphertext: body.envelope.ciphertext,
        protocol_header: body.envelope.protocol_header,
        attachment_id: body.envelope.attachment_id,
    };

    let stored = state
        .repo
        .store_envelope(tenant_id, new_envelope)
        .await
        .map_err(ConversationHandlerError::from)?;

    // Step 3: Fan out to recipient device sessions (fire-and-forget; delivery
    // failures fall back to the offline queue inside `deliver`).
    if let Some(recipient_device_id) = stored.recipient_device_id {
        let event = RtEvent::Message(stored.clone());
        let _ = state
            .wt_manager
            .deliver(tenant_id, recipient_device_id, event)
            .await;
    }

    Ok((
        StatusCode::CREATED,
        Json(SendMessageResponse {
            seq: stored.seq,
            server_ts: stored.server_ts,
        }),
    ))
}

// ── GET /conversations/{conversationID}/messages ──────────────────────────────

/// Query parameters for `GET /conversations/{conversationID}/messages`.
#[derive(Debug, Deserialize)]
pub struct GetMessagesQuery {
    /// Return messages with seq < before_seq (cursor-based pagination).
    pub before_seq: Option<u64>,
    /// Maximum number of messages to return; defaults to 50, clamped to 200.
    pub limit: Option<u64>,
}

/// Response body for `GET /conversations/{conversationID}/messages`.
#[derive(Debug, Serialize)]
pub struct GetMessagesResponse {
    /// Paginated list of message envelopes, ordered ascending by seq.
    pub messages: Vec<MessageEnvelope>,
}

/// Retrieve paginated message history for a conversation.
///
/// # Steps
/// 1. Verify the requester is a participant within the same tenant; return
///    HTTP 403 otherwise.
/// 2. Extract `before_seq` and `limit` query params (`limit` defaults to 50,
///    clamped to 200).
/// 3. Call `MessagingRepository::get_messages` and return the paginated list.
///
/// # Requirements: 9.1, 9.2, 9.3, 9.4, 9.5
pub async fn get_messages(
    State(state): State<ConversationState>,
    auth_user: AuthenticatedUser,
    Path(conversation_id): Path<Uuid>,
    Query(query): Query<GetMessagesQuery>,
) -> Result<Json<GetMessagesResponse>, ConversationHandlerError> {
    let tenant_id = auth_user.tenant_id;
    let user_id = auth_user.user_id.clone();
    let conv_id = ConversationId(conversation_id);

    // Step 1: Verify the requester is a participant.
    let is_participant = state
        .repo
        .is_participant(tenant_id, conv_id, &user_id)
        .await
        .map_err(ConversationHandlerError::from)?;

    if !is_participant {
        return Err(ConversationHandlerError::Forbidden);
    }

    // Step 2 & 3: Retrieve messages using keyset pagination.
    // The repository applies defaults (50) and clamps (max 200) itself, but we
    // also pass through what the caller supplied.
    let params = GetMessagesParams {
        conversation_id: conv_id,
        before_seq: query.before_seq,
        limit: query.limit,
    };

    let messages = state
        .repo
        .get_messages(tenant_id, params)
        .await
        .map_err(ConversationHandlerError::from)?;

    Ok(Json(GetMessagesResponse { messages }))
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_conversation_response_serialises() {
        let resp = CreateConversationResponse {
            conversation_id: Uuid::new_v4(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("conversation_id"));
    }

    #[test]
    fn send_message_response_serialises() {
        let resp = SendMessageResponse {
            seq: 42,
            server_ts: 1_700_000_000_000,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"seq\":42"));
        assert!(json.contains("server_ts"));
    }

    #[test]
    fn get_messages_response_serialises_empty() {
        let resp = GetMessagesResponse { messages: vec![] };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"messages\":[]"));
    }

    #[test]
    fn conversation_handler_error_bad_request_into_response() {
        let err = ConversationHandlerError::BadRequest("test error".to_string());
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn conversation_handler_error_forbidden_into_response() {
        let err = ConversationHandlerError::Forbidden;
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn conversation_handler_error_storage_into_response() {
        let err = ConversationHandlerError::Storage("db down".to_string());
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn messaging_error_not_participant_converts_to_forbidden() {
        let err = ConversationHandlerError::from(MessagingError::NotParticipant);
        assert!(matches!(err, ConversationHandlerError::Forbidden));
    }

    #[test]
    fn messaging_error_converts_to_handler_error() {
        let err = ConversationHandlerError::from(MessagingError::ConversationNotFound);
        assert!(matches!(err, ConversationHandlerError::Storage(_)));
    }

    #[test]
    fn messaging_db_error_converts_to_storage_handler_error() {
        let err = ConversationHandlerError::from(MessagingError::Serialization(
            serde_json::from_str::<serde_json::Value>("not valid json {{{").unwrap_err(),
        ));
        assert!(matches!(err, ConversationHandlerError::Storage(_)));
    }
}