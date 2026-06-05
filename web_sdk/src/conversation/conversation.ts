// src/conversation/conversation.ts

import { TypedEventEmitter } from '../utils/typed-emitter.js';
import type { ChatMessage, ConversationMember } from './chat-message.js';
import type { AttachmentProgress } from '../transport/attachment-client.js';

export interface ConversationEvents {
  message: ChatMessage;
  member_added: { userId: string; devices: string[] };
  member_removed: { userId: string };
}

export interface AttachmentSend {
  promise: Promise<ChatMessage>;
  progress: ReadableStream<AttachmentProgress>;
}

/**
 * Observable object representing a single 1:1 or group conversation thread.
 */
export abstract class Conversation extends TypedEventEmitter<ConversationEvents> {
  abstract readonly conversationId: string;
  abstract readonly type: 'one_to_one' | 'group';
  abstract readonly members: ConversationMember[];

  /**
   * Send a text message.
   * @example
   * const msg = await conversation.send('Hello!');
   * console.log(msg.id, msg.timestamp);
   */
  abstract send(text: string): Promise<ChatMessage>;

  /**
   * Send an encrypted attachment.
   * @example
   * const { promise, progress } = conversation.sendAttachment(file, file.name, file.type);
   * const reader = progress.getReader();
   * // read progress updates...
   * const msg = await promise;
   */
  abstract sendAttachment(
    file: File | Blob | BufferSource,
    filename: string,
    contentType: string,
  ): AttachmentSend;

  /**
   * Fetch message history.
   * @example
   * const messages = await conversation.fetchHistory({ limit: 50 });
   */
  abstract fetchHistory(options?: { limit?: number; beforeSeq?: number }): Promise<ChatMessage[]>;

  /**
   * Mark all messages as read.
   */
  abstract markAsRead(): void;

  /**
   * Add a member to the group (group only — throws for 1:1).
   */
  addMember(_userId: string): Promise<void> {
    throw new Error('addMember is only available on group conversations');
  }

  /**
   * Remove a member from the group (group only — throws for 1:1).
   */
  removeMember(_userId: string): Promise<void> {
    throw new Error('removeMember is only available on group conversations');
  }
}
