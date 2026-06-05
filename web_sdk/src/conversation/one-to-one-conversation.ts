// src/conversation/one-to-one-conversation.ts

import { Conversation, type AttachmentSend } from './conversation.js';
import type { ChatMessage, ConversationMember } from './chat-message.js';
import type { SessionStore } from '../session/session-store.js';
import type { RestClient } from '../transport/rest-client.js';
import type { AttachmentClient, AttachmentProgress } from '../transport/attachment-client.js';
import { RatchetEngine } from '../crypto/ratchet-engine.js';
import type { RtEvent } from '../transport/rt-event.js';
import { base64urlEncode, base64urlDecode, randomUuid } from '../utils/encoding.js';
import { SdkError, SdkErrorCode } from '../errors/sdk-error.js';

export class OneToOneConversation extends Conversation {
  readonly type = 'one_to_one' as const;

  constructor(
    readonly conversationId: string,
    readonly members: ConversationMember[],
    private readonly currentUserId: string,
    private readonly currentDeviceId: string,
    private readonly store: SessionStore,
    private readonly rest: RestClient,
    private readonly attachmentClient: AttachmentClient,
  ) {
    super();
  }

  async send(text: string): Promise<ChatMessage> {
    const state = await this.store.loadRatchetState(this.conversationId);
    if (!state) throw new SdkError(SdkErrorCode.SESSION_NOT_FOUND, `No ratchet state for ${this.conversationId}`);

    const session = RatchetEngine.deserialize(state);
    const plaintext = new TextEncoder().encode(text);
    const { ciphertext, header, nextSession } = await RatchetEngine.encrypt(session, plaintext);

    // Persist BEFORE network (forward secrecy)
    await this.store.saveRatchetState(
      this.conversationId,
      RatchetEngine.serialize(nextSession, this.conversationId),
    );

    const resp = await this.rest.sendMessage(this.conversationId, {
      envelope: {
        conversation_id: this.conversationId,
        ciphertext: base64urlEncode(ciphertext),
        protocol_header: {
          type: 'double_ratchet',
          dh: base64urlEncode(header.dh),
          n: header.n,
          pn: header.pn,
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
        (p) => { try { progressController.enqueue(p); } catch { /* stream closed */ } },
      );

      const state = await this.store.loadRatchetState(this.conversationId);
      if (!state) throw new SdkError(SdkErrorCode.SESSION_NOT_FOUND, `No ratchet state for ${this.conversationId}`);

      // Encrypt the attachment key and metadata
      const metaPlaintext = new TextEncoder().encode(JSON.stringify({
        attachmentId,
        encryptionKey: base64urlEncode(encryptionKey),
        filename,
        contentType,
      }));

      const session = RatchetEngine.deserialize(state);
      const { ciphertext, header, nextSession } = await RatchetEngine.encrypt(session, metaPlaintext);

      await this.store.saveRatchetState(
        this.conversationId,
        RatchetEngine.serialize(nextSession, this.conversationId),
      );

      const resp = await this.rest.sendMessage(this.conversationId, {
        envelope: {
          conversation_id: this.conversationId,
          ciphertext: base64urlEncode(ciphertext),
          protocol_header: {
            type: 'double_ratchet',
            dh: base64urlEncode(header.dh),
            n: header.n,
            pn: header.pn,
          },
          attachment_id: attachmentId,
        },
      });

      try { progressController.close(); } catch { /* already closed */ }

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
        messages.push(makeSystemMessage(m, this.currentUserId));
        continue;
      }
      try {
        const state = await this.store.loadRatchetState(this.conversationId);
        if (!state) throw new Error('No ratchet state');
        const session = RatchetEngine.deserialize(state);
        const header = m.envelope.protocol_header;
        if (header.type !== 'double_ratchet' || !header.dh) throw new Error('Bad header');

        const { plaintext, nextSession } = await RatchetEngine.decrypt(
          session,
          base64urlDecode(m.envelope.ciphertext),
          {
            type: 'double_ratchet',
            dh: base64urlDecode(header.dh),
            n: header.n ?? 0,
            pn: header.pn ?? 0,
          },
        );

        await this.store.saveRatchetState(
          this.conversationId,
          RatchetEngine.serialize(nextSession, this.conversationId),
        );

        const text = new TextDecoder().decode(plaintext);
        messages.push({
          id: m.id,
          conversationId: this.conversationId,
          senderId: m.sender_user_id,
          senderDeviceId: m.sender_device_id,
          type: m.type ?? 'text',
          text,
          attachmentId: m.envelope.attachment_id ?? null,
          attachmentUrl: m.attachment_url ?? null,
          attachmentName: m.attachment_name ?? null,
          timestamp: new Date(m.server_ts),
          seq: m.seq,
          isMine: m.sender_user_id === this.currentUserId,
          decryptionError: false,
        });
      } catch {
        messages.push({
          id: m.id,
          conversationId: this.conversationId,
          senderId: m.sender_user_id,
          senderDeviceId: m.sender_device_id,
          type: 'text',
          text: null,
          attachmentId: null,
          attachmentUrl: null,
          attachmentName: null,
          timestamp: new Date(m.server_ts),
          seq: m.seq,
          isMine: m.sender_user_id === this.currentUserId,
          decryptionError: true,
        });
      }
    }

    return messages;
  }

  markAsRead(): void {
    // Application-level; no-op in base SDK (server may have a separate endpoint)
  }

  async onIncomingMessage(event: Extract<RtEvent, { type: 'message' }>): Promise<void> {
    const state = await this.store.loadRatchetState(this.conversationId);
    if (!state) {
      this.emit('message', makeDecryptionErrorMessage(event, this.currentUserId));
      return;
    }

    const session = RatchetEngine.deserialize(state);
    const header = event.protocolHeader;

    if (header.type !== 'double_ratchet') {
      this.emit('message', makeDecryptionErrorMessage(event, this.currentUserId));
      return;
    }

    try {
      const { plaintext, nextSession } = await RatchetEngine.decrypt(
        session,
        event.ciphertext,
        { type: 'double_ratchet', dh: header.dh, n: header.n, pn: header.pn },
      );

      // Persist before emitting (forward secrecy Property 4)
      await this.store.saveRatchetState(
        this.conversationId,
        RatchetEngine.serialize(nextSession, this.conversationId),
      );

      const text = new TextDecoder().decode(plaintext);
      const msg: ChatMessage = {
        id: randomUuid(),
        conversationId: this.conversationId,
        senderId: event.senderUserId,
        senderDeviceId: event.senderDeviceId,
        type: 'text',
        text,
        attachmentId: event.attachmentId,
        attachmentUrl: null,
        attachmentName: null,
        timestamp: new Date(event.serverTs),
        seq: event.seq,
        isMine: false,
        decryptionError: false,
      };

      this.emit('message', msg);
    } catch {
      this.emit('message', makeDecryptionErrorMessage(event, this.currentUserId));
    }
  }
}

function makeDecryptionErrorMessage(
  event: Extract<RtEvent, { type: 'message' }>,
  _currentUserId: string,
): ChatMessage {
  return {
    id: randomUuid(),
    conversationId: event.conversationId,
    senderId: event.senderUserId,
    senderDeviceId: event.senderDeviceId,
    type: 'text',
    text: null,
    attachmentId: null,
    attachmentUrl: null,
    attachmentName: null,
    timestamp: new Date(event.serverTs),
    seq: event.seq,
    isMine: false,
    decryptionError: true,
  };
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function makeSystemMessage(m: any, currentUserId: string): ChatMessage {
  return {
    id: m.id,
    conversationId: m.conversation_id,
    senderId: m.sender_user_id,
    senderDeviceId: m.sender_device_id,
    type: m.type ?? 'text',
    text: null,
    attachmentId: null,
    attachmentUrl: null,
    attachmentName: null,
    timestamp: new Date(m.server_ts),
    seq: m.seq,
    isMine: m.sender_user_id === currentUserId,
    decryptionError: false,
  };
}
