// src/client/spk-rotator.ts

import { KeyGenerator } from '../crypto/key-generator.js';
import {
  exportPublicKeyRaw,
  exportPrivateKeyPkcs8,
  importEd25519PrivateKey,
} from '../crypto/crypto-utils.js';
import { base64urlEncode, base64urlDecode } from '../utils/encoding.js';
import type { SessionStore } from '../session/session-store.js';
import type { RestClient } from '../transport/rest-client.js';
import type { Logger } from '../utils/logger.js';
import type { SdkError } from '../errors/sdk-error.js';

const CHECK_INTERVAL_MS = 60 * 60 * 1000; // check every hour

/**
 * Periodically rotates the SignedPreKey per the configured rotation interval.
 */
export class SpkRotator {
  private timer: ReturnType<typeof setInterval> | null = null;

  constructor(
    private readonly userId: string,
    private readonly deviceId: string,
    private readonly rotationDays: number,
    private readonly store: SessionStore,
    private readonly rest: RestClient,
    private readonly logger: Logger,
    private readonly onError: (err: SdkError) => void,
  ) {}

  start(): void {
    this.timer = setInterval(() => {
      this.checkAndRotate().catch((err) => {
        this.logger.error('SPK rotation check failed', err);
        this.onError(err as SdkError);
      });
    }, CHECK_INTERVAL_MS);
  }

  stop(): void {
    if (this.timer) {
      clearInterval(this.timer);
      this.timer = null;
    }
  }

  async checkAndRotate(): Promise<void> {
    const record = await this.store.loadDevice(this.userId, this.deviceId);
    if (!record) return;

    const ageMs = Date.now() - record.signedPrekeyCreatedAt;
    const rotationMs = this.rotationDays * 24 * 60 * 60 * 1000;

    if (ageMs < rotationMs) return;

    this.logger.info('Rotating SignedPreKey');

    try {
      const identityEdPriv = await importEd25519PrivateKey(
        base64urlDecode(record.identityKeyEdPriv),
      );

      const newSpk = await KeyGenerator.generateSignedPreKey(
        {
          // publicKey not needed for signing
          publicKey: {} as CryptoKey,
          privateKey: identityEdPriv,
        },
        record.signedPrekeyId + 1,
      );

      const spkPubBytes = await exportPublicKeyRaw(newSpk.keyPair.publicKey);
      const spkPrivBytes = await exportPrivateKeyPkcs8(newSpk.keyPair.privateKey);

      await this.rest.rotateSignedPreKey(this.userId, this.deviceId, {
        signed_prekey_id: newSpk.id,
        signed_prekey: base64urlEncode(spkPubBytes),
        signed_prekey_sig: base64urlEncode(newSpk.signature),
      });

      await this.store.saveDevice({
        ...record,
        signedPrekeyId: newSpk.id,
        signedPrekeyPub: base64urlEncode(spkPubBytes),
        signedPrekeyPriv: base64urlEncode(spkPrivBytes),
        signedPrekeyCreatedAt: Date.now(),
      });

      this.logger.info('SignedPreKey rotated successfully');
    } catch (err) {
      this.logger.error('SPK rotation failed', err);
      this.onError(err as SdkError);
    }
  }
}
