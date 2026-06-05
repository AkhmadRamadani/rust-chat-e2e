# Design Document: flutter-e2e-chat-sdk

## Overview

`rust_e2e_chat_sdk` is a Dart/Flutter package that provides a complete, opinionated client for the `rust-e2e-chat-api` platform. It is structured as a layered architecture:

1. **Crypto layer** — pure Dart Signal Protocol implementation (X3DH, Double Ratchet, Sender Keys) using `cryptography` or `libsodium_dart` for primitives.
2. **Transport layer** — HTTP REST via `dio` and WebSocket via `web_socket_channel`. Handles auth headers, reconnection, and ack frames.
3. **Repository layer** — typed Dart clients for every server endpoint, returning domain types.
4. **Session layer** — manages the full lifecycle of keys, ratchet states, and sender keys across restarts.
5. **Domain layer** — `ChatClient`, `ChatConversation`, `ChatMessage` — the public surface the application developer interacts with.

The SDK is deliberately **zero-knowledge at the transport boundary**: no plaintext ever crosses the repository layer.

---

## Architecture

### High-Level Component Diagram

```
┌────────────────────────────────────────────────────────────────────────┐
│  Application (Flutter App)                                              │
│    ChatClient   ChatConversation   ChatMessage   AttachmentUploadProgress│
└──────────────────────────┬─────────────────────────────────────────────┘
                           │ public API (Streams + Futures)
┌──────────────────────────▼─────────────────────────────────────────────┐
│  Domain Layer                                                           │
│    ChatClient (root)                                                    │
│      ├── ConversationManager (owns ChatConversation map)                │
│      ├── RealTimeEventRouter (dispatches RtEvent → conversations)       │
│      └── OtpkReplenisher (listens for low_otpk, auto-uploads)          │
└──────────────────────────┬─────────────────────────────────────────────┘
                           │
┌─────────────┬────────────▼──────────────┬──────────────────────────────┐
│  Crypto     │  Transport                │  Session                      │
│  X3dhEngine │  WsChannel               │  SessionStore (abstract)      │
│  RatchetEngine│  RestClient (dio)       │  FlutterSecureSessionStore    │
│  SenderKeyEngine│ ConnectionManager     │  InMemorySessionStore         │
│  KeyGenerator│  (reconnect + backoff)   │                               │
└─────────────┴───────────────────────────┴──────────────────────────────┘
```

### Package Structure

```
lib/
  rust_e2e_chat_sdk.dart          ← barrel export
  src/
    client/
      chat_client.dart            ← ChatClient class
      chat_client_config.dart     ← ChatClientConfig
      connection_state.dart       ← ConnectionState enum
    conversations/
      chat_conversation.dart      ← ChatConversation class
      chat_message.dart           ← ChatMessage, ChatMessageType
      conversation_member.dart    ← ConversationMember
    crypto/
      key_generator.dart          ← IdentityKey, SPK, OTPK generation
      x3dh_engine.dart            ← X3DH initiator + responder
      ratchet_engine.dart         ← Double Ratchet
      sender_key_engine.dart      ← Sender Key protocol
      crypto_types.dart           ← Curve25519KeyPair, Ed25519Sig, etc.
    transport/
      rest_client.dart            ← dio-based typed API client
      ws_channel.dart             ← WebSocket wrapper + reconnect
      rt_event.dart               ← RtEvent sealed class
      connection_manager.dart     ← exponential backoff reconnect loop
    session/
      session_store.dart          ← SessionStore abstract interface
      flutter_secure_session_store.dart
      in_memory_session_store.dart
      device_record.dart          ← DeviceRecord (stored private keys)
      ratchet_state.dart          ← RatchetState (serializable)
      sender_key_record.dart      ← SenderKeyRecord
    errors/
      sdk_error.dart              ← sealed SdkError hierarchy
    attachments/
      attachment_client.dart      ← upload + download helpers
      attachment_upload_progress.dart
    utils/
      logger.dart                 ← configurable log sink
      retry.dart                  ← exponential backoff utility
test/
  crypto/
    x3dh_test.dart
    ratchet_test.dart
    sender_key_test.dart
  transport/
    rest_client_test.dart
    ws_channel_test.dart
  session/
    session_store_test.dart
  client/
    chat_client_test.dart
example/
  lib/
    main.dart                     ← minimal demo: init, 1:1 chat, group
```

---

## Components and Interfaces

### 0. Public Entry Point — `ChatClient`

```dart
class ChatClient {
  // Factory constructor — async initialization
  static Future<ChatClient> initialize(ChatClientConfig config);

  // Identity
  String get userId;
  String get deviceId;
  Future<KeyBundle> getPublicKeyBundle();

  // Connection
  Stream<ConnectionState> get connectionState;
  Stream<SdkError> get connectionErrors;
  Stream<SdkError> get storageErrors;
  Stream<String> get warnings;
  Future<void> connect();
  Future<void> disconnect();
  Future<void> updateToken(String newAccessToken);

  // Conversations
  Stream<List<ChatConversation>> get conversations;
  Future<ChatConversation> openConversation(String recipientUserId);
  Future<ChatConversation> createGroup(List<String> memberUserIds);
  Future<ChatConversation?> findConversation(String conversationId);

  // Lifecycle
  Future<void> dispose();
}
```

### 1. Configuration — `ChatClientConfig`

```dart
class ChatClientConfig {
  final String baseUrl;          // e.g. "http://localhost:3000/api"
  final String accessToken;      // OIDC JWT
  final String userId;           // OIDC sub claim
  final String? deviceId;        // null → auto-register
  final SessionStore? sessionStore; // null → platform default
  final bool autoConnect;        // default: true
  final int signedPrekeyRotationDays; // default: 7
  final LogLevel logLevel;       // default: LogLevel.warning
}
```

### 2. Conversation — `ChatConversation`

```dart
abstract class ChatConversation {
  String get conversationId;
  ConversationType get type; // oneToOne | group
  List<ConversationMember> get currentMembers;

  Stream<List<ChatMessage>> get messages;
  Stream<List<ConversationMember>> get members;
  Stream<int> get unreadCount;
  Stream<AttachmentUploadProgress?> get uploadProgress;

  Future<ChatMessage> sendMessage(String text);
  Future<ChatMessage> sendAttachment(
      Uint8List bytes, String filename, String contentType);
  Future<List<ChatMessage>> fetchHistory({int limit = 50, int? beforeSeq});
  Future<void> markAsRead();

  // Group only
  Future<void> addMember(String userId);
  Future<void> removeMember(String userId);
}
```

### 3. Message — `ChatMessage`

```dart
class ChatMessage {
  final String id;                  // local UUID
  final String conversationId;
  final String senderUserId;
  final String? senderDeviceId;
  final ChatMessageType type;       // text | attachment | memberAdded | memberRemoved | system
  final String? text;               // null if attachment-only or decryptionError
  final String? attachmentUrl;      // authenticated download URL
  final String? attachmentName;
  final DateTime timestamp;
  final int seq;
  final bool isMine;
  final bool decryptionError;       // true if decryption failed
}
```

### 4. Crypto Layer

#### 4.1 Key Generator (`key_generator.dart`)

```dart
class KeyGenerator {
  // Generates IdentityKey (Curve25519), SignedPreKey (Curve25519),
  // Ed25519 signature of SPK, and N OneTimePreKeys.
  static Future<PrivateKeyBundle> generateKeyBundle({int otpkCount = 50});

  // Generates a single batch of OTPKs for replenishment.
  static Future<List<OneTimeKeyPair>> generateOneTimePreKeys(int count);

  // Generates a new SenderKey for group use.
  static Future<SenderKeyPair> generateSenderKey();
}
```

#### 4.2 X3DH Engine (`x3dh_engine.dart`)

```dart
class X3dhEngine {
  // Initiator: given recipient's public KeyBundle and sender's private bundle,
  // compute the shared secret and produce the X3DH InitHeader.
  static Future<X3dhInitResult> performX3dh({
    required KeyBundleResponse recipientBundle,
    required PrivateKeyBundle senderBundle,
  });

  // Responder: given the received X3DH header and own private bundle,
  // derive the same shared secret.
  static Future<Uint8List> deriveSharedSecret({
    required X3dhInitHeader header,
    required PrivateKeyBundle recipientBundle,
  });
}

class X3dhInitResult {
  final Uint8List sharedSecret;
  final X3dhInitHeader header; // sent with first message
}
```

#### 4.3 Ratchet Engine (`ratchet_engine.dart`)

```dart
class RatchetEngine {
  // Initialize Double Ratchet from X3DH shared secret (sender side).
  static RatchetSession initSender(Uint8List sharedSecret, Curve25519PublicKey recipientDhKey);

  // Initialize Double Ratchet from X3DH shared secret (receiver side).
  static RatchetSession initReceiver(Uint8List sharedSecret, Curve25519KeyPair localDhPair);

  // Encrypt plaintext, advancing the ratchet.
  static Future<RatchetCiphertext> encrypt(RatchetSession session, Uint8List plaintext);

  // Decrypt ciphertext, advancing or skipping the ratchet.
  static Future<Uint8List> decrypt(RatchetSession session, RatchetCiphertext ciphertext);
}
```

#### 4.4 Sender Key Engine (`sender_key_engine.dart`)

```dart
class SenderKeyEngine {
  // Create a new SenderKey session for the local user in a group.
  static SenderKeySession createSession(SenderKeyPair keyPair);

  // Encrypt a group message with the local SenderKey.
  static Future<Uint8List> encrypt(SenderKeySession session, Uint8List plaintext);

  // Decrypt a group message using the sender's SenderKey.
  static Future<Uint8List> decrypt(SenderKeySession session, Uint8List ciphertext);

  // Serialize/deserialize for SKDM distribution.
  static Uint8List serializeKeyMaterial(SenderKeySession session);
  static SenderKeySession deserializeKeyMaterial(Uint8List bytes);
}
```

### 5. Transport Layer

#### 5.1 REST Client (`rest_client.dart`)

Typed wrapper over `dio`. Each method maps to exactly one API endpoint.

```dart
class RestClient {
  RestClient({required String baseUrl, required String Function() tokenProvider});

  // KDS
  Future<RegisterDeviceResponse> registerDevice(String userId, KeyBundle bundle);
  Future<KeyBundleResponse> getKeyBundle(String userId);
  Future<ReplenishOtpksResponse> replenishOtpks(String userId, String deviceId, List<OneTimePreKey> keys);
  Future<void> rotateSignedPreKey(String userId, String deviceId, SignedPreKeyUpdate update);

  // Conversations
  Future<CreateConversationResponse> createConversation(CreateConversationRequest req);
  Future<SendMessageResponse> sendMessage(String conversationId, MessageEnvelopeRequest req);
  Future<GetMessagesResponse> getMessages(String conversationId, {int limit = 50, int? beforeSeq});

  // Groups
  Future<CreateGroupResponse> createGroup(CreateGroupRequest req);
  Future<void> sendGroupMessage(String conversationId, MessageEnvelopeRequest req);
  Future<void> addGroupMember(String conversationId, String userId, String deviceId);
  Future<void> removeGroupMember(String conversationId, String userId);
  Future<void> distributeGroupSenderKey(String conversationId, List<SkdmRecipient> recipients);

  // Attachments
  Future<UploadResponse> uploadAttachment(Uint8List bytes, String filename, String contentType,
      {void Function(int sent, int total)? onProgress});
}
```

#### 5.2 WebSocket Channel (`ws_channel.dart`)

```dart
class WsChannel {
  // Broadcast stream of decoded RtEvent frames from the server.
  Stream<RtEvent> get events;

  // Connection lifecycle
  Future<void> connect(String wsUrl);
  Future<void> disconnect();

  // Send an ack for a received message.
  Future<void> sendAck(String conversationId, int seq);

  // Respond to server ping.
  Future<void> sendPong();
}
```

#### 5.3 Connection Manager (`connection_manager.dart`)

Manages reconnect lifecycle with exponential backoff.

```dart
class ConnectionManager {
  Stream<ConnectionState> get state;
  Future<void> ensureConnected();
  Future<void> disconnect();
  // Internal: called by WsChannel on disconnect
  void _onDisconnect(Object? error);
}
```

Backoff schedule: 1s → 2s → 4s → 8s → 16s → 32s → 60s (cap).

### 6. Session Layer

#### 6.1 `SessionStore` interface

```dart
abstract class SessionStore {
  Future<void> saveDevice(DeviceRecord record);
  Future<DeviceRecord?> loadDevice(String userId, String deviceId);
  Future<void> saveRatchetState(String conversationId, RatchetState state);
  Future<RatchetState?> loadRatchetState(String conversationId);
  Future<void> saveSenderKey(String conversationId, String userId, SenderKeyRecord record);
  Future<SenderKeyRecord?> loadSenderKey(String conversationId, String userId);
  Future<void> saveConversationMeta(ConversationMeta meta);
  Future<List<ConversationMeta>> loadAllConversations();
  Future<void> clear();
}
```

#### 6.2 Storage Keys (FlutterSecureSessionStore)

All values are JSON-serialized and stored with prefixed keys:

| Key pattern | Value |
|---|---|
| `rce_device_{userId}_{deviceId}` | `DeviceRecord` JSON |
| `rce_ratchet_{conversationId}` | `RatchetState` JSON |
| `rce_senderkey_{conversationId}_{userId}` | `SenderKeyRecord` JSON |
| `rce_conv_{conversationId}` | `ConversationMeta` JSON |

### 7. Real-Time Event Router

```dart
sealed class RtEvent {
  // Deserialized from WebSocket JSON frames
}

class MessageRtEvent extends RtEvent {
  final String conversationId;
  final int seq;
  final String senderUserId;
  final String senderDeviceId;
  final Uint8List ciphertext;
  final Map<String, dynamic> protocolHeader;
  final int serverTs;
  final String? attachmentId;
}

class LowOtpkRtEvent extends RtEvent {
  final String deviceId;
  final int count;
}

class MemberAddedRtEvent extends RtEvent {
  final String conversationId;
  final String userId;
  final List<String> devices;
}

class MemberRemovedRtEvent extends RtEvent {
  final String conversationId;
  final String userId;
}

class SenderKeyDistributionRtEvent extends RtEvent {
  final String conversationId;
  final String senderUserId;
  final Uint8List encryptedSkdm;
}
```

The `RealTimeEventRouter` subscribes to `WsChannel.events` and dispatches to:
- `ConversationManager` → appropriate `ChatConversation` for message / SKDM events
- `OtpkReplenisher` → for `LowOtpkRtEvent`

### 8. Error Hierarchy

```dart
sealed class SdkError implements Exception {
  const SdkError();
}

class NetworkError extends SdkError {
  final int? statusCode;
  final String message;
}

class AuthError extends SdkError {
  final String message;
}

class DecryptionError extends SdkError {
  final String conversationId;
  final int seq;
}

class KeyExchangeError extends SdkError {
  final String recipientUserId;
  final String reason;
}

class StorageError extends SdkError {
  final String reason;
}

class SessionNotFoundError extends SdkError {
  final String deviceId;
}

class FileTooLargeError extends SdkError {
  final int sizeBytes;
}

class UnknownError extends SdkError {
  final Object cause;
}
```

Server `error_code` → `SdkError` mapping:

| Server error_code | SdkError subtype |
|---|---|
| `unauthorized` | `AuthError` |
| `forbidden` | `AuthError` |
| `unknown_tenant` | `AuthError` |
| `tenant_inactive` | `AuthError` |
| `not_found` | `NetworkError(404, ...)` |
| `device_limit_reached` | `NetworkError(409, ...)` |
| `invalid_signed_prekey_signature` | `KeyExchangeError` |
| `storage_unavailable` | `NetworkError(503, ...)` |
| `bad_request` | `NetworkError(400, ...)` |
| `internal_error` | `NetworkError(500, ...)` |

---

## Data Models

### `DeviceRecord` (stored in SessionStore)

```dart
class DeviceRecord {
  final String userId;
  final String deviceId;
  final Uint8List identityKeyPrivate;   // raw bytes
  final Uint8List identityKeyPublic;
  final Uint8List signedPrekeyPrivate;
  final Uint8List signedPrekeyPublic;
  final int signedPrekeyId;
  final List<OtpkRecord> oneTimePrekeys; // remaining unconsumed private OTPKs
  final DateTime signedPrekeyCreatedAt;  // for rotation scheduling
}
```

### `RatchetState` (stored in SessionStore)

```dart
class RatchetState {
  final String conversationId;
  final Uint8List rootKey;
  final Uint8List chainKeySend;
  final Uint8List chainKeyRecv;
  final Uint8List dhSendPub;
  final Uint8List dhSendPriv;
  final Uint8List dhRecvPub;
  final int nSend;   // message counter send chain
  final int nRecv;   // message counter recv chain
  final int pn;      // previous send chain length
  final Map<int, Uint8List> skippedMessageKeys; // for out-of-order messages
}
```

### `SenderKeyRecord` (stored in SessionStore)

```dart
class SenderKeyRecord {
  final String conversationId;
  final String userId;           // owner of this key
  final Uint8List chainKey;
  final int iteration;
  final Uint8List signingKeyPublic;
  final Uint8List? signingKeyPrivate; // only for own key
}
```

---

## Initialization Sequence

```
ChatClient.initialize(config)
  │
  ├── 1. Create RestClient(baseUrl, tokenProvider)
  ├── 2. Load or create SessionStore (platform detection)
  ├── 3. If deviceId == null:
  │       a. KeyGenerator.generateKeyBundle()
  │       b. RestClient.registerDevice(userId, bundle)
  │       c. SessionStore.saveDevice(DeviceRecord)
  │       d. Set deviceId from response
  │   Else:
  │       a. SessionStore.loadDevice(userId, deviceId)
  │       b. If null → throw SessionNotFoundError
  ├── 4. Create WsChannel + ConnectionManager
  ├── 5. Create RealTimeEventRouter
  ├── 6. Create ConversationManager (loads all conversations from SessionStore)
  ├── 7. Create OtpkReplenisher
  ├── 8. If autoConnect → ConnectionManager.ensureConnected()
  └── 9. Return ChatClient
```

## Message Send Sequence (1:1)

```
chatConversation.sendMessage("Hello")
  │
  ├── 1. RatchetEngine.encrypt(ratchetSession, utf8.encode("Hello"))
  │       → RatchetCiphertext { ciphertext, header: { dh, n, pn } }
  ├── 2. Build MessageEnvelopeRequest { conversationId, ciphertext, protocolHeader }
  ├── 3. RestClient.sendMessage(conversationId, request)
  │       → SendMessageResponse { seq, serverTs }
  ├── 4. SessionStore.saveRatchetState(conversationId, updatedSession)
  ├── 5. Build ChatMessage { text: "Hello", seq, timestamp, isMine: true }
  ├── 6. Emit updated messages list to Stream
  └── 7. Return ChatMessage
```

## Message Receive Sequence (WebSocket)

```
WsChannel emits MessageRtEvent
  │
  ├── RealTimeEventRouter.dispatch(event)
  ├── ConversationManager.findOrCreate(conversationId)
  ├── RatchetEngine.decrypt(ratchetSession, ciphertext, header)
  │     → plaintext bytes or throws DecryptionError
  ├── SessionStore.saveRatchetState(conversationId, updatedSession)
  ├── Build ChatMessage { text: utf8.decode(plaintext), seq, ... }
  ├── Emit updated messages list to Stream
  └── WsChannel.sendAck(conversationId, seq)
```

---

## Dependencies

| Package | Version | Purpose |
|---|---|---|
| `dio` | ^5.0.0 | HTTP client with interceptors and multipart support |
| `web_socket_channel` | ^3.0.0 | WebSocket abstraction (dart:io + web) |
| `flutter_secure_storage` | ^9.0.0 | Platform keychain/keystore storage |
| `cryptography` | ^2.7.0 | Curve25519 DH, Ed25519 signing, AES-GCM, HKDF |
| `uuid` | ^4.0.0 | UUID generation |
| `json_annotation` | ^4.8.0 | JSON serialization code generation |
| `build_runner` | ^2.4.0 | Code generation |
| `mocktail` | ^1.0.0 | Mocking in tests |

---

## Platform Support

| Platform | Session Store | Crypto | WebSocket |
|---|---|---|---|
| Android | `flutter_secure_storage` | `cryptography` | `web_socket_channel` |
| iOS | `flutter_secure_storage` | `cryptography` | `web_socket_channel` |
| macOS | `flutter_secure_storage` | `cryptography` | `web_socket_channel` |
| Windows | `flutter_secure_storage` | `cryptography` | `web_socket_channel` |
| Linux | `flutter_secure_storage` | `cryptography` | `web_socket_channel` |
| Web | `InMemorySessionStore` (no secure storage) | `cryptography` | `web_socket_channel` |
| Dart server | `InMemorySessionStore` | `cryptography` | `web_socket_channel` |

> **Web note:** `flutter_secure_storage` does not support web. On web, a warning is emitted and `InMemorySessionStore` is used. Persistent web storage (IndexedDB with encryption) is a future enhancement.

---

## Correctness Properties

All 6 correctness properties from `requirements.md` are enforced at these implementation points:

| Property | Enforced in |
|---|---|
| Zero-Knowledge Transport | `ratchet_engine.dart` — ciphertext only; `rest_client.dart` — body never logged |
| Session Continuity | `session_store.dart` — ratchet state persisted before `sendMessage` returns |
| OTPK Depletion Resilience | `x3dh_engine.dart` — `one_time_prekey` is optional in `KeyBundleResponse` |
| Forward Secrecy Preservation | `ratchet_engine.dart` — state saved atomically via `SessionStore.saveRatchetState` before returning |
| Sender Key Isolation | `sender_key_engine.dart` + `ConversationManager` — new key generated before removal event completes |
| Token Hot-Swap | `rest_client.dart` — `tokenProvider` closure called per request; `ws_channel.dart` — reconnect uses current token |
