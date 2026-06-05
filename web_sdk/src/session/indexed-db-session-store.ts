// src/session/indexed-db-session-store.ts

import type {
  SessionStore,
  DeviceRecord,
  RatchetState,
  SenderKeyRecord,
  ConversationMeta,
} from './session-store.js';
import { MemorySessionStore } from './memory-session-store.js';

const DB_NAME = 'rce_sdk';
const DB_VERSION = 1;

function promisifyRequest<T>(request: IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

function promisifyTransaction(tx: IDBTransaction): Promise<void> {
  return new Promise((resolve, reject) => {
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
    tx.onabort = () => reject(tx.error);
  });
}

/**
 * Browser IndexedDB-backed session store. Default in browser environments.
 * Falls back to in-memory cache on IDB failure.
 * @example
 * const store = new IndexedDbSessionStore();
 */
export class IndexedDbSessionStore implements SessionStore {
  private db: IDBDatabase | null = null;
  private dbPromise: Promise<IDBDatabase> | null = null;
  private readonly fallback = new MemorySessionStore();
  private useFallback = false;

  private openDb(): Promise<IDBDatabase> {
    if (this.db) return Promise.resolve(this.db);
    if (this.dbPromise) return this.dbPromise;

    this.dbPromise = new Promise((resolve, reject) => {
      const request = indexedDB.open(DB_NAME, DB_VERSION);

      request.onupgradeneeded = (event) => {
        const db = (event.target as IDBOpenDBRequest).result;
        if (!db.objectStoreNames.contains('devices')) {
          db.createObjectStore('devices');
        }
        if (!db.objectStoreNames.contains('ratchetStates')) {
          db.createObjectStore('ratchetStates');
        }
        if (!db.objectStoreNames.contains('senderKeys')) {
          db.createObjectStore('senderKeys');
        }
        if (!db.objectStoreNames.contains('conversations')) {
          db.createObjectStore('conversations');
        }
      };

      request.onsuccess = () => {
        this.db = request.result;
        resolve(request.result);
      };

      request.onerror = () => {
        this.useFallback = true;
        reject(request.error);
      };
    });

    return this.dbPromise;
  }

  private async withStore<T>(
    storeName: string,
    mode: IDBTransactionMode,
    fn: (store: IDBObjectStore) => IDBRequest<T>,
  ): Promise<T> {
    if (this.useFallback) {
      throw new Error('IDB unavailable');
    }
    const db = await this.openDb();
    const tx = db.transaction(storeName, mode);
    const store = tx.objectStore(storeName);
    const request = fn(store);
    const result = await promisifyRequest(request);
    await promisifyTransaction(tx);
    return result;
  }

  private async safeRun<T>(
    op: () => Promise<T>,
    fallbackOp: () => Promise<T>,
  ): Promise<T> {
    if (this.useFallback) return fallbackOp();
    try {
      return await op();
    } catch {
      this.useFallback = true;
      return fallbackOp();
    }
  }

  async saveDevice(record: DeviceRecord): Promise<void> {
    await this.safeRun(
      () => this.withStore('devices', 'readwrite', (s) => s.put(record, `${record.userId}::${record.deviceId}`)).then(() => undefined),
      () => this.fallback.saveDevice(record),
    );
  }

  async loadDevice(userId: string, deviceId: string): Promise<DeviceRecord | null> {
    return this.safeRun(
      () => this.withStore<DeviceRecord | undefined>('devices', 'readonly', (s) => s.get(`${userId}::${deviceId}`)).then(v => v ?? null),
      () => this.fallback.loadDevice(userId, deviceId),
    );
  }

  async saveRatchetState(conversationId: string, state: RatchetState): Promise<void> {
    await this.safeRun(
      () => this.withStore('ratchetStates', 'readwrite', (s) => s.put(state, conversationId)).then(() => undefined),
      () => this.fallback.saveRatchetState(conversationId, state),
    );
  }

  async loadRatchetState(conversationId: string): Promise<RatchetState | null> {
    return this.safeRun(
      () => this.withStore<RatchetState | undefined>('ratchetStates', 'readonly', (s) => s.get(conversationId)).then(v => v ?? null),
      () => this.fallback.loadRatchetState(conversationId),
    );
  }

  async saveSenderKey(conversationId: string, userId: string, record: SenderKeyRecord): Promise<void> {
    await this.safeRun(
      () => this.withStore('senderKeys', 'readwrite', (s) => s.put(record, `${conversationId}::${userId}`)).then(() => undefined),
      () => this.fallback.saveSenderKey(conversationId, userId, record),
    );
  }

  async loadSenderKey(conversationId: string, userId: string): Promise<SenderKeyRecord | null> {
    return this.safeRun(
      () => this.withStore<SenderKeyRecord | undefined>('senderKeys', 'readonly', (s) => s.get(`${conversationId}::${userId}`)).then(v => v ?? null),
      () => this.fallback.loadSenderKey(conversationId, userId),
    );
  }

  async saveConversationMeta(meta: ConversationMeta): Promise<void> {
    await this.safeRun(
      () => this.withStore('conversations', 'readwrite', (s) => s.put(meta, meta.conversationId)).then(() => undefined),
      () => this.fallback.saveConversationMeta(meta),
    );
  }

  async loadAllConversations(): Promise<ConversationMeta[]> {
    return this.safeRun(
      async () => {
        const db = await this.openDb();
        return new Promise<ConversationMeta[]>((resolve, reject) => {
          const tx = db.transaction('conversations', 'readonly');
          const store = tx.objectStore('conversations');
          const req = store.getAll();
          req.onsuccess = () => resolve(req.result as ConversationMeta[]);
          req.onerror = () => reject(req.error);
        });
      },
      () => this.fallback.loadAllConversations(),
    );
  }

  async clear(): Promise<void> {
    await this.safeRun(
      async () => {
        const db = await this.openDb();
        const stores = ['devices', 'ratchetStates', 'senderKeys', 'conversations'];
        await new Promise<void>((resolve, reject) => {
          const tx = db.transaction(stores, 'readwrite');
          for (const name of stores) tx.objectStore(name).clear();
          tx.oncomplete = () => resolve();
          tx.onerror = () => reject(tx.error);
        });
      },
      () => this.fallback.clear(),
    );
  }
}
