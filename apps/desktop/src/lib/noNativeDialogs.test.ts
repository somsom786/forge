import { describe, expect, it } from 'vitest';

// Tauri replaces window.confirm/alert/prompt with a dialog plugin we deliberately do not grant, so they must never be used.
const files = import.meta.glob('../**/*.{ts,tsx}', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;
const PATTERN = /(\b(window|globalThis|self)\.(confirm|alert|prompt)\s*\()|((^|[^\w.$])(confirm|alert|prompt)\s*\()/m;

describe('no native dialogs', () => {
  it('scans source files', () => expect(Object.keys(files).length).toBeGreaterThan(20));
  it('never calls window.confirm / alert / prompt', () => {
    const offenders = Object.entries(files)
      .filter(([name]) => !name.endsWith('.test.ts') && !name.endsWith('.test.tsx'))
      .filter(([, src]) => PATTERN.test(src))
      .map(([name]) => name);
    expect(offenders).toEqual([]);
  });
});
