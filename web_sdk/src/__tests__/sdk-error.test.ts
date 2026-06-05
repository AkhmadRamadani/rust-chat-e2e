// src/__tests__/sdk-error.test.ts

import { describe, it, expect } from 'vitest';
import { SdkError, SdkErrorCode } from '../errors/sdk-error.js';

describe('SdkError', () => {
  it('extends Error', () => {
    const e = new SdkError(SdkErrorCode.NETWORK_ERROR, 'oops');
    expect(e).toBeInstanceOf(Error);
    expect(e).toBeInstanceOf(SdkError);
    expect(e.name).toBe('SdkError');
  });

  it('maps server error codes', () => {
    const cases: Array<[string, SdkErrorCode]> = [
      ['unauthorized', SdkErrorCode.AUTH_ERROR],
      ['forbidden', SdkErrorCode.AUTH_ERROR],
      ['unknown_tenant', SdkErrorCode.AUTH_ERROR],
      ['tenant_inactive', SdkErrorCode.AUTH_ERROR],
      ['not_found', SdkErrorCode.NETWORK_ERROR],
      ['device_limit_reached', SdkErrorCode.DEVICE_LIMIT_REACHED],
      ['invalid_signed_prekey_signature', SdkErrorCode.INVALID_SIGNATURE],
      ['storage_unavailable', SdkErrorCode.NETWORK_ERROR],
      ['bad_request', SdkErrorCode.NETWORK_ERROR],
      ['internal_error', SdkErrorCode.NETWORK_ERROR],
    ];
    for (const [serverCode, expected] of cases) {
      const err = SdkError.fromApiResponse(400, { error_code: serverCode, message: 'test' });
      expect(err.code).toBe(expected);
    }
  });

  it('falls back to UNKNOWN_ERROR for unmapped codes', () => {
    const err = SdkError.fromApiResponse(500, { error_code: 'something_new' });
    expect(err.code).toBe(SdkErrorCode.UNKNOWN_ERROR);
  });

  it('carries statusCode', () => {
    const err = SdkError.fromApiResponse(404, { error_code: 'not_found' });
    expect(err.statusCode).toBe(404);
  });

  it('static helpers set correct codes', () => {
    expect(SdkError.network('x').code).toBe(SdkErrorCode.NETWORK_ERROR);
    expect(SdkError.auth('x').code).toBe(SdkErrorCode.AUTH_ERROR);
    expect(SdkError.decryption('x').code).toBe(SdkErrorCode.DECRYPTION_ERROR);
    expect(SdkError.storage('x').code).toBe(SdkErrorCode.STORAGE_ERROR);
    expect(SdkError.sessionNotFound('u', 'd').code).toBe(SdkErrorCode.SESSION_NOT_FOUND);
  });
});
