# rust_e2e_chat_sdk

A complete Dart/Flutter client SDK for the `rust-e2e-chat-api` platform.

Provides X3DH + Double Ratchet 1:1 encryption, Sender Key group messaging, WebSocket real-time delivery with auto-reconnect, and a reactive (Stream-based) public API — all behind a clean, idiomatic Dart surface.

---

## Platform Support

| Platform | Session Store | WebSocket |
|---|---|---|
| Android | `flutter_secure_storage` | ✅ |
| iOS | `flutter_secure_storage` | ✅ |
| macOS | `flutter_secure_storage` | ✅ |
| Windows | `flutter_secure_storage` | ✅ |
| Linux | `flutter_secure_storage` | ✅ |
| Web | `InMemorySessionStore` (no persistence) | ✅ |

---

## Quick Start

### 1. Add dependency

```yaml
dependencies:
  rust_e2e_chat_sdk: ^0.1.0
```

### 2. Initialize

```dart
import 'package:rust_e2e_chat_sdk/rust_e2e_chat_sdk.dart';

final client = await ChatClient.initialize(ChatClientConfig(
  baseUrl: 'https://api.example.com/api',
  accessToken: myOidcToken,  // OIDC JWT
  userId: myUserId,           // OIDC sub claim
));
```

On first call (no `deviceId`), the SDK:
1. Generates a full key bundle (IdentityKey, SignedPreKey, 50 OTPKs)
2. Registers the device via `POST /users/{userId}/devices`
3. Persists all private key material in the platform keychain
4. Opens a WebSocket connection

### 3. Send and receive 1:1 messages

```dart
// Open (or resume) a conversation with another user
final convo = await client.openConversation('user-bob');

// Send a message (encrypted with Double Ratchet)
await convo.sendMessage('Hello, Bob!');

// Listen for new messages
convo.messages.listen((messages) {
  for (final msg in messages) {
    if (!msg.isMine) {
      print('[${msg.senderUserId}]: ${msg.text}');
    }
  }
});
```

### 4. Group messaging

```dart
// Create a group (generates SenderKey, distributes to all members)
final group = await client.createGroup(['user-bob', 'user-carol']);

await group.sendMessage('Welcome everyone!');

// Add / remove members (triggers automatic key rotation)
await group.addMember('user-dave');
await group.removeMember('user-carol');
```

### 5. File attachments

```dart
final bytes = await File('photo.jpg').readAsBytes();
await convo.sendAttachment(bytes, 'photo.jpg', 'image/jpeg');

// Monitor upload progress
convo.uploadProgress.listen((progress) {
  if (progress != null) {
    print('${(progress.fraction * 100).toStringAsFixed(0)}%');
  }
});
```

### 6. Connection management

```dart
// Observe connection state (auto-reconnects with exponential backoff)
client.connectionState.listen((state) {
  print('Connection: $state');
});

// Update token without re-initializing
await client.updateToken(newOidcToken);

// Clean shutdown
await client.dispose();
```

---

## Testing

```dart
import 'package:rust_e2e_chat_sdk/testing.dart';

final mockClient = MockChatClient(userId: 'test-user');
final convo = mockClient.stubConversation('conv-1', recipientUserId: 'alice');

// Inject a fake incoming message
mockClient.simulateIncomingMessage('conv-1', 'alice', 'Hello!');

// Assert on sent messages
await convo.sendMessage('Hi back!');
expect((convo as MockChatConversation).sentMessages.last.text, 'Hi back!');
```

---

## Error Handling

All public methods throw subtypes of `SdkError`:

```dart
try {
  await client.openConversation('unknown-user');
} on AuthError catch (e) {
  print('Not authorized: ${e.message}');
} on NetworkError catch (e) {
  print('Network error ${e.statusCode}: ${e.message}');
} on KeyExchangeError catch (e) {
  print('Key exchange failed: ${e.reason}');
} on SdkError catch (e) {
  print('SDK error: $e');
}
```

---

## Correctness guarantees

| Property | Enforcement |
|---|---|
| **Zero-Knowledge Transport** | Plaintext never crosses the transport layer |
| **Session Continuity** | Ratchet state persisted before `sendMessage` returns |
| **OTPK Depletion Resilience** | X3DH proceeds without OTPK; warning emitted |
| **Forward Secrecy** | Ratchet state advanced atomically on every message |
| **Sender Key Isolation** | Removed members never receive new SenderKey |
| **Token Hot-Swap** | `updateToken` takes effect on next request and WS reconnect |

---

## Dependencies

| Package | Purpose |
|---|---|
| `dio` | HTTP client with interceptors and multipart |
| `web_socket_channel` | WebSocket (dart:io + web) |
| `flutter_secure_storage` | Platform keychain/keystore |
| `cryptography` | Curve25519, Ed25519, AES-GCM, HKDF |
| `uuid` | UUID generation |
