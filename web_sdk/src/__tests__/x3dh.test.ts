// src/__tests__/x3dh.test.ts

import { describe, it, expect } from 'vitest';
import { KeyGenerator } from '../crypto/key-generator.js';
import { X3dhEngine } from '../crypto/x3dh-engine.js';
import { exportPublicKeyRaw } from '../crypto/crypto-utils.js';
import { base64urlEncode } from '../utils/encoding.js';
import type { KeyBundleResponse } from '../transport/api-types.js';

async function bundleToResponse(bundle: Awaited<ReturnType<typeof KeyGenerator.generateKeyBundle>>, deviceId = 'dev-1'): Promise<KeyBundleResponse> {
  const [ikDhPub, ikEdPub, spkPub, otpkPub] = await Promise.all([
    exportPublicKeyRaw(bundle.identityKeyDh.publicKey),
    exportPublicKeyRaw(bundle.identityKeyEd.publicKey),
    exportPublicKeyRaw(bundle.signedPrekey.keyPair.publicKey),
    exportPublicKeyRaw(bundle.otpks[0]!.keyPair.publicKey),
  ]);

  return {
    device_id: deviceId,
    identity_key: base64urlEncode(ikDhPub),
    identity_key_ed: base64urlEncode(ikEdPub),
    signed_prekey_id: bundle.signedPrekey.id,
    signed_prekey: base64urlEncode(spkPub),
    signed_prekey_sig: base64urlEncode(bundle.signedPrekey.signature),
    one_time_prekey: { id: bundle.otpks[0]!.id, key: base64urlEncode(otpkPub) },
  };
}

describe('X3dhEngine', () => {
  it('initiator and responder derive identical shared secret (with OTPK)', async () => {
    const aliceBundle = await KeyGenerator.generateKeyBundle(5);
    const bobBundle = await KeyGenerator.generateKeyBundle(5);
    const bobResponse = await bundleToResponse(bobBundle);

    const { sharedSecret: aliceSecret, header } = await X3dhEngine.performX3dh({
      recipientBundle: bobResponse,
      senderBundle: aliceBundle,
    });

    const aliceIkDhPub = await exportPublicKeyRaw(aliceBundle.identityKeyDh.publicKey);

    const bobSecret = await X3dhEngine.deriveSharedSecret({
      header,
      recipientBundle: bobBundle,
      senderIkDhPubBytes: aliceIkDhPub,
    });

    expect(Buffer.from(aliceSecret).toString('hex'))
      .toBe(Buffer.from(bobSecret).toString('hex'));
  });

  it('works without OTPK (depleted scenario — Property 3)', async () => {
    const aliceBundle = await KeyGenerator.generateKeyBundle(5);
    const bobBundle = await KeyGenerator.generateKeyBundle(5);

    const bobResponse = await bundleToResponse(bobBundle);
    bobResponse.one_time_prekey = null; // simulate depleted OTPKs

    const { sharedSecret: aliceSecret, header } = await X3dhEngine.performX3dh({
      recipientBundle: bobResponse,
      senderBundle: aliceBundle,
    });
    expect(header.otpkId).toBeUndefined();

    const aliceIkDhPub = await exportPublicKeyRaw(aliceBundle.identityKeyDh.publicKey);
    const bobSecret = await X3dhEngine.deriveSharedSecret({
      header,
      recipientBundle: bobBundle,
      senderIkDhPubBytes: aliceIkDhPub,
    });

    expect(Buffer.from(aliceSecret).toString('hex'))
      .toBe(Buffer.from(bobSecret).toString('hex'));
  });

  it('rejects tampered SPK signature', async () => {
    const aliceBundle = await KeyGenerator.generateKeyBundle(1);
    const bobBundle = await KeyGenerator.generateKeyBundle(1);
    const bobResponse = await bundleToResponse(bobBundle);

    // Tamper the signature
    bobResponse.signed_prekey_sig = base64urlEncode(new Uint8Array(64).fill(0xaa));

    await expect(
      X3dhEngine.performX3dh({ recipientBundle: bobResponse, senderBundle: aliceBundle }),
    ).rejects.toThrow();
  });
});
