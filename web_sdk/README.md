# @rust-e2e-chat/sdk

Framework-agnostic TypeScript SDK for the `rust-e2e-chat-api` platform.  
End-to-end encrypted 1:1 and group chat — X3DH + Double Ratchet + Sender Keys — built entirely on the Web Crypto API (`crypto.subtle`). Zero mandatory runtime dependencies.

---

## Installation

```bash
npm install @rust-e2e-chat/sdk
```

**CDN / `<script>` tag (UMD):**

```html
<script src="https://unpkg.com/@rust-e2e-chat/sdk/dist/browser.min.js"></script>
<!-- window.RustChat.ChatClient, window.RustChat.SdkError, etc. -->
```

---

## Quick Start

### 1. Create a client

```typescript
import { ChatClient } from '@rust-e2e-chat/sdk';

const client = await ChatClient.create({
  baseUrl: 'https://api.example.com',
  accessToken: myJwt,   // OIDC JWT
  userId: 'user-alice', // OIDC sub
});

client.on('connection', ({ state }) => {
  console.log('Connection:', state); // 'connected' | 'reconnecting' | ...
});

client.on('error', (err) => {
  console.error(err.code, err.message);
});
```

### 2. 1:1 messaging

```typescript
const conv = await client.openConversation('user-bob');

conv.on('message', (msg) => {
  if (!msg.isMine) {
    console.log(`${msg.senderId}: ${msg.text}`);
  }
});

const sent = await conv.send('Hello Bob!');
console.log('Sent at seq', sent.seq);

// Fetch history
const history = await conv.fetchHistory({ limit: 50 });
```

### 3. Group chat

```typescript
const group = await client.createGroup(['user-bob', 'user-carol']);

group.on('message', (msg) => console.log(msg.senderId, msg.text));
group.on('member_added', ({ userId }) => console.log(userId, 'joined'));
group.on('member_removed', ({ userId }) => console.log(userId, 'left'));

await group.send('Hey everyone!');
await group.addMember('user-dave');
await group.removeMember('user-bob'); // rotates SenderKey automatically
```

### 4. Attachments

```typescript
const { promise, progress } = conv.sendAttachment(file, file.name, file.type);

const reader = progress.getReader();
while (true) {
  const { done, value } = await reader.read();
  if (done) break;
  console.log(`${value.sent}/${value.total} bytes`);
}

const msg = await promise;
console.log('Attachment ID:', msg.attachmentId);
```

---

## Configuration

```typescript
interface ChatClientConfig {
  baseUrl: string;                    // API server base URL
  accessToken: string;                // OIDC JWT
  userId: string;                     // OIDC sub
  deviceId?: string;                  // omit to auto-register a new device
  sessionStore?: SessionStore;        // default: IndexedDbSessionStore in browser
  autoConnect?: boolean;              // default: true
  signedPrekeyRotationDays?: number;  // default: 7
  logLevel?: 'debug' | 'info' | 'warn' | 'error' | 'silent'; // default: 'warn'
}
```

---

## Token Hot-Swap

```typescript
// After an OIDC token refresh — no re-init needed
client.updateToken(newJwt);
```

---

## Custom SessionStore

Implement the `SessionStore` interface to plug in any storage backend:

```typescript
import type { SessionStore } from '@rust-e2e-chat/sdk';

class MySessionStore implements SessionStore {
  async saveDevice(record) { /* ... */ }
  async loadDevice(userId, deviceId) { /* ... */ }
  async saveRatchetState(conversationId, state) { /* ... */ }
  async loadRatchetState(conversationId) { /* ... */ }
  async saveSenderKey(conversationId, userId, record) { /* ... */ }
  async loadSenderKey(conversationId, userId) { /* ... */ }
  async saveConversationMeta(meta) { /* ... */ }
  async loadAllConversations() { /* ... */ }
  async clear() { /* ... */ }
}

const client = await ChatClient.create({
  // ...
  sessionStore: new MySessionStore(),
});
```

**Built-in stores:**
- `IndexedDbSessionStore` — browser default; persistent across page reloads
- `MemorySessionStore` — in-memory; for tests and ephemeral sessions  
- `NodeFileSessionStore` — Node.js filesystem; import from `@rust-e2e-chat/sdk/node`

---

## Node.js (filesystem store)

```typescript
import { ChatClient } from '@rust-e2e-chat/sdk/node';
import { NodeFileSessionStore } from '@rust-e2e-chat/sdk/node';

const client = await ChatClient.create({
  // ...
  sessionStore: new NodeFileSessionStore('./chat-sessions'),
});
```

---

## Testing

Use `MockChatClient` to unit-test your chat UI without a live server:

```typescript
import { createMockClient } from '@rust-e2e-chat/sdk/testing';

describe('my chat feature', () => {
  it('receives a message', async () => {
    const client = createMockClient({ userId: 'alice' }); // auto-teardown via afterEach

    const conv = await client.openConversation('bob');
    const received: ChatMessage[] = [];
    conv.on('message', (m) => received.push(m));

    client.simulateIncomingMessage(conv.conversationId, 'bob', 'Hey!');
    expect(received[0].text).toBe('Hey!');
  });
});
```

**Simulation methods:**
- `simulateIncomingMessage(convId, senderId, text)`
- `simulateConnectionState(state)`
- `simulateMemberAdded(convId, userId)`
- `simulateMemberRemoved(convId, userId)`

---

## Error Handling

All public methods reject with `SdkError`:

```typescript
import { SdkError, SdkErrorCode } from '@rust-e2e-chat/sdk';

try {
  await client.openConversation('user-bob');
} catch (err) {
  if (err instanceof SdkError) {
    switch (err.code) {
      case SdkErrorCode.AUTH_ERROR:       // re-authenticate
      case SdkErrorCode.SESSION_NOT_FOUND: // clear local state
      case SdkErrorCode.NETWORK_ERROR:    // retry
      case SdkErrorCode.DECRYPTION_ERROR: // show error in UI
    }
  }
}
```

---

## Runtime Compatibility

| Environment | Supported |
|---|---|
| Chrome ≥ 89 | ✅ |
| Firefox ≥ 86 | ✅ |
| Safari ≥ 15 | ✅ |
| Node.js ≥ 20 | ✅ |
| Deno ≥ 1.28 | ✅ |
| Bun ≥ 0.6 | ✅ |
| Web Workers | ✅ |

No polyfills required. All cryptography via `crypto.subtle`.

---

## Framework Adapters

- `@rust-e2e-chat/react` — React hooks (`useChat`, `useConversation`)
- `@rust-e2e-chat/vue` — Vue 3 composables
- `@rust-e2e-chat/svelte` — Svelte stores

---

## Security Properties

| Property | Guarantee |
|---|---|
| Zero-Knowledge Transport | Plaintext never crosses REST/WebSocket layer |
| Forward Secrecy | Ratchet state persisted before every send/receive |
| OTPK Depletion Resilience | X3DH works with or without a one-time prekey |
| Sender Key Isolation | New SenderKey generated on every `removeMember` |
| Token Hot-Swap | New token used on all subsequent requests + next WS reconnect |
| Zero Unhandled Rejections | All background errors emitted as `'error'` events |
