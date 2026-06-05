// testing/vitest-helpers.ts
/// <reference types="vitest/globals" />

import { MockChatClient } from './mock-chat-client.js';
import type { ChatClientConfig } from '../src/client/chat-client-config.js';

let activeClients: MockChatClient[] = [];

/**
 * Create a MockChatClient that is automatically torn down after each test.
 * Requires vitest globals (afterEach is called automatically).
 *
 * @example
 * import { createMockClient } from '@rust-e2e-chat/sdk/testing';
 *
 * describe('my chat feature', () => {
 *   it('sends a message', async () => {
 *     const client = createMockClient();
 *     const conv = await client.openConversation('user-bob');
 *     const msg = await conv.send('Hello');
 *     expect(msg.text).toBe('Hello');
 *   });
 * });
 */
export function createMockClient(config?: Partial<ChatClientConfig>): MockChatClient {
  const client = new MockChatClient(config);
  activeClients.push(client);
  return client;
}

// Register afterEach cleanup if vitest globals are available
if (typeof afterEach !== 'undefined') {
  afterEach(async () => {
    for (const client of activeClients) {
      await client.destroy();
    }
    activeClients = [];
  });
}
