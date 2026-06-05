// src/transport/connection-manager.ts

import { TypedEventEmitter } from '../utils/typed-emitter.js';
import { WsManager } from './ws-manager.js';
import { sleep } from '../utils/retry.js';
import type { Logger } from '../utils/logger.js';

export type ConnectionState = 'connecting' | 'connected' | 'reconnecting' | 'disconnected';

const BACKOFF_DELAYS = [1000, 2000, 4000, 8000, 16000, 32000, 60000];

/**
 * Manages WebSocket connection lifecycle with exponential backoff reconnect.
 */
export class ConnectionManager {
  state: ConnectionState = 'disconnected';
  readonly stateChanges = new TypedEventEmitter<{ change: { state: ConnectionState } }>();

  private reconnecting = false;
  private destroyed = false;
  private connectResolvers: Array<() => void> = [];
  private backoffIndex = 0;

  constructor(
    private readonly wsManager: WsManager,
    private readonly getWsUrl: () => string,
    private readonly logger: Logger,
  ) {
    wsManager.events.on('open', () => {
      this.backoffIndex = 0;
      this.setState('connected');
      // Resolve all pending connect() promises
      const resolvers = this.connectResolvers.splice(0);
      for (const r of resolvers) r();
    });

    wsManager.events.on('close', ({ code }) => {
      if (this.destroyed) return;
      // Normal close (1000) only if we initiated it
      if (this.state !== 'disconnected') {
        this.scheduleReconnect();
      }
    });

    wsManager.events.on('error', () => {
      if (this.destroyed) return;
      if (this.state !== 'disconnected') {
        this.scheduleReconnect();
      }
    });
  }

  async ensureConnected(): Promise<void> {
    if (this.state === 'connected') return;
    if (this.state === 'connecting' || this.reconnecting) {
      return new Promise<void>((resolve) => {
        this.connectResolvers.push(resolve);
      });
    }
    this.doConnect();
    return new Promise<void>((resolve) => {
      this.connectResolvers.push(resolve);
    });
  }

  async disconnect(): Promise<void> {
    this.reconnecting = false;
    this.setState('disconnected');
    this.wsManager.disconnect();
  }

  destroy(): void {
    this.destroyed = true;
    this.wsManager.disconnect();
    this.setState('disconnected');
  }

  private doConnect(): void {
    this.setState('connecting');
    const url = this.getWsUrl();
    this.wsManager.connect(url);
  }

  private scheduleReconnect(): void {
    if (this.reconnecting || this.destroyed) return;
    this.reconnecting = true;
    this.setState('reconnecting');

    const delay = BACKOFF_DELAYS[Math.min(this.backoffIndex, BACKOFF_DELAYS.length - 1)] ?? 60000;
    this.backoffIndex++;

    this.logger.info(`Reconnecting in ${delay}ms`);

    sleep(delay).then(() => {
      if (this.destroyed) return;
      this.reconnecting = false;
      this.doConnect();
    });
  }

  private setState(state: ConnectionState): void {
    this.state = state;
    this.stateChanges.emit('change', { state });
  }
}
