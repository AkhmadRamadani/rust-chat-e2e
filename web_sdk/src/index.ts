// src/index.ts — public surface of @rust-e2e-chat/sdk

export { ChatClient } from './client/chat-client.js';
export type { ChatClientEvents } from './client/chat-client.js';
export type { ChatClientConfig } from './client/chat-client-config.js';
export type { ConnectionState } from './transport/connection-manager.js';

export type { Conversation, ConversationEvents, AttachmentSend } from './conversation/conversation.js';
export type { ChatMessage, ConversationMember } from './conversation/chat-message.js';

export type { KeyBundle } from './crypto/crypto-types.js';

export type {
  SessionStore,
  DeviceRecord,
  RatchetState,
  SenderKeyRecord,
  ConversationMeta,
} from './session/session-store.js';

export { MemorySessionStore } from './session/memory-session-store.js';
export { IndexedDbSessionStore } from './session/indexed-db-session-store.js';

export { SdkError, SdkErrorCode } from './errors/sdk-error.js';

export type { AttachmentProgress } from './transport/attachment-client.js';
