//! Shared domain types for the rust-e2e-chat-api workspace.
//!
//! All types implement `Serialize` / `Deserialize` via serde so they can be
//! used across HTTP/3 REST responses, WebTransport stream payloads, and
//! PostgreSQL JSONB columns.

use bytes::Bytes;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── Bytes serde helpers ───────────────────────────────────────────────────────

/// Serde (de)serialization helpers for `bytes::Bytes`.
///
/// `bytes::Bytes` does not implement `serde::Serialize`/`Deserialize`
/// natively.  We serialize it as a byte sequence using `serde_bytes` so that
/// binary formats (MessagePack, bincode) emit raw bytes and JSON emits
/// base64-encoded data.
mod bytes_serde {
    use bytes::Bytes;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S: Serializer>(bytes: &Bytes, ser: S) -> Result<S::Ok, S::Error> {
        serde_bytes::serialize(bytes.as_ref(), ser)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<Bytes, D::Error> {
        let vec: Vec<u8> = serde_bytes::deserialize(de)?;
        Ok(Bytes::from(vec))
    }
}

// ── Identity / Key types ──────────────────────────────────────────────────────

/// Newtype wrapper for a tenant identifier (server-assigned UUID).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TenantId(pub Uuid);

/// Tenant configuration loaded from the `tenants` table.
#[derive(Debug, Clone)]
pub struct TenantConfig {
    pub tenant_id:   TenantId,
    pub name:        String,
    /// JWT `iss` claim value; used to locate JWKS endpoint.
    pub oidc_issuer: String,
    pub active:      bool,
}

/// Newtype wrapper for a user identifier (OIDC `sub` claim).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UserId(pub String);

/// Newtype wrapper for a device identifier (server-assigned UUID).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeviceId(pub Uuid);

/// Newtype wrapper for a conversation identifier (server-assigned UUID).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ConversationId(pub Uuid);

/// Raw Curve25519 public key (32 bytes).
///
/// We use `serde_bytes` so the key is serialised as a compact byte sequence
/// (base64 in JSON, raw bytes in binary formats like MessagePack/bincode)
/// rather than as an array of 32 integers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Curve25519PublicKey(
    #[serde(with = "serde_bytes")] pub [u8; 32],
);

/// Raw Ed25519 signature (64 bytes).
///
/// Same serde strategy as [`Curve25519PublicKey`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ed25519Signature(
    #[serde(with = "serde_bytes")] pub [u8; 64],
);

// ── Key Bundle ─────────────────────────────────────────────────────────────────

/// A device's full key bundle submitted during device registration.
///
/// Contains the identity key, the current signed pre-key, and up to 100
/// one-time pre-keys.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyBundle {
    pub identity_key: Curve25519PublicKey,
    pub signed_prekey_id: u64,
    pub signed_prekey: Curve25519PublicKey,
    pub signed_prekey_sig: Ed25519Signature,
    /// 0–100 one-time pre-keys.
    pub one_time_prekeys: Vec<OneTimePreKey>,
}

/// A single one-time pre-key entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OneTimePreKey {
    pub key_id: u64,
    pub public_key: Curve25519PublicKey,
}

/// KDS response for `GET /users/{userID}/key-bundle`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyBundleResponse {
    pub device_id: DeviceId,
    pub identity_key: Curve25519PublicKey,
    pub signed_prekey_id: u64,
    pub signed_prekey: Curve25519PublicKey,
    pub signed_prekey_sig: Ed25519Signature,
    /// `None` when the OTPK pool is depleted.
    pub one_time_prekey: Option<OneTimePreKey>,
    /// Present when `one_time_prekey` is `None` to signal pool exhaustion.
    pub otpk_warning: Option<OtpkWarning>,
}

/// Warning emitted when a device's one-time pre-key pool is depleted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OtpkWarning {
    Depleted,
}

// ── Messages ──────────────────────────────────────────────────────────────────

/// A fully-formed message envelope stored in and delivered from the server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageEnvelope {
    pub conversation_id: ConversationId,
    pub seq: u64,
    pub sender_user_id: UserId,
    pub sender_device_id: DeviceId,
    pub recipient_user_id: Option<UserId>,
    pub recipient_device_id: Option<DeviceId>,
    #[serde(with = "bytes_serde")]
    pub ciphertext: Bytes,
    pub protocol_header: ProtocolHeader,
    pub server_ts: u64,
    /// Optional attachment linked to this message.
    pub attachment_id: Option<Uuid>,
}

/// Input type for creating a new message envelope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewMessageEnvelope {
    pub conversation_id: ConversationId,
    pub sender_user_id: UserId,
    pub sender_device_id: DeviceId,
    pub recipient_user_id: Option<UserId>,
    pub recipient_device_id: Option<DeviceId>,
    #[serde(with = "bytes_serde")]
    pub ciphertext: Bytes,
    pub protocol_header: ProtocolHeader,
    /// Optional attachment linked to this message.
    pub attachment_id: Option<Uuid>,
}

/// Discriminated union describing the cryptographic framing of a message.
///
/// The `"type"` JSON field is used as the tag.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ProtocolHeader {
    /// Initial X3DH message: contains the sender's ephemeral key material and
    /// the first Double Ratchet header.
    X3dhInit {
        sender_identity_key: Curve25519PublicKey,
        ephemeral_key: Curve25519PublicKey,
        used_signed_prekey_id: u64,
        /// `None` when no OTPK was available during X3DH.
        used_otpk_id: Option<u64>,
        dr_header: DoubleRatchetHeader,
    },
    /// Subsequent Double Ratchet messages after the initial X3DH handshake.
    DoubleRatchet(DoubleRatchetHeader),
    /// Group Sender Key message.
    SenderKey(SenderKeyHeader),
}

/// Double Ratchet per-message header.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoubleRatchetHeader {
    pub ratchet_key: Curve25519PublicKey,
    /// Number of messages in the previous sending chain.
    pub prev_chain_n: u32,
    /// Message number in the current sending chain.
    pub msg_n: u32,
}

/// Sender Key ratchet header for group messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SenderKeyHeader {
    pub sender_id: UserId,
    pub chain_id: u32,
    pub iteration: u32,
}

// ── Conversations ─────────────────────────────────────────────────────────────

/// Discriminates between 1:1 and group conversations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConversationKind {
    Direct,
    Group,
}

/// A single participant in a conversation, identified by both user and device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMember {
    pub user_id: UserId,
    pub device_id: DeviceId,
}

// ── Real-Time Events ──────────────────────────────────────────────────────────

/// Events delivered over a WebTransport session.
///
/// The `"event"` JSON field is used as the tag.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event")]
pub enum RtEvent {
    /// A new encrypted message ready for delivery.
    Message(MessageEnvelope),
    /// The device's OTPK pool has dropped below the low-watermark threshold.
    LowOtpk { device_id: DeviceId, count: u32 },
    /// A new member has been added to a group conversation.
    MemberAdded {
        conversation_id: ConversationId,
        user_id: UserId,
        devices: Vec<DeviceId>,
    },
    /// A member has been removed from a group conversation.
    MemberRemoved {
        conversation_id: ConversationId,
        user_id: UserId,
    },
    /// A SenderKey distribution message is ready for this device.
    SenderKeyDistribution {
        conversation_id: ConversationId,
        sender_user_id: UserId,
        /// Encrypted SenderKey distribution message opaque to the server.
        #[serde(with = "bytes_serde")]
        encrypted_skdm: Bytes,
    },
}

/// Client acknowledgement datagram payload.
///
/// Sent by the client over a WebTransport datagram to confirm delivery of a
/// specific message sequence number.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AckDatagram {
    pub conversation_id: ConversationId,
    pub seq: u64,
}

// ── Error types ───────────────────────────────────────────────────────────────

/// Structured error response body returned by all API endpoints on failure.
///
/// Maps to the `ApiError` JSON schema defined in the design document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiError {
    /// Machine-readable error code; one of the constants in [`error_codes`].
    pub error_code: String,
    /// Human-readable description of the error.
    pub message: String,
    /// UUID threaded through all internal components for correlation in logs.
    pub request_id: Uuid,
}

/// All `error_code` string constants used in [`ApiError`] responses.
pub mod error_codes {
    /// Malformed JSON or missing required fields.
    pub const BAD_REQUEST: &str = "bad_request";
    /// No `Authorization` header was present on the request.
    pub const MISSING_TOKEN: &str = "missing_token";
    /// JWT signature verification failed or the token is expired.
    pub const INVALID_TOKEN: &str = "invalid_token";
    /// The authenticated user is not allowed to access the requested resource.
    pub const FORBIDDEN: &str = "forbidden";
    /// The authenticated user is not a member of the targeted conversation.
    pub const NOT_A_MEMBER: &str = "not_a_member";
    /// The requested resource does not exist.
    pub const NOT_FOUND: &str = "not_found";
    /// The user already has 5 registered devices.
    pub const DEVICE_LIMIT_REACHED: &str = "device_limit_reached";
    /// A 1:1 conversation between these participants already exists.
    pub const CONVERSATION_EXISTS: &str = "conversation_exists";
    /// The SignedPreKey's Ed25519 signature did not verify against the IdentityKey.
    pub const INVALID_SIGNED_PREKEY_SIGNATURE: &str = "invalid_signed_prekey_signature";
    /// Other key-bundle validation failure.
    pub const INVALID_KEY_BUNDLE: &str = "invalid_key_bundle";
    /// PostgreSQL or Redis operation failed.
    pub const STORAGE_UNAVAILABLE: &str = "storage_unavailable";
    /// Unhandled internal server error (panic fallback).
    pub const INTERNAL_ERROR: &str = "internal_error";
    /// The requested tenant is not registered in the platform.
    pub const UNKNOWN_TENANT: &str = "unknown_tenant";
    /// The requested tenant exists but has been deactivated.
    pub const TENANT_INACTIVE: &str = "tenant_inactive";
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Serialization round-trip tests ────────────────────────────────────────

    #[test]
    fn user_id_roundtrip() {
        let original = UserId("user-abc-123".to_string());
        let json = serde_json::to_string(&original).unwrap();
        let decoded: UserId = serde_json::from_str(&json).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn device_id_roundtrip() {
        let id = Uuid::new_v4();
        let original = DeviceId(id);
        let json = serde_json::to_string(&original).unwrap();
        let decoded: DeviceId = serde_json::from_str(&json).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn conversation_id_roundtrip() {
        let id = Uuid::new_v4();
        let original = ConversationId(id);
        let json = serde_json::to_string(&original).unwrap();
        let decoded: ConversationId = serde_json::from_str(&json).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn curve25519_public_key_roundtrip() {
        let key_bytes = [42u8; 32];
        let original = Curve25519PublicKey(key_bytes);
        let json = serde_json::to_string(&original).unwrap();
        let decoded: Curve25519PublicKey = serde_json::from_str(&json).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn ed25519_signature_roundtrip() {
        let sig_bytes = [7u8; 64];
        let original = Ed25519Signature(sig_bytes);
        let json = serde_json::to_string(&original).unwrap();
        let decoded: Ed25519Signature = serde_json::from_str(&json).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn key_bundle_roundtrip() {
        let bundle = KeyBundle {
            identity_key: Curve25519PublicKey([1u8; 32]),
            signed_prekey_id: 1,
            signed_prekey: Curve25519PublicKey([2u8; 32]),
            signed_prekey_sig: Ed25519Signature([3u8; 64]),
            one_time_prekeys: vec![
                OneTimePreKey {
                    key_id: 10,
                    public_key: Curve25519PublicKey([4u8; 32]),
                },
            ],
        };
        let json = serde_json::to_string(&bundle).unwrap();
        let decoded: KeyBundle = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.signed_prekey_id, bundle.signed_prekey_id);
        assert_eq!(decoded.one_time_prekeys.len(), 1);
        assert_eq!(decoded.one_time_prekeys[0].key_id, 10);
    }

    #[test]
    fn otpk_warning_depleted_roundtrip() {
        let original = OtpkWarning::Depleted;
        let json = serde_json::to_string(&original).unwrap();
        let decoded: OtpkWarning = serde_json::from_str(&json).unwrap();
        assert_eq!(original, decoded);
    }

    #[test]
    fn protocol_header_x3dh_init_roundtrip() {
        let header = ProtocolHeader::X3dhInit {
            sender_identity_key: Curve25519PublicKey([1u8; 32]),
            ephemeral_key: Curve25519PublicKey([2u8; 32]),
            used_signed_prekey_id: 99,
            used_otpk_id: Some(5),
            dr_header: DoubleRatchetHeader {
                ratchet_key: Curve25519PublicKey([3u8; 32]),
                prev_chain_n: 0,
                msg_n: 0,
            },
        };
        let json = serde_json::to_string(&header).unwrap();
        assert!(json.contains(r#""type":"X3dhInit""#));
        let decoded: ProtocolHeader = serde_json::from_str(&json).unwrap();
        if let ProtocolHeader::X3dhInit { used_signed_prekey_id, used_otpk_id, .. } = decoded {
            assert_eq!(used_signed_prekey_id, 99);
            assert_eq!(used_otpk_id, Some(5));
        } else {
            panic!("Expected X3dhInit variant");
        }
    }

    #[test]
    fn protocol_header_double_ratchet_roundtrip() {
        let header = ProtocolHeader::DoubleRatchet(DoubleRatchetHeader {
            ratchet_key: Curve25519PublicKey([5u8; 32]),
            prev_chain_n: 3,
            msg_n: 7,
        });
        let json = serde_json::to_string(&header).unwrap();
        assert!(json.contains(r#""type":"DoubleRatchet""#));
        let decoded: ProtocolHeader = serde_json::from_str(&json).unwrap();
        if let ProtocolHeader::DoubleRatchet(dr) = decoded {
            assert_eq!(dr.prev_chain_n, 3);
            assert_eq!(dr.msg_n, 7);
        } else {
            panic!("Expected DoubleRatchet variant");
        }
    }

    #[test]
    fn protocol_header_sender_key_roundtrip() {
        let header = ProtocolHeader::SenderKey(SenderKeyHeader {
            sender_id: UserId("user-xyz".to_string()),
            chain_id: 2,
            iteration: 15,
        });
        let json = serde_json::to_string(&header).unwrap();
        assert!(json.contains(r#""type":"SenderKey""#));
        let decoded: ProtocolHeader = serde_json::from_str(&json).unwrap();
        if let ProtocolHeader::SenderKey(sk) = decoded {
            assert_eq!(sk.chain_id, 2);
            assert_eq!(sk.iteration, 15);
        } else {
            panic!("Expected SenderKey variant");
        }
    }

    #[test]
    fn message_envelope_roundtrip() {
        let envelope = MessageEnvelope {
            conversation_id: ConversationId(Uuid::new_v4()),
            seq: 42,
            sender_user_id: UserId("alice".to_string()),
            sender_device_id: DeviceId(Uuid::new_v4()),
            recipient_user_id: Some(UserId("bob".to_string())),
            recipient_device_id: Some(DeviceId(Uuid::new_v4())),
            ciphertext: Bytes::from_static(b"encrypted-payload"),
            protocol_header: ProtocolHeader::DoubleRatchet(DoubleRatchetHeader {
                ratchet_key: Curve25519PublicKey([9u8; 32]),
                prev_chain_n: 1,
                msg_n: 5,
            }),
            server_ts: 1_700_000_000_000,
            attachment_id: None,
        };
        let json = serde_json::to_string(&envelope).unwrap();
        let decoded: MessageEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.seq, 42);
        assert_eq!(decoded.server_ts, 1_700_000_000_000);
    }

    #[test]
    fn conversation_kind_variants_roundtrip() {
        for kind in [ConversationKind::Direct, ConversationKind::Group] {
            let json = serde_json::to_string(&kind).unwrap();
            let decoded: ConversationKind = serde_json::from_str(&json).unwrap();
            assert_eq!(kind, decoded);
        }
    }

    #[test]
    fn rt_event_low_otpk_roundtrip() {
        let event = RtEvent::LowOtpk {
            device_id: DeviceId(Uuid::new_v4()),
            count: 3,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""event":"LowOtpk""#));
        let decoded: RtEvent = serde_json::from_str(&json).unwrap();
        if let RtEvent::LowOtpk { count, .. } = decoded {
            assert_eq!(count, 3);
        } else {
            panic!("Expected LowOtpk variant");
        }
    }

    #[test]
    fn rt_event_member_added_roundtrip() {
        let event = RtEvent::MemberAdded {
            conversation_id: ConversationId(Uuid::new_v4()),
            user_id: UserId("carol".to_string()),
            devices: vec![DeviceId(Uuid::new_v4())],
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains(r#""event":"MemberAdded""#));
        let decoded: RtEvent = serde_json::from_str(&json).unwrap();
        if let RtEvent::MemberAdded { user_id, devices, .. } = decoded {
            assert_eq!(user_id.0, "carol");
            assert_eq!(devices.len(), 1);
        } else {
            panic!("Expected MemberAdded variant");
        }
    }

    #[test]
    fn ack_datagram_roundtrip() {
        let datagram = AckDatagram {
            conversation_id: ConversationId(Uuid::new_v4()),
            seq: 100,
        };
        let json = serde_json::to_string(&datagram).unwrap();
        let decoded: AckDatagram = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.seq, 100);
    }

    #[test]
    fn api_error_roundtrip() {
        let error = ApiError {
            error_code: error_codes::INVALID_TOKEN.to_string(),
            message: "Token has expired.".to_string(),
            request_id: Uuid::new_v4(),
        };
        let json = serde_json::to_string(&error).unwrap();
        let decoded: ApiError = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.error_code, error_codes::INVALID_TOKEN);
        assert_eq!(decoded.request_id, error.request_id);
    }

    #[test]
    fn all_error_codes_are_non_empty() {
        let codes = [
            error_codes::BAD_REQUEST,
            error_codes::MISSING_TOKEN,
            error_codes::INVALID_TOKEN,
            error_codes::FORBIDDEN,
            error_codes::NOT_A_MEMBER,
            error_codes::NOT_FOUND,
            error_codes::DEVICE_LIMIT_REACHED,
            error_codes::CONVERSATION_EXISTS,
            error_codes::INVALID_SIGNED_PREKEY_SIGNATURE,
            error_codes::INVALID_KEY_BUNDLE,
            error_codes::STORAGE_UNAVAILABLE,
            error_codes::INTERNAL_ERROR,
        ];
        for code in codes {
            assert!(!code.is_empty(), "error_code must not be empty: {code}");
        }
    }
}
