/** A single-consumer async queue usable as an AsyncIterable (for streaming-input mode). */
export class AsyncQueue<T> implements AsyncIterable<T> {
  private items: T[] = [];
  private waiters: Array<(r: IteratorResult<T>) => void> = [];
  private closed = false;

  push(item: T): void {
    if (this.closed) return;
    const w = this.waiters.shift();
    if (w) w({ value: item, done: false });
    else this.items.push(item);
  }

  close(): void {
    this.closed = true;
    for (const w of this.waiters.splice(0)) w({ value: undefined, done: true });
  }

  [Symbol.asyncIterator](): AsyncIterator<T> {
    return {
      next: () => {
        const item = this.items.shift();
        if (item !== undefined) return Promise.resolve({ value: item, done: false });
        if (this.closed) return Promise.resolve({ value: undefined, done: true });
        return new Promise((resolve) => this.waiters.push(resolve));
      },
    };
  }
}

/** Truncate a string to `max` bytes (UTF-8 safe), reporting whether it was cut. */
export function truncate(s: string, max: number): { text: string; truncated: boolean } {
  const buf = Buffer.from(s, 'utf8');
  if (buf.length <= max) return { text: s, truncated: false };
  let end = max;
  // Do not cut inside a multi-byte sequence.
  while (end > 0 && (buf[end]! & 0xc0) === 0x80) end--;
  return { text: buf.subarray(0, end).toString('utf8') + `… [truncated ${buf.length - end} bytes]`, truncated: true };
}

/** Deeply truncate every string in a JSON-like value. */
export function truncateDeep<T>(v: T, max: number): T {
  if (typeof v === 'string') return truncate(v, max).text as unknown as T;
  if (Array.isArray(v)) return v.slice(0, 200).map((x) => truncateDeep(x, max)) as unknown as T;
  if (v && typeof v === 'object') {
    const out: Record<string, unknown> = {};
    for (const [k, val] of Object.entries(v as Record<string, unknown>)) out[k] = truncateDeep(val, max);
    return out as T;
  }
  return v;
}

export function errMessage(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
