# Design Document: web-e2e-chat-sdk

## Overview

`@rust-e2e-chat/sdk` is a framework-agnostic TypeScript package providing a complete client for the `rust-e2e-chat-api` platform. It is structured as a strict layered architecture with no circular dependencies:

1. **Crypto layer** — Signal Protocol primitives (X3DH, Double Ratchet, Sender Keys) built entirely on `crypto.subtle` (Web Crypto API). Works identically in browser, Node.js ≥ 18, Deno, and Bun.
2. **Transport layer** — Typed `fetch`-based REST client and a `WebSocket` wrapper with auto-reconnect and ack management.
3. **Session layer** — Pluggable `SessionStore` interface with `IndexedDbSessionStore` (browser), `MemorySessionStore` (tests/server), and `NodeFileSessionStore` (Node.js persistent) implementations.
4. **Domain layer** — `ChatClient` and `Conversation` classes exposing the developer-facing event-driven API.

The SDK is **zero-knowledge at the transport boundary**: no plaintext or private key bytes ever cross the REST/WebSocket layer.

---

## Architecture

### High-Level Component Diagram

```
┌─────────────────────────────────────────────────────────────────────────┐
│  Application Code (React, Vue, Svelte, plain JS, Node.js server, etc.) │
│    ChatClient   Conversation   ChatMessage   AttachmentProgress         │
└────────────────────────────┬────────────────────────────────────────────┘
                             │  EventEmitter API (.on / .off / .emit)
┌────────────────────────────▼────────────────────────────────────────────┐
│  Domain Layer                                                           │
│    ChatClient                                                           │
│      ├── ConversationRegistry   (Map<conversationId, Conversation>)     │
│      ├── RtEventRouter          (routes WS frames → conversations)      │
│      ├── OtpkReplenisher        (auto-uploads OTPKs on low_otpk event)  │
│      └── SpkRotator             (periodic SignedPreKey rotation)        │
│    Conversation (OneToOne | Group)                                      │
└──────────┬─────────────────────────────────────┬────────────────────────┘
           │                                     │
┌──────────▼──────────┐   ┌─────────────────────▼──────────────────────┐
│  Crypto Layer        │   │  Transport Layer                           │
│  X3dhEngine          │   │  RestClient (fetch)                        │
│  RatchetEngine       │   │  WsManager (WebSocket + reconnect)         │
│  SenderKeyEngine     │   │  ConnectionManager (backoff)               │
│  KeyGenerator        │   │  AttachmentClient                          │
└──────────┬───────────┘   └─────────────────────┬──────────────────────┘
           │                                     │
           └──────────────┬──────────────────────┘
                          │
               ┌──────────▼───────────┐
               │  Session Layer       │
               │  SessionStore (iface)│
               │  IndexedDbSessionStore│
               │  MemorySessionStore  │
               │  NodeFileSessionStore│
               └──────────────────────┘
```

### Package Layout

```
packages/sdk/
  src/
    index.ts                       ← barrel export (public surface only)
    client/
      chat-client.ts               ← ChatClient class
      chat-client-config.ts        ← ChatClientConfig type
      connection-state.ts          ← ConnectionState union type
      conversation-registry.ts     ← ConversationRegistry
      rt-event-router.ts           ← RtEventRouter
      otpk-replenisher.ts          ← OtpkReplenisher
      spk-rotator.ts               ← SpkRotator
    conversation/
      conversation.ts              ← Conversation abstract base
      one-to-one-conversation.ts   ← OneToOneConversation
      group-conversation.ts        ← GroupConversation
      chat-message.ts              ← ChatMessage type
      conversation-meta.ts         ← ConversationMeta (stored)
    crypto/
      key-generator.ts             ← KeyGenerator
      x3dh-engine.ts               ← X3dhEngine
      ratchet-engine.ts            ← RatchetEngine
      sender-key-engine.ts         ← SenderKeyEngine
      crypto-utils.ts              ← shared helpers (HKDF, AES-GCM, etc.)
      crypto-types.ts              ← Curve25519KeyPair, RatchetState, etc.
    transport/
      rest-client.ts               ← RestClient (typed fetch wrapper)
      ws-manager.ts                ← WsManager (WebSocket + reconnect)
      connection-manager.ts        ← ConnectionManager (backoff)
      attachment-client.ts         ← AttachmentClient
      rt-event.ts                  ← RtEvent discriminated union
      api-types.ts                 ← all API request/response DTOs
    session/
      session-store.ts             ← SessionStore interface + types
      indexed-db-session-store.ts  ← browser default
      memory-session-store.ts      ← test / server
      node-file-session-store.ts   ← Node.js persistent
    errors/
      sdk-error.ts                 ← SdkError class + SdkErrorCode enum
    utils/
      logger.ts                    ← configurable logger (redacts keys/text)
      retry.ts                     ← exponential backoff
      typed-emitter.ts             ← TypedEventEmitter<Events>
      encoding.ts                  ← base64url encode/decode, hex helpers
  testing/
    index.ts                       ← MockChatClient export
    mock-chat-client.ts
    mock-conversation.ts
  dist/
    index.js                       ← ESM
    index.cjs                      ← CJS
    index.d.ts                     ← TypeScript declarations
    browser.min.js                 ← UMD browser bundle
  package.json
  tsconfig.json
  vitest.config.ts
  README.md
```

---

## Components and Interfaces

### 0. Public Entry Point — `ChatClient`

```typescript
class ChatClient extends TypedEventEmitter<ChatClientEvents> {
  // Factory
  static create(config: ChatClientConfig): Promise<ChatClient>;

  // Identity
  readonly userId: string;
  readonly deviceId: string;
  getPublicKeyBundle(): Promise<KeyBundle>;

  // Connection
  connect(): Promise<void>;
  disconnect(): Promise<void>;
  updateToken(newToken: string): void;

  // Conversations
  openConversation(recipientUserId: string): Promise<Conversation>;
  createGroup(memberUserIds: string[]): Promise<Conversation>;
  findConversation(conversationId: string): Conversation | undefined;

  // Lifecycle
  destroy(): Promise<void>;
}

interface ChatClientEvents {
  connection: { state: ConnectionState };
  conversation: { conversation: Conversation };
  error: SdkError;
  storage_error: SdkError;
}

type ConnectionState = 'connecting' | 'connected' | 'reconnecting' | 'disconnected';
```

### 1. Configuration

```typescript
interface ChatClientConfig {
  baseUrl: string;                    // e.g. "http://localhost:3000/api"
  accessToken: string;                // OIDC JWT
  userId: string;                     // OIDC sub
  deviceId?: string;                  // undefined → auto-register
  sessionStore?: SessionStore;        // default: platform-detected
  autoConnect?: boolean;              // default: true
  signedPrekeyRotationDays?: number;  // default: 7
  logLevel?: 'debug' | 'info' | 'warn' | 'error' | 'silent'; // default: 'warn'
}
```

### 2. Conversation

```typescript
abstract class Conversation extends TypedEventEmitter<ConversationEvents> {
  readonly conversationId: string;
  readonly type: 'one_to_one' | 'group';
  readonly members: ConversationMember[];

  send(text: string): Promise<ChatMessage>;
  sendAttachment(
    file: File | Blob | BufferSource,
    filename: string,
    contentType: string
  ): AttachmentSend;   // { promise: Promise<ChatMessage>; progress: ReadableStream<AttachmentProgress> }
  fetchHistory(options?: { limit?: number; beforeSeq?: number }): Promise<ChatMessage[]>;
  markAsRead(): void;

  // Group only (throws if one_to_one)
  addMember(userId: string): Promise<void>;
  removeMember(userId: string): Promise<void>;
}

interface ConversationEvents {
  message: ChatMessage;
  member_added: { userId: string; devices: string[] };
  member_removed: { userId: string };
}
```

### 3. ChatMessage

```typescript
interface ChatMessage {
  readonly id: string;               // client-generated UUID
  readonly conversationId: string;
  readonly senderId: string;
  readonly senderDeviceId: string | null;
  readonly type: 'text' | 'attachment' | 'member_added' | 'member_removed';
  readonly text: string | null;      // null if attachment-only or decryptionError
  readonly attachmentId: string | null;
  readonly attachmentUrl: string | null; // authenticated download URL
  readonly attachmentName: string | null;
  readonly timestamp: Date;
  readonly seq: number;
  readonly isMine: boolean;
  readonly decryptionError: boolean;
}
```

### 4. Error Class

```typescript
enum SdkErrorCode {
  NETWORK_ERROR      = 'NETWORK_ERROR',
  AUTH_ERROR         = 'AUTH_ERROR',
  DECRYPTION_ERROR   = 'DECRYPTION_ERROR',
  KEY_EXCHANGE_ERROR = 'KEY_EXCHANGE_ERROR',
  STORAGE_ERROR      = 'STORAGE_ERROR',
  SESSION_NOT_FOUND  = 'SESSION_NOT_FOUND',
  FILE_TOO_LARGE     = 'FILE_TOO_LARGE',
  DEVICE_LIMIT_REACHED = 'DEVICE_LIMIT_REACHED',
  INVALID_SIGNATURE  = 'INVALID_SIGNATURE',
  UNKNOWN_ERROR      = 'UNKNOWN_ERROR',
}

class SdkError extends Error {
  readonly code: SdkErrorCode;
  readonly statusCode?: number;    // HTTP status if applicable
  readonly cause?: unknown;

  static fromApiResponse(status: number, body: { error_code?: string; message?: string }): SdkError;
}
```

Server `error_code` → `SdkErrorCode` mapping:

| Server `error_code` | `SdkErrorCode` |
|---|---|
| `unauthorized` | `AUTH_ERROR` |
| `forbidden` | `AUTH_ERROR` |
| `unknown_tenant` | `AUTH_ERROR` |
| `tenant_inactive` | `AUTH_ERROR` |
| `not_found` | `NETWORK_ERROR` (404) |
| `device_limit_reached` | `DEVICE_LIMIT_REACHED` |
| `invalid_signed_prekey_signature` | `INVALID_SIGNATURE` |
| `storage_unavailable` | `NETWORK_ERROR` (503) |
| `bad_request` | `NETWORK_ERROR` (400) |
| `internal_error` | `NETWORK_ERROR` (500) |

---

## Crypto Layer Design

All primitives use `crypto.subtle`. No third-party crypto library is required.

### 4.1 Key Generator (`key-generator.ts`)

```typescript
class KeyGenerator {
  // Generate full private key bundle for device registration.
  static async generateKeyBundle(otpkCount = 50): Promise<PrivateKeyBundle>;

  // Generate a batch of OTPKs for replenishment.
  static async generateOtpks(count: number): Promise<OtpkKeyPair[]>;

  // Generate a new SenderKey for group use.
  static async generateSenderKey(): Promise<SenderKeyMaterial>;

  // Generate a new SignedPreKey + signature for rotation.
  static async generateSignedPreKey(
    identityKey: CryptoKeyPair,   // Ed25519
    id: number
  ): Promise<SignedPreKeyPair>;
}
```

**Key types used:**
- IdentityKey: `X25519` key pair (DH) + matching `Ed25519` key pair (signing). Derived from the same seed.
- SignedPreKey: `X25519` key pair; signature computed with `Ed25519` private key.
- OTPKs: `X25519` key pairs; each with a numeric ID.
- SenderKey: 32-byte CSPRNG chain key + `Ed25519` signing key pair.

### 4.2 X3DH Engine (`x3dh-engine.ts`)

```typescript
class X3dhEngine {
  // Initiator: compute shared secret + X3DH header to send with first message.
  static async performX3dh(params: {
    recipientBundle: KeyBundleResponse;
    senderBundle: PrivateKeyBundle;
  }): Promise<X3dhResult>;
  // X3dhResult = { sharedSecret: Uint8Array; header: X3dhHeader }

  // Responder: derive the same shared secret from the received header.
  static async deriveSharedSecret(params: {
    header: X3dhHeader;
    recipientBundle: PrivateKeyBundle;
  }): Promise<Uint8Array>;
}
```

**X3DH computation order** (Signal spec):
```
DH1 = DH(IK_sender, SPK_recipient)
DH2 = DH(EK_sender, IK_recipient)
DH3 = DH(EK_sender, SPK_recipient)
DH4 = DH(EK_sender, OTPK_recipient)  // omitted if OTPK depleted
SK  = HKDF(DH1 || DH2 || DH3 [|| DH4], salt=0x00*32, info="X3DH")
```

### 4.3 Ratchet Engine (`ratchet-engine.ts`)

```typescript
class RatchetEngine {
  static initSender(sharedSecret: Uint8Array, recipientDhPub: Uint8Array): RatchetSession;
  static initReceiver(sharedSecret: Uint8Array, localDhKeyPair: CryptoKeyPair): RatchetSession;
  static async encrypt(session: RatchetSession, plaintext: Uint8Array): Promise<EncryptResult>;
  // EncryptResult = { ciphertext: Uint8Array; header: RatchetHeader; nextSession: RatchetSession }
  static async decrypt(session: RatchetSession, ciphertext: Uint8Array, header: RatchetHeader): Promise<DecryptResult>;
  // DecryptResult = { plaintext: Uint8Array; nextSession: RatchetSession }
}

interface RatchetSession {
  rootKey: Uint8Array;
  chainKeySend: Uint8Array;
  chainKeyRecv: Uint8Array;
  dhSendPub: Uint8Array;
  dhSendPriv: Uint8Array;
  dhRecvPub: Uint8Array;
  nSend: number;
  nRecv: number;
  pn: number;
  skippedMessageKeys: Map<string, Uint8Array>;  // key: `${dhPub}:${n}`
}
```

Skipped message keys are capped at **2000 entries** to prevent memory growth.

### 4.4 Sender Key Engine (`sender-key-engine.ts`)

```typescript
class SenderKeyEngine {
  static async createSession(material: SenderKeyMaterial): Promise<SenderKeySession>;
  static async encrypt(session: SenderKeySession, plaintext: Uint8Array): Promise<{ ciphertext: Uint8Array; nextSession: SenderKeySession }>;
  static async decrypt(session: SenderKeySession, ciphertext: Uint8Array): Promise<{ plaintext: Uint8Array; nextSession: SenderKeySession }>;
  static serializeKeyMaterial(session: SenderKeySession): Uint8Array;  // for SKDM
  static deserializeKeyMaterial(bytes: Uint8Array): SenderKeySession;
}
```

---

## Transport Layer Design

### 5.1 REST Client (`rest-client.ts`)

```typescript
class RestClient {
  constructor(private readonly baseUrl: string, private readonly getToken: () => string) {}

  // KDS
  registerDevice(userId: string, bundle: KeyBundleRequest): Promise<RegisterDeviceResponse>;
  getKeyBundle(userId: string): Promise<KeyBundleResponse>;
  replenishOtpks(userId: string, deviceId: string, keys: OtpkEntry[]): Promise<ReplenishResponse>;
  rotateSignedPreKey(userId: string, deviceId: string, update: SignedPreKeyUpdate): Promise<void>;

  // Conversations
  createConversation(req: CreateConversationRequest): Promise<CreateConversationResponse>;
  sendMessage(conversationId: string, req: MessageEnvelopeRequest): Promise<SendMessageResponse>;
  getMessages(conversationId: string, params?: { limit?: number; beforeSeq?: number }): Promise<GetMessagesResponse>;

  // Groups
  createGroup(req: CreateGroupRequest): Promise<CreateGroupResponse>;
  sendGroupMessage(conversationId: string, req: MessageEnvelopeRequest): Promise<SendMessageResponse>;
  addGroupMember(conversationId: string, userId: string, deviceId: string): Promise<void>;
  removeGroupMember(conversationId: string, userId: string): Promise<void>;
  distributeGroupSenderKey(conversationId: string, recipients: SkdmRecipient[]): Promise<void>;

  // Attachments
  uploadAttachment(
    data: FormData,
    onProgress?: (sent: number, total: number) => void
  ): Promise<UploadResponse>;
}
```

**Request conventions:**
- `Authorization: Bearer {token}` on every request (via `getToken()` closure — honours hot-swap).
- Non-2xx responses are parsed as `{ error_code, message, request_id }` and thrown as `SdkError`.
- Retry: up to 3 attempts on 503 and network errors with 500ms linear backoff; no retry on 4xx.
- All key bytes in JSON are **base64url encoded** (no `=` padding).

### 5.2 WebSocket Manager (`ws-manager.ts`)

```typescript
class WsManager {
  readonly events: TypedEventEmitter<WsManagerEvents>;

  connect(wsUrl: string): void;
  disconnect(): void;
  sendAck(conversationId: string, seq: number): void;
  sendPong(): void;
}

interface WsManagerEvents {
  frame: RtEvent;
  open: void;
  close: { code: number; reason: string };
  error: Event;
}
```

Frame parsing converts raw JSON text frames into the `RtEvent` discriminated union (unknown types are silently ignored for forward compatibility).

### 5.3 Connection Manager (`connection-manager.ts`)

```typescript
class ConnectionManager {
  readonly state: ConnectionState;
  readonly stateChanges: TypedEventEmitter<{ change: { state: ConnectionState } }>;

  ensureConnected(): Promise<void>;
  disconnect(): Promise<void>;
}
```

Reconnect schedule: 1s → 2s → 4s → 8s → 16s → 32s → 60s (cap).
On reconnect, URL is reconstructed with current token from `getToken()` closure.

### 5.4 RtEvent Discriminated Union (`rt-event.ts`)

```typescript
type RtEvent =
  | { type: 'message';                   conversationId: string; seq: number; senderUserId: string; senderDeviceId: string; ciphertext: Uint8Array; protocolHeader: ProtocolHeader; serverTs: number; attachmentId: string | null }
  | { type: 'low_otpk';                  deviceId: string; count: number }
  | { type: 'member_added';              conversationId: string; userId: string; devices: string[] }
  | { type: 'member_removed';            conversationId: string; userId: string }
  | { type: 'sender_key_distribution';   conversationId: string; senderUserId: string; encryptedSkdm: Uint8Array };
```

---

## Session Layer Design

### 6.1 SessionStore Interface

```typescript
interface SessionStore {
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
```

### 6.2 DeviceRecord (stored)

```typescript
interface DeviceRecord {
  userId: string;
  deviceId: string;
  identityKeyDhPriv: Uint8Array;    // X25519 private
  identityKeyDhPub: Uint8Array;
  identityKeyEdPriv: Uint8Array;    // Ed25519 private (for SPK signing)
  identityKeyEdPub: Uint8Array;
  signedPrekeyPriv: Uint8Array;
  signedPrekeyPub: Uint8Array;
  signedPrekeyId: number;
  signedPrekeyCreatedAt: number;    // Unix epoch ms
  otpks: OtpkRecord[];              // remaining private OTPKs by ID
}
```

### 6.3 RatchetState (stored)

```typescript
interface RatchetState {
  conversationId: string;
  rootKey: string;          // base64
  chainKeySend: string;
  chainKeyRecv: string;
  dhSendPub: string;
  dhSendPriv: string;
  dhRecvPub: string;
  nSend: number;
  nRecv: number;
  pn: number;
  skippedMessageKeys: Record<string, string>;  // key: `${dhPub}:${n}`, value: base64 key
}
```

### 6.4 IndexedDB Schema (IndexedDbSessionStore)

Database: `rce_sdk`, version `1`.

| Object Store | Key | Value |
|---|---|---|
| `devices` | `${userId}::${deviceId}` | `DeviceRecord` JSON |
| `ratchetStates` | `${conversationId}` | `RatchetState` JSON |
| `senderKeys` | `${conversationId}::${userId}` | `SenderKeyRecord` JSON |
| `conversations` | `${conversationId}` | `ConversationMeta` JSON |

### 6.5 NodeFileSessionStore Layout

```
{storageDir}/
  devices/
    {userId}__{deviceId}.json
  ratchet/
    {conversationId}.json
  senderkeys/
    {conversationId}__{userId}.json
  conversations/
    {conversationId}.json
```

---

## Initialization Sequence

```
ChatClient.create(config)
  │
  ├── 1. Detect platform → choose default SessionStore
  ├── 2. Create RestClient(baseUrl, () => currentToken)
  ├── 3. If deviceId undefined:
  │       a. KeyGenerator.generateKeyBundle()
  │       b. RestClient.registerDevice(userId, publicBundle)
  │       c. SessionStore.saveDevice(privateRecord)
  │       d. Set this.deviceId from response
  │   Else:
  │       a. SessionStore.loadDevice(userId, deviceId)
  │       b. If null → throw SdkError.SESSION_NOT_FOUND
  ├── 4. Create WsManager + ConnectionManager
  ├── 5. Create RtEventRouter → subscribes to WsManager.events.frame
  ├── 6. Create ConversationRegistry
  │       → SessionStore.loadAllConversations()
  │       → restore each as OneToOneConversation or GroupConversation
  ├── 7. Create OtpkReplenisher (listens to RtEventRouter for low_otpk)
  ├── 8. Create SpkRotator (checks rotation interval on timer)
  ├── 9. If autoConnect → ConnectionManager.ensureConnected()
  └── 10. Return ChatClient
```

## Message Send Sequence (1:1)

```
conversation.send("Hello")
  │
  ├── 1. TextEncoder.encode("Hello") → plaintext: Uint8Array
  ├── 2. RatchetEngine.encrypt(session, plaintext)
  │       → { ciphertext, header, nextSession }
  ├── 3. SessionStore.saveRatchetState(conversationId, nextSession) [before network]
  ├── 4. RestClient.sendMessage(conversationId, { ciphertext, protocolHeader: header })
  │       → { seq, serverTs }
  ├── 5. Build ChatMessage { text: "Hello", seq, isMine: true, ... }
  ├── 6. this.emit('message', chatMessage)
  └── 7. Resolve Promise<ChatMessage>
```

## Message Receive Sequence (WebSocket → Conversation)

```
WsManager emits RtEvent { type: 'message', ... }
  │
  ├── RtEventRouter.route(event)
  ├── ConversationRegistry.getOrCreate(conversationId)
  ├── RatchetEngine.decrypt(session, ciphertext, header)
  │     → { plaintext, nextSession } OR throws DecryptionError
  ├── SessionStore.saveRatchetState(conversationId, nextSession)
  ├── Build ChatMessage { text: TextDecoder.decode(plaintext), ... }
  ├── conversation.emit('message', chatMessage)
  └── WsManager.sendAck(conversationId, seq)
```

## Group SenderKey Distribution Sequence

```
client.createGroup(['bob', 'carol'])
  │
  ├── 1. RestClient.createGroup({ members: [...] })
  │       → { conversationId, members }
  ├── 2. SenderKeyEngine.createSession(generated material)
  ├── 3. SessionStore.saveSenderKey(conversationId, myUserId, record)
  ├── 4. For each member:
  │       a. RestClient.getKeyBundle(memberId)
  │       b. If no 1:1 ratchet: X3dhEngine.performX3dh(...)
  │       c. RatchetEngine.encrypt(1:1 session, SenderKeyEngine.serialize(session))
  │       d. Collect SkdmRecipient { userId, deviceId, encryptedSkdm }
  ├── 5. RestClient.distributeGroupSenderKey(conversationId, recipients)
  ├── 6. Create GroupConversation + ConversationRegistry.register(...)
  └── 7. Resolve Promise<Conversation>
```

---

## Data Models

### API Request/Response DTOs (selected)

```typescript
// POST /users/{userId}/devices
interface KeyBundleRequest {
  identity_key: string;        // base64url X25519 public
  signed_prekey_id: number;
  signed_prekey: string;       // base64url X25519 public
  signed_prekey_sig: string;   // base64url Ed25519 signature
  one_time_prekeys: { id: number; key: string }[];
}
interface RegisterDeviceResponse {
  device_id: string;           // UUID
}

// GET /users/{userId}/key-bundle
interface KeyBundleResponse {
  device_id: string;
  identity_key: string;
  signed_prekey_id: number;
  signed_prekey: string;
  signed_prekey_sig: string;
  one_time_prekey: { id: number; key: string } | null;
}

// POST /conversations  (X3DH init message)
interface CreateConversationRequest {
  recipient_user_id: string;
  recipient_device_id: string;
  envelope: {
    conversation_id: string;      // "00000000-0000-0000-0000-000000000000"
    ciphertext: string;           // base64url
    protocol_header: {
      type: 'x3dh_init';
      ek: string;                 // ephemeral key, base64url
      spk_id: number;
      otpk_id?: number;
    };
  };
}
interface CreateConversationResponse {
  conversation_id: string;
}

// POST /conversations/{id}/messages  (Double Ratchet)
interface MessageEnvelopeRequest {
  envelope: {
    conversation_id: string;
    ciphertext: string;
    protocol_header: {
      type: 'double_ratchet' | 'sender_key';
      dh?: string;      // double_ratchet: current DH ratchet pub key
      n?: number;
      pn?: number;
      chain_id?: number; // sender_key
      iteration?: number;
    };
    attachment_id?: string;
  };
}
interface SendMessageResponse {
  seq: number;
  server_ts: number;           // Unix epoch ms
}
```

---

## Platform Detection and Compatibility

```typescript
// src/session/platform.ts
export function detectDefaultSessionStore(): SessionStore {
  // Browser (has window.indexedDB)
  if (typeof indexedDB !== 'undefined') {
    return new IndexedDbSessionStore();
  }
  // Node.js (has process)
  if (typeof process !== 'undefined' && process.versions?.node) {
    return new MemorySessionStore(); // NodeFileSessionStore if path provided
  }
  // Web Worker, Deno, Bun, etc.
  return new MemorySessionStore();
}
```

**Runtime compatibility table:**

| API | Chrome 89+ | Firefox 86+ | Safari 15+ | Node 18+ | Deno 1.28+ | Bun 0.6+ |
|---|---|---|---|---|---|---|
| `crypto.subtle` | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| `crypto.getRandomValues` | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| `WebSocket` | ✅ | ✅ | ✅ | ✅ (`ws` polyfill optional) | ✅ | ✅ |
| `fetch` | ✅ | ✅ | ✅ | ✅ (≥18) | ✅ | ✅ |
| `indexedDB` | ✅ | ✅ | ✅ | ❌ | ❌ | ❌ |
| `TextEncoder` | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |

---

## Build Configuration

```json
// package.json (relevant fields)
{
  "name": "@rust-e2e-chat/sdk",
  "version": "1.0.0",
  "type": "module",
  "exports": {
    ".": {
      "import": "./dist/index.js",
      "require": "./dist/index.cjs",
      "types": "./dist/index.d.ts"
    },
    "./testing": {
      "import": "./testing/index.js",
      "require": "./testing/index.cjs",
      "types": "./testing/index.d.ts"
    }
  },
  "files": ["dist", "testing"],
  "dependencies": {},
  "devDependencies": {
    "typescript": "^5.4",
    "tsup": "^8.0",
    "vitest": "^1.6"
  },
  "sideEffects": false
}
```

Build tool: **tsup** (wraps esbuild). Targets: ESM + CJS + `.d.ts` + browser UMD.

---

## Correctness Properties

All 7 correctness properties from `requirements.md` are enforced at:

| Property | Enforced in |
|---|---|
| Zero-Knowledge Transport | `ratchet-engine.ts` returns ciphertext only; `rest-client.ts` never logs request bodies |
| Session Continuity | `ratchet-engine.ts` persists state via `SessionStore.saveRatchetState` before resolving |
| OTPK Depletion Resilience | `x3dh-engine.ts` accepts `null` OTPK and skips DH4 step |
| Forward Secrecy Preservation | ratchet state saved BEFORE network call in send sequence |
| Sender Key Isolation | `group-conversation.ts` generates new SenderKey before `removeMember` resolves |
| Token Hot-Swap | `rest-client.ts` uses `getToken()` closure; `connection-manager.ts` rebuilds URL on reconnect |
| Zero Unhandled Rejections | `otpk-replenisher.ts`, `spk-rotator.ts`, `rt-event-router.ts` all wrap async handlers in `try/catch → emit('error', ...)` |
