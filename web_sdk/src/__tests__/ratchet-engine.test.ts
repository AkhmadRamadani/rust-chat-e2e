// src/__tests__/ratchet-engine.test.ts

import { describe, it, expect } from 'vitest';
import { RatchetEngine } from '../crypto/ratchet-engine.js';
import { generateX25519KeyPair, exportPublicKeyRaw } from '../crypto/crypto-utils.js';

async function initPair() {
  const sharedSecret = new Uint8Array(32);
  crypto.getRandomValues(sharedSecret);

  const bobKp = await generateX25519KeyPair();
  const bobPub = await exportPublicKeyRaw(bobKp.publicKey);

  const aliceSend = await RatchetEngine.initSender(sharedSecret, bobPub);
  const bobRecv = await RatchetEngine.initReceiver(sharedSecret, bobKp);

  return { aliceSend, bobRecv };
}

describe('RatchetEngine', () => {
  it('encrypts and decrypts a single message', async () => {
    const { aliceSend, bobRecv } = await initPair();
    const plaintext = new TextEncoder().encode('hello world');

    const { ciphertext, header, nextSession: aliceNext } = await RatchetEngine.encrypt(aliceSend, plaintext);
    const { plaintext: decrypted } = await RatchetEngine.decrypt(bobRecv, ciphertext, header);

    expect(new TextDecoder().decode(decrypted)).toBe('hello world');
    expect(aliceNext.nSend).toBe(1);
  });

  it('supports multiple messages in sequence', async () => {
    const { aliceSend, bobRecv } = await initPair();

    let aliceSession = aliceSend;
    let bobSession = bobRecv;
    const messages = ['msg1', 'msg2', 'msg3'];

    for (const text of messages) {
      const { ciphertext, header, nextSession } = await RatchetEngine.encrypt(
        aliceSession, new TextEncoder().encode(text),
      );
      aliceSession = nextSession;
      const { plaintext, nextSession: bobNext } = await RatchetEngine.decrypt(bobSession, ciphertext, header);
      bobSession = bobNext;
      expect(new TextDecoder().decode(plaintext)).toBe(text);
    }
  });

  it('handles out-of-order delivery via skipped keys', async () => {
    const { aliceSend, bobRecv } = await initPair();

    const plaintext1 = new TextEncoder().encode('first');
    const plaintext2 = new TextEncoder().encode('second');

    const { ciphertext: ct1, header: h1, nextSession: a1 } = await RatchetEngine.encrypt(aliceSend, plaintext1);
    const { ciphertext: ct2, header: h2, nextSession: _a2 } = await RatchetEngine.encrypt(a1, plaintext2);

    // Deliver msg2 first
    const { plaintext: dec2, nextSession: b1 } = await RatchetEngine.decrypt(bobRecv, ct2, h2);
    expect(new TextDecoder().decode(dec2)).toBe('second');

    // Then deliver msg1 (should use skipped key)
    const { plaintext: dec1 } = await RatchetEngine.decrypt(b1, ct1, h1);
    expect(new TextDecoder().decode(dec1)).toBe('first');
  });

  it('serializes and deserializes session state', async () => {
    const { aliceSend } = await initPair();
    const serialized = RatchetEngine.serialize(aliceSend, 'conv-1');
    const restored = RatchetEngine.deserialize(serialized);

    expect(restored.nSend).toBe(aliceSend.nSend);
    expect(Buffer.from(restored.rootKey).toString('hex'))
      .toBe(Buffer.from(aliceSend.rootKey).toString('hex'));
  });

  it('different ciphertext for same plaintext (IVs are random)', async () => {
    const { aliceSend } = await initPair();
    const pt = new TextEncoder().encode('same message');
    const { ciphertext: ct1 } = await RatchetEngine.encrypt(aliceSend, pt);
    const { ciphertext: ct2 } = await RatchetEngine.encrypt(aliceSend, pt);
    expect(Buffer.from(ct1).toString('hex')).not.toBe(Buffer.from(ct2).toString('hex'));
  });
});
