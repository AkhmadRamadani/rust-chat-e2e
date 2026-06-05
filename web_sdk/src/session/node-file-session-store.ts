// src/session/node-file-session-store.ts
// Only included in Node.js environments. Tree-shaken from browser bundles.

import type {
  SessionStore,
  DeviceRecord,
  RatchetState,
  SenderKeyRecord,
  ConversationMeta,
} from './session-store.js';

/**
 * Node.js filesystem-backed session store. Uses atomic write (tmp+rename).
 * @example
 * const store = new NodeFileSessionStore('./chat-sessions');
 */
export class NodeFileSessionStore implements SessionStore {
  private readonly dir: string;
  private fsPromises: typeof import('node:fs/promises') | null = null;
  private pathModule: typeof import('node:path') | null = null;

  constructor(storageDir: string) {
    this.dir = storageDir;
  }

  private async fs(): Promise<typeof import('node:fs/promises')> {
    if (!this.fsPromises) {
      this.fsPromises = await import('node:fs/promises');
    }
    return this.fsPromises;
  }

  private async path(): Promise<typeof import('node:path')> {
    if (!this.pathModule) {
      this.pathModule = await import('node:path');
    }
    return this.pathModule;
  }

  private async ensureDir(subDir: string): Promise<string> {
    const fs = await this.fs();
    const p = await this.path();
    const fullDir = p.join(this.dir, subDir);
    await fs.mkdir(fullDir, { recursive: true });
    return fullDir;
  }

  private async writeAtomic(filePath: string, data: string): Promise<void> {
    const fs = await this.fs();
    const tmpPath = `${filePath}.tmp`;
    await fs.writeFile(tmpPath, data, 'utf8');
    await fs.rename(tmpPath, filePath);
  }

  private async readJson<T>(filePath: string): Promise<T | null> {
    const fs = await this.fs();
    try {
      const content = await fs.readFile(filePath, 'utf8');
      return JSON.parse(content) as T;
    } catch {
      return null;
    }
  }

  async saveDevice(record: DeviceRecord): Promise<void> {
    const p = await this.path();
    const dir = await this.ensureDir('devices');
    const file = p.join(dir, `${record.userId}__${record.deviceId}.json`);
    await this.writeAtomic(file, JSON.stringify(record));
  }

  async loadDevice(userId: string, deviceId: string): Promise<DeviceRecord | null> {
    const p = await this.path();
    const dir = await this.ensureDir('devices');
    return this.readJson<DeviceRecord>(p.join(dir, `${userId}__${deviceId}.json`));
  }

  async saveRatchetState(conversationId: string, state: RatchetState): Promise<void> {
    const p = await this.path();
    const dir = await this.ensureDir('ratchet');
    await this.writeAtomic(p.join(dir, `${conversationId}.json`), JSON.stringify(state));
  }

  async loadRatchetState(conversationId: string): Promise<RatchetState | null> {
    const p = await this.path();
    const dir = await this.ensureDir('ratchet');
    return this.readJson<RatchetState>(p.join(dir, `${conversationId}.json`));
  }

  async saveSenderKey(conversationId: string, userId: string, record: SenderKeyRecord): Promise<void> {
    const p = await this.path();
    const dir = await this.ensureDir('senderkeys');
    await this.writeAtomic(p.join(dir, `${conversationId}__${userId}.json`), JSON.stringify(record));
  }

  async loadSenderKey(conversationId: string, userId: string): Promise<SenderKeyRecord | null> {
    const p = await this.path();
    const dir = await this.ensureDir('senderkeys');
    return this.readJson<SenderKeyRecord>(p.join(dir, `${conversationId}__${userId}.json`));
  }

  async saveConversationMeta(meta: ConversationMeta): Promise<void> {
    const p = await this.path();
    const dir = await this.ensureDir('conversations');
    await this.writeAtomic(p.join(dir, `${meta.conversationId}.json`), JSON.stringify(meta));
  }

  async loadAllConversations(): Promise<ConversationMeta[]> {
    const fs = await this.fs();
    const p = await this.path();
    const dir = await this.ensureDir('conversations');
    try {
      const files = await fs.readdir(dir);
      const results = await Promise.all(
        files.filter((f) => f.endsWith('.json')).map((f) =>
          this.readJson<ConversationMeta>(p.join(dir, f)),
        ),
      );
      return results.filter((r): r is ConversationMeta => r !== null);
    } catch {
      return [];
    }
  }

  async clear(): Promise<void> {
    const fs = await this.fs();
    try {
      await fs.rm(this.dir, { recursive: true, force: true });
    } catch {
      // ignore
    }
  }
}
