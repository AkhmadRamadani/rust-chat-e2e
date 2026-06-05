// src/transport/ws-manager.ts

import { TypedEventEmitter } from '../utils/typed-emitter.js';
import { parseRtEvent, type RtEvent } from './rt-event.js';
import type { Logger } from '../utils/logger.js';

export interface WsManagerEvents {
  frame: RtEvent;
  open: void;
  close: { code: number; reason: string };
  error: Event;
}

const PING_TIMEOUT_MS = 10_000;
const PONG_DEADLINE_MS = 5_000;

/**
 * WebSocket wrapper with ping/pong handling and frame parsing.
 * Reconnect logic is handled by ConnectionManager.
 */
export class WsManager {
  readonly events = new TypedEventEmitter<WsManagerEvents>();

  private ws: WebSocket | null = null;
  private pingTimer: ReturnType<typeof setInterval> | null = null;
  private pongDeadline: ReturnType<typeof setTimeout> | null = null;

  constructor(private readonly logger: Logger) {}

  connect(wsUrl: string): void {
    this.disconnect();

    this.logger.debug('WsManager connecting', wsUrl.replace(/token=[^&]+/, 'token=REDACTED'));
    const ws = new WebSocket(wsUrl);
    this.ws = ws;

    ws.onopen = () => {
      this.logger.debug('WebSocket opened');
      this.startPingTimer();
      this.events.emit('open');
    };

    ws.onmessage = (event) => {
      if (typeof event.data !== 'string') return;
      let raw: unknown;
      try {
        raw = JSON.parse(event.data);
      } catch {
        this.logger.warn('Failed to parse WS frame');
        return;
      }

      const rtEvent = parseRtEvent(raw);
      if (!rtEvent) return;

      if (rtEvent.type === 'ping') {
        this.sendPong();
        return;
      }

      this.events.emit('frame', rtEvent);
    };

    ws.onclose = (event) => {
      this.logger.debug('WebSocket closed', event.code, event.reason);
      this.clearTimers();
      this.events.emit('close', { code: event.code, reason: event.reason });
    };

    ws.onerror = (event) => {
      this.logger.warn('WebSocket error');
      this.events.emit('error', event);
    };
  }

  disconnect(): void {
    this.clearTimers();
    if (this.ws) {
      this.ws.onopen = null;
      this.ws.onmessage = null;
      this.ws.onclose = null;
      this.ws.onerror = null;
      if (this.ws.readyState === WebSocket.OPEN || this.ws.readyState === WebSocket.CONNECTING) {
        this.ws.close(1000, 'disconnect');
      }
      this.ws = null;
    }
  }

  sendAck(conversationId: string, seq: number): void {
    this.send(JSON.stringify({ type: 'ack', conversation_id: conversationId, seq }));
  }

  sendPong(): void {
    this.send(JSON.stringify({ type: 'pong' }));
    if (this.pongDeadline) {
      clearTimeout(this.pongDeadline);
      this.pongDeadline = null;
    }
  }

  private send(data: string): void {
    if (this.ws?.readyState === WebSocket.OPEN) {
      this.ws.send(data);
    }
  }

  private startPingTimer(): void {
    this.pingTimer = setInterval(() => {
      this.pongDeadline = setTimeout(() => {
        this.logger.warn('Pong deadline exceeded, reconnecting');
        this.ws?.close(4001, 'pong timeout');
      }, PONG_DEADLINE_MS);
    }, PING_TIMEOUT_MS);
  }

  private clearTimers(): void {
    if (this.pingTimer) { clearInterval(this.pingTimer); this.pingTimer = null; }
    if (this.pongDeadline) { clearTimeout(this.pongDeadline); this.pongDeadline = null; }
  }
}
