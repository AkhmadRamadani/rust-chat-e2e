// src/session/memory-session-store.ts

import type {
  SessionStore,
  DeviceRecord,
  RatchetState,
  SenderKeyRecord,
  ConversationMeta,
} from './session-store.js';

/**
 * In-memory session store. No persistence — suitable for tests and ephemeral sessions.
 * @example
 * const client = await ChatClient.create({ ..., sessionStore: new MemorySessionStore() });
 */
export class MemorySessionStore implements SessionStore {
  private readonly devices = new Map<string, DeviceRecord>();
  private readonly ratchetStates = new Map<string, RatchetState>();
  private readonly senderKeys = new Map<string, SenderKeyRecord>();
  private readonly conversations = new Map<string, ConversationMeta>();

  async saveDevice(record: DeviceRecord): Promise<void> {
    this.devices.set(`${record.userId}::${record.deviceId}`, record);
  }

  async loadDevice(userId: string, deviceId: string): Promise<DeviceRecord | null> {
    return this.devices.get(`${userId}::${deviceId}`) ?? null;
  }

  async saveRatchetState(conversationId: string, state: RatchetState): Promise<void> {
    this.ratchetStates.set(conversationId, state);
  }

  async loadRatchetState(conversationId: string): Promise<RatchetState | null> {
    return this.ratchetStates.get(conversationId) ?? null;
  }

  async saveSenderKey(
    conversationId: string,
    userId: string,
    record: SenderKeyRecord,
  ): Promise<void> {
    this.senderKeys.set(`${conversationId}::${userId}`, record);
  }

  async loadSenderKey(
    conversationId: string,
    userId: string,
  ): Promise<SenderKeyRecord | null> {
    return this.senderKeys.get(`${conversationId}::${userId}`) ?? null;
  }

  async saveConversationMeta(meta: ConversationMeta): Promise<void> {
    this.conversations.set(meta.conversationId, meta);
  }

  async loadAllConversations(): Promise<ConversationMeta[]> {
    return Array.from(this.conversations.values());
  }

  async clear(): Promise<void> {
    this.devices.clear();
    this.ratchetStates.clear();
    this.senderKeys.clear();
    this.conversations.clear();
  }
}
