// src/transport/rt-event.ts

import { base64urlDecode } from '../utils/encoding.js';

export type ProtocolHeader =
  | {
      type: 'x3dh_init';
      ek: Uint8Array;
      spkId: number;
      otpkId?: number;
    }
  | {
      type: 'double_ratchet';
      dh: Uint8Array;
      n: number;
      pn: number;
    }
  | {
      type: 'sender_key';
      chainId: number;
      iteration: number;
    };

export type RtEvent =
  | {
      type: 'message';
      conversationId: string;
      seq: number;
      senderUserId: string;
      senderDeviceId: string;
      ciphertext: Uint8Array;
      protocolHeader: ProtocolHeader;
      serverTs: number;
      attachmentId: string | null;
    }
  | {
      type: 'low_otpk';
      deviceId: string;
      count: number;
    }
  | {
      type: 'member_added';
      conversationId: string;
      userId: string;
      devices: string[];
    }
  | {
      type: 'member_removed';
      conversationId: string;
      userId: string;
    }
  | {
      type: 'sender_key_distribution';
      conversationId: string;
      senderUserId: string;
      encryptedSkdm: Uint8Array;
    }
  | {
      type: 'ping';
    };

// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function parseRtEvent(raw: any): RtEvent | null {
  if (!raw || typeof raw.type !== 'string') return null;

  switch (raw.type) {
    case 'message': {
      if (!raw.envelope) return null;
      const header = parseProtocolHeader(raw.envelope.protocol_header);
      if (!header) return null;
      return {
        type: 'message',
        conversationId: String(raw.conversation_id ?? raw.envelope.conversation_id),
        seq: Number(raw.seq),
        senderUserId: String(raw.sender_user_id),
        senderDeviceId: String(raw.sender_device_id),
        ciphertext: base64urlDecode(String(raw.envelope.ciphertext)),
        protocolHeader: header,
        serverTs: Number(raw.server_ts),
        attachmentId: raw.envelope.attachment_id ?? null,
      };
    }
    case 'low_otpk':
      return {
        type: 'low_otpk',
        deviceId: String(raw.device_id),
        count: Number(raw.count),
      };
    case 'member_added':
      return {
        type: 'member_added',
        conversationId: String(raw.conversation_id),
        userId: String(raw.user_id),
        devices: Array.isArray(raw.devices) ? raw.devices.map(String) : [],
      };
    case 'member_removed':
      return {
        type: 'member_removed',
        conversationId: String(raw.conversation_id),
        userId: String(raw.user_id),
      };
    case 'sender_key_distribution':
      return {
        type: 'sender_key_distribution',
        conversationId: String(raw.conversation_id),
        senderUserId: String(raw.sender_user_id),
        encryptedSkdm: base64urlDecode(String(raw.encrypted_skdm)),
      };
    case 'ping':
      return { type: 'ping' };
    default:
      // Unknown event types silently discarded for forward compatibility
      return null;
  }
}

// eslint-disable-next-line @typescript-eslint/no-explicit-any
function parseProtocolHeader(h: any): ProtocolHeader | null {
  if (!h || typeof h.type !== 'string') return null;
  switch (h.type) {
    case 'x3dh_init':
      return {
        type: 'x3dh_init',
        ek: base64urlDecode(String(h.ek)),
        spkId: Number(h.spk_id),
        ...(h.otpk_id !== undefined ? { otpkId: Number(h.otpk_id) } : {}),
      } as ProtocolHeader;
    case 'double_ratchet':
      return {
        type: 'double_ratchet',
        dh: base64urlDecode(String(h.dh)),
        n: Number(h.n),
        pn: Number(h.pn),
      };
    case 'sender_key':
      return {
        type: 'sender_key',
        chainId: Number(h.chain_id),
        iteration: Number(h.iteration),
      };
    default:
      return null;
  }
}
