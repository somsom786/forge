import { execFileSync, spawnSync } from 'node:child_process';
import { copyFileSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { tmpdir } from 'node:os';
import { expect, it } from 'vitest';

it('runs the shipped bundle without redistributing or requiring the Claude SDK for other providers', () => {
  const root = resolve(import.meta.dirname, '..');
  execFileSync(process.execPath, [join(root, 'scripts/bundle.mjs')], { cwd: root, timeout: 30_000, stdio: 'pipe' });
  const inputs = JSON.parse(readFileSync(join(root, 'bundle/inputs.json'), 'utf8')) as string[];
  expect(inputs.some((p) => p.replaceAll('\\', '/').includes('node_modules/@anthropic-ai/'))).toBe(false);
  const dir = mkdtempSync(join(tmpdir(), 'forge-bundle-'));
  try {
    const main = join(dir, 'main.mjs');
    copyFileSync(join(root, 'bundle/main.js'), main);
    const start = { t: 'start', sessionId: 'fixture', provider: 'fake', cwd: dir, prompt: 'FAKE:snoop SNOOP=fixture.txt', permissionMode: 'default' };
    const env = { ...process.env, FORGE_ALLOW_FAKE_PROVIDER: '1', FORGE_CLAUDE_SDK_PATH: join(dir, 'missing-sdk') };
    const fake = spawnSync(process.execPath, [main], { cwd: dir, input: JSON.stringify(start) + '\n', env, encoding: 'utf8', timeout: 15_000 });
    expect(fake.status, fake.stderr).toBe(0);
    expect(fake.stdout).toContain('"status":"success"');
    const claude = spawnSync(process.execPath, [main], { cwd: dir, input: JSON.stringify({ ...start, provider: 'claude' }) + '\n', env, encoding: 'utf8', timeout: 15_000 });
    expect(claude.stdout).toContain('separately installed Agent SDK');
    expect(claude.stdout).not.toContain('"status":"success"');
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}, 45_000);
