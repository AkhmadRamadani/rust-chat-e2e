// src/client/otpk-replenisher.ts

import { KeyGenerator } from '../crypto/key-generator.js';
import {
  exportPublicKeyRaw,
  exportPrivateKeyPkcs8,
} from '../crypto/crypto-utils.js';
import { base64urlEncode } from '../utils/encoding.js';
import type { SessionStore } from '../session/session-store.js';
import type { RestClient } from '../transport/rest-client.js';
import type { Logger } from '../utils/logger.js';
import type { SdkError } from '../errors/sdk-error.js';
import type { TypedEventEmitter } from '../utils/typed-emitter.js';

const REPLENISH_COUNT = 50;
const LOW_OTPK_THRESHOLD = 10;

/**
 * Listens for low_otpk events and automatically replenishes one-time prekeys.
 */
export class OtpkReplenisher {
  constructor(
    private readonly userId: string,
    private readonly deviceId: string,
    private readonly store: SessionStore,
    private readonly rest: RestClient,
    private readonly logger: Logger,
    private readonly onError: (err: SdkError) => void,
  ) {}

  async handleLowOtpk(count: number): Promise<void> {
    if (count >= LOW_OTPK_THRESHOLD) return;

    this.logger.info(`Low OTPK count (${count}), replenishing ${REPLENISH_COUNT}`);

    try {
      const deviceRecord = await this.store.loadDevice(this.userId, this.deviceId);
      if (!deviceRecord) return;

      const newOtpks = await KeyGenerator.generateOtpks(REPLENISH_COUNT, deviceRecord.nextOtpkId);

      // Build public key entries for server
      const entries = await Promise.all(
        newOtpks.map(async (o) => ({
          id: o.id,
          key: base64urlEncode(await exportPublicKeyRaw(o.keyPair.publicKey)),
        })),
      );

      await this.rest.replenishOtpks(this.userId, this.deviceId, entries);

      // Persist new private OTPKs
      const newPrivateOtpks = await Promise.all(
        newOtpks.map(async (o) => ({
          id: o.id,
          privateKey: base64urlEncode(await exportPrivateKeyPkcs8(o.keyPair.privateKey)),
          publicKey: base64urlEncode(await exportPublicKeyRaw(o.keyPair.publicKey)),
        })),
      );

      await this.store.saveDevice({
        ...deviceRecord,
        otpks: [...deviceRecord.otpks, ...newPrivateOtpks],
        nextOtpkId: deviceRecord.nextOtpkId + REPLENISH_COUNT,
      });

      this.logger.info(`Replenished ${REPLENISH_COUNT} OTPKs`);
    } catch (err) {
      this.logger.error('OTPK replenishment failed', err);
      this.onError(err as SdkError);
    }
  }
}
