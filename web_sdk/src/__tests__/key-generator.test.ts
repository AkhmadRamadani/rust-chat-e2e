// src/__tests__/key-generator.test.ts

import { describe, it, expect } from 'vitest';
import { KeyGenerator } from '../crypto/key-generator.js';
import { exportPublicKeyRaw, verifySPKSignature } from '../crypto/crypto-utils.js';

describe('KeyGenerator', () => {
  it('generateKeyBundle produces valid bundle', async () => {
    const bundle = await KeyGenerator.generateKeyBundle(5);
    expect(bundle.otpks).toHaveLength(5);
    expect(bundle.nextOtpkId).toBe(6);
    expect(bundle.signedPrekey.id).toBe(1);
  });

  it('SPK signature self-verifies', async () => {
    const bundle = await KeyGenerator.generateKeyBundle(1);
    const spkPub = await exportPublicKeyRaw(bundle.signedPrekey.keyPair.publicKey);
    const valid = await verifySPKSignature(
      spkPub,
      bundle.signedPrekey.signature,
      bundle.identityKeyEd.publicKey,
    );
    expect(valid).toBe(true);
  });

  it('OTPKs have sequential unique IDs', async () => {
    const otpks = await KeyGenerator.generateOtpks(10, 5);
    expect(otpks).toHaveLength(10);
    const ids = otpks.map((o) => o.id);
    expect(ids).toEqual([5, 6, 7, 8, 9, 10, 11, 12, 13, 14]);
  });

  it('all OTPKs have distinct public keys', async () => {
    const otpks = await KeyGenerator.generateOtpks(5, 1);
    const pubKeys = await Promise.all(
      otpks.map((o) => exportPublicKeyRaw(o.keyPair.publicKey).then((b) => Buffer.from(b).toString('hex'))),
    );
    const unique = new Set(pubKeys);
    expect(unique.size).toBe(5);
  });

  it('generateSenderKey produces 32-byte chain key', async () => {
    const sk = await KeyGenerator.generateSenderKey();
    expect(sk.chainKey.byteLength).toBe(32);
    expect(sk.signingKey.publicKey).toBeDefined();
    expect(sk.signingKey.privateKey).toBeDefined();
  });
});
