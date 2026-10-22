// localStorage access that never throws (private windows, blocked storage, previews).
export function readStore(key: string): string | null {
  try {
    return window.localStorage.getItem(key);
  } catch {
    return null;
  }
}

export function writeStore(key: string, value: string): void {
  try {
    window.localStorage.setItem(key, value);
  } catch {
    /* storage unavailable: the UI keeps working without persistence */
  }
}

export function readNumber(key: string, fallback: number): number {
  const v = readStore(key);
  if (v === null) return fallback;
  const n = Number(v);
  return Number.isFinite(n) ? n : fallback;
}
