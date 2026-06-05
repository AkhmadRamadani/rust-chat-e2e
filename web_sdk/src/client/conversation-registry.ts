// src/client/conversation-registry.ts

import type { Conversation } from '../conversation/conversation.js';
import { OneToOneConversation } from '../conversation/one-to-one-conversation.js';
import { GroupConversation } from '../conversation/group-conversation.js';
import type { SessionStore } from '../session/session-store.js';
import type { RestClient } from '../transport/rest-client.js';
import type { AttachmentClient } from '../transport/attachment-client.js';
import { KeyGenerator } from '../crypto/key-generator.js';
import type { Logger } from '../utils/logger.js';
import type { TypedEventEmitter } from '../utils/typed-emitter.js';

export interface ConversationRegistryEvents {
  new_conversation: { conversation: Conversation };
}

/**
 * Owns the Map<conversationId, Conversation> cache and restores sessions from the store.
 */
export class ConversationRegistry {
  private readonly cache = new Map<string, Conversation>();

  constructor(
    private readonly currentUserId: string,
    private readonly currentDeviceId: string,
    private readonly store: SessionStore,
    private readonly rest: RestClient,
    private readonly attachmentClient: AttachmentClient,
    private readonly logger: Logger,
    private readonly emitNewConversation: (conversation: Conversation) => void,
    private readonly distributeSkdm: (conversationId: string, memberUserIds: string[]) => Promise<void>,
  ) {}

  /**
   * Load all persisted conversations from the store and rebuild instances.
   */
  async loadAll(): Promise<void> {
    const metas = await this.store.loadAllConversations();
    for (const meta of metas) {
      if (this.cache.has(meta.conversationId)) continue;
      const conv = this.buildConversation(meta.conversationId, meta.type, meta.members.map(m => ({
        userId: m.userId,
        deviceId: m.deviceId,
      })));
      this.cache.set(meta.conversationId, conv);
    }
    this.logger.debug(`Loaded ${metas.length} conversations from store`);
  }

  /**
   * Get an existing conversation or create a new instance.
   */
  getOrCreate(
    conversationId: string,
    type: 'one_to_one' | 'group',
    members: Array<{ userId: string; deviceId: string }>,
  ): Conversation {
    if (this.cache.has(conversationId)) {
      return this.cache.get(conversationId)!;
    }
    const conv = this.buildConversation(conversationId, type, members);
    this.cache.set(conversationId, conv);
    this.emitNewConversation(conv);
    return conv;
  }

  /**
   * Register a conversation that was created externally (e.g. openConversation).
   */
  register(conversation: Conversation): void {
    if (!this.cache.has(conversation.conversationId)) {
      this.cache.set(conversation.conversationId, conversation);
      this.emitNewConversation(conversation);
    }
  }

  get(conversationId: string): Conversation | undefined {
    return this.cache.get(conversationId);
  }

  private buildConversation(
    conversationId: string,
    type: 'one_to_one' | 'group',
    members: Array<{ userId: string; deviceId: string }>,
  ): Conversation {
    if (type === 'one_to_one') {
      return new OneToOneConversation(
        conversationId,
        members,
        this.currentUserId,
        this.currentDeviceId,
        this.store,
        this.rest,
        this.attachmentClient,
      );
    }
    return new GroupConversation(
      conversationId,
      members,
      this.currentUserId,
      this.currentDeviceId,
      this.store,
      this.rest,
      this.attachmentClient,
      KeyGenerator,
      this.distributeSkdm,
    );
  }
}
