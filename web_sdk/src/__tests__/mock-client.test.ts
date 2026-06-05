// src/__tests__/mock-client.test.ts

import { describe, it, expect, vi } from 'vitest';
import { MockChatClient } from '../../testing/mock-chat-client.js';

describe('MockChatClient', () => {
  it('opens a 1:1 conversation', async () => {
    const client = new MockChatClient({ userId: 'alice' });
    const conv = await client.openConversation('bob');
    expect(conv.type).toBe('one_to_one');
    expect(conv.members.some((m) => m.userId === 'bob')).toBe(true);
  });

  it('returns same conversation on second openConversation call', async () => {
    const client = new MockChatClient({ userId: 'alice' });
    const conv1 = await client.openConversation('bob');
    const conv2 = await client.openConversation('bob');
    expect(conv1.conversationId).toBe(conv2.conversationId);
  });

  it('creates a group', async () => {
    const client = new MockChatClient({ userId: 'alice' });
    const group = await client.createGroup(['bob', 'carol']);
    expect(group.type).toBe('group');
    expect(group.members).toHaveLength(3);
  });

  it('send() resolves with a ChatMessage', async () => {
    const client = new MockChatClient({ userId: 'alice' });
    const conv = await client.openConversation('bob');
    const msg = await conv.send('Hello!');
    expect(msg.text).toBe('Hello!');
    expect(msg.isMine).toBe(true);
  });

  it('simulateIncomingMessage triggers message event', async () => {
    const client = new MockChatClient({ userId: 'alice' });
    const conv = await client.openConversation('bob');
    const handler = vi.fn();
    conv.on('message', handler);
    client.simulateIncomingMessage(conv.conversationId, 'bob', 'Hey Alice!');
    expect(handler).toHaveBeenCalledOnce();
    expect(handler.mock.calls[0]![0].text).toBe('Hey Alice!');
    expect(handler.mock.calls[0]![0].isMine).toBe(false);
  });

  it('simulateConnectionState emits connection event', () => {
    const client = new MockChatClient();
    const handler = vi.fn();
    client.on('connection', handler);
    client.simulateConnectionState('reconnecting');
    expect(handler).toHaveBeenCalledWith({ state: 'reconnecting' });
  });

  it('simulateMemberAdded emits on conversation', async () => {
    const client = new MockChatClient({ userId: 'alice' });
    const group = await client.createGroup(['bob']);
    const handler = vi.fn();
    group.on('member_added', handler);
    client.simulateMemberAdded(group.conversationId, 'carol');
    expect(handler).toHaveBeenCalledWith({ userId: 'carol', devices: ['mock-device-carol'] });
  });

  it('simulateMemberRemoved emits on conversation', async () => {
    const client = new MockChatClient({ userId: 'alice' });
    const group = await client.createGroup(['bob']);
    const handler = vi.fn();
    group.on('member_removed', handler);
    client.simulateMemberRemoved(group.conversationId, 'bob');
    expect(handler).toHaveBeenCalledWith({ userId: 'bob' });
  });

  it('destroy() clears listeners', async () => {
    const client = new MockChatClient();
    const handler = vi.fn();
    client.on('connection', handler);
    await client.destroy();
    client.simulateConnectionState('connected');
    expect(handler).not.toHaveBeenCalled();
  });
});
