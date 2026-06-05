// src/crypto/x3dh-engine.ts

import type { PrivateKeyBundle, X3dhResult, X3dhHeader } from './crypto-types.js';
import type { KeyBundleResponse } from '../transport/api-types.js';
import {
  generateX25519KeyPair,
  exportPublicKeyRaw,
  x25519Dh,
  importX25519PublicKey,
  hkdf,
  verifySPKSignature,
  importEd25519PublicKey,
} from './crypto-utils.js';
import { HKDF_INFO } from './crypto-utils.js';
import { concatBytes, base64urlDecode } from '../utils/encoding.js';
import { SdkError, SdkErrorCode } from '../errors/sdk-error.js';

/**
 * X3DH (Extended Triple Diffie-Hellman) key agreement engine.
 * Establishes a shared session root key before the first message.
 */
export class X3dhEngine {
  /**
   * Initiator: fetch recipient key bundle, compute X3DH shared secret, produce envelope header.
   * @example
   * const { sharedSecret, header } = await X3dhEngine.performX3dh({ recipientBundle, senderBundle });
   */
  static async performX3dh(params: {
    recipientBundle: KeyBundleResponse;
    senderBundle: PrivateKeyBundle;
  }): Promise<X3dhResult> {
    const { recipientBundle, senderBundle } = params;

    // Verify SPK signature before trusting recipient's keys
    const spkPubBytes = base64urlDecode(recipientBundle.signed_prekey);
    const spkSigBytes = base64urlDecode(recipientBundle.signed_prekey_sig);
    const recipientEdPubBytes = base64urlDecode(recipientBundle.identity_key_ed);
    const recipientEdPub = await importEd25519PublicKey(recipientEdPubBytes);

    const sigValid = await verifySPKSignature(spkPubBytes, spkSigBytes, recipientEdPub);
    if (!sigValid) {
      throw new SdkError(
        SdkErrorCode.INVALID_SIGNATURE,
        'Recipient signed prekey signature verification failed',
      );
    }

    // Import recipient's public keys
    const recipientIkDhPub = await importX25519PublicKey(
      base64urlDecode(recipientBundle.identity_key),
    );
    const recipientSpkPub = await importX25519PublicKey(spkPubBytes);

    // Generate ephemeral key
    const ekKp = await generateX25519KeyPair();
    const ekPub = await exportPublicKeyRaw(ekKp.publicKey);

    // DH computations
    // DH1 = DH(IK_sender, SPK_recipient)
    const dh1 = await x25519Dh(senderBundle.identityKeyDh.privateKey, recipientSpkPub);
    // DH2 = DH(EK_sender, IK_recipient)
    const dh2 = await x25519Dh(ekKp.privateKey, recipientIkDhPub);
    // DH3 = DH(EK_sender, SPK_recipient)
    const dh3 = await x25519Dh(ekKp.privateKey, recipientSpkPub);

    let otpkId: number | undefined;
    let dhInputs: Uint8Array;

    if (recipientBundle.one_time_prekey) {
      // DH4 = DH(EK_sender, OTPK_recipient)
      const otpkPub = await importX25519PublicKey(
        base64urlDecode(recipientBundle.one_time_prekey.key),
      );
      const dh4 = await x25519Dh(ekKp.privateKey, otpkPub);
      dhInputs = concatBytes(dh1, dh2, dh3, dh4);
      otpkId = recipientBundle.one_time_prekey.id;
    } else {
      // OTPK depleted — still valid per spec (Property 3)
      dhInputs = concatBytes(dh1, dh2, dh3);
    }

    const sharedSecret = await hkdf(dhInputs, HKDF_INFO.X3DH, 32);

    const header: X3dhHeader = {
      type: 'x3dh_init',
      ek: ekPub,
      spkId: recipientBundle.signed_prekey_id,
      ...(otpkId !== undefined ? { otpkId } : {}),
    };

    return { sharedSecret, header };
  }

  /**
   * Responder: derive the same shared secret from the X3DH header.
   */
  static async deriveSharedSecret(params: {
    header: X3dhHeader;
    recipientBundle: PrivateKeyBundle;
    senderIkDhPubBytes: Uint8Array;
  }): Promise<Uint8Array> {
    const { header, recipientBundle, senderIkDhPubBytes } = params;

    const senderEkPub = await importX25519PublicKey(header.ek);
    const senderIkDhPub = await importX25519PublicKey(senderIkDhPubBytes);

    // DH1 = DH(SPK_recipient, IK_sender)
    const dh1 = await x25519Dh(recipientBundle.signedPrekey.keyPair.privateKey, senderIkDhPub);
    // DH2 = DH(IK_recipient, EK_sender)
    const dh2 = await x25519Dh(recipientBundle.identityKeyDh.privateKey, senderEkPub);
    // DH3 = DH(SPK_recipient, EK_sender)
    const dh3 = await x25519Dh(recipientBundle.signedPrekey.keyPair.privateKey, senderEkPub);

    let dhInputs: Uint8Array;

    if (header.otpkId !== undefined) {
      const otpk = recipientBundle.otpks.find((o) => o.id === header.otpkId);
      if (!otpk) {
        throw new SdkError(SdkErrorCode.KEY_EXCHANGE_ERROR, `OTPK ${header.otpkId} not found`);
      }
      const dh4 = await x25519Dh(otpk.keyPair.privateKey, senderEkPub);
      dhInputs = concatBytes(dh1, dh2, dh3, dh4);
    } else {
      dhInputs = concatBytes(dh1, dh2, dh3);
    }

    return hkdf(dhInputs, HKDF_INFO.X3DH, 32);
  }
}
