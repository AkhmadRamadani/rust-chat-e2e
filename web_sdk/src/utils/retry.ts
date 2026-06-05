// src/utils/retry.ts

export interface RetryOptions {
  attempts: number;
  backoffMs: number;
}

/**
 * Retry a function up to `attempts` times with linear backoff.
 */
export async function withRetry<T>(
  fn: () => Promise<T>,
  options: RetryOptions,
): Promise<T> {
  let lastError: unknown;
  for (let attempt = 0; attempt < options.attempts; attempt++) {
    try {
      return await fn();
    } catch (err) {
      lastError = err;
      if (attempt < options.attempts - 1) {
        await sleep(options.backoffMs);
      }
    }
  }
  throw lastError;
}

export interface ExponentialBackoffOptions {
  initialDelayMs?: number;
  maxDelayMs?: number;
  factor?: number;
}

/**
 * Async generator that yields delay values following exponential backoff.
 * Useful for reconnect loops.
 * @example
 * for await (const delay of exponentialBackoff({ maxDelayMs: 60_000 })) {
 *   await sleep(delay);
 *   try { await connect(); break; } catch {}
 * }
 */
export async function* exponentialBackoff(
  options: ExponentialBackoffOptions = {},
): AsyncGenerator<number, never, void> {
  const { initialDelayMs = 1000, maxDelayMs = 60_000, factor = 2 } = options;
  let delay = initialDelayMs;
  while (true) {
    yield delay;
    delay = Math.min(delay * factor, maxDelayMs);
  }
}

export function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
