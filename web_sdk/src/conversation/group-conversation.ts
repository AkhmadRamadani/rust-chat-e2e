// src/conversation/group-conversation.ts

import { Conversation, type AttachmentSend } from './conversation.js';
import type { ChatMessage, ConversationMember } from './chat-message.js';
import type { SessionStore } from '../session/session-store.js';
import type { RestClient } from '../transport/rest-client.js';
import type { AttachmentClient, AttachmentProgress } from '../transport/attachment-client.js';
import { SenderKeyEngine } from '../crypto/sender-key-engine.js';
import { RatchetEngine } from '../crypto/ratchet-engine.js';
import type { RtEvent } from '../transport/rt-event.js';
import { base64urlEncode, randomUuid } from '../utils/encoding.js';
import { SdkError, SdkErrorCode } from '../errors/sdk-error.js';
import type { KeyGenerator } from '../crypto/key-generator.js';

export class GroupConversation extends Conversation {
  readonly type = 'group' as const;

  // Pending SKDM queue: if we receive a message before its SKDM, hold it up to 60s
  private readonly pendingMessages = new Map<string, { event: Extract<RtEvent, { type: 'message' }>; timestamp: number }>();

  constructor(
    readonly conversationId: string,
    public members: ConversationMember[],
    private readonly currentUserId: string,
    private readonly currentDeviceId: string,
    private readonly store: SessionStore,
    private readonly rest: RestClient,
    private readonly attachmentClient: AttachmentClient,
    private readonly keyGenerator: typeof KeyGenerator,
    private readonly distributeSkdm: (conversationId: string, memberUserIds: string[]) => Promise<void>,
  ) {
    super();
  }

  async send(text: string): Promise<ChatMessage> {
    const record = await this.store.loadSenderKey(this.conversationId, this.currentUserId);
    if (!record) throw new SdkError(SdkErrorCode.SESSION_NOT_FOUND, 'No sender key for group');

    const session = await SenderKeyEngine.fromRecord(record);
    const plaintext = new TextEncoder().encode(text);
    const { ciphertext, nextSession } = await SenderKeyEngine.encrypt(session, plaintext);

    const nextRecord = await SenderKeyEngine.toRecord(nextSession, this.conversationId, this.currentUserId);
    await this.store.saveSenderKey(this.conversationId, this.currentUserId, nextRecord);

    const resp = await this.rest.sendMessage(this.conversationId, {
      envelope: {
        conversation_id: this.conversationId,
        ciphertext: base64urlEncode(ciphertext),
        protocol_header: {
          type: 'sender_key',
          chain_id: session.chainId,
          iteration: session.iteration,
        },
      },
    });

    const msg: ChatMessage = {
      id: randomUuid(),
      conversationId: this.conversationId,
      senderId: this.currentUserId,
      senderDeviceId: this.currentDeviceId,
      type: 'text',
      text,
      attachmentId: null,
      attachmentUrl: null,
      attachmentName: null,
      timestamp: new Date(resp.server_ts),
      seq: resp.seq,
      isMine: true,
      decryptionError: false,
    };

    this.emit('message', msg);
    return msg;
  }

  sendAttachment(
    file: File | Blob | BufferSource,
    filename: string,
    contentType: string,
  ): AttachmentSend {
    let progressController!: ReadableStreamDefaultController<AttachmentProgress>;
    const progress = new ReadableStream<AttachmentProgress>({
      start(controller) { progressController = controller; },
    });

    const promise = (async (): Promise<ChatMessage> => {
      const { attachmentId, encryptionKey } = await this.attachmentClient.upload(
        file, filename, contentType,
        (p) => { try { progressController.enqueue(p); } catch { /* closed */ } },
      );

      const record = await this.store.loadSenderKey(this.conversationId, this.currentUserId);
      if (!record) throw new SdkError(SdkErrorCode.SESSION_NOT_FOUND, 'No sender key');

      const session = await SenderKeyEngine.fromRecord(record);
      const metaPlaintext = new TextEncoder().encode(JSON.stringify({
        attachmentId,
        encryptionKey: base64urlEncode(encryptionKey),
        filename,
      }));
      const { ciphertext, nextSession } = await SenderKeyEngine.encrypt(session, metaPlaintext);
      await this.store.saveSenderKey(
        this.conversationId, this.currentUserId,
        await SenderKeyEngine.toRecord(nextSession, this.conversationId, this.currentUserId),
      );

      const resp = await this.rest.sendMessage(this.conversationId, {
        envelope: {
          conversation_id: this.conversationId,
          ciphertext: base64urlEncode(ciphertext),
          protocol_header: { type: 'sender_key', chain_id: session.chainId, iteration: session.iteration },
          attachment_id: attachmentId,
        },
      });

      try { progressController.close(); } catch { /* ignore */ }

      const msg: ChatMessage = {
        id: randomUuid(),
        conversationId: this.conversationId,
        senderId: this.currentUserId,
        senderDeviceId: this.currentDeviceId,
        type: 'attachment',
        text: null,
        attachmentId,
        attachmentUrl: null,
        attachmentName: filename,
        timestamp: new Date(resp.server_ts),
        seq: resp.seq,
        isMine: true,
        decryptionError: false,
      };

      this.emit('message', msg);
      return msg;
    })();

    return { promise, progress };
  }

  async fetchHistory(options?: { limit?: number; beforeSeq?: number }): Promise<ChatMessage[]> {
    const resp = await this.rest.getMessages(this.conversationId, options);
    const messages: ChatMessage[] = [];

    for (const m of resp.messages) {
      if (!m.envelope) {
        messages.push({
          id: m.id, conversationId: this.conversationId,
          senderId: m.sender_user_id, senderDeviceId: m.sender_device_id,
          type: m.type ?? 'text', text: null, attachmentId: null,
          attachmentUrl: null, attachmentName: null,
          timestamp: new Date(m.server_ts), seq: m.seq,
          isMine: m.sender_user_id === this.currentUserId, decryptionError: false,
        });
        continue;
      }
      try {
        const record = await this.store.loadSenderKey(this.conversationId, m.sender_user_id);
        if (!record) throw new Error('No sender key');
        const session = await SenderKeyEngine.fromRecord(record);
        const { plaintext, nextSession } = await SenderKeyEngine.decrypt(
          session,
          Buffer.from(m.envelope.ciphertext, 'base64'),
        );
        await this.store.saveSenderKey(
          this.conversationId, m.sender_user_id,
          await SenderKeyEngine.toRecord(nextSession, this.conversationId, m.sender_user_id),
        );
        messages.push({
          id: m.id, conversationId: this.conversationId,
          senderId: m.sender_user_id, senderDeviceId: m.sender_device_id,
          type: 'text', text: new TextDecoder().decode(plaintext),
          attachmentId: m.envelope.attachment_id ?? null,
          attachmentUrl: m.attachment_url ?? null, attachmentName: m.attachment_name ?? null,
          timestamp: new Date(m.server_ts), seq: m.seq,
          isMine: m.sender_user_id === this.currentUserId, decryptionError: false,
        });
      } catch {
        messages.push({
          id: m.id, conversationId: this.conversationId,
          senderId: m.sender_user_id, senderDeviceId: m.sender_device_id,
          type: 'text', text: null, attachmentId: null, attachmentUrl: null, attachmentName: null,
          timestamp: new Date(m.server_ts), seq: m.seq,
          isMine: m.sender_user_id === this.currentUserId, decryptionError: true,
        });
      }
    }
    return messages;
  }

  markAsRead(): void { /* no-op */ }

  override async addMember(userId: string): Promise<void> {
    const bundle = await this.rest.getKeyBundle(userId);
    await this.rest.addGroupMember(this.conversationId, userId, bundle.device_id);
    await this.distributeSkdm(this.conversationId, [userId]);
  }

  override async removeMember(userId: string): Promise<void> {
    await this.rest.removeGroupMember(this.conversationId, userId);
    // Generate new sender key (Property 5: Sender Key Isolation)
    const material = await this.keyGenerator.generateSenderKey();
    const newSession = await SenderKeyEngine.createSession(material);
    await this.store.saveSenderKey(
      this.conversationId, this.currentUserId,
      await SenderKeyEngine.toRecord(newSession, this.conversationId, this.currentUserId),
    );

    // Distribute to remaining members (excluding removed user)
    const remaining = this.members
      .filter((m) => m.userId !== userId)
      .map((m) => m.userId);
    await this.distributeSkdm(this.conversationId, remaining);

    this.members = this.members.filter((m) => m.userId !== userId);
    this.emit('member_removed', { userId });
  }

  async onSkdm(event: Extract<RtEvent, { type: 'sender_key_distribution' }>): Promise<void> {
    try {
      // SKDM is encrypted with our 1:1 ratchet - that's handled by the calling layer
      // Here we receive already-decrypted SenderKeySession bytes
      const session = await SenderKeyEngine.deserializeKeyMaterial(event.encryptedSkdm);
      await this.store.saveSenderKey(
        this.conversationId, event.senderUserId,
        await SenderKeyEngine.toRecord(session, this.conversationId, event.senderUserId),
      );

      // Flush any pending messages from this sender
      const pending = this.pendingMessages.get(event.senderUserId);
      if (pending) {
        this.pendingMessages.delete(event.senderUserId);
        await this.onIncomingMessage(pending.event);
      }
    } catch {
      // ignore SKDM errors
    }
  }

  async onMemberChange(event: Extract<RtEvent, { type: 'member_added' | 'member_removed' }>): Promise<void> {
    if (event.type === 'member_added') {
      for (const deviceId of event.devices) {
        if (!this.members.some((m) => m.userId === event.userId && m.deviceId === deviceId)) {
          this.members.push({ userId: event.userId, deviceId });
        }
      }
      this.emit('member_added', { userId: event.userId, devices: event.devices });
    } else {
      this.members = this.members.filter((m) => m.userId !== event.userId);
      this.emit('member_removed', { userId: event.userId });
    }
  }

  async onIncomingMessage(event: Extract<RtEvent, { type: 'message' }>): Promise<void> {
    const record = await this.store.loadSenderKey(this.conversationId, event.senderUserId);
    if (!record) {
      // Queue up to 60s
      if (!this.pendingMessages.has(event.senderUserId)) {
        this.pendingMessages.set(event.senderUserId, { event, timestamp: Date.now() });
        setTimeout(() => {
          const p = this.pendingMessages.get(event.senderUserId);
          if (p && Date.now() - p.timestamp >= 60_000) {
            this.pendingMessages.delete(event.senderUserId);
            this.emit('message', makeDecryptionErrorMsg(event));
          }
        }, 60_000);
      }
      return;
    }

    try {
      const session = await SenderKeyEngine.fromRecord(record);
      const { plaintext, nextSession } = await SenderKeyEngine.decrypt(session, event.ciphertext);
      await this.store.saveSenderKey(
        this.conversationId, event.senderUserId,
        await SenderKeyEngine.toRecord(nextSession, this.conversationId, event.senderUserId),
      );
      const text = new TextDecoder().decode(plaintext);
      this.emit('message', {
        id: randomUuid(), conversationId: this.conversationId,
        senderId: event.senderUserId, senderDeviceId: event.senderDeviceId,
        type: 'text', text, attachmentId: event.attachmentId,
        attachmentUrl: null, attachmentName: null,
        timestamp: new Date(event.serverTs), seq: event.seq,
        isMine: false, decryptionError: false,
      });
    } catch {
      this.emit('message', makeDecryptionErrorMsg(event));
    }
  }
}

function makeDecryptionErrorMsg(event: Extract<RtEvent, { type: 'message' }>): ChatMessage {
  return {
    id: randomUuid(), conversationId: event.conversationId,
    senderId: event.senderUserId, senderDeviceId: event.senderDeviceId,
    type: 'text', text: null, attachmentId: null, attachmentUrl: null, attachmentName: null,
    timestamp: new Date(event.serverTs), seq: event.seq,
    isMine: false, decryptionError: true,
  };
}
