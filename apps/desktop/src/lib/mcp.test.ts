import { describe, expect, it } from 'vitest';
import { parseAllowlist } from './mcp';

describe('parseAllowlist', () => {
  it('splits on commas, spaces and newlines and drops duplicates', () => {
    expect(parseAllowlist('search, read\nsearch  ')).toEqual(['search', 'read']);
  });
  it('accepts full tool names and keeps only the tool part', () => {
    expect(parseAllowlist('mcp__docs__search mcp__my_docs__read')).toEqual(['search', 'read']);
  });
  it('empty means no allow-list (every tool may be asked for)', () => {
    expect(parseAllowlist('  ')).toEqual([]);
  });
});
