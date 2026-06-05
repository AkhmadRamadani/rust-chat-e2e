# Implementation Plan: web-e2e-chat-sdk

## Overview

Incremental implementation of a framework-agnostic TypeScript SDK (`@rust-e2e-chat/sdk`) for the `rust-e2e-chat-api` platform. The SDK provides: X3DH + Double Ratchet 1:1 encryption, Sender Key group encryption, typed `fetch`-based REST client, WebSocket real-time delivery with auto-reconnect, pluggable session storage (IndexedDB / Memory / Node filesystem), and an event-driven public API. Zero mandatory runtime dependencies; all crypto via `crypto.subtle`.

---

## Tasks

- [ ] 0. Package scaffold and shared infrastructure
  - [ ] 0.1 Initialize npm package `@rust-e2e-chat/sdk`
    - `package.json` with `"type": "module"`, dual ESM/CJS `"exports"`, `"sideEffects": false`
    - `tsconfig.json`: `"target": "ES2020"`, `"lib": ["ES2020", "DOM"]`, strict mode
    - `tsup.config.ts`: targets `esm` + `cjs` + `dts`, browser UMD via separate config
    - `vitest.config.ts` with browser-compatible environment
    - _Requirements: 10.1, 10.5_
  - [ ] 0.2 Create `TypedEventEmitter<Events>` utility in `src/utils/typed-emitter.ts`
    - Thin wrapper over Node.js-compatible `EventEmitter` pattern; fully typed generic
    - Supports `on`, `off`, `once`, `emit` with type-safe event maps
    - _Requirements: 8.1, 8.2_
  - [ ] 0.3 Create encoding utilities in `src/utils/encoding.ts`
    - `base64urlEncode(bytes: Uint8Array): string` — no padding, URL-safe alphabet
    - `base64urlDecode(str: string): Uint8Array`
    - `hexEncode` / `hexDecode` for diagnostic output
    - _Requirements: 2.1 (key format note)_
  - [ ] 0.4 Create `Logger` in `src/utils/logger.ts`
    - Configurable `logLevel`; `redact(value)` helper strips key bytes and message text
    - All log calls include a `[rce-sdk]` prefix; no plaintext ever logged
    - _Requirements: 9.4 (zero-knowledge), 0.2_
  - [ ] 0.5 Create `retry` utility in `src/utils/retry.ts`
    - `withRetry(fn, { attempts, backoffMs }): Promise<T>` — linear backoff for REST retries
    - `withExponentialBackoff(fn, { maxDelayMs }): AsyncGenerator` — for reconnect loop
    - _Requirements: 1.3_
  - [ ] 0.6 Create all API DTO types in `src/transport/api-types.ts`
    - `KeyBundleRequest/Response`, `RegisterDeviceResponse`, `CreateConversationRequest/Response`, `MessageEnvelopeRequest`, `SendMessageResponse`, `GetMessagesResponse`, `CreateGroupRequest/Response`, `SkdmRecipient`, `UploadResponse`, `ReplenishResponse`, `SignedPreKeyUpdate`
    - All `Uint8Array` fields are `string` in JSON (base64url); conversion handled in RestClient
    - _Requirements: 0.1, 0.3_

- [ ] 1. Error hierarchy
  - [ ] 1.1 Implement `SdkErrorCode` enum and `SdkError` class in `src/errors/sdk-error.ts`
    - Extends `Error`; fields: `code: SdkErrorCode`, `statusCode?: number`, `cause?: unknown`
    - _Requirements: 9.1, 9.2_
  - [ ] 1.2 Implement `SdkError.fromApiResponse(status, body)` static factory
    - Maps server `error_code` strings → `SdkErrorCode` enum values
    - Falls back to `UNKNOWN_ERROR` for unmapped codes
    - _Requirements: 9.3_
  - [ ] 1.3 Write unit tests for error mapping
    - All documented server error codes map to expected SdkErrorCode
    - Non-2xx with unknown error_code maps to UNKNOWN_ERROR

- [ ] 2. Session store implementations
  - [ ] 2.1 Define `SessionStore` interface and all stored record types in `src/session/session-store.ts`
    - `DeviceRecord`, `RatchetState`, `SenderKeyRecord`, `ConversationMeta` types
    - All `Uint8Array` stored as base64 strings in JSON
    - _Requirements: 7.1_
  - [ ] 2.2 Implement `MemorySessionStore` in `src/session/memory-session-store.ts`
    - Plain `Map` fields; `clear()` resets all maps; no I/O
    - _Requirements: 7.3_
  - [ ] 2.3 Implement `IndexedDbSessionStore` in `src/session/indexed-db-session-store.ts`
    - DB name: `rce_sdk`, version `1`; four object stores: `devices`, `ratchetStates`, `senderKeys`, `conversations`
    - `open()` is lazy (first access); wraps IDB callbacks in Promises
    - On IDB error: emits `storage_error` and falls back to in-memory cache for current session
    - _Requirements: 7.2, 7.5_
  - [ ] 2.4 Implement `NodeFileSessionStore` in `src/session/node-file-session-store.ts`
    - Uses `node:fs/promises`; directory structure as per design doc
    - File writes are atomic: write to `.tmp` then `rename`
    - Only included in CJS/ESM output, not browser bundle (tree-shaken via `exports` map)
    - _Requirements: 7.4, 10.5_
  - [ ] 2.5 Implement platform detection in `src/session/platform.ts`
    - `detectDefaultSessionStore(): SessionStore` — `IndexedDbSessionStore` if `indexedDB` global exists, else `MemorySessionStore`
    - _Requirements: 7.2, 7.3_
  - [ ] 2.6 Write session store tests
    - Round-trip save/load all record types in MemorySessionStore
    - IndexedDbSessionStore tests using `fake-indexeddb` vitest package
    - _Requirements: 7.6_

- [ ] 3. Checkpoint — session layer

- [ ] 4. Cryptographic key generation
  - [ ] 4.1 Implement `KeyGenerator.generateKeyBundle()` in `src/crypto/key-generator.ts`
    - IdentityKey: `crypto.subtle.generateKey({ name: 'X25519' }, true, ['deriveKey', 'deriveBits'])` for DH
    - IdentityKey Ed25519: `crypto.subtle.generateKey({ name: 'Ed25519' }, true, ['sign', 'verify'])` for SPK signing
    - SignedPreKey: same X25519 generation; Ed25519 sign SPK pub bytes with Identity Ed25519 private
    - 50 OTPK X25519 pairs with sequential IDs starting at 1
    - _Requirements: 2.1, 2.2_
  - [ ] 4.2 Implement `KeyGenerator.generateOtpks(count)` for replenishment
    - Generates X25519 key pairs; IDs start from `nextOtpkId` in `DeviceRecord`
    - _Requirements: 2.4_
  - [ ] 4.3 Implement `KeyGenerator.generateSenderKey()`
    - 32-byte chain key via `crypto.getRandomValues`
    - Ed25519 signing key pair for message authentication
    - _Requirements: 5.1_
  - [ ] 4.4 Implement `KeyGenerator.generateSignedPreKey(identityEdKey, id)`
    - New X25519 key pair; sign exported pub bytes with Ed25519 private key
    - _Requirements: 2.5_
  - [ ] 4.5 Implement SPK self-verification in `src/crypto/crypto-utils.ts`
    - `verifySPKSignature(spkPub, sig, identityEdPub): Promise<boolean>`
    - SDK verifies its own generated SPK signature before registration
    - _Requirements: 2.3_
  - [ ] 4.6 Write key generation tests
    - SPK signature self-verifies; OTPK keys are unique X25519 pairs; SenderKey round-trips serialization

- [ ] 5. X3DH implementation
  - [ ] 5.1 Implement `X3dhEngine.performX3dh()` (initiator) in `src/crypto/x3dh-engine.ts`
    - Ephemeral X25519 key generation
    - DH1: `ECDH(IK_sender_priv, SPK_recipient_pub)`, DH2: `ECDH(EK_sender_priv, IK_recipient_pub)`, DH3: `ECDH(EK_sender_priv, SPK_recipient_pub)`, DH4: `ECDH(EK_sender_priv, OTPK_recipient_pub)` (if OTPK available)
    - HKDF: `crypto.subtle.deriveBits({ name: 'HKDF', hash: 'SHA-256', salt: 0x00*32, info: 'X3DH' }, concat(DH1..4), 256)`
    - Returns `{ sharedSecret: Uint8Array; header: X3dhHeader }`
    - _Requirements: 3.1, 5.2_
  - [ ] 5.2 Implement `X3dhEngine.deriveSharedSecret()` (responder)
    - Mirror computation using own private keys from `DeviceRecord`
    - _Requirements: 5.2_
  - [ ] 5.3 Write X3DH interoperability tests
    - Initiator + responder derive identical 32-byte secret
    - Test with OTPK present and with OTPK null (depleted)
    - _Requirements: 3.4 (Property 3)_

- [ ] 6. Double Ratchet implementation
  - [ ] 6.1 Implement `RatchetEngine.initSender()` and `RatchetEngine.initReceiver()` in `src/crypto/ratchet-engine.ts`
    - Initialize `RatchetSession` from X3DH shared secret
    - `initSender`: sender does first DH ratchet step immediately
    - _Requirements: 4.1_
  - [ ] 6.2 Implement `RatchetEngine.encrypt(session, plaintext)`
    - Advance chain key via HKDF-SHA-256; derive message key; AES-256-GCM encrypt with random IV
    - Returns `{ ciphertext: Uint8Array; header: RatchetHeader; nextSession: RatchetSession }`
    - Ciphertext includes IV prepended (first 12 bytes)
    - _Requirements: 4.1_
  - [ ] 6.3 Implement `RatchetEngine.decrypt(session, ciphertext, header)`
    - DH ratchet step if `header.dh !== session.dhRecvPub`; store skipped message keys (cap 2000)
    - AES-256-GCM decrypt; throw `SdkError.DECRYPTION_ERROR` on auth failure
    - Returns `{ plaintext: Uint8Array; nextSession: RatchetSession }`
    - _Requirements: 4.7_
  - [ ] 6.4 Implement `RatchetState` JSON serialization
    - All `Uint8Array` → base64url string; `skippedMessageKeys` Map → Record<string, string>
    - _Requirements: 4.4_
  - [ ] 6.5 Write Double Ratchet tests
    - Bidirectional exchange; out-of-order messages; session serialization round-trip; skipped key cap enforcement
    - _Requirements: 4.4, 4.5, Property 4_

- [ ] 7. Sender Key implementation
  - [ ] 7.1 Implement `SenderKeyEngine.createSession()` in `src/crypto/sender-key-engine.ts`
    - Accepts `SenderKeyMaterial`; initializes chain key and iteration counter
    - _Requirements: 5.1_
  - [ ] 7.2 Implement `SenderKeyEngine.encrypt(session, plaintext)`
    - Advance chain key via HMAC-SHA-256; derive message key; AES-256-GCM encrypt
    - Ed25519 sign the ciphertext with SenderKey signing key
    - Returns `{ ciphertext: Uint8Array; nextSession: SenderKeySession }`
    - _Requirements: 5.3_
  - [ ] 7.3 Implement `SenderKeyEngine.decrypt(session, ciphertext)`
    - Verify Ed25519 signature; advance chain key to matching iteration; AES-256-GCM decrypt
    - Throw `SdkError.DECRYPTION_ERROR` on signature or auth failure
    - _Requirements: 5.3_
  - [ ] 7.4 Implement `serializeKeyMaterial` and `deserializeKeyMaterial`
    - Compact binary format; encrypted by Double Ratchet for transit in SKDM
    - _Requirements: 5.2_
  - [ ] 7.5 Write Sender Key tests
    - Encrypt/decrypt; multi-iteration; signature verification failure; key isolation after removal

- [ ] 8. Checkpoint — crypto layer complete

- [ ] 9. REST transport layer
  - [ ] 9.1 Implement `RestClient` in `src/transport/rest-client.ts`
    - All methods use `fetch(url, { headers: { Authorization: 'Bearer ' + getToken() } })`
    - Non-2xx: parse body as `{ error_code, message }` → `SdkError.fromApiResponse()`
    - Retry interceptor: up to 3 retries on 503 + network errors with 500ms linear backoff
    - Key bytes: `base64urlEncode` on request, `base64urlDecode` on response
    - _Requirements: 9.4, 0.6 (hot-swap via closure)_
  - [ ] 9.2 Implement all KDS methods
    - `registerDevice`, `getKeyBundle`, `replenishOtpks`, `rotateSignedPreKey`
    - _Requirements: 0.3, 0.4, 2.4, 2.5_
  - [ ] 9.3 Implement all conversation methods
    - `createConversation`, `sendMessage`, `getMessages`
    - _Requirements: 4.1, 4.6_
  - [ ] 9.4 Implement all group methods
    - `createGroup`, `sendGroupMessage`, `addGroupMember`, `removeGroupMember`, `distributeGroupSenderKey`
    - _Requirements: 5.1, 5.6_
  - [ ] 9.5 Implement `AttachmentClient` in `src/transport/attachment-client.ts`
    - Upload via `fetch` with `FormData`; progress via `ReadableStream` wrapping `XMLHttpRequest` (browser) or `node:stream` (Node.js)
    - _Requirements: 6.1, 6.2, 6.4_
  - [ ] 9.6 Write REST client tests using `vitest` + `msw` (Mock Service Worker)
    - Auth header injection; error code mapping; 503 retry; attachment upload

- [ ] 10. WebSocket transport layer
  - [ ] 10.1 Implement `WsManager` in `src/transport/ws-manager.ts`
    - Wraps native `WebSocket`; parses JSON text frames → `RtEvent` discriminated union
    - Responds to `{ type: 'ping' }` with `{ type: 'pong' }` automatically within 5s
    - `sendAck(conversationId, seq)` sends `{ "conversation_id": "...", "seq": N }`
    - Unknown `type` fields are silently ignored (forward compatibility)
    - _Requirements: 1.2, 1.6_
  - [ ] 10.2 Implement `RtEvent` discriminated union parsing in `src/transport/rt-event.ts`
    - Parse JSON string; validate `type` discriminant; decode base64url `ciphertext` and `encrypted_skdm` fields to `Uint8Array`
    - _Requirements: 8.1, 8.2_
  - [ ] 10.3 Implement `ConnectionManager` with exponential backoff in `src/transport/connection-manager.ts`
    - Reconnect loop: 1s → 2s → 4s → 8s → 16s → 32s → 60s cap
    - Emits `'connection'` events on state transitions
    - Rebuilds WS URL with current token on each reconnect attempt
    - All errors caught → `ChatClient.emit('error', ...)`
    - _Requirements: 1.3, 1.7, Property 7_
  - [ ] 10.4 Write WebSocket transport tests
    - Ping/pong auto-response; ack frame format; RtEvent JSON parsing; backoff timing

- [ ] 11. OTPK replenisher
  - [ ] 11.1 Implement `OtpkReplenisher` in `src/client/otpk-replenisher.ts`
    - Subscribes to `RtEventRouter` for `low_otpk` events
    - On event: `KeyGenerator.generateOtpks(50)` → `RestClient.replenishOtpks` → `SessionStore.saveDevice(updated)`
    - Errors: caught and emitted as `ChatClient.emit('error', ...)`
    - _Requirements: 2.4, Property 7_

- [ ] 12. SPK rotation scheduler
  - [ ] 12.1 Implement `SpkRotator` in `src/client/spk-rotator.ts`
    - Checks `DeviceRecord.signedPrekeyCreatedAt` vs `config.signedPrekeyRotationDays` on a daily `setInterval`
    - On trigger: `KeyGenerator.generateSignedPreKey(identityEdKey, newId)` → `RestClient.rotateSignedPreKey` → `SessionStore.saveDevice(updated)`
    - Errors: caught and emitted as `ChatClient.emit('error', ...)`
    - _Requirements: 2.5_

- [ ] 13. Checkpoint — transport + automation layer

- [ ] 14. Domain layer — Conversation
  - [ ] 14.1 Implement `OneToOneConversation extends Conversation` in `src/conversation/one-to-one-conversation.ts`
    - `send(text)`: RatchetEngine.encrypt → SessionStore.save → RestClient.sendMessage → emit `'message'`
    - `onIncomingMessage(rtEvent)`: RatchetEngine.decrypt → SessionStore.save → emit `'message'` → WsManager.sendAck
    - `fetchHistory(options)`: RestClient.getMessages → decrypt each → return sorted by seq
    - Decryption failures emit `ChatMessage { decryptionError: true }` via `'message'` event
    - _Requirements: 4.1–4.7_
  - [ ] 14.2 Implement `GroupConversation extends Conversation` in `src/conversation/group-conversation.ts`
    - `send(text)`: SenderKeyEngine.encrypt → RestClient.sendGroupMessage → emit `'message'`
    - `onIncomingGroupMessage(rtEvent)`: SenderKeyEngine.decrypt (or queue if no SKDM yet) → emit `'message'`
    - `onSkdm(rtEvent)`: decrypt SKDM via 1:1 ratchet → SessionStore.saveSenderKey → process queued messages
    - `onMemberAdded`: distribute new SKDM to new member → emit `'member_added'`
    - `onMemberRemoved`: generate new SenderKey → distribute to remaining members → emit `'member_removed'`
    - SKDM queue: pending messages held up to 60s before `decryptionError: true`
    - _Requirements: 5.1–5.7_
  - [ ] 14.3 Implement `ConversationRegistry` in `src/client/conversation-registry.ts`
    - Owns `Map<string, Conversation>` cache; `getOrCreate(id, type)` restores from `SessionStore` if not cached
    - `loadAll()` at init: reads `SessionStore.loadAllConversations()` → restores all as appropriate type
    - Emits `ChatClient.emit('conversation', { conversation })` on new conversation creation
    - _Requirements: 3.2, 3.5, 8.1_
  - [ ] 14.4 Implement `RtEventRouter` in `src/client/rt-event-router.ts`
    - Subscribes to `WsManager.events` on `frame`
    - Routes `message` → `ConversationRegistry.getOrCreate(cid).onIncomingMessage(...)`
    - Routes `sender_key_distribution` → `GroupConversation.onSkdm(...)`
    - Routes `member_added` / `member_removed` → `GroupConversation.onMemberChange(...)`
    - Routes `low_otpk` → `OtpkReplenisher`
    - All handlers wrapped in `try/catch → ChatClient.emit('error', ...)`
    - _Requirements: 5.2, 5.4, 5.5, 8.1–8.3, Property 7_

- [ ] 15. Domain layer — ChatClient
  - [ ] 15.1 Implement `ChatClient.create(config)` in `src/client/chat-client.ts`
    - Full initialization sequence as per design doc
    - Platform-detect default `SessionStore`; wire all components
    - `destroy()`: `ConnectionManager.disconnect()` → clear all intervals → remove all listeners
    - _Requirements: 0.1–0.5, 0.7_
  - [ ] 15.2 Implement `openConversation(recipientUserId)`
    - Check `ConversationRegistry` cache first (idempotent)
    - `RestClient.getKeyBundle(recipientUserId)` → `X3dhEngine.performX3dh` → `RestClient.createConversation` → `SessionStore.saveRatchetState` → register + return `OneToOneConversation`
    - _Requirements: 3.1–3.5_
  - [ ] 15.3 Implement `createGroup(memberUserIds)`
    - `RestClient.createGroup` → `SenderKeyEngine.createSession` → for each member: getKeyBundle + encrypt SKDM → `RestClient.distributeGroupSenderKey`
    - Returns `GroupConversation`
    - _Requirements: 5.1_
  - [ ] 15.4 Implement `updateToken(newToken)` and `getPublicKeyBundle()`
    - `updateToken`: atomically update internal `currentToken`; triggers WS reconnect
    - `getPublicKeyBundle`: read from `SessionStore.loadDevice`; export public bytes only
    - _Requirements: 0.6, 0.7, 2.7, Property 6_

- [ ] 16. Checkpoint — full domain layer

- [ ] 17. Barrel export and tree-shaking
  - [ ] 17.1 Write `src/index.ts` barrel exporting all public types
    - Exports: `ChatClient`, `Conversation`, `ChatMessage`, `KeyBundle`, `SessionStore`, `IndexedDbSessionStore`, `MemorySessionStore`, `SdkError`, `SdkErrorCode`, `ChatClientConfig`, `ConversationMeta`, `ConnectionState`, `AttachmentProgress`
    - `NodeFileSessionStore` exported separately via `src/node.ts` to avoid browser bundle inclusion
    - _Requirements: 8.3, 10.5_

- [ ] 18. Testing library
  - [ ] 18.1 Implement `MockChatClient` in `testing/mock-chat-client.ts`
    - Backed by `MemorySessionStore` + fake transport (no real WebSocket or fetch)
    - `simulateIncomingMessage(conversationId, senderId, text)` — injects decrypted `ChatMessage` into conversation
    - `simulateConnectionState(state)` — emits `'connection'` event
    - `simulateMemberAdded(conversationId, userId)` — emits on conversation
    - `simulateMemberRemoved(conversationId, userId)` — emits on conversation
    - _Requirements: 11.1, 11.2_
  - [ ] 18.2 Write vitest helper `testing/vitest-helpers.ts`
    - `createMockClient(config?): MockChatClient` — auto-tears down in `afterEach`
    - _Requirements: 11.3_

- [ ] 19. Browser bundle
  - [ ] 19.1 Build UMD browser bundle `dist/browser.min.js`
    - Exposes `window.RustChat` with `{ ChatClient, MemorySessionStore, IndexedDbSessionStore, SdkError }`
    - No `NodeFileSessionStore` included
    - Minified; source map at `dist/browser.min.js.map`
    - _Requirements: 10.6_

- [ ] 20. Documentation and examples
  - [ ] 20.1 Write `README.md` with quick-start guide
    - Installation (npm + CDN `<script>` tag)
    - 1:1 chat example (< 20 lines)
    - Group chat example
    - Custom `SessionStore` example
    - Framework adapter references (`@rust-e2e-chat/react`, etc.)
    - _Requirements: 11.5_
  - [ ] 20.2 Add JSDoc `@example` comments to all public methods
    - _Requirements: 11.4_

- [ ] 21. Final checkpoint
  - `npm test` — all vitest tests pass
  - `tsc --noEmit` — zero TypeScript errors
  - `npm run build` — ESM + CJS + `.d.ts` + UMD emit successfully
  - Bundle size check: core ESM bundle < 80 KB unminified (crypto is the largest)
  - Smoke test against live `rust-e2e-chat-api`: `ChatClient.create` → `openConversation` → `send` → receive via WebSocket → round-trip verified

---

## Notes

- **crypto.subtle key import/export**: Use `extractable: true` on all generated keys so they can be exported to `Uint8Array` for `SessionStore` persistence. Export format: `'raw'` for symmetric keys, `'raw'` for X25519 public, `'pkcs8'` for private keys.
- **Base64url encoding**: All key bytes exchanged with the server use base64url without padding (`=`). The `encoding.ts` util handles this. Use `base64url.encode(bytes)` consistently.
- **HKDF info strings**: Use UTF-8 encoded ASCII strings as HKDF `info` parameters: `"X3DH"` for X3DH, `"RatchetRoot"` for root key derivation, `"RatchetChain"` for chain key steps, `"SenderKey"` for Sender Key derivation.
- **AES-GCM IV**: Always 12 bytes from `crypto.getRandomValues`; prepend to ciphertext for storage and transmission.
- **Skipped message keys**: Key in `RatchetState.skippedMessageKeys` is `${base64url(dhPub)}:${n}`. Cap at 2000 entries; evict oldest when exceeded.
- **IndexedDB**: Wrap all IDB operations in a `promisify()` helper. Use a single `transaction` per `SessionStore` method to ensure atomicity. Never hold transactions across `await` boundaries.
- **Web Worker compatibility**: `src/index.ts` must not reference `window`, `document`, or `localStorage`. All DOM globals accessed through the `SessionStore` abstraction or feature-detected. The `WsManager` uses the global `WebSocket` constructor directly (available in Workers).
- **Node.js WebSocket**: In Node.js < 21, the global `WebSocket` may not be available. Detect with `typeof WebSocket !== 'undefined'`; if missing, require the user to pass a `WebSocket` constructor in `ChatClientConfig`. In Node.js ≥ 21, global `WebSocket` is available.
- **No plaintext in logs**: The `Logger.redact()` utility replaces any value that is a `string > 20 chars` in sensitive positions with `[REDACTED]`. Specifically: `ChatMessage.text`, any `Uint8Array` with `> 16 bytes` (likely key material), `accessToken`.
- **`sendAttachment` progress on non-browser**: `XMLHttpRequest` progress is browser-only. In Node.js, use `node:stream` readable with `data` events to track bytes written. `ReadableStream` wrapper normalizes both.
- **Forward compatibility**: The `RtEvent` parser uses a `switch` on `type` with a `default: return null` branch. Unknown event types are silently discarded to support future server-side additions without SDK updates.

---

## Task Dependency Graph

```json
{
  "waves": [
    { "id": 0,  "tasks": ["0.1", "0.2", "0.3", "0.4", "0.5", "0.6"] },
    { "id": 1,  "tasks": ["1.1", "1.2", "1.3"] },
    { "id": 2,  "tasks": ["2.1", "2.2", "2.3", "2.4", "2.5", "2.6"] },
    { "id": 3,  "tasks": ["4.1", "4.2", "4.3", "4.4", "4.5", "4.6"] },
    { "id": 4,  "tasks": ["5.1", "5.2", "5.3"] },
    { "id": 5,  "tasks": ["6.1", "6.2", "6.3", "6.4", "6.5"] },
    { "id": 6,  "tasks": ["7.1", "7.2", "7.3", "7.4", "7.5"] },
    { "id": 7,  "tasks": ["9.1", "9.2", "9.3", "9.4", "9.5", "9.6"] },
    { "id": 8,  "tasks": ["10.1", "10.2", "10.3", "10.4"] },
    { "id": 9,  "tasks": ["11.1", "12.1"] },
    { "id": 10, "tasks": ["14.1", "14.2", "14.3", "14.4"] },
    { "id": 11, "tasks": ["15.1", "15.2", "15.3", "15.4"] },
    { "id": 12, "tasks": ["17.1"] },
    { "id": 13, "tasks": ["18.1", "18.2", "19.1"] },
    { "id": 14, "tasks": ["20.1", "20.2"] },
    { "id": 15, "tasks": ["21"] }
  ]
}
```
