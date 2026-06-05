// src/utils/logger.ts

export type LogLevel = 'debug' | 'info' | 'warn' | 'error' | 'silent';

const LEVEL_RANK: Record<LogLevel, number> = {
  debug: 0,
  info: 1,
  warn: 2,
  error: 3,
  silent: 4,
};

const PREFIX = '[rce-sdk]';

/**
 * Replaces sensitive values with [REDACTED] to ensure no plaintext leaks into logs.
 */
export function redact(value: unknown): unknown {
  if (typeof value === 'string' && value.length > 20) return '[REDACTED]';
  if (value instanceof Uint8Array && value.byteLength > 16) return '[REDACTED:bytes]';
  if (value !== null && typeof value === 'object') {
    const safe: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(value as Record<string, unknown>)) {
      // Always redact known sensitive keys
      const sensitiveKeys = ['text', 'plaintext', 'accessToken', 'token', 'key', 'priv', 'secret', 'password'];
      if (sensitiveKeys.some((s) => k.toLowerCase().includes(s))) {
        safe[k] = '[REDACTED]';
      } else {
        safe[k] = redact(v);
      }
    }
    return safe;
  }
  return value;
}

export class Logger {
  private readonly level: LogLevel;
  private readonly rank: number;

  constructor(level: LogLevel = 'warn') {
    this.level = level;
    this.rank = LEVEL_RANK[level];
  }

  debug(message: string, ...args: unknown[]): void {
    if (this.rank <= LEVEL_RANK.debug) {
      console.debug(PREFIX, message, ...args.map(redact));
    }
  }

  info(message: string, ...args: unknown[]): void {
    if (this.rank <= LEVEL_RANK.info) {
      console.info(PREFIX, message, ...args.map(redact));
    }
  }

  warn(message: string, ...args: unknown[]): void {
    if (this.rank <= LEVEL_RANK.warn) {
      console.warn(PREFIX, message, ...args.map(redact));
    }
  }

  error(message: string, ...args: unknown[]): void {
    if (this.rank <= LEVEL_RANK.error) {
      console.error(PREFIX, message, ...args.map(redact));
    }
  }
}
