# Requirements Document: web-e2e-chat-sdk

## Introduction

A framework-agnostic TypeScript/JavaScript SDK (`@rust-e2e-chat/sdk`) that provides a complete, type-safe client for the `rust-e2e-chat-api` platform. The SDK targets all modern JavaScript runtimes: browser (Chrome, Firefox, Safari, Edge), Node.js ≥ 18, Deno, Bun, and Web Workers. It abstracts all Signal Protocol cryptographic operations (X3DH key agreement, Double Ratchet 1:1 messaging, Sender Key group messaging), WebSocket lifecycle management, REST API calls, and secure local key storage behind a clean, event-driven, Promise-based TypeScript API.

The SDK ships as a single npm package with dual CJS/ESM output and zero mandatory runtime dependencies. Cryptographic primitives are provided by the Web Crypto API (`crypto.subtle`) which is natively available in all target environments. Storage adapters are pluggable: an `IndexedDB` adapter is the default in browsers, `node:crypto`+filesystem in Node.js, and an in-memory fallback for tests and ephemeral sessions.

Framework integration packages (`@rust-e2e-chat/react`, `@rust-e2e-chat/vue`, `@rust-e2e-chat/svelte`) are out of scope for this spec and are separate packages that wrap this core SDK.

## Glossary

- **ChatClient**: The root class of the SDK. Created once per application instance; manages auth, connection, sessions, and crypto state.
- **TenantId**: A UUID identifying the tenant; derived from the OIDC JWT `tid` claim.
- **UserId**: The caller's OIDC `sub` claim value, unique within a tenant.
- **DeviceId**: A UUID returned by the server after device registration. Persisted locally and reused across sessions.
- **KeyBundle**: Public key material for a device: IdentityKey, SignedPreKey (SPK), SPK signature, and OneTimePreKeys (OTPKs).
- **PrivateKeyBundle**: The locally-stored private halves of the KeyBundle. Never leaves the device.
- **X3DH**: Extended Triple Diffie-Hellman. Client-side protocol that establishes a shared session root key before the first message.
- **DoubleRatchet**: The Double Ratchet Algorithm. Derives unique per-message keys providing forward secrecy and break-in recovery.
- **SenderKey**: A per-user, per-group symmetric key. Each member generates their own SenderKey and distributes it (encrypted) to every other member via 1:1 X3DH channels.
- **ConversationId**: A UUID from the server identifying a 1:1 or group conversation.
- **MessageEnvelope**: Server-stored record: ciphertext, protocol header, seq number, timestamp. Never contains plaintext.
- **ChatMessage**: The SDK's decrypted, developer-facing message object: `{ id, text, attachmentUrl, senderId, timestamp, seq, isMine, decryptionError }`.
- **EventEmitter**: The SDK uses a typed Node.js-compatible `EventEmitter` pattern — `client.on('message', handler)`.
- **SessionStore**: Interface for durable local storage of private keys, ratchet states, and session metadata.
- **Conversation**: An observable object with send/receive methods and event emission for a single 1:1 or group thread.
- **RtEvent**: A real-time WebSocket event frame: `message`, `low_otpk`, `member_added`, `member_removed`, `sender_key_distribution`.

---

## Requirements

### Requirement 0: SDK Initialization and Configuration

**User Story:** As a JavaScript developer, I want to initialize the SDK with a single async call so that I can start chatting without understanding the underlying protocol details.

#### Acceptance Criteria

1. THE SDK SHALL export a `ChatClient` class with a static `ChatClient.create(config: ChatClientConfig): Promise<ChatClient>` factory that accepts `baseUrl`, `accessToken`, `userId`, and optional `deviceId`, `sessionStore`, `autoConnect`, and `logLevel`.
2. THE `ChatClientConfig` type SHALL accept: `baseUrl: string` (required), `accessToken: string` (required), `userId: string` (required), `deviceId?: string` (null triggers automatic registration), `sessionStore?: SessionStore` (defaults to `IndexedDbSessionStore` in browsers, `MemorySessionStore` elsewhere), `autoConnect?: boolean` (default `true`), `signedPrekeyRotationDays?: number` (default `7`), `logLevel?: 'debug' | 'info' | 'warn' | 'error' | 'silent'` (default `'warn'`).
3. WHEN `ChatClient.create` is called and `deviceId` is undefined, THE SDK SHALL generate a new key bundle, register the device via `POST /users/{userId}/devices`, persist the returned `DeviceId` and private key material in the `SessionStore`, and expose `client.deviceId: string`.
4. WHEN `ChatClient.create` is called and `deviceId` is provided, THE SDK SHALL load the existing private key bundle from the `SessionStore` and skip device registration.
5. IF the `SessionStore` contains no key material for the given `deviceId`, THE SDK SHALL reject with `SdkError.SESSION_NOT_FOUND` and NOT silently generate a new device.
6. THE SDK SHALL expose `client.updateToken(newToken: string): void` to hot-swap the access token without re-initialization. All subsequent requests and the next WebSocket reconnect SHALL use the new token.
7. THE SDK SHALL expose `client.destroy(): Promise<void>` that closes the WebSocket, removes all event listeners, cancels all timers, and releases all resources.

---

### Requirement 1: Transport and Connection Management

**User Story:** As a developer, I want the SDK to manage the WebSocket connection automatically so that my app always has a live real-time session without writing connection lifecycle code.

#### Acceptance Criteria

1. WHEN `autoConnect` is `true` (default), THE SDK SHALL connect to `ws(s)://{host}/ws?token={accessToken}&device_id={deviceId}` immediately after initialization and emit `'connection'` events with state `'connecting' | 'connected' | 'reconnecting' | 'disconnected'`.
2. THE SDK SHALL respond to `{"type":"ping"}` frames from the server with `{"type":"pong"}` within 5 seconds. If it cannot respond within 10 seconds, it SHALL close and reconnect.
3. WHEN the WebSocket drops unexpectedly, THE SDK SHALL automatically reconnect using exponential backoff: 1s → 2s → 4s → 8s → 16s → 32s → 60s (cap). Each reconnect attempt SHALL use the current `accessToken` (honouring any `updateToken` calls).
4. THE SDK SHALL expose `client.connect(): Promise<void>` and `client.disconnect(): Promise<void>` for manual lifecycle control when `autoConnect` is false.
5. WHEN a WebSocket reconnects successfully, THE SDK SHALL drain any queued offline messages from the server and emit them through the appropriate conversation event handlers.
6. THE SDK SHALL send an `AckDatagram` (`{"conversation_id":"...","seq":N}`) for each message after it has been successfully decrypted and emitted to the application layer.
7. THE SDK SHALL emit `'error'` events on the `ChatClient` instance for connection errors without throwing unhandled rejections.

---

### Requirement 2: Key Bundle Generation and Management

**User Story:** As a developer, I want the SDK to handle all cryptographic key generation and maintenance transparently so that my application never touches raw key bytes.

#### Acceptance Criteria

1. THE SDK SHALL generate all required key material (IdentityKey pair, SignedPreKey pair, SPK Ed25519 signature, and at least 20 OneTimePreKeys) during device registration using `crypto.subtle`.
2. ALL key generation SHALL use `crypto.getRandomValues` for secure randomness; the SDK SHALL NOT use `Math.random` or any non-CSPRNG source.
3. THE SDK SHALL verify that the generated SignedPreKey Ed25519 signature is valid before submitting the key bundle to the server.
4. WHEN the server delivers a `low_otpk` WebSocket event with `count < 10`, THE SDK SHALL automatically generate 50 new OTPKs and upload them via `PUT /users/{userId}/devices/{deviceId}/one-time-prekeys` without any application code.
5. THE SDK SHALL rotate the SignedPreKey after the configured `signedPrekeyRotationDays` interval (default 7 days). Rotation uploads the new SPK via `PUT /users/{userId}/devices/{deviceId}/signed-prekey`.
6. ALL private key material SHALL be stored exclusively in the `SessionStore` and SHALL never appear in logs, error messages, or be transmitted to any endpoint other than during initial registration.
7. THE SDK SHALL expose `client.getPublicKeyBundle(): Promise<KeyBundle>` returning the device's public `KeyBundle` for display or diagnostic use.

---

### Requirement 3: 1:1 Conversation Initiation (X3DH)

**User Story:** As a developer, I want to open a 1:1 conversation with another user by calling a single method so that the X3DH handshake is invisible to my application code.

#### Acceptance Criteria

1. THE SDK SHALL expose `client.openConversation(recipientUserId: string): Promise<Conversation>` which fetches the recipient's key bundle, performs X3DH client-side, creates the encrypted initial envelope, calls `POST /conversations`, and returns a `Conversation` object.
2. WHEN `openConversation` is called for a `recipientUserId` for which a conversation already exists in the local session cache, THE SDK SHALL return the cached `Conversation` without re-doing X3DH.
3. THE SDK SHALL store the negotiated X3DH shared secret and all Double Ratchet state in the `SessionStore` keyed by `conversationId`.
4. IF the recipient's key bundle has no OTPK (depleted pool), THE SDK SHALL still proceed with X3DH using only the IdentityKey and SignedPreKey and SHALL emit a `'warn'` log. It SHALL NOT throw an error.
5. THE `openConversation` method SHALL be idempotent: calling it multiple times for the same `recipientUserId` SHALL return the same `Conversation` instance.

---

### Requirement 4: 1:1 Messaging (Double Ratchet)

**User Story:** As a developer, I want to send and receive text messages in a 1:1 conversation using a simple event-driven API so that encryption and ratcheting are completely transparent.

#### Acceptance Criteria

1. THE `Conversation` object SHALL expose `conversation.send(text: string): Promise<ChatMessage>` that encrypts the text via the Double Ratchet, calls `POST /conversations/{conversationId}/messages`, and resolves with the sent `ChatMessage`.
2. THE `Conversation` object SHALL emit `'message'` events (`conversation.on('message', (msg: ChatMessage) => void)`) when a new message arrives, after decryption.
3. WHEN a `RtEvent` of type `message` arrives for this conversation, THE SDK SHALL decrypt the ciphertext using the stored Double Ratchet session and emit the decrypted `ChatMessage` via the `'message'` event.
4. THE SDK SHALL persist updated Double Ratchet state to the `SessionStore` after every send and receive, before emitting to the application.
5. WHEN the app reloads and `openConversation` is called for an existing conversation, THE SDK SHALL restore the Double Ratchet session from the `SessionStore` and continue without re-doing X3DH.
6. THE `Conversation` object SHALL expose `conversation.fetchHistory(options?: { limit?: number; beforeSeq?: number }): Promise<ChatMessage[]>` which calls `GET /conversations/{conversationId}/messages` and decrypts each envelope.
7. IF decryption of a received message fails, THE SDK SHALL emit a `ChatMessage` with `text: null` and `decryptionError: true` via the `'message'` event rather than throwing.

---

### Requirement 5: Group Conversations (Sender Keys)

**User Story:** As a developer, I want to create and participate in group conversations with a simple API, with all Sender Key management handled by the SDK.

#### Acceptance Criteria

1. THE SDK SHALL expose `client.createGroup(memberUserIds: string[]): Promise<Conversation>` which calls `POST /groups`, generates a new SenderKey for the creator, fetches each member's key bundle, and distributes the SenderKey to all members via `POST /groups/{conversationId}/sender-key-distribution`.
2. WHEN a `RtEvent` of type `sender_key_distribution` is received, THE SDK SHALL decrypt the SKDM using the existing 1:1 Double Ratchet session with the sender, store the decrypted SenderKey in the `SessionStore`, and NOT emit a visible message event.
3. THE `Conversation` object for a group SHALL expose the same `send(text)`, `on('message', ...)`, and `fetchHistory()` interface as 1:1 conversations; internally encryption uses the Sender Key ratchet.
4. WHEN a `RtEvent` of type `member_added` arrives, THE SDK SHALL automatically distribute the local user's new SenderKey to the new member and emit a `'member_added'` event on the conversation with `{ userId: string }`.
5. WHEN a `RtEvent` of type `member_removed` arrives, THE SDK SHALL automatically generate a new SenderKey, distribute it to all remaining members (excluding the removed user), and emit a `'member_removed'` event on the conversation with `{ userId: string }`.
6. THE `Conversation` object SHALL expose `conversation.addMember(userId: string): Promise<void>` and `conversation.removeMember(userId: string): Promise<void>`.
7. IF the SenderKey for a group message sender is not in the `SessionStore` yet, THE SDK SHALL queue the encrypted envelope and retry decryption for up to 60 seconds after SKDM receipt before emitting it as `decryptionError: true`.

---

### Requirement 6: File Attachments

**User Story:** As a developer, I want to upload and reference file attachments in conversations with progress reporting and a simple API.

#### Acceptance Criteria

1. THE `Conversation` object SHALL expose `conversation.sendAttachment(file: File | Blob | BufferSource, filename: string, contentType: string): Promise<ChatMessage>` which uploads the file via `POST /attachments` (multipart/form-data) and then sends a message envelope with the returned `attachmentId`.
2. THE `sendAttachment` method SHALL expose upload progress via a returned `{ promise: Promise<ChatMessage>; progress: ReadableStream<AttachmentProgress> }` object, where `AttachmentProgress = { filename: string; bytesUploaded: number; totalBytes: number }`.
3. WHEN a `ChatMessage` is received with a non-null `attachmentId`, THE SDK SHALL populate `message.attachmentUrl` with the authenticated download URL: `{baseUrl}/attachments/{attachmentId}?token={accessToken}`.
4. THE SDK SHALL validate that the file size does not exceed 100 MB before initiating an upload and reject with `SdkError.FILE_TOO_LARGE` if exceeded.
5. THE SDK SHALL NOT cache attachment bytes locally; serving and caching attachment content is the application's responsibility.

---

### Requirement 7: Session Store Interface and Implementations

**User Story:** As a developer, I want the SDK to persist keys and ratchet state out-of-the-box, while allowing me to provide a custom store for advanced use cases.

#### Acceptance Criteria

1. THE SDK SHALL define a `SessionStore` TypeScript interface with: `saveDevice(record: DeviceRecord): Promise<void>`, `loadDevice(userId: string, deviceId: string): Promise<DeviceRecord | null>`, `saveRatchetState(conversationId: string, state: RatchetState): Promise<void>`, `loadRatchetState(conversationId: string): Promise<RatchetState | null>`, `saveSenderKey(conversationId: string, userId: string, record: SenderKeyRecord): Promise<void>`, `loadSenderKey(conversationId: string, userId: string): Promise<SenderKeyRecord | null>`, `saveConversationMeta(meta: ConversationMeta): Promise<void>`, `loadAllConversations(): Promise<ConversationMeta[]>`, `clear(): Promise<void>`.
2. THE SDK SHALL provide `IndexedDbSessionStore` as the default browser implementation. All values SHALL be JSON-serialized and stored in a versioned IndexedDB database named `rce_sdk`.
3. THE SDK SHALL provide `MemorySessionStore` for Node.js server environments, tests, and ephemeral browser sessions. Data is held in memory only.
4. THE SDK SHALL provide `NodeFileSessionStore` for Node.js persistent use, storing JSON files in a configurable directory, protected by OS file permissions.
5. WHEN the `SessionStore` fails to persist (e.g. storage quota), THE SDK SHALL emit a `'storage_error'` event on the `ChatClient` instance without losing the in-memory state for the current session.
6. THE `SessionStore` interface SHALL be fully mockable: the SDK SHALL NOT use any module-level singleton state not routed through the store.

---

### Requirement 8: Event-Driven API and TypeScript Types

**User Story:** As a TypeScript developer, I want all SDK state changes exposed through a fully-typed event-emitter API so that I can integrate with any framework or state management library.

#### Acceptance Criteria

1. THE `ChatClient` class SHALL extend a typed `EventEmitter` and emit the following events with their types:
   - `'connection'`: `{ state: ConnectionState }` where `ConnectionState = 'connecting' | 'connected' | 'reconnecting' | 'disconnected'`
   - `'conversation'`: `{ conversation: Conversation }` when a new conversation is discovered from the server
   - `'error'`: `SdkError`
   - `'storage_error'`: `SdkError`
2. THE `Conversation` class SHALL extend a typed `EventEmitter` and emit:
   - `'message'`: `ChatMessage`
   - `'member_added'`: `{ userId: string; devices: string[] }`
   - `'member_removed'`: `{ userId: string }`
   - `'typing'`: `{ userId: string }` (future, reserved)
3. ALL public types (`ChatClient`, `Conversation`, `ChatMessage`, `KeyBundle`, `SessionStore`, `SdkError`, `ChatClientConfig`, `ConversationMeta`, `ConnectionState`, `AttachmentProgress`) SHALL be exported from the top-level `index.ts` barrel.
4. THE SDK SHALL ship TypeScript declaration files (`.d.ts`) for all exported types.
5. THE SDK SHALL be consumable from plain JavaScript (no TypeScript required) with full JSDoc documentation on the compiled output.
6. THE SDK SHALL NOT require any peer dependencies. Framework adapters (React hooks, Vue composables) are separate optional packages.

---

### Requirement 9: Error Handling

**User Story:** As a developer, I want all SDK errors translated into a consistent, typed error class so that I can display meaningful messages to users and handle errors predictably.

#### Acceptance Criteria

1. THE SDK SHALL define an `SdkError` class extending `Error` with: `code: SdkErrorCode` (a string enum), `message: string`, `cause?: unknown`, and `statusCode?: number` for HTTP-derived errors.
2. THE `SdkErrorCode` enum SHALL include: `NETWORK_ERROR`, `AUTH_ERROR`, `DECRYPTION_ERROR`, `KEY_EXCHANGE_ERROR`, `STORAGE_ERROR`, `SESSION_NOT_FOUND`, `FILE_TOO_LARGE`, `DEVICE_LIMIT_REACHED`, `INVALID_SIGNATURE`, `UNKNOWN_ERROR`.
3. THE SDK SHALL map server `error_code` values from the API JSON response to the corresponding `SdkErrorCode` (e.g. `unauthorized` → `AUTH_ERROR`, `storage_unavailable` → `NETWORK_ERROR`).
4. ALL Promise-returning public methods SHALL reject with an `SdkError` instance; no raw `fetch` errors, `DOMException`s, or third-party errors SHALL leak to the caller.
5. THE SDK SHALL never emit unhandled promise rejections; all async errors in background tasks (reconnect loop, OTPK replenisher) SHALL be emitted as `'error'` events on `ChatClient`.

---

### Requirement 10: Browser and Runtime Compatibility

**User Story:** As a developer, I want the SDK to work in all modern JavaScript environments without polyfills or build configuration so that I can drop it into any project.

#### Acceptance Criteria

1. THE SDK SHALL be published as an npm package with both ESM (`"exports"."import"`) and CJS (`"exports"."require"`) entry points, targeting ES2020 syntax.
2. THE SDK SHALL have zero mandatory runtime dependencies in `package.json`; all required APIs (`fetch`, `WebSocket`, `crypto.subtle`, `crypto.getRandomValues`, `TextEncoder`, `TextDecoder`) are available natively in all target environments (Chrome ≥ 89, Firefox ≥ 86, Safari ≥ 15, Node.js ≥ 18, Deno ≥ 1.28, Bun ≥ 0.6).
3. THE SDK SHALL NOT use `window`, `document`, `localStorage`, or `sessionStorage` directly; all browser globals SHALL be accessed through the `SessionStore` abstraction or feature-detected at runtime.
4. THE SDK SHALL be usable in Web Workers (no `window` dependency in the core bundle).
5. THE SDK SHALL support tree-shaking: unused exports (e.g. `NodeFileSessionStore`) SHALL NOT appear in browser bundles.
6. THE SDK SHALL export a pre-built browser bundle (`dist/browser.min.js`) as a `<script>` tag UMD build exposing `window.RustChat`.

---

### Requirement 11: Testing and Developer Experience

**User Story:** As a developer writing tests for my application, I want the SDK to expose a mock-friendly interface so that I can unit-test my chat features without a live server.

#### Acceptance Criteria

1. THE SDK SHALL export a `MockChatClient` class from `@rust-e2e-chat/sdk/testing` that implements the same `ChatClient` interface backed by `MemorySessionStore` and fake transport.
2. THE `MockChatClient` SHALL expose: `simulateIncomingMessage(conversationId: string, senderId: string, text: string): void`, `simulateConnectionState(state: ConnectionState): void`, `simulateMemberAdded(conversationId: string, userId: string): void`, `simulateMemberRemoved(conversationId: string, userId: string): void`.
3. THE SDK SHALL ship a `vitest` (or `jest`-compatible) test helper that automatically creates a `MockChatClient` and cleans up after each test via `afterEach`.
4. ALL public methods and events SHALL have JSDoc `@example` comments.
5. THE SDK package SHALL include a `README.md` with: quick-start (3 code blocks), 1:1 example, group example, custom `SessionStore` example, and framework adapter references.

---

## Correctness Properties

### Property 1: Zero-Knowledge Transport
*For any* message sent or received, the plaintext content SHALL exist only above the `Conversation.send()` / `'message'` event boundary. The SDK SHALL never log, persist, or transmit unencrypted message content; all bytes passed to the transport layer SHALL be ciphertext.

**Validates: Requirements 4.1, 4.4, 5.3**

### Property 2: Session Continuity
*For any* `deviceId` D with a persisted `SessionStore`: if the page reloads and the same `deviceId` is provided to `ChatClient.create`, the resulting `ChatClient` SHALL be able to decrypt all future messages in all existing conversations without re-doing X3DH or Sender Key distribution.

**Validates: Requirements 4.4, 4.5, 5.2**

### Property 3: OTPK Depletion Resilience
*For any* call to `openConversation` where the recipient has zero remaining OTPKs, the SDK SHALL still establish a session (without OTPK) and SHALL NOT throw or reject; the resulting session SHALL be cryptographically valid.

**Validates: Requirement 3.4**

### Property 4: Forward Secrecy Preservation
*For any* Double Ratchet send or receive, the updated ratchet state SHALL be persisted to the `SessionStore` before the operation resolves. A page reload SHALL resume from the persisted state and SHALL NOT reuse any previously-derived ratchet key.

**Validates: Requirements 4.4, 2.5**

### Property 5: Sender Key Isolation
*For any* group G with members M₁…Mₙ, after member Mₓ is removed, Mₓ SHALL NOT receive the new SenderKey. Any group messages sent after removal SHALL NOT be decryptable by Mₓ.

**Validates: Requirement 5.5**

### Property 6: Token Hot-Swap
*For any* call to `client.updateToken(newToken)`, all subsequent `fetch` calls and the next WebSocket reconnect SHALL use `newToken`. No request authenticated with the old token SHALL be made after `updateToken` returns.

**Validates: Requirement 0.6**

### Property 7: Zero Unhandled Rejections
*For any* background task (reconnect, OTPK replenishment, SPK rotation, SKDM distribution), all errors SHALL be caught internally and emitted as `'error'` events. The SDK SHALL NEVER produce an `unhandledRejection` or `uncaughtException`.

**Validates: Requirement 9.5**
