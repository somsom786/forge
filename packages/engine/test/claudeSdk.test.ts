import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { afterEach, expect, it } from 'vitest';
import { CLAUDE_SDK_VERSION, loadClaudeSdk, resolveClaudeSdk } from '../src/adapters/claudeSdk.js';

const temporary: string[] = [];
afterEach(() => { for (const dir of temporary.splice(0)) rmSync(dir, { recursive: true, force: true }); });

it('loads a separately installed, pinned SDK without starting a provider', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'forge-sdk-'));
  temporary.push(dir);
  writeFileSync(join(dir, 'package.json'), JSON.stringify({ name: '@anthropic-ai/claude-agent-sdk', version: CLAUDE_SDK_VERSION }));
  writeFileSync(join(dir, 'sdk.mjs'), 'export function query() { throw new Error("never invoke a model in this fixture"); }');
  expect(typeof (await loadClaudeSdk({ FORGE_CLAUDE_SDK_PATH: dir })).query).toBe('function');
  writeFileSync(join(dir, 'package.json'), JSON.stringify({ name: '@anthropic-ai/claude-agent-sdk', version: '0.0.1' }));
  expect(() => resolveClaudeSdk({ FORGE_CLAUDE_SDK_PATH: dir })).toThrow('expected @anthropic-ai/claude-agent-sdk');
});

it('refuses invalid explicit overrides without falling back to another SDK', () => {
  let fallback = false;
  expect(() => resolveClaudeSdk({ FORGE_CLAUDE_SDK_PATH: 'relative/sdk' }, () => { fallback = true; return ''; })).toThrow('absolute package directory');
  expect(fallback).toBe(false);
  expect(() => resolveClaudeSdk({}, () => { throw new Error('missing fixture'); })).toThrow('separately installed');
});
