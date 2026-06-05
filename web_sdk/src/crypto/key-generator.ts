// src/crypto/key-generator.ts

import type {
  PrivateKeyBundle,
  OtpkKeyPair,
  SignedPreKeyPair,
  SenderKeyMaterial,
  Curve25519KeyPair,
  Ed25519KeyPair,
} from './crypto-types.js';
import {
  generateX25519KeyPair,
  generateEd25519KeyPair,
  exportPublicKeyRaw,
  ed25519Sign,
} from './crypto-utils.js';

/**
 * Generates all cryptographic key material using Web Crypto API only.
 * No Math.random() — all randomness via crypto.getRandomValues.
 */
export class KeyGenerator {
  /**
   * Generate a full private key bundle for device registration.
   * Includes identity key pair, signed prekey, and `otpkCount` one-time prekeys.
   * @example
   * const bundle = await KeyGenerator.generateKeyBundle(50);
   */
  static async generateKeyBundle(otpkCount = 50): Promise<PrivateKeyBundle> {
    const [identityDhRaw, identityEdRaw] = await Promise.all([
      generateX25519KeyPair(),
      generateEd25519KeyPair(),
    ]);

    const identityKeyDh: Curve25519KeyPair = {
      publicKey: identityDhRaw.publicKey,
      privateKey: identityDhRaw.privateKey,
    };
    const identityKeyEd: Ed25519KeyPair = {
      publicKey: identityEdRaw.publicKey,
      privateKey: identityEdRaw.privateKey,
    };

    const signedPrekey = await KeyGenerator.generateSignedPreKey(identityKeyEd, 1);

    const otpks = await KeyGenerator.generateOtpks(otpkCount, 1);

    return {
      identityKeyDh,
      identityKeyEd,
      signedPrekey,
      otpks,
      nextOtpkId: otpkCount + 1,
    };
  }

  /**
   * Generate a batch of new OTPKs for replenishment.
   * @param count Number of OTPKs to generate
   * @param startId Starting numeric ID for sequential IDs
   */
  static async generateOtpks(count: number, startId = 1): Promise<OtpkKeyPair[]> {
    const results: OtpkKeyPair[] = [];
    for (let i = 0; i < count; i++) {
      const kp = await generateX25519KeyPair();
      results.push({
        id: startId + i,
        keyPair: { publicKey: kp.publicKey, privateKey: kp.privateKey },
      });
    }
    return results;
  }

  /**
   * Generate a new SignedPreKey signed with the Ed25519 identity key.
   * @example
   * const spk = await KeyGenerator.generateSignedPreKey(identityEdKey, 2);
   */
  static async generateSignedPreKey(
    identityEdKey: Ed25519KeyPair,
    id: number,
  ): Promise<SignedPreKeyPair> {
    const kp = await generateX25519KeyPair();
    const pubBytes = await exportPublicKeyRaw(kp.publicKey);
    const signature = await ed25519Sign(identityEdKey.privateKey, pubBytes);

    return {
      id,
      keyPair: { publicKey: kp.publicKey, privateKey: kp.privateKey },
      signature,
      createdAt: Date.now(),
    };
  }

  /**
   * Generate a new SenderKey for group conversations.
   */
  static async generateSenderKey(): Promise<SenderKeyMaterial> {
    const chainKey = new Uint8Array(32);
    crypto.getRandomValues(chainKey);

    const signingKp = await generateEd25519KeyPair();
    const chainIdBytes = new Uint8Array(4);
    crypto.getRandomValues(chainIdBytes);
    const chainId = new DataView(chainIdBytes.buffer).getUint32(0, false);

    return {
      chainKey,
      chainId,
      signingKey: {
        publicKey: signingKp.publicKey,
        privateKey: signingKp.privateKey,
      },
    };
  }
}
