// src/transport/api-types.ts
// All Uint8Array fields are represented as base64url strings in JSON.
// Conversion is handled in RestClient.

export interface KeyBundleRequest {
  identity_key: string;
  identity_key_ed: string;
  signed_prekey_id: number;
  signed_prekey: string;
  signed_prekey_sig: string;
  one_time_prekeys: OtpkEntry[];
}

export interface OtpkEntry {
  id: number;
  key: string; // base64url
}

export interface RegisterDeviceResponse {
  device_id: string;
}

export interface KeyBundleResponse {
  device_id: string;
  identity_key: string;      // base64url X25519 public
  identity_key_ed: string;   // base64url Ed25519 public
  signed_prekey_id: number;
  signed_prekey: string;     // base64url X25519 public
  signed_prekey_sig: string; // base64url Ed25519 signature
  one_time_prekey: OtpkEntry | null;
}

export interface CreateConversationRequest {
  recipient_user_id: string;
  recipient_device_id: string;
  envelope: {
    conversation_id: string;
    ciphertext: string; // base64url
    protocol_header: {
      type: 'x3dh_init';
      ek: string;       // ephemeral key, base64url
      spk_id: number;
      otpk_id?: number;
    };
  };
}

export interface CreateConversationResponse {
  conversation_id: string;
}

export interface MessageEnvelopeRequest {
  envelope: {
    conversation_id: string;
    ciphertext: string;
    protocol_header: {
      type: 'double_ratchet' | 'sender_key';
      dh?: string;
      n?: number;
      pn?: number;
      chain_id?: number;
      iteration?: number;
    };
    attachment_id?: string;
  };
}

export interface SendMessageResponse {
  seq: number;
  server_ts: number; // Unix epoch ms
}

export interface GetMessagesResponse {
  messages: ServerMessage[];
  has_more: boolean;
}

export interface ServerMessage {
  id: string;
  conversation_id: string;
  sender_user_id: string;
  sender_device_id: string | null;
  seq: number;
  server_ts: number;
  envelope: {
    ciphertext: string;
    protocol_header: {
      type: string;
      dh?: string;
      n?: number;
      pn?: number;
      chain_id?: number;
      iteration?: number;
      ek?: string;
      spk_id?: number;
      otpk_id?: number;
    };
    attachment_id?: string;
  } | null;
  type: 'text' | 'attachment' | 'member_added' | 'member_removed';
  attachment_url?: string | null;
  attachment_name?: string | null;
}

export interface CreateGroupRequest {
  members: string[];
}

export interface CreateGroupResponse {
  conversation_id: string;
  members: GroupMemberInfo[];
}

export interface GroupMemberInfo {
  user_id: string;
  devices: string[];
}

export interface SkdmRecipient {
  user_id: string;
  device_id: string;
  encrypted_skdm: string; // base64url
}

export interface DistributeSkdmRequest {
  recipients: SkdmRecipient[];
}

export interface UploadResponse {
  attachment_id: string;
  url: string;
}

export interface ReplenishResponse {
  uploaded: number;
}

export interface SignedPreKeyUpdate {
  signed_prekey_id: number;
  signed_prekey: string;
  signed_prekey_sig: string;
}

export interface ApiErrorBody {
  error_code?: string;
  message?: string;
  request_id?: string;
}
