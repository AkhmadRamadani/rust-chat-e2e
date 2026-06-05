// testing/mock-chat-client.ts

import { TypedEventEmitter } from '../src/utils/typed-emitter.js';
import { MemorySessionStore } from '../src/session/memory-session-store.js';
import type { Conversation } from '../src/conversation/conversation.js';
import type { ChatMessage } from '../src/conversation/chat-message.js';
import type { AttachmentSend } from '../src/conversation/conversation.js';
import type { SessionStore } from '../src/session/session-store.js';
import type { KeyBundle } from '../src/crypto/crypto-types.js';
import type { ConnectionState } from '../src/transport/connection-manager.js';
import type { SdkError } from '../src/errors/sdk-error.js';
import type { ChatClientEvents } from '../src/client/chat-client.js';
import type { ChatClientConfig } from '../src/client/chat-client-config.js';
import { randomUuid } from '../src/utils/encoding.js';

class MockConversation extends TypedEventEmitter<{
  message: ChatMessage;
  member_added: { userId: string; devices: string[] };
  member_removed: { userId: string };
}> implements Conversation {
  readonly type: 'one_to_one' | 'group';
  readonly members: Array<{ userId: string; deviceId: string }>;

  constructor(
    readonly conversationId: string,
    type: 'one_to_one' | 'group',
    members: Array<{ userId: string; deviceId: string }>,
    private readonly currentUserId: string,
  ) {
    super();
    this.type = type;
    this.members = members;
  }

  async send(text: string): Promise<ChatMessage> {
    const msg: ChatMessage = {
      id: randomUuid(),
      conversationId: this.conversationId,
      senderId: this.currentUserId,
      senderDeviceId: 'mock-device',
      type: 'text',
      text,
      attachmentId: null,
      attachmentUrl: null,
      attachmentName: null,
      timestamp: new Date(),
      seq: Date.now(),
      isMine: true,
      decryptionError: false,
    };
    this.emit('message', msg);
    return msg;
  }

  sendAttachment(
    _file: File | Blob | BufferSource,
    filename: string,
    _contentType: string,
  ): AttachmentSend {
    const promise = Promise.resolve<ChatMessage>({
      id: randomUuid(),
      conversationId: this.conversationId,
      senderId: this.currentUserId,
      senderDeviceId: 'mock-device',
      type: 'attachment',
      text: null,
      attachmentId: 'mock-attachment-id',
      attachmentUrl: 'https://example.com/mock-attachment',
      attachmentName: filename,
      timestamp: new Date(),
      seq: Date.now(),
      isMine: true,
      decryptionError: false,
    });
    const progress = new ReadableStream({ start(c) { c.close(); } });
    return { promise, progress };
  }

  async fetchHistory(_options?: { limit?: number; beforeSeq?: number }): Promise<ChatMessage[]> {
    return [];
  }

  markAsRead(): void { /* no-op */ }

  async addMember(_userId: string): Promise<void> {
    if (this.type === 'one_to_one') throw new Error('Not a group');
  }

  async removeMember(_userId: string): Promise<void> {
    if (this.type === 'one_to_one') throw new Error('Not a group');
  }

  // Testing hooks
  _injectMessage(msg: ChatMessage): void {
    this.emit('message', msg);
  }
}

/**
 * Mock ChatClient for unit testing application code without a live server.
 * @example
 * import { createMockClient } from '@rust-e2e-chat/sdk/testing';
 * const client = createMockClient();
 * const conv = await client.openConversation('user-bob');
 * client.simulateIncomingMessage(conv.conversationId, 'user-bob', 'Hey!');
 */
export class MockChatClient extends TypedEventEmitter<ChatClientEvents> {
  readonly userId: string;
  readonly deviceId: string;

  private readonly store: SessionStore;
  private connectionState: ConnectionState = 'disconnected';
  private readonly conversations = new Map<string, MockConversation>();

  constructor(config?: Partial<ChatClientConfig>) {
    super();
    this.userId = config?.userId ?? 'mock-user';
    this.deviceId = 'mock-device';
    this.store = config?.sessionStore ?? new MemorySessionStore();
  }

  async connect(): Promise<void> {
    this.simulateConnectionState('connected');
  }

  async disconnect(): Promise<void> {
    this.simulateConnectionState('disconnected');
  }

  updateToken(_newToken: string): void { /* no-op */ }

  async openConversation(recipientUserId: string): Promise<Conversation> {
    // Look for existing
    for (const conv of this.conversations.values()) {
      if (conv.type === 'one_to_one' && conv.members.some((m) => m.userId === recipientUserId)) {
        return conv;
      }
    }
    const id = randomUuid();
    const conv = new MockConversation(
      id,
      'one_to_one',
      [
        { userId: this.userId, deviceId: this.deviceId },
        { userId: recipientUserId, deviceId: 'mock-device-remote' },
      ],
      this.userId,
    );
    this.conversations.set(id, conv);
    this.emit('conversation', { conversation: conv });
    return conv;
  }

  async createGroup(memberUserIds: string[]): Promise<Conversation> {
    const id = randomUuid();
    const members = [
      { userId: this.userId, deviceId: this.deviceId },
      ...memberUserIds.map((uid) => ({ userId: uid, deviceId: `mock-device-${uid}` })),
    ];
    const conv = new MockConversation(id, 'group', members, this.userId);
    this.conversations.set(id, conv);
    this.emit('conversation', { conversation: conv });
    return conv;
  }

  findConversation(conversationId: string): Conversation | undefined {
    return this.conversations.get(conversationId);
  }

  async getPublicKeyBundle(): Promise<KeyBundle> {
    return {
      deviceId: this.deviceId,
      identityKeyDhPub: new Uint8Array(32),
      identityKeyEdPub: new Uint8Array(32),
      signedPrekeyId: 1,
      signedPrekeyPub: new Uint8Array(32),
      signedPrekeySig: new Uint8Array(64),
    };
  }

  async destroy(): Promise<void> {
    this.removeAllListeners();
  }

  // --- Testing Simulation API ---

  /**
   * Simulate an incoming message in a conversation.
   * @example
   * client.simulateIncomingMessage(conv.conversationId, 'user-bob', 'Hi there!');
   */
  simulateIncomingMessage(conversationId: string, senderId: string, text: string): void {
    const conv = this.conversations.get(conversationId);
    if (!conv) throw new Error(`No conversation: ${conversationId}`);
    conv._injectMessage({
      id: randomUuid(),
      conversationId,
      senderId,
      senderDeviceId: `mock-device-${senderId}`,
      type: 'text',
      text,
      attachmentId: null,
      attachmentUrl: null,
      attachmentName: null,
      timestamp: new Date(),
      seq: Date.now(),
      isMine: false,
      decryptionError: false,
    });
  }

  /**
   * Simulate a connection state change.
   * @example
   * client.simulateConnectionState('reconnecting');
   */
  simulateConnectionState(state: ConnectionState): void {
    this.connectionState = state;
    this.emit('connection', { state });
  }

  /**
   * Simulate a member being added to a group conversation.
   */
  simulateMemberAdded(conversationId: string, userId: string): void {
    const conv = this.conversations.get(conversationId);
    if (!conv) throw new Error(`No conversation: ${conversationId}`);
    conv.emit('member_added', { userId, devices: [`mock-device-${userId}`] });
  }

  /**
   * Simulate a member being removed from a group conversation.
   */
  simulateMemberRemoved(conversationId: string, userId: string): void {
    const conv = this.conversations.get(conversationId);
    if (!conv) throw new Error(`No conversation: ${conversationId}`);
    conv.emit('member_removed', { userId });
  }
}
