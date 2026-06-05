// src/crypto/sender-key-engine.ts

import type { SenderKeyMaterial, SenderKeySession } from './crypto-types.js';
import {
  hkdf,
  aesGcmEncrypt,
  aesGcmDecrypt,
  exportPublicKeyRaw,
  exportPrivateKeyPkcs8,
  importEd25519PublicKey,
  importEd25519PrivateKey,
  ed25519Sign,
  ed25519Verify,
} from './crypto-utils.js';
import { HKDF_INFO } from './crypto-utils.js';
import { base64urlEncode, base64urlDecode, concatBytes } from '../utils/encoding.js';
import { SdkError, SdkErrorCode } from '../errors/sdk-error.js';
import type { SenderKeyRecord } from '../session/session-store.js';

/**
 * Sender Key encryption engine for group conversations.
 * Each member has their own sending chain; others derive message keys from distributed material.
 */
export class SenderKeyEngine {
  /**
   * Create a new SenderKeySession from generated material.
   */
  static async createSession(material: SenderKeyMaterial): Promise<SenderKeySession> {
    return {
      chainKey: material.chainKey,
      chainId: material.chainId,
      iteration: 0,
      signingKey: material.signingKey,
    };
  }

  /**
   * Encrypt a message using the sender key chain.
   */
  static async encrypt(
    session: SenderKeySession,
    plaintext: Uint8Array,
  ): Promise<{ ciphertext: Uint8Array; nextSession: SenderKeySession }> {
    const messageKey = await hkdf(session.chainKey, HKDF_INFO.SENDER_KEY, 32);
    const nextChainKey = await hkdf(session.chainKey, HKDF_INFO.RATCHET_CHAIN, 32);

    const ciphertext = await aesGcmEncrypt(messageKey, plaintext);

    // Sign the ciphertext
    const sig = await ed25519Sign(session.signingKey.privateKey, ciphertext);
    const combined = concatBytes(sig, ciphertext);

    const nextSession: SenderKeySession = {
      ...session,
      chainKey: nextChainKey,
      iteration: session.iteration + 1,
    };

    return { ciphertext: combined, nextSession };
  }

  /**
   * Decrypt a message using the sender key chain.
   */
  static async decrypt(
    session: SenderKeySession,
    ciphertext: Uint8Array,
  ): Promise<{ plaintext: Uint8Array; nextSession: SenderKeySession }> {
    // Extract signature (64 bytes) and actual ciphertext
    if (ciphertext.byteLength < 65) {
      throw new SdkError(SdkErrorCode.DECRYPTION_ERROR, 'Sender key ciphertext too short');
    }
    const sig = ciphertext.slice(0, 64);
    const encryptedData = ciphertext.slice(64);

    // Verify signature
    const valid = await ed25519Verify(session.signingKey.publicKey, sig, encryptedData);
    if (!valid) {
      throw new SdkError(SdkErrorCode.DECRYPTION_ERROR, 'Sender key signature verification failed');
    }

    const messageKey = await hkdf(session.chainKey, HKDF_INFO.SENDER_KEY, 32);
    const nextChainKey = await hkdf(session.chainKey, HKDF_INFO.RATCHET_CHAIN, 32);

    try {
      const plaintext = await aesGcmDecrypt(messageKey, encryptedData);
      const nextSession: SenderKeySession = {
        ...session,
        chainKey: nextChainKey,
        iteration: session.iteration + 1,
      };
      return { plaintext, nextSession };
    } catch (err) {
      throw SdkError.decryption('Sender key AES-GCM decryption failed', err);
    }
  }

  /**
   * Serialize a SenderKeySession for distribution (SKDM).
   */
  static async serializeKeyMaterial(session: SenderKeySession): Promise<Uint8Array> {
    const sigPub = await exportPublicKeyRaw(session.signingKey.publicKey);
    const sigPriv = session.signingKey.privateKey
      ? await exportPrivateKeyPkcs8(session.signingKey.privateKey)
      : new Uint8Array(0);

    const data = {
      chainKey: base64urlEncode(session.chainKey),
      chainId: session.chainId,
      iteration: session.iteration,
      signingKeyPub: base64urlEncode(sigPub),
      signingKeyPriv: base64urlEncode(sigPriv),
    };
    return new TextEncoder().encode(JSON.stringify(data));
  }

  /**
   * Deserialize received SKDM bytes into a SenderKeySession (no private key for others).
   */
  static async deserializeKeyMaterial(bytes: Uint8Array): Promise<SenderKeySession> {
    const data = JSON.parse(new TextDecoder().decode(bytes)) as {
      chainKey: string;
      chainId: number;
      iteration: number;
      signingKeyPub: string;
      signingKeyPriv?: string;
    };

    const sigPubKey = await importEd25519PublicKey(base64urlDecode(data.signingKeyPub));

    let sigPrivKey: CryptoKey | undefined;
    if (data.signingKeyPriv && data.signingKeyPriv.length > 0) {
      try {
        sigPrivKey = await importEd25519PrivateKey(base64urlDecode(data.signingKeyPriv));
      } catch {
        // No private key available (received SKDM)
      }
    }

    return {
      chainKey: base64urlDecode(data.chainKey),
      chainId: data.chainId,
      iteration: data.iteration,
      signingKey: {
        publicKey: sigPubKey,
        privateKey: sigPrivKey as CryptoKey,
      },
    };
  }

  /**
   * Serialize to a storable SenderKeyRecord.
   */
  static async toRecord(
    session: SenderKeySession,
    conversationId: string,
    userId: string,
  ): Promise<SenderKeyRecord> {
    const sigPub = await exportPublicKeyRaw(session.signingKey.publicKey);
    let sigPriv: string | undefined;
    if (session.signingKey.privateKey) {
      try {
        const privBytes = await exportPrivateKeyPkcs8(session.signingKey.privateKey);
        sigPriv = base64urlEncode(privBytes);
      } catch {
        // extractable may be false
      }
    }
    return {
      conversationId,
      userId,
      chainKey: base64urlEncode(session.chainKey),
      chainId: session.chainId,
      iteration: session.iteration,
      signingKeyPub: base64urlEncode(sigPub),
      ...(sigPriv !== undefined ? { signingKeyPriv: sigPriv } : {}),
    };
  }

  /**
   * Restore a SenderKeySession from a stored record.
   */
  static async fromRecord(record: SenderKeyRecord): Promise<SenderKeySession> {
    const sigPubKey = await importEd25519PublicKey(base64urlDecode(record.signingKeyPub));

    let sigPrivKey: CryptoKey | undefined;
    if (record.signingKeyPriv) {
      try {
        sigPrivKey = await importEd25519PrivateKey(base64urlDecode(record.signingKeyPriv));
      } catch {
        // ignore
      }
    }

    return {
      chainKey: base64urlDecode(record.chainKey),
      chainId: record.chainId,
      iteration: record.iteration,
      signingKey: {
        publicKey: sigPubKey,
        privateKey: sigPrivKey as CryptoKey,
      },
    };
  }
}
