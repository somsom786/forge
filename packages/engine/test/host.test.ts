// End-to-end tests of the real engine process over its real stdio protocol, driven by the
// scripted TEST agent. These prove FORGE's own plumbing (permission round-trip, file effects,
// cancellation, disconnect handling) — they say nothing about live provider interoperability.

import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process';
import { mkdtempSync, existsSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterEach, describe, expect, it } from 'vitest';

const MAIN = join(dirname(fileURLToPath(import.meta.url)), '..', 'dist', 'main.js');

class Engine {
  proc: ChildProcessWithoutNullStreams;
  lines: any[] = [];
  private buf = '';
  exitCode: number | null = null;
  constructor(env: Record<string, string> = { FORGE_ALLOW_FAKE_PROVIDER: '1' }) {
    this.proc = spawn(process.execPath, [MAIN], { env: { ...process.env, ...env } });
    this.proc.stdout.on('data', (d) => {
      this.buf += d.toString();
      let i;
      while ((i = this.buf.indexOf('\n')) >= 0) {
        const l = this.buf.slice(0, i);
        this.buf = this.buf.slice(i + 1);
        if (l.trim()) this.lines.push(JSON.parse(l));
      }
    });
    this.proc.on('exit', (c) => (this.exitCode = c));
  }
  send(o: object) { this.proc.stdin.write(JSON.stringify(o) + '\n'); }
  async waitFor(pred: (m: any) => boolean, ms = 8000): Promise<any> {
    const t0 = Date.now();
    for (;;) {
      const m = this.lines.find(pred);
      if (m) return m;
      if (Date.now() - t0 > ms) throw new Error('timeout; saw: ' + JSON.stringify(this.lines.map((l) => l.t + ':' + (l.kind ?? ''))));
      await new Promise((r) => setTimeout(r, 15));
    }
  }
  async waitExit(ms = 8000): Promise<number | null> {
    const t0 = Date.now();
    while (this.exitCode === null) {
      if (Date.now() - t0 > ms) throw new Error('engine did not exit');
      await new Promise((r) => setTimeout(r, 15));
    }
    return this.exitCode;
  }
  kill() { try { this.proc.kill(); } catch { /* gone */ } }
}

const engines: Engine[] = [];
const dirs: string[] = [];
afterEach(() => { engines.forEach((e) => e.kill()); engines.length = 0; dirs.forEach((d) => rmSync(d, { recursive: true, force: true })); dirs.length = 0; });

function setup(prompt: string, env?: Record<string, string>) {
  const cwd = mkdtempSync(join(tmpdir(), 'forge-engine-'));
  dirs.push(cwd);
  const e = new Engine(env);
  engines.push(e);
  e.send({ t: 'start', sessionId: '11111111-2222-3333-4444-555555555555', provider: 'fake', cwd, prompt, permissionMode: 'default' });
  return { e, cwd };
}

describe('engine host protocol (scripted test agent)', () => {
  it('completes a permission round-trip: nothing is written until allowed', async () => {
    const { e, cwd } = setup('add a note');
    const ready = await e.waitFor((m) => m.t === 'ready');
    expect(ready.provider).toBe('fake');
    expect(ready.capabilities.resume.supported).toBe(false);

    const perm = await e.waitFor((m) => m.t === 'permission');
    expect(perm.tool).toBe('Write');
    expect(existsSync(join(cwd, 'FORGE_TEST.md'))).toBe(false); // blocked on the core's decision
    e.send({ t: 'permission_result', requestId: perm.requestId, behavior: 'allow' });

    const done = await e.waitFor((m) => m.t === 'turn_complete');
    expect(done.status).toBe('success');
    expect(readFileSync(join(cwd, 'FORGE_TEST.md'), 'utf8')).toContain('add a note');
    expect(await e.waitExit()).toBe(0);

    const seqs = e.lines.filter((m) => m.t === 'event').map((m) => m.seq);
    expect(seqs).toEqual([...seqs].sort((a, b) => a - b));
    expect(new Set(seqs).size).toBe(seqs.length); // strictly increasing, no duplicates
    expect(e.lines.some((m) => m.t === 'event' && m.kind === 'file.changed')).toBe(true);
  });

  it('a denied permission leaves the filesystem untouched and is reported', async () => {
    const { e, cwd } = setup('add a note');
    const perm = await e.waitFor((m) => m.t === 'permission');
    e.send({ t: 'permission_result', requestId: perm.requestId, behavior: 'deny', message: 'no' });
    const done = await e.waitFor((m) => m.t === 'turn_complete');
    expect(done.permissionDenials).toBe(1);
    expect(existsSync(join(cwd, 'FORGE_TEST.md'))).toBe(false);
  });

  it('cancel aborts a hung session and denies outstanding permission requests', async () => {
    const { e } = setup('FAKE:hang');
    await e.waitFor((m) => m.t === 'ready');
    await e.waitFor((m) => m.t === 'event' && m.kind === 'agent.text');
    e.send({ t: 'cancel' });
    const done = await e.waitFor((m) => m.t === 'turn_complete');
    expect(done.status).toBe('interrupted');
    expect(await e.waitExit()).toBe(0);
  });

  it('cancel while a permission is pending resolves it as denied', async () => {
    const { e, cwd } = setup('add a note');
    await e.waitFor((m) => m.t === 'permission');
    e.send({ t: 'cancel' });
    const done = await e.waitFor((m) => m.t === 'turn_complete');
    expect(done.status).toBe('interrupted');
    expect(existsSync(join(cwd, 'FORGE_TEST.md'))).toBe(false);
  });

  it('exits on its own when the core disconnects (no orphaned agents)', async () => {
    const { e } = setup('FAKE:hang');
    await e.waitFor((m) => m.t === 'ready');
    e.proc.stdin.end();
    expect(await e.waitExit(10_000)).not.toBeNull();
  }, 15_000);

  it('a crashing agent surfaces as a non-zero exit without a turn_complete', async () => {
    const { e } = setup('FAKE:crash');
    expect(await e.waitExit()).toBe(7);
    expect(e.lines.some((m) => m.t === 'turn_complete')).toBe(false);
  });

  it('refuses the scripted agent unless explicitly enabled', async () => {
    const { e } = setup('x', { FORGE_ALLOW_FAKE_PROVIDER: '0' });
    const f = await e.waitFor((m) => m.t === 'fatal');
    expect(f.message).toMatch(/disabled/);
    expect(await e.waitExit()).toBe(1);
  });

  it('keeps stdout clean: every line is valid protocol JSON', async () => {
    const { e } = setup('add a note');
    const perm = await e.waitFor((m) => m.t === 'permission');
    e.send({ t: 'permission_result', requestId: perm.requestId, behavior: 'allow' });
    await e.waitFor((m) => m.t === 'turn_complete');
    for (const m of e.lines) expect(typeof m.t).toBe('string');
  });
});
