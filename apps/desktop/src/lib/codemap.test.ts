import { describe, expect, it } from 'vitest';
import { buildTree, indexedAt, kindShort, languageSummary, relevantCodeSummary, skippedSummary, visibleDirs } from './codemap';
import type { DirSummary, RelevantCode } from './types';

const dir = (path: string, files: string[] = []): DirSummary => ({
  path, totalFiles: files.length, totalSymbols: 0, languages: {}, topSymbols: [],
  files: files.map((f) => ({ path: path ? `${path}/${f}` : f, language: 'Rust', bytes: 10, symbols: 1, parseErrors: false, skipped: null })),
});

describe('code map tree', () => {
  it('nests directories under the root, sorted, and fills in missing ancestors', () => {
    const root = buildTree([dir('src/b'), dir(''), dir('src'), dir('docs'), dir('web/src/api', ['client.ts'])]);
    expect(root.name).toBe('/');
    expect(root.children.map((c) => c.path)).toEqual(['docs', 'src', 'web']);
    expect(root.children[1].children.map((c) => c.name)).toEqual(['b']);
    const api = root.children[2].children[0].children[0];
    expect(api.path).toBe('web/src/api');
    expect(api.summary.files[0].path).toBe('web/src/api/client.ts');
    expect(root.children[2].summary.files).toEqual([]); // filled-in ancestor has an empty summary
  });

  it('filters by directory or file path and keeps the ancestors of matches', () => {
    const root = buildTree([dir('', ['README.md']), dir('src', ['main.rs']), dir('src/auth', ['token.rs']), dir('docs', ['auth.md'])]);
    expect([...visibleDirs(root, '')].sort()).toEqual(['', 'docs', 'src', 'src/auth']);
    expect([...visibleDirs(root, 'TOKEN')].sort()).toEqual(['', 'src', 'src/auth']);
    expect([...visibleDirs(root, 'auth')].sort()).toEqual(['', 'docs', 'src', 'src/auth']);
    expect([...visibleDirs(root, 'nothing-matches')]).toEqual([]);
  });
});

describe('code map wording', () => {
  it('orders languages by file count with unparsed files last', () => {
    expect(languageSummary({ Other: 9, Rust: 3, TypeScript: 5, Python: 3 })).toBe('TypeScript 5 · Python 3 · Rust 3 · Other 9');
  });

  it('lists only the skip reasons that happened', () => {
    expect(skippedSummary({ tooLarge: 2, binary: 0, unreadable: 0, excludedDirs: 1, symlinks: 0, parseFailed: 0, overFileLimit: 0 })).toEqual([
      '2 too large to parse', '1 in .git, node_modules or target (always excluded)',
    ]);
  });

  it('says which tree was indexed', () => {
    expect(indexedAt({ head: 'abcdef1234567890', dirty: true })).toBe('commit abcdef1234 + uncommitted changes');
    expect(indexedAt({ head: null, dirty: true })).toBe('working tree (no commits yet)');
    expect(kindShort('function')).toBe('fn');
    expect(kindShort('macro')).toBe('macro');
  });

  it('never claims code pointers were sent when none were', () => {
    const rc = (o: Partial<RelevantCode>): RelevantCode => ({ items: [], omitted: 0, terms: ['token'], budgetBytes: 2000, usedBytes: 0, indexHead: 'a', indexDirty: false, indexTruncated: null, note: null, ...o });
    expect(relevantCodeSummary(undefined)).toMatch(/before code intelligence/);
    expect(relevantCodeSummary(rc({ note: 'code index unavailable: not a git repository' }))).toBe('code index unavailable: not a git repository');
    expect(relevantCodeSummary(rc({}))).toBe('No symbol names matched the objective.');
    expect(relevantCodeSummary(rc({ terms: [] }))).toMatch(/no words/);
    const item = { name: 'a', kind: 'function' as const, file: 'a.rs', startLine: 1, endLine: 2, signature: 'fn a()', container: null, score: 1, reasons: ['name matches "a"'] };
    expect(relevantCodeSummary(rc({ items: [item], usedBytes: 40, omitted: 3 }))).toBe('1 code pointer included (40 of 2000 bytes; 3 more matched but did not fit)');
  });
});
