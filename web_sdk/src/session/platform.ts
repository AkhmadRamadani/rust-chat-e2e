// src/session/platform.ts

import type { SessionStore } from './session-store.js';
import { IndexedDbSessionStore } from './indexed-db-session-store.js';
import { MemorySessionStore } from './memory-session-store.js';

/**
 * Detect the best default SessionStore for the current runtime.
 */
export function detectDefaultSessionStore(): SessionStore {
  if (typeof indexedDB !== 'undefined') {
    return new IndexedDbSessionStore();
  }
  return new MemorySessionStore();
}
