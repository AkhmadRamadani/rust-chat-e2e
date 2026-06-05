// src/errors/sdk-error.ts

/**
 * All possible SDK error codes.
 */
export enum SdkErrorCode {
  NETWORK_ERROR = 'NETWORK_ERROR',
  AUTH_ERROR = 'AUTH_ERROR',
  DECRYPTION_ERROR = 'DECRYPTION_ERROR',
  KEY_EXCHANGE_ERROR = 'KEY_EXCHANGE_ERROR',
  STORAGE_ERROR = 'STORAGE_ERROR',
  SESSION_NOT_FOUND = 'SESSION_NOT_FOUND',
  FILE_TOO_LARGE = 'FILE_TOO_LARGE',
  DEVICE_LIMIT_REACHED = 'DEVICE_LIMIT_REACHED',
  INVALID_SIGNATURE = 'INVALID_SIGNATURE',
  UNKNOWN_ERROR = 'UNKNOWN_ERROR',
}

/**
 * All SDK errors are instances of SdkError, providing a consistent typed surface.
 * @example
 * try {
 *   await client.openConversation('user-123');
 * } catch (err) {
 *   if (err instanceof SdkError && err.code === SdkErrorCode.AUTH_ERROR) {
 *     // Re-authenticate
 *   }
 * }
 */
export class SdkError extends Error {
  readonly code: SdkErrorCode;
  readonly statusCode?: number;
  readonly cause?: unknown;

  constructor(
    code: SdkErrorCode,
    message: string,
    options?: { cause?: unknown; statusCode?: number },
  ) {
    super(message);
    this.name = 'SdkError';
    this.code = code;
    this.cause = options?.cause;
    if (options?.statusCode !== undefined) {
      // Use Object.defineProperty to bypass readonly+exactOptionalPropertyTypes
      Object.defineProperty(this, 'statusCode', { value: options.statusCode, writable: false, enumerable: true, configurable: false });
    }
    // Ensure correct prototype chain in ES5 transpile scenarios
    Object.setPrototypeOf(this, SdkError.prototype);
  }

  /**
   * Map a server API error response to a typed SdkError.
   */
  static fromApiResponse(
    status: number,
    body: { error_code?: string; message?: string },
  ): SdkError {
    const code = SERVER_ERROR_CODE_MAP[body.error_code ?? ''] ?? SdkErrorCode.UNKNOWN_ERROR;
    const message = body.message ?? `HTTP ${status}`;
    return new SdkError(code, message, { statusCode: status });
  }

  static network(message: string, cause?: unknown): SdkError {
    return new SdkError(SdkErrorCode.NETWORK_ERROR, message, { cause });
  }

  static auth(message: string): SdkError {
    return new SdkError(SdkErrorCode.AUTH_ERROR, message);
  }

  static decryption(message: string, cause?: unknown): SdkError {
    return new SdkError(SdkErrorCode.DECRYPTION_ERROR, message, { cause });
  }

  static storage(message: string, cause?: unknown): SdkError {
    return new SdkError(SdkErrorCode.STORAGE_ERROR, message, { cause });
  }

  static sessionNotFound(userId: string, deviceId: string): SdkError {
    return new SdkError(
      SdkErrorCode.SESSION_NOT_FOUND,
      `No session found for user ${userId} device ${deviceId}`,
    );
  }
}

const SERVER_ERROR_CODE_MAP: Record<string, SdkErrorCode> = {
  unauthorized: SdkErrorCode.AUTH_ERROR,
  forbidden: SdkErrorCode.AUTH_ERROR,
  unknown_tenant: SdkErrorCode.AUTH_ERROR,
  tenant_inactive: SdkErrorCode.AUTH_ERROR,
  not_found: SdkErrorCode.NETWORK_ERROR,
  device_limit_reached: SdkErrorCode.DEVICE_LIMIT_REACHED,
  invalid_signed_prekey_signature: SdkErrorCode.INVALID_SIGNATURE,
  storage_unavailable: SdkErrorCode.NETWORK_ERROR,
  bad_request: SdkErrorCode.NETWORK_ERROR,
  internal_error: SdkErrorCode.NETWORK_ERROR,
};
