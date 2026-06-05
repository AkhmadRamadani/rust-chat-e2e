// src/__tests__/encoding.test.ts

import { describe, it, expect } from 'vitest';
import {
  base64urlEncode,
  base64urlDecode,
  hexEncode,
  hexDecode,
  concatBytes,
  randomUuid,
} from '../utils/encoding.js';

describe('base64url', () => {
  it('round-trips arbitrary bytes', () => {
    const original = new Uint8Array([0, 1, 127, 128, 255, 31, 200]);
    expect(base64urlDecode(base64urlEncode(original))).toEqual(original);
  });

  it('produces URL-safe alphabet (no +, /, =)', () => {
    const bytes = new Uint8Array(64).fill(0xff);
    const encoded = base64urlEncode(bytes);
    expect(encoded).not.toContain('+');
    expect(encoded).not.toContain('/');
    expect(encoded).not.toContain('=');
  });

  it('decodes padded base64url', () => {
    const bytes = new Uint8Array([1, 2, 3]);
    const encoded = base64urlEncode(bytes);
    expect(base64urlDecode(encoded)).toEqual(bytes);
  });
});

describe('hex', () => {
  it('round-trips bytes', () => {
    const b = new Uint8Array([0xde, 0xad, 0xbe, 0xef]);
    expect(hexEncode(b)).toBe('deadbeef');
    expect(hexDecode('deadbeef')).toEqual(b);
  });
});

describe('concatBytes', () => {
  it('concatenates multiple arrays', () => {
    const a = new Uint8Array([1, 2]);
    const b = new Uint8Array([3, 4]);
    const c = new Uint8Array([5]);
    expect(concatBytes(a, b, c)).toEqual(new Uint8Array([1, 2, 3, 4, 5]));
  });
});

describe('randomUuid', () => {
  it('produces valid UUID v4 format', () => {
    const uuid = randomUuid();
    expect(uuid).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
  });

  it('generates unique values', () => {
    const ids = new Set(Array.from({ length: 100 }, () => randomUuid()));
    expect(ids.size).toBe(100);
  });
});
