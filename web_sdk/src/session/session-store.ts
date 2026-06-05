// src/session/session-store.ts
// All Uint8Array values are serialized as base64url strings in persistent storage.

export interface OtpkRecord {
  id: number;
  privateKey: string;  // base64url
  publicKey: string;   // base64url
}

export interface DeviceRecord {
  userId: string;
  deviceId: string;
  identityKeyDhPriv: string;   // base64url X25519 private
  identityKeyDhPub: string;    // base64url X25519 public
  identityKeyEdPriv: string;   // base64url Ed25519 private
  identityKeyEdPub: string;    // base64url Ed25519 public
  signedPrekeyPriv: string;    // base64url X25519 private
  signedPrekeyPub: string;     // base64url X25519 public
  signedPrekeyId: number;
  signedPrekeyCreatedAt: number; // Unix epoch ms
  otpks: OtpkRecord[];
  nextOtpkId: number;
}

export interface RatchetState {
  conversationId: string;
  rootKey: string;         // base64url
  chainKeySend: string;    // base64url
  chainKeyRecv: string;    // base64url
  dhSendPub: string;       // base64url
  dhSendPriv: string;      // base64url
  dhRecvPub: string;       // base64url
  nSend: number;
  nRecv: number;
  pn: number;
  skippedMessageKeys: Record<string, string>; // `${dhPub}:${n}` → base64url key
}

export interface SenderKeyRecord {
  conversationId: string;
  userId: string;
  chainKey: string;    // base64url
  chainId: number;
  iteration: number;
  signingKeyPriv?: string; // base64url Ed25519 private (own key only)
  signingKeyPub: string;   // base64url Ed25519 public
}

export interface ConversationMeta {
  conversationId: string;
  type: 'one_to_one' | 'group';
  members: ConversationMemberRecord[];
  createdAt: number; // Unix epoch ms
  lastSeq: number;
}

export interface ConversationMemberRecord {
  userId: string;
  deviceId: string;
}

/**
 * Pluggable durable storage for private keys, ratchet states, and session metadata.
 * All implementations must be atomic within a single method call.
 */
export interface SessionStore {
  saveDevice(record: DeviceRecord): Promise<void>;
  loadDevice(userId: string, deviceId: string): Promise<DeviceRecord | null>;

  saveRatchetState(conversationId: string, state: RatchetState): Promise<void>;
  loadRatchetState(conversationId: string): Promise<RatchetState | null>;

  saveSenderKey(conversationId: string, userId: string, record: SenderKeyRecord): Promise<void>;
  loadSenderKey(conversationId: string, userId: string): Promise<SenderKeyRecord | null>;

  saveConversationMeta(meta: ConversationMeta): Promise<void>;
  loadAllConversations(): Promise<ConversationMeta[]>;

  clear(): Promise<void>;
}
