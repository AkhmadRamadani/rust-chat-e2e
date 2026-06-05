# Implementation Plan: flutter-e2e-chat-sdk

## Overview

Incremental implementation of a Dart/Flutter SDK for the `rust-e2e-chat-api` platform. The SDK provides: X3DH + Double Ratchet 1:1 encryption, Sender Key group encryption, WebSocket real-time delivery with auto-reconnect, REST API typed client, secure local session storage, and a developer-friendly reactive (Stream-based) public API.

---

## Tasks

- [ ] 0. Package scaffold and shared types
  - [ ] 0.1 Initialize Dart package `rust_e2e_chat_sdk` with `pubspec.yaml`
    - Dependencies: `dio`, `web_socket_channel`, `flutter_secure_storage`, `cryptography`, `uuid`, `json_annotation`
    - Dev dependencies: `build_runner`, `json_serializable`, `mocktail`, `test`, `flutter_test`
    - _Requirements: 10.3_
  - [ ] 0.2 Create barrel export file `lib/rust_e2e_chat_sdk.dart`
    - Exports all public classes; nothing from `src/` is public except through this file
    - _Requirements: 10.3_
  - [ ] 0.3 Define all shared crypto value types in `src/crypto/crypto_types.dart`
    - `Curve25519KeyPair`, `Ed25519KeyPair`, `OneTimeKeyPair`, `OtpkRecord`, `PrivateKeyBundle`, `KeyBundle`, `SignedPreKeyUpdate`, `X3dhInitHeader`, `RatchetCiphertext`, `SenderKeyPair`
    - _Requirements: 2.1, 3.3_
  - [ ] 0.4 Define API request/response DTOs in `src/transport/api_models.dart`
    - `RegisterDeviceRequest`, `RegisterDeviceResponse`, `KeyBundleResponse`, `CreateConversationRequest/Response`, `SendMessageRequest/Response`, `CreateGroupRequest/Response`, `UploadResponse`, etc.
    - All DTOs annotated with `@JsonSerializable()` and generated via `build_runner`
    - _Requirements: 0.1, 0.3_

- [ ] 1. Error hierarchy
  - [ ] 1.1 Implement sealed class `SdkError` and all subtypes in `src/errors/sdk_error.dart`
    - `NetworkError`, `AuthError`, `DecryptionError`, `KeyExchangeError`, `StorageError`, `SessionNotFoundError`, `FileTooLargeError`, `UnknownError`
    - _Requirements: 9.1_
  - [ ] 1.2 Implement server error_code → SdkError mapping utility
    - `SdkError.fromApiResponse(int statusCode, String? errorCode, String message) → SdkError`
    - _Requirements: 9.3_

- [ ] 2. Session store
  - [ ] 2.1 Define `SessionStore` abstract interface in `src/session/session_store.dart`
    - `saveDevice`, `loadDevice`, `saveRatchetState`, `loadRatchetState`, `saveSenderKey`, `loadSenderKey`, `saveConversationMeta`, `loadAllConversations`, `clear`
    - _Requirements: 7.1_
  - [ ] 2.2 Implement `InMemorySessionStore` in `src/session/in_memory_session_store.dart`
    - All storage in `Map` fields; `clear()` resets all maps
    - _Requirements: 7.3_
  - [ ] 2.3 Implement `FlutterSecureSessionStore` in `src/session/flutter_secure_session_store.dart`
    - Uses `flutter_secure_storage`; all values JSON-serialized with prefixed keys (`rce_device_`, `rce_ratchet_`, etc.)
    - Wraps storage exceptions and emits through `storageErrors` stream
    - _Requirements: 7.2, 7.4_
  - [ ] 2.4 Implement platform detection helper
    - Returns `FlutterSecureSessionStore` on iOS/Android/Desktop, `InMemorySessionStore` on web/server
    - Emits warning log on web
    - _Requirements: 7.2, 7.3_
  - [ ] 2.5 Write unit tests for both store implementations
    - Round-trip save/load for all record types; test `SessionNotFoundError`
    - _Requirements: 7.5_

- [ ] 3. Checkpoint — session layer

- [ ] 4. Cryptographic key generation
  - [ ] 4.1 Implement `KeyGenerator.generateKeyBundle()` in `src/crypto/key_generator.dart`
    - Generate Curve25519 IdentityKey pair using `cryptography` package
    - Generate Curve25519 SignedPreKey pair
    - Compute Ed25519 signature of SPK public key using IdentityKey's Ed25519 signing key
    - Generate 50 OneTimePreKey pairs
    - _Requirements: 2.1, 2.2_
  - [ ] 4.2 Implement `KeyGenerator.generateOneTimePreKeys(int count)`
    - Used for replenishment uploads
    - _Requirements: 2.4_
  - [ ] 4.3 Implement `KeyGenerator.generateSenderKey()`
    - Generates a SenderKey chain key and signing key pair
    - _Requirements: 5.1_
  - [ ] 4.4 Implement SPK signature self-verification
    - SDK verifies its own generated signature before submitting to server
    - _Requirements: 2.3_
  - [ ] 4.5 Write unit tests for key generation
    - SPK signature verifies correctly; OTPK pairs are unique; SenderKey round-trips

- [ ] 5. X3DH implementation
  - [ ] 5.1 Implement `X3dhEngine.performX3dh()` (initiator side) in `src/crypto/x3dh_engine.dart`
    - Ephemeral key generation, DH computations (IK⊗SPK, EK⊗IK, EK⊗SPK, EK⊗OTPK), HKDF → 32-byte root key
    - Populates `X3dhInitHeader` with `ek`, `spk_id`, `otpk_id`
    - _Requirements: 3.1, 5.2_
  - [ ] 5.2 Implement `X3dhEngine.deriveSharedSecret()` (responder side)
    - Mirror of initiator: same DH computation order using own private keys
    - _Requirements: 5.2_
  - [ ] 5.3 Write X3DH interoperability tests
    - Initiator and responder derive identical 32-byte secrets
    - Test with and without OTPK (depleted pool case)
    - _Requirements: 3.4 (Property 3)_

- [ ] 6. Double Ratchet implementation
  - [ ] 6.1 Implement `RatchetEngine.initSender()` and `RatchetEngine.initReceiver()` in `src/crypto/ratchet_engine.dart`
    - Initialize root key, send/recv chain keys, DH ratchet state from X3DH shared secret
    - _Requirements: 4.1_
  - [ ] 6.2 Implement `RatchetEngine.encrypt(session, plaintext)`
    - Advance chain key → message key via HKDF; encrypt with AES-256-GCM
    - Return updated `RatchetState` + `RatchetCiphertext`
    - _Requirements: 4.1_
  - [ ] 6.3 Implement `RatchetEngine.decrypt(session, ciphertext)`
    - Perform DH ratchet step if new DH key; derive chain key → message key; decrypt AES-256-GCM
    - Handle skipped message keys (store up to 2000 per session)
    - Throw `DecryptionError` on authentication failure
    - _Requirements: 4.7_
  - [ ] 6.4 Implement `RatchetState` serialization/deserialization to/from JSON
    - All `Uint8List` fields base64-encoded; `skippedMessageKeys` serialized as `{seq: base64}`
    - _Requirements: 4.4_
  - [ ] 6.5 Write Double Ratchet tests
    - Bidirectional message exchange; out-of-order messages; session restore from serialized state
    - _Requirements: 4.4, 4.5 (Property 4)_

- [ ] 7. Sender Key implementation
  - [ ] 7.1 Implement `SenderKeyEngine.createSession()` in `src/crypto/sender_key_engine.dart`
    - Initialize chain key from SenderKeyPair; store iteration counter
    - _Requirements: 5.1_
  - [ ] 7.2 Implement `SenderKeyEngine.encrypt(session, plaintext)`
    - Advance chain key; encrypt with AES-256-GCM; sign with Ed25519 signing key
    - _Requirements: 5.3_
  - [ ] 7.3 Implement `SenderKeyEngine.decrypt(session, ciphertext)`
    - Verify Ed25519 signature; advance chain key to match iteration; decrypt
    - Throw `DecryptionError` on signature or decryption failure
    - _Requirements: 5.3_
  - [ ] 7.4 Implement `serializeKeyMaterial` / `deserializeKeyMaterial` for SKDM payloads
    - The serialized blob is then encrypted by the Double Ratchet for transit
    - _Requirements: 5.2_
  - [ ] 7.5 Write Sender Key tests
    - Multi-member encryption; iteration advance; signature verification; key isolation after member removal

- [ ] 8. Checkpoint — crypto layer complete

- [ ] 9. REST transport layer
  - [ ] 9.1 Implement `RestClient` in `src/transport/rest_client.dart`
    - Wraps `dio`; uses `tokenProvider` closure for `Authorization: Bearer` header
    - Interceptor maps non-2xx responses to `SdkError` via `SdkError.fromApiResponse`
    - Retry interceptor: 3 attempts on 5xx and network errors with linear 500ms backoff
    - _Requirements: 9.2, 1.7_
  - [ ] 9.2 Implement all KDS endpoint methods
    - `registerDevice`, `getKeyBundle`, `replenishOtpks`, `rotateSignedPreKey`
    - _Requirements: 0.3, 0.4, 2.4, 2.5_
  - [ ] 9.3 Implement all conversation endpoint methods
    - `createConversation`, `sendMessage`, `getMessages`
    - _Requirements: 4.1, 4.6_
  - [ ] 9.4 Implement all group endpoint methods
    - `createGroup`, `sendGroupMessage`, `addGroupMember`, `removeGroupMember`, `distributeGroupSenderKey`
    - _Requirements: 5.1, 5.6_
  - [ ] 9.5 Implement attachment endpoint methods with progress callbacks
    - `uploadAttachment` using `dio` `FormData` + `onSendProgress`
    - _Requirements: 6.1, 6.2_
  - [ ] 9.6 Write REST client tests using `mocktail` + `dio_test`
    - Auth header injection; error mapping; retry on 503; multipart upload progress

- [ ] 10. WebSocket transport layer
  - [ ] 10.1 Implement `WsChannel` in `src/transport/ws_channel.dart`
    - `web_socket_channel` wrapping; frame parsing → `RtEvent` sealed class
    - Responds to `{"type":"ping"}` with `{"type":"pong"}` automatically
    - `sendAck(conversationId, seq)` sends `{"conversation_id":"...","seq":N}`
    - _Requirements: 1.2, 1.6_
  - [ ] 10.2 Implement `ConnectionManager` with exponential backoff
    - Reconnect schedule: 1s → 2s → 4s → 8s → 16s → 32s → 60s cap
    - Emits `ConnectionState` values on state transitions
    - On reconnect: reconstructs WS URL with current token via `tokenProvider`
    - _Requirements: 1.3, 1.5_
  - [ ] 10.3 Implement `RtEvent` sealed class with all 5 subtypes
    - Parses JSON frames; unknown type fields are ignored (forward compatibility)
    - _Requirements: 8.1–8.4_
  - [ ] 10.4 Write WebSocket channel tests
    - Ping/pong; ack frame format; RtEvent parsing; reconnect backoff timing

- [ ] 11. OTPK replenisher
  - [ ] 11.1 Implement `OtpkReplenisher` in `src/client/otpk_replenisher.dart`
    - Subscribes to `low_otpk` RtEvent from the router
    - On receive: generates 50 new OTPKs, uploads, updates `DeviceRecord` in `SessionStore`
    - _Requirements: 2.4_

- [ ] 12. SPK rotation scheduler
  - [ ] 12.1 Implement periodic SPK rotation in `src/client/spk_rotator.dart`
    - Checks `DeviceRecord.signedPrekeyCreatedAt` vs `signedPrekeyRotationDays` config
    - On trigger: generates new SPK + signature, uploads via `rotateSignedPreKey`, updates `SessionStore`
    - _Requirements: 2.5_

- [ ] 13. Checkpoint — transport + automation layer

- [ ] 14. Domain layer — ChatConversation
  - [ ] 14.1 Implement `OneToOneConversation` in `src/conversations/`
    - Implements `ChatConversation` interface
    - `sendMessage`: encrypt → REST → persist ratchet state → emit to stream
    - `fetchHistory`: REST → decrypt each envelope → emit to stream
    - `unreadCount`: increments on incoming, resets on `markAsRead()`
    - _Requirements: 4.1, 4.2, 4.6, 8.4_
  - [ ] 14.2 Implement `GroupConversation` in `src/conversations/`
    - `sendMessage`: SenderKey encrypt → REST → emit to stream
    - `addMember`: REST + re-key + new SKDM distribution to new member
    - `removeMember`: REST + key rotation + new SKDM distribution to all remaining
    - `members` stream updated on `member_added` / `member_removed` RtEvents
    - _Requirements: 5.3, 5.4, 5.5, 5.6_
  - [ ] 14.3 Implement `ConversationManager` in `src/client/conversation_manager.dart`
    - Owns a `Map<String, ChatConversation>` cache
    - `getOrCreate(conversationId)` restores from `SessionStore` if needed
    - Loads all conversations from `SessionStore` at init
    - Emits to `conversations` broadcast stream on any mutation
    - _Requirements: 3.2, 3.5, 8.1_
  - [ ] 14.4 Implement `RealTimeEventRouter` in `src/client/rt_event_router.dart`
    - Subscribes to `WsChannel.events`
    - Routes `MessageRtEvent` → `ConversationManager.findOrCreate(conversationId).onIncomingMessage`
    - Routes `SenderKeyDistributionRtEvent` → `GroupConversation.onSkdm`
    - Routes `MemberAddedRtEvent` / `MemberRemovedRtEvent` → `GroupConversation.onMemberChange`
    - Routes `LowOtpkRtEvent` → `OtpkReplenisher`
    - _Requirements: 5.2, 5.4, 5.5, 8.1–8.4_

- [ ] 15. Domain layer — ChatClient
  - [ ] 15.1 Implement `ChatClient.initialize()` in `src/client/chat_client.dart`
    - Full initialization sequence: store setup → device registration or restore → ws connect
    - Wires all components: `RestClient`, `WsChannel`, `ConnectionManager`, `RtEventRouter`, `ConversationManager`, `OtpkReplenisher`, `SpkRotator`
    - _Requirements: 0.1, 0.2, 0.3, 0.4, 0.5_
  - [ ] 15.2 Implement `ChatClient.openConversation(recipientUserId)`
    - Fetch key bundle → X3DH → create conversation → persist state → return `OneToOneConversation`
    - Cache check: return existing `ChatConversation` if already open
    - _Requirements: 3.1, 3.2, 3.3, 3.4, 3.5_
  - [ ] 15.3 Implement `ChatClient.createGroup(memberUserIds)`
    - Create group on server → generate SenderKey → fetch all member key bundles → distribute SKDM
    - _Requirements: 5.1_
  - [ ] 15.4 Implement `ChatClient.updateToken(newToken)` and `ChatClient.dispose()`
    - `updateToken`: updates `tokenProvider` closure; WS reconnects with new token on next connect
    - `dispose`: disconnect WS, cancel timers, close all stream controllers
    - _Requirements: 0.6, 0.7 (Property 6)_

- [ ] 16. Checkpoint — full domain layer

- [ ] 17. Example app
  - [ ] 17.1 Create `example/` Flutter app in `example/`
    - Screen 1: Initialize SDK with base URL + token input
    - Screen 2: Open 1:1 conversation + send/receive messages
    - Screen 3: Create group + send messages
    - _Requirements: 10.5_
  - [ ] 17.2 Wire connection state indicator in example app UI

- [ ] 18. Testing library
  - [ ] 18.1 Create `lib/testing.dart` barrel exposing `MockChatClient`
    - Implement `MockChatClient` extending `ChatClient` interface using `mocktail`
    - `simulateIncomingMessage(conversationId, senderUserId, text)` injects a decrypted `ChatMessage` into the target conversation stream
    - `simulateConnectionState(ConnectionState)` emits to the connection state stream
    - _Requirements: 10.1, 10.2_

- [ ] 19. Documentation
  - [ ] 19.1 Add `///` doc comments to all public classes and methods
    - Include `@example` usage in `ChatClient`, `ChatConversation`, `ChatMessage`
    - _Requirements: 10.4_
  - [ ] 19.2 Write `README.md` with quick-start guide
    - Installation; 5-line initialization example; 1:1 messaging example; group example

- [ ] 20. Final checkpoint
  - `dart test` passes across all test files
  - `dart analyze` reports no issues
  - Example app builds and runs on iOS Simulator + Android Emulator
  - `openConversation` → `sendMessage` → receive via WebSocket round-trip verified against live `rust-e2e-chat-api` stack

---

## Notes

- **Crypto primitives**: Use the `cryptography` package (pure Dart, supports all platforms). `Cryptography.instance.x25519()` for DH, `Ed25519()` for signing, `AesGcm.with256bits()` for symmetric encryption, `Hkdf(hmac: Hmac.sha256(), outputLength: 32)` for key derivation.
- **Key encoding**: All key bytes exchanged with the server are base64url-encoded (no padding). Use `base64Url.encode(bytes)`.
- **OTPK tracking**: The `DeviceRecord` stores private OTPKs by ID. When the server consumes an OTPK during key bundle fetch (from the recipient's perspective), the SDK does not need to track which OTPK was consumed — that is managed server-side. The SDK only tracks its own remaining uploaded private OTPKs.
- **Message ordering**: `messages` streams always emit the full list sorted by `seq` ascending. `fetchHistory` pages backward using `before_seq`.
- **Skipped message keys**: Store in `RatchetState.skippedMessageKeys` with a cap of 2000 entries to prevent unbounded memory growth.
- **Web platform**: `flutter_secure_storage` is not supported on web. On web, fall back to `InMemorySessionStore` and log a warning. IndexedDB-backed encrypted storage is a future enhancement.
- **No plaintext logging**: The `Logger` utility must strip all `ChatMessage.text` and raw key bytes before writing to `dart:developer`. Use `[REDACTED]` in log lines touching sensitive fields.

---

## Task Dependency Graph

```json
{
  "waves": [
    { "id": 0, "tasks": ["0.1", "0.2", "0.3", "0.4"] },
    { "id": 1, "tasks": ["1.1", "1.2"] },
    { "id": 2, "tasks": ["2.1", "2.2", "2.3", "2.4", "2.5"] },
    { "id": 3, "tasks": ["4.1", "4.2", "4.3", "4.4", "4.5"] },
    { "id": 4, "tasks": ["5.1", "5.2", "5.3"] },
    { "id": 5, "tasks": ["6.1", "6.2", "6.3", "6.4", "6.5"] },
    { "id": 6, "tasks": ["7.1", "7.2", "7.3", "7.4", "7.5"] },
    { "id": 7, "tasks": ["9.1", "9.2", "9.3", "9.4", "9.5", "9.6"] },
    { "id": 8, "tasks": ["10.1", "10.2", "10.3", "10.4"] },
    { "id": 9, "tasks": ["11.1", "12.1"] },
    { "id": 10, "tasks": ["14.1", "14.2", "14.3", "14.4"] },
    { "id": 11, "tasks": ["15.1", "15.2", "15.3", "15.4"] },
    { "id": 12, "tasks": ["17.1", "17.2", "18.1"] },
    { "id": 13, "tasks": ["19.1", "19.2"] },
    { "id": 14, "tasks": ["20"] }
  ]
}
```
