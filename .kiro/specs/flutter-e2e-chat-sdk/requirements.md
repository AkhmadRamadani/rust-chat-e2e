# Requirements Document: flutter-e2e-chat-sdk

## Introduction

A Dart/Flutter SDK that provides a complete, type-safe client for the `rust-e2e-chat-api` platform. The SDK abstracts all Signal Protocol cryptographic operations (X3DH key agreement, Double Ratchet 1:1 messaging, Sender Key group messaging), WebSocket lifecycle management, REST API calls, and secure local key storage behind a clean, reactive, idiomatic Dart public API. Developers integrating this SDK only need to supply their OIDC access token and user identity; all protocol complexity is handled internally.

The SDK is published as a standalone Dart package (`rust_e2e_chat_sdk`) that targets Flutter mobile (iOS, Android), desktop (macOS, Windows, Linux), and pure Dart server environments. It uses `flutter_secure_storage` for key persistence on mobile/desktop and an in-memory fallback for tests and server contexts. Network transport uses `web_socket_channel` for real-time events and `dio` (or `http`) for REST calls.

## Glossary

- **ChatClient**: The root public class of the SDK. Encapsulates all state: auth, connection, crypto sessions, and cached messages.
- **TenantId**: A UUID identifying the tenant. Derived from the OIDC JWT's `tid` claim.
- **UserId**: The caller's OIDC `sub` claim value, identifying the user within a tenant.
- **DeviceId**: A UUID returned by the server after device registration. Stored locally and reused across sessions.
- **KeyBundle**: The set of public keys for a device: IdentityKey, SignedPreKey (SPK), SignedPreKey signature, and a list of OneTimePreKeys (OTPKs).
- **PrivateKeyBundle**: The locally-stored private halves of the KeyBundle.
- **X3DH**: Extended Triple Diffie-Hellman. Client-side key agreement protocol that establishes a shared session root key before the first message.
- **DoubleRatchet**: The Double Ratchet Algorithm. Derives unique per-message keys after X3DH session establishment, providing forward secrecy and break-in recovery.
- **SenderKey**: A per-user, per-group symmetric key. Each group member generates their own SenderKey and distributes it (encrypted) to every other member via 1:1 X3DH channels.
- **ConversationId**: A UUID returned by the server identifying a 1:1 or group conversation.
- **MessageEnvelope**: A server-side record: ciphertext, protocol header, sequence number, timestamp, and sender identity. Never contains plaintext.
- **ChatMessage**: The SDK's decrypted, developer-facing message type containing `text`, `attachmentUrl`, `sender`, `timestamp`, and `seq`.
- **RtEvent**: A real-time WebSocket event frame from the server (message, low_otpk, member_added, member_removed, sender_key_distribution).
- **SessionStore**: The interface for durable local storage of private key material, ratchet states, and session metadata.
- **ChatConversation**: An observable object representing a conversation: participant list, message stream, and unread count.
- **AttachmentUpload**: A progress-aware upload handle returned when sending a file attachment.

---

## Requirements

### Requirement 0: SDK Initialization and Configuration

**User Story:** As a Flutter developer, I want to initialize the SDK with a single configuration call so that I can start chatting without understanding the underlying protocol details.

#### Acceptance Criteria

1. THE SDK SHALL expose a `ChatClient.initialize(config: ChatClientConfig)` factory that accepts the API base URL, the OIDC access token, the caller's `userId`, and an optional `sessionStore` implementation.
2. THE `ChatClientConfig` SHALL accept `baseUrl` (String, required), `accessToken` (String, required), `userId` (String, required), `deviceId` (String?, optional — null triggers automatic registration), `sessionStore` (SessionStore?, optional — defaults to `FlutterSecureSessionStore` on mobile/desktop and `InMemorySessionStore` on other platforms), and `autoConnect` (bool, defaults to true).
3. WHEN `ChatClient.initialize` is called and `deviceId` is null, THE SDK SHALL automatically generate a new key bundle, register the device via `POST /users/{userId}/devices`, persist the returned `DeviceId` and private key material in the `SessionStore`, and expose the `DeviceId` via `ChatClient.deviceId`.
4. WHEN `ChatClient.initialize` is called and `deviceId` is non-null, THE SDK SHALL load the existing private key bundle from the `SessionStore` and skip device registration.
5. IF the `SessionStore` contains no key material for the given `deviceId`, THE SDK SHALL throw `SdkError.sessionNotFound` and NOT silently generate a new device.
6. THE SDK SHALL expose a `ChatClient.updateToken(String newAccessToken)` method to refresh the access token in-place without requiring re-initialization.
7. THE SDK SHALL expose a `ChatClient.dispose()` method that closes the WebSocket, cancels all timers, and releases all resources.

---

### Requirement 1: Transport and Connection Management

**User Story:** As a developer, I want the SDK to manage the WebSocket connection automatically so that my app always has a live real-time session without writing connection lifecycle code.

#### Acceptance Criteria

1. WHEN `autoConnect` is true (default), THE SDK SHALL connect to `GET /ws?token={accessToken}&device_id={deviceId}` immediately after successful initialization and expose connection state via `ChatClient.connectionState` (a `Stream<ConnectionState>` where `ConnectionState` is one of `connecting`, `connected`, `reconnecting`, `disconnected`).
2. THE SDK SHALL respond to `{"type":"ping"}` server frames with `{"type":"pong"}` within 5 seconds.
3. WHEN the WebSocket connection drops unexpectedly, THE SDK SHALL automatically attempt to reconnect using exponential backoff starting at 1 second, doubling on each failure, capped at 60 seconds.
4. THE SDK SHALL expose `ChatClient.connect()` and `ChatClient.disconnect()` for manual lifecycle control when `autoConnect` is false.
5. WHEN a WebSocket reconnection succeeds, THE SDK SHALL automatically drain any queued offline messages from the server and emit them through the appropriate conversation streams.
6. THE SDK SHALL send an `AckDatagram` (`{"conversation_id":"...","seq":N}`) for each message after it has been successfully decrypted and delivered to the application layer.
7. THE SDK SHALL surface connection errors through `ChatClient.connectionErrors` (a `Stream<SdkError>`) without crashing the application.

---

### Requirement 2: Key Bundle Generation and Management

**User Story:** As a developer, I want the SDK to handle all cryptographic key generation and maintenance transparently so that my app never handles raw key bytes.

#### Acceptance Criteria

1. THE SDK SHALL generate all required key material (IdentityKey pair, SignedPreKey pair, SignedPreKey Ed25519 signature, and at least 20 OneTimePreKeys) during device registration.
2. THE SDK SHALL generate new key material using platform-appropriate secure randomness; on mobile, it SHALL prefer the OS secure random source.
3. THE SDK SHALL verify that the generated `SignedPreKey` Ed25519 signature is valid before submitting the key bundle to the server.
4. WHEN the server delivers a `low_otpk` real-time event with `count < 10`, THE SDK SHALL automatically generate 50 new OneTimePreKeys and upload them via `PUT /users/{userId}/devices/{deviceId}/one-time-prekeys` without requiring any application code.
5. THE SDK SHALL rotate the `SignedPreKey` after a configurable interval (`signedPrekeyRotationDays`, default 7 days), uploading the new key via `PUT /users/{userId}/devices/{deviceId}/signed-prekey`.
6. All private key material SHALL be stored exclusively in the `SessionStore` and SHALL never appear in logs, error messages, or be transmitted to any endpoint.
7. THE SDK SHALL expose `ChatClient.getPublicKeyBundle()` returning the device's public `KeyBundle` for display or diagnostic purposes.

---

### Requirement 3: 1:1 Conversation Initiation (X3DH)

**User Story:** As a developer, I want to open a 1:1 conversation with another user by calling a single method so that the X3DH handshake is invisible to my application code.

#### Acceptance Criteria

1. THE SDK SHALL expose `ChatClient.openConversation(recipientUserId: String) -> Future<ChatConversation>` which fetches the recipient's key bundle, performs X3DH client-side, creates the encrypted initial envelope, and calls `POST /conversations`.
2. WHEN `openConversation` is called and a conversation with that `recipientUserId` already exists in the local session cache, THE SDK SHALL return the cached `ChatConversation` without making a network call to start a new session.
3. THE SDK SHALL store the negotiated X3DH shared secret and all Double Ratchet state in the `SessionStore` keyed by `ConversationId` and `DeviceId`.
4. IF the recipient's key bundle has no OneTimePreKey (OTPK depleted), THE SDK SHALL still proceed with X3DH using only the IdentityKey and SignedPreKey, and SHALL log a warning to `ChatClient.warnings`.
5. THE `openConversation` method SHALL be idempotent: calling it multiple times with the same `recipientUserId` SHALL always return the same `ChatConversation` object (by identity).

---

### Requirement 4: 1:1 Messaging (Double Ratchet)

**User Story:** As a developer, I want to send and receive text messages in a 1:1 conversation using a simple API so that encryption and ratcheting are completely transparent.

#### Acceptance Criteria

1. THE `ChatConversation` object SHALL expose a `sendMessage(String text) -> Future<ChatMessage>` method that encrypts the text using the Double Ratchet, sends `POST /conversations/{conversationId}/messages`, and returns the optimistically-added `ChatMessage`.
2. THE `ChatConversation` object SHALL expose a `messages` property of type `Stream<List<ChatMessage>>` that emits the full ordered message list whenever a new message arrives or is sent.
3. WHEN a `RtEvent` of type `message` is received over WebSocket targeting the current conversation, THE SDK SHALL decrypt the ciphertext using the Double Ratchet ratchet state, construct a `ChatMessage`, and emit it through the `messages` stream.
4. THE SDK SHALL persist updated Double Ratchet state to the `SessionStore` after every send and receive operation, ensuring the state survives app restarts.
5. WHEN the app restarts and `openConversation` is called for an existing conversation, THE SDK SHALL restore the Double Ratchet session from the `SessionStore` and continue without re-doing X3DH.
6. THE `ChatConversation` object SHALL expose `fetchHistory({int limit = 50, int? beforeSeq}) -> Future<List<ChatMessage>>` which calls `GET /conversations/{conversationId}/messages` and decrypts each envelope.
7. IF decryption of a received message fails (e.g. out-of-order key exhaustion), THE SDK SHALL emit a `ChatMessage` with `text = null` and `decryptionError = true` rather than crashing.

---

### Requirement 5: Group Conversations (Sender Keys)

**User Story:** As a developer, I want to create and participate in group conversations using a single clean API, with all Sender Key management handled by the SDK.

#### Acceptance Criteria

1. THE SDK SHALL expose `ChatClient.createGroup(memberUserIds: List<String>) -> Future<ChatConversation>` which calls `POST /groups`, generates a new SenderKey for the creator, fetches each member's key bundle, and distributes the SenderKey to every member via `POST /groups/{conversationId}/sender-key-distribution`.
2. WHEN a `RtEvent` of type `sender_key_distribution` is received, THE SDK SHALL decrypt the SKDM using the existing 1:1 Double Ratchet session with the sender, store the decrypted SenderKey in the `SessionStore` keyed by `(ConversationId, SenderUserId)`, and silently acknowledge without emitting a visible message.
3. THE `ChatConversation` for a group SHALL expose the same `sendMessage(String text)` and `messages` stream interface as 1:1 conversations; internally, encryption uses the SenderKey ratchet.
4. WHEN a new member is added (`RtEvent: member_added`), THE SDK SHALL automatically generate a new SenderKey for the local user, distribute it to the new member, and emit a system `ChatMessage` of type `memberAdded` through the conversation stream.
5. WHEN a member is removed (`RtEvent: member_removed`), THE SDK SHALL automatically generate a new SenderKey (key rotation), distribute it to all remaining members, and emit a system `ChatMessage` of type `memberRemoved`. The removed member SHALL NOT receive the new key.
6. THE SDK SHALL expose `ChatConversation.addMember(String userId) -> Future<void>` and `ChatConversation.removeMember(String userId) -> Future<void>` which call the corresponding server endpoints and trigger SenderKey redistribution.
7. IF the SenderKey for a group message sender is not yet available in the `SessionStore`, THE SDK SHALL queue the message envelope and retry decryption after the SKDM is received, up to 60 seconds, before marking it as `decryptionError`.

---

### Requirement 6: File Attachments

**User Story:** As a developer, I want to send and receive file attachments in conversations with a simple API and progress reporting so that I do not need to manage multipart uploads.

#### Acceptance Criteria

1. THE `ChatConversation` object SHALL expose `sendAttachment(Uint8List bytes, String filename, String contentType) -> Future<ChatMessage>` which uploads the file via `POST /attachments` (multipart/form-data), then sends a message envelope referencing the returned `attachment_id`.
2. THE `sendAttachment` method SHALL expose upload progress via `ChatConversation.uploadProgress` (a `Stream<AttachmentUploadProgress>`) where `AttachmentUploadProgress` contains `filename`, `bytesUploaded`, and `totalBytes`.
3. WHEN a `ChatMessage` is received that contains a non-null `attachmentId`, THE SDK SHALL expose the download URL as `ChatMessage.attachmentUrl` pointing to `GET /attachments/{attachmentId}` with the current access token embedded.
4. THE SDK SHALL validate that the file size does not exceed 100 MB before initiating an upload and throw `SdkError.fileTooLarge` if the limit is exceeded.
5. THE SDK SHALL NOT cache attachment bytes locally; attachment loading is the responsibility of the application (e.g., via `cached_network_image`). The SDK only provides the authenticated URL.

---

### Requirement 7: Session Store Interface and Implementations

**User Story:** As a developer, I want the SDK to persist keys and ratchet state securely out-of-the-box, while allowing me to provide a custom store for advanced use cases.

#### Acceptance Criteria

1. THE SDK SHALL define a `SessionStore` abstract interface with the following methods: `saveDevice(DeviceRecord)`, `loadDevice(String userId, String deviceId) -> Future<DeviceRecord?>`, `saveRatchetState(String conversationId, RatchetState)`, `loadRatchetState(String conversationId) -> Future<RatchetState?>`, `saveSenderKey(String conversationId, String userId, SenderKeyRecord)`, `loadSenderKey(String conversationId, String userId) -> Future<SenderKeyRecord?>`, `clear()`.
2. THE SDK SHALL provide `FlutterSecureSessionStore` as the default implementation on iOS, Android, macOS, Windows, and Linux, backed by `flutter_secure_storage`. All stored values SHALL be JSON-serialized and AES-encrypted by the platform secure storage.
3. THE SDK SHALL provide `InMemorySessionStore` for testing and server environments, which holds all state in memory and is cleared on dispose.
4. WHEN the `SessionStore` fails to persist (e.g. storage quota exceeded), THE SDK SHALL emit the error through `ChatClient.storageErrors` (a `Stream<SdkError>`) without losing the in-memory state for the current session.
5. THE `SessionStore` interface SHALL be fully mockable in tests: the SDK SHALL NOT use any static or global state not routed through the store.

---

### Requirement 8: Observable State and Streams

**User Story:** As a Flutter developer, I want all SDK state changes (new messages, connection changes, member changes) exposed as Dart Streams so that I can integrate natively with StreamBuilder, Riverpod, Bloc, or any other state management library.

#### Acceptance Criteria

1. THE SDK SHALL expose `ChatClient.conversations` as `Stream<List<ChatConversation>>` that emits the full updated list whenever a conversation is added or updated.
2. EACH `ChatConversation` SHALL expose `messages` as `Stream<List<ChatMessage>>` ordered by ascending sequence number.
3. EACH `ChatConversation` SHALL expose `members` as `Stream<List<ConversationMember>>` that emits updated membership when `member_added` or `member_removed` events arrive.
4. EACH `ChatConversation` SHALL expose `unreadCount` as `Stream<int>` that increments when messages arrive while the conversation is not active and resets to 0 when `markAsRead()` is called.
5. ALL streams SHALL be broadcast streams (multi-subscriber) and SHALL replay the last-emitted value to new subscribers.
6. THE SDK SHALL NOT use `setState` or any Flutter widget lifecycle API; it SHALL be usable from pure Dart code with no Flutter dependency.

---

### Requirement 9: Error Handling

**User Story:** As a developer, I want the SDK to translate all network, cryptographic, and storage errors into a consistent, typed error hierarchy so that I can display meaningful messages to users.

#### Acceptance Criteria

1. THE SDK SHALL define a sealed class `SdkError` with the following subtypes: `SdkError.network(int? statusCode, String message)`, `SdkError.auth(String message)`, `SdkError.decryption(String conversationId, int seq)`, `SdkError.keyExchange(String recipientUserId, String reason)`, `SdkError.storage(String reason)`, `SdkError.sessionNotFound(String deviceId)`, `SdkError.fileTooLarge(int sizeBytes)`, `SdkError.unknown(Object cause)`.
2. ALL public `Future`-returning methods SHALL throw subtypes of `SdkError`; no raw exceptions from dependencies SHALL leak to the caller.
3. THE SDK SHALL map server error codes from the API's JSON `error_code` field to the corresponding `SdkError` subtype (e.g. `unauthorized` → `SdkError.auth`, `storage_unavailable` → `SdkError.network`).
4. THE SDK SHALL never throw synchronously from getters or stream accessors; errors SHALL always be delivered asynchronously via streams or Future completions.
5. THE SDK SHALL log internal errors with a configurable `Logger` (defaulting to `dart:developer` `log`). The log level SHALL be configurable via `ChatClientConfig.logLevel`.

---

### Requirement 10: Testing and Developer Experience

**User Story:** As a developer writing tests for my application, I want the SDK to expose a mock-friendly interface so that I can unit-test my chat features without a real server.

#### Acceptance Criteria

1. THE SDK SHALL expose a `MockChatClient` class (in a `rust_e2e_chat_sdk_testing` library) that implements the same interface as `ChatClient` and allows callers to inject test messages, simulate connection events, and assert on sent messages.
2. THE `MockChatClient` SHALL support `simulateIncomingMessage(String conversationId, String senderUserId, String text)` and `simulateConnectionState(ConnectionState)`.
3. THE SDK SHALL export all public types in a single `package:rust_e2e_chat_sdk/rust_e2e_chat_sdk.dart` barrel file.
4. ALL public API members SHALL have doc comments including usage examples.
5. THE SDK package SHALL include an `example/` directory with a minimal Flutter app demonstrating: initialization, opening a 1:1 conversation, sending/receiving messages, and group creation.

---

## Correctness Properties

### Property 1: Zero-Knowledge Transport
*For any* message sent or received, the plaintext content SHALL exist only in the application layer (above the SDK public API). The SDK SHALL never log, persist, or transmit unencrypted message content; all bytes handed to the transport layer SHALL be ciphertext.

**Validates: Requirements 4.1, 4.4, 5.3**

### Property 2: Session Continuity
*For any* `deviceId` D with a persisted session: if the app restarts and the same `deviceId` is provided to `ChatClient.initialize`, the resulting `ChatClient` SHALL be able to decrypt all future messages in all existing conversations without re-doing X3DH or Sender Key distribution.

**Validates: Requirements 4.4, 4.5, 5.2**

### Property 3: OTPK Depletion Resilience
*For any* call to `openConversation` where the recipient has zero remaining OTPKs, the SDK SHALL still establish a session (without OTPK) and NOT throw an error; the resulting session SHALL be cryptographically valid.

**Validates: Requirement 3.4**

### Property 4: Forward Secrecy Preservation
*For any* Double Ratchet send or receive, the ratchet state SHALL be advanced and persisted before the operation is considered complete. A crashed or killed app SHALL resume from the persisted state and NOT reuse any previously-used ratchet step.

**Validates: Requirements 4.4, 2.5**

### Property 5: Sender Key Isolation
*For any* group G with members M₁…Mₙ, after member Mₓ is removed, Mₓ SHALL NOT receive the new SenderKey generated during the removal event. Any group messages sent after removal SHALL NOT be decryptable by Mₓ.

**Validates: Requirement 5.5**

### Property 6: Token Hot-Swap
*For any* call to `ChatClient.updateToken(newToken)`, all subsequent REST requests and the next WebSocket reconnect SHALL use `newToken`. No request with the old token SHALL succeed after `updateToken` returns.

**Validates: Requirement 0.6**
