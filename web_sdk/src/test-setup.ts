// src/test-setup.ts
// Polyfill WebCrypto for Node.js test environments
import { webcrypto } from 'node:crypto';

if (typeof globalThis.crypto === 'undefined') {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  (globalThis as any).crypto = webcrypto;
}
