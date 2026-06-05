// src/utils/typed-emitter.ts

type Listener<T> = T extends void ? () => void : (event: T) => void;

/**
 * A minimal, type-safe EventEmitter that works in all JS environments.
 * @example
 * const emitter = new TypedEventEmitter<{ message: string; close: void }>();
 * emitter.on('message', (msg) => console.log(msg));
 * emitter.emit('message', 'hello');
 */
export class TypedEventEmitter<Events extends object> {
  private readonly _listeners = new Map<keyof Events, Set<Listener<unknown>>>();

  on<K extends keyof Events>(event: K, listener: Listener<Events[K]>): this {
    if (!this._listeners.has(event)) {
      this._listeners.set(event, new Set());
    }
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    this._listeners.get(event)!.add(listener as any);
    return this;
  }

  once<K extends keyof Events>(event: K, listener: Listener<Events[K]>): this {
    const wrapped: Listener<Events[K]> = ((data: Events[K]) => {
      this.off(event, wrapped);
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      (listener as any)(data);
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    }) as any;
    return this.on(event, wrapped);
  }

  off<K extends keyof Events>(event: K, listener: Listener<Events[K]>): this {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    this._listeners.get(event)?.delete(listener as any);
    return this;
  }

  emit<K extends keyof Events>(
    ...args: Events[K] extends void ? [event: K] : [event: K, data: Events[K]]
  ): boolean {
    const [event, data] = args;
    const set = this._listeners.get(event);
    if (!set || set.size === 0) return false;
    for (const listener of set) {
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      (listener as any)(data);
    }
    return true;
  }

  removeAllListeners(event?: keyof Events): this {
    if (event !== undefined) {
      this._listeners.delete(event);
    } else {
      this._listeners.clear();
    }
    return this;
  }
}
