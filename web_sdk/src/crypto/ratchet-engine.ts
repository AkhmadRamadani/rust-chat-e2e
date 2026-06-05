// src/crypto/ratchet-engine.ts

import type { RatchetSession, EncryptResult, DecryptResult, RatchetHeader } from './crypto-types.js';
import {
  generateX25519KeyPair,
  exportPublicKeyRaw,
  exportPrivateKeyPkcs8,
  importX25519PublicKey,
  importX25519PrivateKey,
  x25519Dh,
  hkdf,
  aesGcmEncrypt,
  aesGcmDecrypt,
} from './crypto-utils.js';
import { HKDF_INFO } from './crypto-utils.js';
import { base64urlEncode, base64urlDecode } from '../utils/encoding.js';
import { SdkError, SdkErrorCode } from '../errors/sdk-error.js';
import type { RatchetState } from '../session/session-store.js';

const MAX_SKIP = 2000;

/**
 * Double Ratchet Algorithm engine providing forward secrecy and break-in recovery.
 */
export class RatchetEngine {
  /**
   * Initialize a ratchet session from the sender's (initiator) side.
   */
  static async initSender(
    sharedSecret: Uint8Array,
    recipientDhPubBytes: Uint8Array,
  ): Promise<RatchetSession> {
    const dhKp = await generateX25519KeyPair();
    const dhPub = await exportPublicKeyRaw(dhKp.publicKey);
    const dhPriv = await exportPrivateKeyPkcs8(dhKp.privateKey);

    const recipientDhPub = await importX25519PublicKey(recipientDhPubBytes);
    const dhOut = await x25519Dh(dhKp.privateKey, recipientDhPub);
    const [newRootKey, chainKeySend] = await ratchetKdf(sharedSecret, dhOut);

    return {
      rootKey: newRootKey,
      chainKeySend,
      chainKeyRecv: new Uint8Array(32),
      dhSendPub: dhPub,
      dhSendPriv: dhPriv,
      dhRecvPub: recipientDhPubBytes,
      nSend: 0,
      nRecv: 0,
      pn: 0,
      skippedMessageKeys: new Map(),
    };
  }

  /**
   * Initialize a ratchet session from the receiver's (responder) side.
   */
  static async initReceiver(
    sharedSecret: Uint8Array,
    localDhKeyPair: { publicKey: CryptoKey; privateKey: CryptoKey },
  ): Promise<RatchetSession> {
    const dhPub = await exportPublicKeyRaw(localDhKeyPair.publicKey);
    const dhPriv = await exportPrivateKeyPkcs8(localDhKeyPair.privateKey);

    return {
      rootKey: sharedSecret,
      chainKeySend: new Uint8Array(32),
      chainKeyRecv: new Uint8Array(32),
      dhSendPub: dhPub,
      dhSendPriv: dhPriv,
      dhRecvPub: new Uint8Array(32),
      nSend: 0,
      nRecv: 0,
      pn: 0,
      skippedMessageKeys: new Map(),
    };
  }

  /**
   * Encrypt a message, advancing the sending chain.
   */
  static async encrypt(
    session: RatchetSession,
    plaintext: Uint8Array,
  ): Promise<EncryptResult> {
    const [chainKey, messageKey] = await chainKdf(session.chainKeySend);

    const ciphertext = await aesGcmEncrypt(messageKey, plaintext);

    const header: RatchetHeader = {
      type: 'double_ratchet',
      dh: session.dhSendPub,
      n: session.nSend,
      pn: session.pn,
    };

    const nextSession: RatchetSession = {
      ...session,
      chainKeySend: chainKey,
      nSend: session.nSend + 1,
      skippedMessageKeys: new Map(session.skippedMessageKeys),
    };

    return { ciphertext, header, nextSession };
  }

  /**
   * Decrypt a message, advancing the receiving chain.
   */
  static async decrypt(
    session: RatchetSession,
    ciphertext: Uint8Array,
    header: RatchetHeader,
  ): Promise<DecryptResult> {
    // Check skipped message keys first
    const skippedKey = `${base64urlEncode(header.dh)}:${header.n}`;
    if (session.skippedMessageKeys.has(skippedKey)) {
      const messageKey = session.skippedMessageKeys.get(skippedKey)!;
      const nextSkipped = new Map(session.skippedMessageKeys);
      nextSkipped.delete(skippedKey);
      try {
        const plaintext = await aesGcmDecrypt(messageKey, ciphertext);
        return {
          plaintext,
          nextSession: { ...session, skippedMessageKeys: nextSkipped },
        };
      } catch (err) {
        throw SdkError.decryption('Failed to decrypt with skipped message key', err);
      }
    }

    let currentSession = { ...session, skippedMessageKeys: new Map(session.skippedMessageKeys) };

    // DH ratchet step if new sender DH key
    const dhHeaderEnc = base64urlEncode(header.dh);
    const dhRecvEnc = base64urlEncode(currentSession.dhRecvPub);

    if (dhHeaderEnc !== dhRecvEnc) {
      // Skip ahead in current receiving chain (pn messages)
      currentSession = await skipMessageKeys(currentSession, header.pn);
      // Do DH ratchet step
      currentSession = await dhRatchetStep(currentSession, header.dh);
    }

    // Skip ahead to message n
    currentSession = await skipMessageKeys(currentSession, header.n);

    // Derive message key
    const [nextChainKey, messageKey] = await chainKdf(currentSession.chainKeyRecv);
    currentSession = {
      ...currentSession,
      chainKeyRecv: nextChainKey,
      nRecv: currentSession.nRecv + 1,
    };

    try {
      const plaintext = await aesGcmDecrypt(messageKey, ciphertext);
      return { plaintext, nextSession: currentSession };
    } catch (err) {
      throw SdkError.decryption('AES-GCM decryption failed', err);
    }
  }

  /**
   * Serialize RatchetSession to storable RatchetState.
   */
  static serialize(session: RatchetSession, conversationId: string): RatchetState {
    const skippedMessageKeys: Record<string, string> = {};
    for (const [k, v] of session.skippedMessageKeys) {
      skippedMessageKeys[k] = base64urlEncode(v);
    }
    return {
      conversationId,
      rootKey: base64urlEncode(session.rootKey),
      chainKeySend: base64urlEncode(session.chainKeySend),
      chainKeyRecv: base64urlEncode(session.chainKeyRecv),
      dhSendPub: base64urlEncode(session.dhSendPub),
      dhSendPriv: base64urlEncode(session.dhSendPriv),
      dhRecvPub: base64urlEncode(session.dhRecvPub),
      nSend: session.nSend,
      nRecv: session.nRecv,
      pn: session.pn,
      skippedMessageKeys,
    };
  }

  /**
   * Deserialize a stored RatchetState to a RatchetSession.
   */
  static deserialize(state: RatchetState): RatchetSession {
    const skippedMessageKeys = new Map<string, Uint8Array>();
    for (const [k, v] of Object.entries(state.skippedMessageKeys)) {
      skippedMessageKeys.set(k, base64urlDecode(v));
    }
    return {
      rootKey: base64urlDecode(state.rootKey),
      chainKeySend: base64urlDecode(state.chainKeySend),
      chainKeyRecv: base64urlDecode(state.chainKeyRecv),
      dhSendPub: base64urlDecode(state.dhSendPub),
      dhSendPriv: base64urlDecode(state.dhSendPriv),
      dhRecvPub: base64urlDecode(state.dhRecvPub),
      nSend: state.nSend,
      nRecv: state.nRecv,
      pn: state.pn,
      skippedMessageKeys,
    };
  }
}

// --- Helpers ---

async function ratchetKdf(
  rootKey: Uint8Array,
  dhOutput: Uint8Array,
): Promise<[Uint8Array, Uint8Array]> {
  const output = await hkdf(dhOutput, HKDF_INFO.RATCHET_ROOT, 64, rootKey);
  return [output.slice(0, 32), output.slice(32, 64)];
}

async function chainKdf(chainKey: Uint8Array): Promise<[Uint8Array, Uint8Array]> {
  const [nextChain, msgKey] = await Promise.all([
    hkdf(chainKey, HKDF_INFO.RATCHET_CHAIN, 32),
    hkdf(chainKey, HKDF_INFO.MESSAGE_KEY, 32),
  ]);
  return [nextChain, msgKey];
}

async function skipMessageKeys(
  session: RatchetSession,
  until: number,
): Promise<RatchetSession> {
  if (session.nRecv >= until) return session;

  const totalSkip = until - session.nRecv;
  if (session.skippedMessageKeys.size + totalSkip > MAX_SKIP) {
    throw new SdkError(SdkErrorCode.DECRYPTION_ERROR, 'Too many skipped messages');
  }

  const skipped = new Map(session.skippedMessageKeys);
  let chainKey = session.chainKeyRecv;
  const dhKey = base64urlEncode(session.dhRecvPub);
  let n = session.nRecv;

  while (n < until) {
    const [nextChain, messageKey] = await chainKdf(chainKey);
    skipped.set(`${dhKey}:${n}`, messageKey);
    chainKey = nextChain;
    n++;
  }

  return { ...session, chainKeyRecv: chainKey, nRecv: n, skippedMessageKeys: skipped };
}

async function dhRatchetStep(
  session: RatchetSession,
  newDhRecvPub: Uint8Array,
): Promise<RatchetSession> {
  // Generate new DH key pair
  const newDhKp = await generateX25519KeyPair();
  const newDhPub = await exportPublicKeyRaw(newDhKp.publicKey);
  const newDhPriv = await exportPrivateKeyPkcs8(newDhKp.privateKey);

  const recipientPub = await importX25519PublicKey(newDhRecvPub);

  // Receiving chain
  const dhRecv = await x25519Dh(await importX25519PrivateKey(session.dhSendPriv), recipientPub);
  const [rootKey1, chainKeyRecv] = await ratchetKdf(session.rootKey, dhRecv);

  // Sending chain
  const dhSend = await x25519Dh(newDhKp.privateKey, recipientPub);
  const [rootKey2, chainKeySend] = await ratchetKdf(rootKey1, dhSend);

  return {
    ...session,
    rootKey: rootKey2,
    chainKeySend,
    chainKeyRecv,
    dhSendPub: newDhPub,
    dhSendPriv: newDhPriv,
    dhRecvPub: newDhRecvPub,
    nSend: 0,
    nRecv: 0,
    pn: session.nSend,
  };
}
