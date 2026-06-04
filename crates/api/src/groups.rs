//! Group conversation REST handlers — group creation, messaging, membership
//! management, and SenderKey distribution.
//!
//! # Route table
//!
//! | Method   | Path                                              | Handler                        |
//! |----------|---------------------------------------------------|--------------------------------|
//! | `POST`   | `/groups`                                         | [`create_group`]               |
//! | `POST`   | `/groups/{conversationID}/messages`               | [`send_group_message`]         |
//! | `POST`   | `/groups/{conversationID}/members`                | [`add_group_member`]           |
//! | `DELETE` | `/groups/{conversationID}/members/{userID}`       | [`remove_group_member`]        |
//! | `POST`   | `/groups/{conversationID}/sender-key-distribution`| [`distribute_sender_key`]      |

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use auth::AuthenticatedUser;
use common::{
    error_codes, ApiError, ConversationId, ConversationMember, DeviceId, NewMessageEnvelope,
    RtEvent, UserId,
};
use groups::{GroupError, GroupRepository};
use messaging::{MessagingError, MessagingRepository};
use realtime::WebTransportManager;

// ── Shared handler state ──────────────────────────────────────────────────────

/// Axum state shared across all group handlers.
#[derive(Clone)]
pub struct GroupState {
    /// Group membership repository (PostgreSQL).
    pub group_repo: Arc<dyn GroupRepository>,
    /// Messaging repository for storing envelopes (PostgreSQL).
    pub messaging_repo: Arc<dyn MessagingRepository>,
    /// WebTransport manager for delivering real-time events.
    pub wt_manager: Arc<dyn WebTransportManager>,
}

impl std::fmt::Debug for GroupState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GroupState").finish_non_exhaustive()
    }
}

// ── Error type ────────────────────────────────────────────────────────────────

/// Group-handler errors mapped to structured `ApiError` responses.
#[derive(Debug)]
pub enum GroupHandlerError {
    /// HTTP 400 — invalid request (e.g., wrong member count).
    BadRequest(String),
    /// HTTP 403 — requester is not a member of this group.
    NotAMember,
    /// HTTP 404 — group conversation not found.
    NotFound,
    /// HTTP 503 — storage layer error.
    Storage(String),
}

impl IntoResponse for GroupHandlerError {
    fn into_response(self) -> Response {
        match self {
            GroupHandlerError::BadRequest(msg) => (
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    error_code: error_codes::BAD_REQUEST.to_string(),
                    message: msg,
                    request_id: Uuid::new_v4(),
                }),
            )
                .into_response(),
            GroupHandlerError::NotAMember => (
                StatusCode::FORBIDDEN,
                Json(ApiError {
                    error_code: error_codes::NOT_A_MEMBER.to_string(),
                    message: "You are not a member of this group conversation.".to_string(),
                    request_id: Uuid::new_v4(),
                }),
            )
                .into_response(),
            GroupHandlerError::NotFound => (
                StatusCode::NOT_FOUND,
                Json(ApiError {
                    error_code: error_codes::NOT_FOUND.to_string(),
                    message: "Group conversation not found.".to_string(),
                    request_id: Uuid::new_v4(),
                }),
            )
                .into_response(),
            GroupHandlerError::Storage(msg) => {
                tracing::error!("Group storage error: {msg}");
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

impl From<GroupError> for GroupHandlerError {
    fn from(err: GroupError) -> Self {
        match err {
            GroupError::NotFound => GroupHandlerError::NotFound,
            GroupError::NotMember => GroupHandlerError::NotAMember,
            GroupError::Database(e) => GroupHandlerError::Storage(e.to_string()),
        }
    }
}

impl From<MessagingError> for GroupHandlerError {
    fn from(err: MessagingError) -> Self {
        match err {
            MessagingError::ConversationNotFound => {
                GroupHandlerError::Storage("Conversation not found.".to_string())
            }
            MessagingError::NotParticipant => GroupHandlerError::NotAMember,
            MessagingError::Database(e) => GroupHandlerError::Storage(e.to_string()),
            MessagingError::Serialization(e) => GroupHandlerError::Storage(e.to_string()),
        }
    }
}

// ── POST /groups ──────────────────────────────────────────────────────────────

/// Request body for `POST /groups`.
///
/// The creator's identity is derived from the auth token. The `members` list
/// contains the **additional** member (UserID, DeviceID) pairs, so total
/// membership = 1 (creator) + members.len().
#[derive(Debug, Deserialize)]
pub struct CreateGroupRequest {
    /// Additional member (UserID, DeviceID) pairs to include in the new group.
    ///
    /// Must contain between 2 and 999 entries (inclusive).
    pub members: Vec<MemberRef>,
}

/// A (UserID, DeviceID) pair used in several group requests.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct MemberRef {
    pub user_id: String,
    pub device_id: Uuid,
}

/// Response body for `POST /groups`.
#[derive(Debug, Serialize)]
pub struct CreateGroupResponse {
    /// The newly assigned group conversation UUID.
    pub conversation_id: Uuid,
    /// Final member list (creator + all supplied members).
    pub members: Vec<MemberRef>,
}

/// Create a new group conversation.
///
/// # Steps
/// 1. Validate that `members` contains 2–999 entries; return HTTP 400 otherwise.
/// 2. Build the full initial member list (creator + supplied members).
/// 3. Call `GroupRepository::create_group(tenant_id, ...)` within a transaction.
/// 4. Return HTTP 201 with `{ conversation_id, members }`.
///
/// # Requirements: 7.1, 7.2
pub async fn create_group(
    State(state): State<GroupState>,
    auth_user: AuthenticatedUser,
    Json(body): Json<CreateGroupRequest>,
) -> Result<Response, GroupHandlerError> {
    let tenant_id = auth_user.tenant_id;
    let creator_user_id = auth_user.user_id.clone();
    let creator_device_id = auth_user.device_id.unwrap_or(DeviceId(Uuid::nil()));

    // Step 1: Validate member count — must be 2–999 additional members.
    let extra_count = body.members.len();
    if extra_count < 2 || extra_count > 999 {
        return Err(GroupHandlerError::BadRequest(format!(
            "Group requires 2–999 additional members; got {extra_count}."
        )));
    }

    // Step 2: Build the full initial member list (creator + additional members).
    let mut initial_members: Vec<ConversationMember> = Vec::with_capacity(extra_count + 1);

    // Insert the creator first.
    initial_members.push(ConversationMember {
        user_id: creator_user_id,
        device_id: creator_device_id,
    });

    for member_ref in &body.members {
        initial_members.push(ConversationMember {
            user_id: UserId(member_ref.user_id.clone()),
            device_id: DeviceId(member_ref.device_id),
        });
    }

    // Step 3: Persist the group conversation and member rows atomically.
    let (conversation_id, stored_members) = state
        .group_repo
        .create_group(tenant_id, initial_members)
        .await
        .map_err(GroupHandlerError::from)?;

    // Step 4: Build and return the response.
    let resp_members: Vec<MemberRef> = stored_members
        .into_iter()
        .map(|m| MemberRef {
            user_id: m.user_id.0,
            device_id: m.device_id.0,
        })
        .collect();

    Ok((
        StatusCode::CREATED,
        Json(CreateGroupResponse {
            conversation_id: conversation_id.0,
            members: resp_members,
        }),
    )
        .into_response())
}

// ── POST /groups/{conversationID}/messages ────────────────────────────────────

/// Request body for `POST /groups/{conversationID}/messages`.
///
/// Carries the encrypted ciphertext and Sender Key ratchet header.
/// No recipient fields — the server fans out to all current members.
#[derive(Debug, Deserialize)]
pub struct SendGroupMessageRequest {
    /// The message envelope to store and fan out.
    pub envelope: NewMessageEnvelope,
}

/// Response body for a successfully stored group message.
#[derive(Debug, Serialize)]
pub struct SendGroupMessageResponse {
    /// Server-assigned sequence number for the stored envelope.
    pub seq: u64,
    /// Server receipt timestamp in Unix epoch milliseconds.
    pub server_ts: u64,
}

/// Store and fan-out a message to all current group members.
///
/// # Steps
/// 1. Verify the requester is a current group member; return HTTP 403 otherwise.
/// 2. Store the `MessageEnvelope` (no recipient fields) via
///    `MessagingRepository::store_envelope`.
/// 3. Fetch all current member device sessions via `GroupRepository::get_members`.
/// 4. Fan out `RtEvent::Message` to each member device via
///    `WebTransportManager::deliver` (fire-and-forget).
///
/// # Requirements: 8.1, 8.2, 8.3, 8.5
pub async fn send_group_message(
    State(state): State<GroupState>,
    auth_user: AuthenticatedUser,
    Path(conversation_id): Path<Uuid>,
    Json(body): Json<SendGroupMessageRequest>,
) -> Result<(StatusCode, Json<SendGroupMessageResponse>), GroupHandlerError> {
    let tenant_id = auth_user.tenant_id;
    let sender_user_id = auth_user.user_id.clone();
    let sender_device_id = auth_user.device_id.unwrap_or(DeviceId(Uuid::nil()));
    let conv_id = ConversationId(conversation_id);

    // Step 1: Verify the requester is a current member.
    let is_member = state
        .group_repo
        .is_member(tenant_id, conv_id, sender_user_id.clone())
        .await
        .map_err(GroupHandlerError::from)?;

    if !is_member {
        return Err(GroupHandlerError::NotAMember);
    }

    // Step 2: Store the envelope. Group messages have no recipient fields.
    let new_envelope = NewMessageEnvelope {
        conversation_id: conv_id,
        sender_user_id: sender_user_id.clone(),
        sender_device_id,
        recipient_user_id: None,
        recipient_device_id: None,
        ciphertext: body.envelope.ciphertext,
        protocol_header: body.envelope.protocol_header,
        attachment_id: body.envelope.attachment_id,
    };

    let stored = state
        .messaging_repo
        .store_envelope(tenant_id, new_envelope)
        .await
        .map_err(GroupHandlerError::from)?;

    // Step 3: Fetch all current group members.
    let members = state
        .group_repo
        .get_members(tenant_id, conv_id)
        .await
        .map_err(GroupHandlerError::from)?;

    // Step 4: Fan out to every member device (fire-and-forget).
    let event = RtEvent::Message(stored.clone());
    for member in members {
        let _ = state
            .wt_manager
            .deliver(tenant_id, member.device_id, event.clone())
            .await;
    }

    Ok((
        StatusCode::CREATED,
        Json(SendGroupMessageResponse {
            seq: stored.seq,
            server_ts: stored.server_ts,
        }),
    ))
}

// ── POST /groups/{conversationID}/members ─────────────────────────────────────

/// Request body for `POST /groups/{conversationID}/members`.
#[derive(Debug, Deserialize)]
pub struct AddGroupMemberRequest {
    /// The user ID of the new member.
    pub user_id: String,
    /// The device ID of the new member.
    pub device_id: Uuid,
}

/// Add a new member to a group conversation.
///
/// # Steps
/// 1. Call `GroupRepository::add_member(tenant_id, ...)`.
/// 2. Fetch all current member list (post-add) via `GroupRepository::get_members`.
/// 3. Broadcast `RtEvent::MemberAdded` to all member device sessions via
///    `WebTransportManager::deliver` (fire-and-forget; target is within 1 s).
/// 4. Return HTTP 201.
///
/// # Requirements: 7.2, 7.5
pub async fn add_group_member(
    State(state): State<GroupState>,
    auth_user: AuthenticatedUser,
    Path(conversation_id): Path<Uuid>,
    Json(body): Json<AddGroupMemberRequest>,
) -> Result<StatusCode, GroupHandlerError> {
    let tenant_id = auth_user.tenant_id;
    let conv_id = ConversationId(conversation_id);
    let new_user_id = UserId(body.user_id);
    let new_device_id = DeviceId(body.device_id);

    // Step 1: Persist the new member row.
    state
        .group_repo
        .add_member(tenant_id, conv_id, new_user_id.clone(), new_device_id)
        .await
        .map_err(GroupHandlerError::from)?;

    // Step 2: Fetch the updated full member list (including the new member).
    let members = state
        .group_repo
        .get_members(tenant_id, conv_id)
        .await
        .map_err(GroupHandlerError::from)?;

    // Step 3: Broadcast MemberAdded to all member sessions.
    let event = RtEvent::MemberAdded {
        conversation_id: conv_id,
        user_id: new_user_id.clone(),
        devices: vec![new_device_id],
    };
    for member in members {
        let _ = state
            .wt_manager
            .deliver(tenant_id, member.device_id, event.clone())
            .await;
    }

    Ok(StatusCode::CREATED)
}

// ── DELETE /groups/{conversationID}/members/{userID} ─────────────────────────

/// Remove a member from a group conversation.
///
/// # Steps
/// 1. Call `GroupRepository::remove_member(tenant_id, ...)`.
/// 2. Fetch the remaining member list via `GroupRepository::get_members`.
/// 3. Broadcast `RtEvent::MemberRemoved` to all remaining member sessions.
/// 4. Return HTTP 204.
///
/// Subsequent message submissions from the removed user return HTTP 403
/// because `is_member` in the send handler will return `false`.
///
/// # Requirements: 7.2, 7.6
pub async fn remove_group_member(
    State(state): State<GroupState>,
    auth_user: AuthenticatedUser,
    Path((conversation_id, user_id)): Path<(Uuid, String)>,
) -> Result<StatusCode, GroupHandlerError> {
    let tenant_id = auth_user.tenant_id;
    let conv_id = ConversationId(conversation_id);
    let removed_user_id = UserId(user_id);

    // Step 1: Remove all device rows for this user from the group.
    state
        .group_repo
        .remove_member(tenant_id, conv_id, removed_user_id.clone())
        .await
        .map_err(GroupHandlerError::from)?;

    // Step 2: Fetch the remaining member list (after removal).
    let remaining_members = state
        .group_repo
        .get_members(tenant_id, conv_id)
        .await
        .map_err(GroupHandlerError::from)?;

    // Step 3: Broadcast MemberRemoved to all remaining sessions.
    let event = RtEvent::MemberRemoved {
        conversation_id: conv_id,
        user_id: removed_user_id,
    };
    for member in remaining_members {
        let _ = state
            .wt_manager
            .deliver(tenant_id, member.device_id, event.clone())
            .await;
    }

    Ok(StatusCode::NO_CONTENT)
}

// ── POST /groups/{conversationID}/sender-key-distribution ────────────────────

/// A single recipient entry for a SenderKey distribution message.
#[derive(Debug, Deserialize)]
pub struct SkdmRecipient {
    pub user_id: String,
    pub device_id: Uuid,
    /// Base64-encoded encrypted SKDM for this recipient.
    pub encrypted_skdm: Vec<u8>,
}

/// Request body for `POST /groups/{conversationID}/sender-key-distribution`.
#[derive(Debug, Deserialize)]
pub struct DistributeSenderKeyRequest {
    /// One encrypted copy of the distribution message per recipient device.
    pub recipients: Vec<SkdmRecipient>,
}

/// Distribute SenderKey material to all group member devices.
///
/// # Steps
/// 1. Verify the sender is a current group member; return HTTP 403 otherwise.
/// 2. Call `GroupRepository::store_skdm(tenant_id, ...)` to persist one row
///    per recipient.
/// 3. Deliver `RtEvent::SenderKeyDistribution` to each recipient device session
///    via `WebTransportManager::deliver` (fire-and-forget).
/// 4. Return HTTP 201.
///
/// # Requirements: 7.3, 7.4, 7.7
pub async fn distribute_sender_key(
    State(state): State<GroupState>,
    auth_user: AuthenticatedUser,
    Path(conversation_id): Path<Uuid>,
    Json(body): Json<DistributeSenderKeyRequest>,
) -> Result<StatusCode, GroupHandlerError> {
    let tenant_id = auth_user.tenant_id;
    let sender_user_id = auth_user.user_id.clone();
    let sender_device_id = auth_user.device_id.unwrap_or(DeviceId(Uuid::nil()));
    let conv_id = ConversationId(conversation_id);

    // Step 1: Verify the sender is a current member.
    let is_member = state
        .group_repo
        .is_member(tenant_id, conv_id, sender_user_id.clone())
        .await
        .map_err(GroupHandlerError::from)?;

    if !is_member {
        return Err(GroupHandlerError::NotAMember);
    }

    // Step 2: Persist one SKDM row per recipient.
    let recipient_tuples: Vec<(UserId, DeviceId, Vec<u8>)> = body
        .recipients
        .iter()
        .map(|r| {
            (
                UserId(r.user_id.clone()),
                DeviceId(r.device_id),
                r.encrypted_skdm.clone(),
            )
        })
        .collect();

    state
        .group_repo
        .store_skdm(
            tenant_id,
            conv_id,
            sender_user_id.clone(),
            sender_device_id,
            recipient_tuples,
        )
        .await
        .map_err(GroupHandlerError::from)?;

    // Step 3: Deliver the SKDM to each recipient device session.
    for recipient in &body.recipients {
        let event = RtEvent::SenderKeyDistribution {
            conversation_id: conv_id,
            sender_user_id: sender_user_id.clone(),
            encrypted_skdm: Bytes::from(recipient.encrypted_skdm.clone()),
        };
        let _ = state
            .wt_manager
            .deliver(tenant_id, DeviceId(recipient.device_id), event)
            .await;
    }

    Ok(StatusCode::CREATED)
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_group_response_serialises() {
        let resp = CreateGroupResponse {
            conversation_id: Uuid::new_v4(),
            members: vec![MemberRef {
                user_id: "user-1".to_string(),
                device_id: Uuid::new_v4(),
            }],
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("conversation_id"));
        assert!(json.contains("members"));
        assert!(json.contains("user-1"));
    }

    #[test]
    fn send_group_message_response_serialises() {
        let resp = SendGroupMessageResponse {
            seq: 7,
            server_ts: 1_700_000_000_000,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"seq\":7"));
        assert!(json.contains("server_ts"));
    }

    #[test]
    fn group_handler_error_bad_request_status() {
        let err = GroupHandlerError::BadRequest("too few members".to_string());
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn group_handler_error_not_a_member_status() {
        let err = GroupHandlerError::NotAMember;
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[test]
    fn group_handler_error_not_found_status() {
        let err = GroupHandlerError::NotFound;
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn group_handler_error_storage_status() {
        let err = GroupHandlerError::Storage("db error".to_string());
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn group_error_not_found_converts_to_not_found() {
        let err = GroupHandlerError::from(GroupError::NotFound);
        assert!(matches!(err, GroupHandlerError::NotFound));
    }

    #[test]
    fn group_error_not_member_converts_to_not_a_member() {
        let err = GroupHandlerError::from(GroupError::NotMember);
        assert!(matches!(err, GroupHandlerError::NotAMember));
    }

    #[test]
    fn messaging_error_not_participant_converts_to_not_a_member() {
        let err = GroupHandlerError::from(MessagingError::NotParticipant);
        assert!(matches!(err, GroupHandlerError::NotAMember));
    }

    #[test]
    fn messaging_error_conversation_not_found_converts_to_storage() {
        let err = GroupHandlerError::from(MessagingError::ConversationNotFound);
        assert!(matches!(err, GroupHandlerError::Storage(_)));
    }
}
