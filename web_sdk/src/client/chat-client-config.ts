// src/client/chat-client-config.ts

import type { SessionStore } from '../session/session-store.js';
import type { LogLevel } from '../utils/logger.js';

/**
 * Configuration for ChatClient.create().
 */
export interface ChatClientConfig {
  /** Base URL of the API server, e.g. "https://api.example.com" */
  baseUrl: string;
  /** OIDC JWT access token */
  accessToken: string;
  /** OIDC sub claim — the caller's user ID */
  userId: string;
  /** Device ID — if undefined, a new device will be registered */
  deviceId?: string;
  /** Custom session store. Defaults to IndexedDbSessionStore in browsers, MemorySessionStore elsewhere */
  sessionStore?: SessionStore;
  /** Connect automatically on create(). Default: true */
  autoConnect?: boolean;
  /** Days between SignedPreKey rotations. Default: 7 */
  signedPrekeyRotationDays?: number;
  /** Log level. Default: 'warn' */
  logLevel?: LogLevel;
  /** Custom WebSocket constructor (Node.js < 21 without global WebSocket) */
  WebSocketImpl?: typeof WebSocket;
}
