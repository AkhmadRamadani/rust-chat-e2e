// src/conversation/chat-message.ts

/**
 * The SDK's decrypted, developer-facing message object.
 */
export interface ChatMessage {
  readonly id: string;
  readonly conversationId: string;
  readonly senderId: string;
  readonly senderDeviceId: string | null;
  readonly type: 'text' | 'attachment' | 'member_added' | 'member_removed';
  readonly text: string | null;
  readonly attachmentId: string | null;
  readonly attachmentUrl: string | null;
  readonly attachmentName: string | null;
  readonly timestamp: Date;
  readonly seq: number;
  readonly isMine: boolean;
  readonly decryptionError: boolean;
}

export interface ConversationMember {
  userId: string;
  deviceId: string;
}
