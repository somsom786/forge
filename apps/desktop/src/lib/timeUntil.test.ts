import { describe, expect, it } from 'vitest';
import { timeUntil } from './format';

describe('timeUntil', () => {
  it('counts down and never shows a negative time', () => {
    const now = Date.parse('2026-01-01T00:00:00Z');
    expect(timeUntil('2026-01-01T00:29:30Z', now)).toBe('in 29m');
    expect(timeUntil('2026-01-01T00:00:45Z', now)).toBe('in 45s');
    expect(timeUntil('2025-12-31T23:59:00Z', now)).toBe('now');
    expect(timeUntil(null, now)).toBe('—');
  });
});
