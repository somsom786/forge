import { describe, expect, it } from 'vitest';
import {
  escapeInvisible, hasInvisible, invisibleSegments,
  canSave, classifyDiff, customHookSpec, dryRunView, groupInventory, isChangedOnDisk, newFilePath, oddChars, revealOdd, validateHookSpec, validateName,
} from './harness';
import type { HarnessItem, HookDryRun } from './types';

describe('classifyDiff', () => {
  it('classifies headers, hunks and changed lines', () => {
    const d = '--- a/x\n+++ b/x\n@@ -1,2 +1,2 @@\n keep\n-old\n+new\n\\ No newline at end of file\n';
    expect(classifyDiff(d).map((l) => l.kind)).toEqual(['meta', 'meta', 'hunk', 'ctx', 'del', 'add', 'meta']);
  });
  it('does not treat a deleted line starting with "-- " as a header inside a hunk', () => {
    const d = '@@ -1 +1 @@\n--- not header\n+++ also content\n';
    expect(classifyDiff(d).map((l) => l.kind)).toEqual(['hunk', 'del', 'add']);
  });
  it('is empty for no diff', () => expect(classifyDiff('')).toEqual([]));
});

describe('new file paths', () => {
  it('maps fixed files', () => {
    expect(newFilePath('CLAUDE.md', '')).toEqual({ ok: true, rel: 'CLAUDE.md' });
    expect(newFilePath('settings', '')).toEqual({ ok: true, rel: '.claude/settings.json' });
  });
  it('builds named paths and strips .md', () => {
    expect(newFilePath('rule', 'testing.md')).toEqual({ ok: true, rel: '.claude/rules/testing.md' });
    expect(newFilePath('skill', 'deploy')).toEqual({ ok: true, rel: '.claude/skills/deploy/SKILL.md' });
    expect(newFilePath('agent', 'reviewer')).toEqual({ ok: true, rel: '.claude/agents/reviewer.md' });
    expect(newFilePath('command', 'ship')).toEqual({ ok: true, rel: '.claude/commands/ship.md' });
  });
  it('rejects traversal, separators and empty names', () => {
    for (const bad of ['', '..', '../x', 'a/b', 'a\\b', '.hidden', 'a b']) expect(newFilePath('rule', bad).ok).toBe(false);
    expect(validateName('ok-name_1.2')).toBeNull();
  });
});

describe('hook spec validation', () => {
  const ok = { event: 'PreToolUse', matcher: 'Bash', command: 'echo hi', timeoutSecs: 30 };
  it('accepts a valid spec', () => expect(validateHookSpec(ok)).toEqual([]));
  it('rejects multi-line commands, bad timeouts and missing fields', () => {
    expect(validateHookSpec({ ...ok, command: 'a\nb' })).toContain('The command must be a single line.');
    expect(validateHookSpec({ ...ok, timeoutSecs: 0 })).toHaveLength(1);
    expect(validateHookSpec({ ...ok, timeoutSecs: 601 })).toHaveLength(1);
    expect(validateHookSpec({ ...ok, timeoutSecs: 1.5 })).toHaveLength(1);
    expect(validateHookSpec({ ...ok, event: '', command: ' ' })).toHaveLength(2);
  });
  it('maps an empty matcher to null', () => expect(customHookSpec({ ...ok, matcher: ' ' }).matcher).toBeNull());
});

describe('odd characters', () => {
  it('finds control, zero-width and bidi characters and reveals them', () => {
    const t = 'echo​ hi‮\t';
    expect(oddChars(t).map((o) => o.code)).toEqual(['U+200B', 'U+202E', 'U+0009']);
    expect(revealOdd(t)).toBe('echo⟦U+200B⟧ hi⟦U+202E⟧⟦U+0009⟧');
  });
  it('leaves plain ASCII alone', () => expect(oddChars('npm test --silent')).toEqual([]));
});

describe('canSave', () => {
  const base = { problems: [], risks: [], unchanged: false };
  it('blocks on problems or no change', () => {
    expect(canSave({ ...base, problems: ['bad json'] }, true)).toBe(false);
    expect(canSave({ ...base, unchanged: true }, true)).toBe(false);
  });
  it('requires acknowledgement when hooks run shell commands', () => {
    const risks = [{ event: 'Stop', command: 'x', display: 'x', risks: ['runs shell'] }];
    expect(canSave({ ...base, risks }, false)).toBe(false);
    expect(canSave({ ...base, risks }, true)).toBe(true);
  });
  it('detects the changed-on-disk refusal', () => expect(isChangedOnDisk('file changed on disk since you opened it')).toBe(true));
});

describe('dryRunView', () => {
  const r = (o: Partial<HookDryRun>): HookDryRun => ({ shell: 'bash', exitCode: 0, timedOut: false, stdout: '', stderr: '', interpretation: '', input: { a: 1 }, durationMs: 12, scrubbedEnv: ['API_TOKEN'], ...o });
  it('summarises outcomes', () => {
    expect(dryRunView(r({})).tone).toBe('ok');
    expect(dryRunView(r({ exitCode: 2 }))).toMatchObject({ outcome: 'exit_nonzero', headline: 'Exit code 2', tone: 'bad' });
    expect(dryRunView(r({ timedOut: true, exitCode: null })).outcome).toBe('timed_out');
    expect(dryRunView(r({ exitCode: null })).outcome).toBe('no_exit');
  });
  it('exposes withheld env and pretty input JSON', () => {
    const v = dryRunView(r({}));
    expect(v.withheldEnv).toEqual(['API_TOKEN']);
    expect(v.inputJson).toBe('{\n  "a": 1\n}');
  });
});

describe('groupInventory', () => {
  const it_ = (kind: HarnessItem['kind'], name: string) => ({ kind, name }) as HarnessItem;
  it('orders groups by kind and drops empty ones', () => {
    const g = groupInventory([it_('mcp', 'm'), it_('rule', 'b'), it_('rule', 'a'), it_('instructions', 'CLAUDE.md')]);
    expect(g.map((x) => x.label)).toEqual(['Instructions', 'Rules', 'MCP']);
    expect(g[1].items.map((x) => x.name)).toEqual(['a', 'b']);
  });
});

describe('invisible character escaping', () => {
  it('escapes zero-width, bidi, BOM and control characters', () => {
    expect(escapeInvisible('a\u200Bb\u202Ec\u2066d\uFEFFe\u0007')).toBe('a<U+200B>b<U+202E>c<U+2066>d<U+FEFF>e<U+0007>');
  });
  it('keeps newline, tab, and ordinary non-ASCII text', () => {
    expect(escapeInvisible('line\n\tcafé 日本')).toBe('line\n\tcafé 日本');
    expect(hasInvisible('plain')).toBe(false);
  });
  it('marks escaped parts as invisible segments for distinct styling', () => {
    expect(invisibleSegments('x\u200By')).toEqual([
      { text: 'x', invisible: false }, { text: '<U+200B>', invisible: true }, { text: 'y', invisible: false },
    ]);
  });
  it('covers range edges', () => {
    for (const c of [0x200b, 0x200f, 0x202a, 0x202e, 0x2060, 0x2069]) expect(hasInvisible(String.fromCodePoint(c))).toBe(true);
    for (const c of [0x200a, 0x2010, 0x2029, 0x202f, 0x205f, 0x206a]) expect(hasInvisible(String.fromCodePoint(c))).toBe(false);
  });
});
