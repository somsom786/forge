// Interoperability with a REAL third-party ACP agent: @agentclientprotocol/claude-agent-acp (the ACP project's Claude Code
// bridge, Apache-2.0), run unchanged through FORGE's ACP adapter. Its model is the scripted local server
// (fixtures/fake-anthropic.mjs), so no account or quota is involved; the agent, its Claude Code and the protocol are real.
// Separate from acp.test.ts, which uses FORGE's own scripted ACP agent.
//
// Needs the agent installed in FORGE_ACP_INTEROP_DIR (CI does this with a pinned version). Skipped otherwise, unless
// FORGE_REQUIRE_ACP_INTEROP=1.
import { spawn, type ChildProcess } from 'node:child_process';
import { existsSync, mkdtempSync, realpathSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createInterface } from 'node:readline';
import { fileURLToPath } from 'node:url';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { createAcpAdapter } from '../src/adapters/acp.js';
import type { TurnResult } from '../src/adapters/claudeNormalize.js';
import type { PermissionDecision, PermissionRequest, RunContext } from '../src/adapters/types.js';

const dir = process.env.FORGE_ACP_INTEROP_DIR;
const installed = !!dir && existsSync(join(dir, 'node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js'));
if (process.env.FORGE_REQUIRE_ACP_INTEROP === '1' && !installed) throw new Error(`claude-agent-acp is required but not installed in ${dir}`);
const FX = (n: string) => fileURLToPath(new URL(`./fixtures/${n}`, import.meta.url));

let model: ChildProcess;
beforeAll(async () => {
  if (!installed) return;
  model = spawn(process.execPath, [FX('fake-anthropic.mjs')], { stdio: ['pipe', 'pipe', 'inherit'] });
  const line = await new Promise<string>((r) => createInterface({ input: model.stdout! }).once('line', r));
  process.env.FAKE_MODEL_PORT = String(JSON.parse(line).port);
  process.env.ACP_INTEROP_AGENT_DIR = dir;
  process.env.FAKE_CLAUDE_CONFIG_DIR = mkdtempSync(join(tmpdir(), 'acp-interop-cfg-'));
});
afterAll(() => model?.kill());

async function run(prompt: string, decide: (r: PermissionRequest) => PermissionDecision) {
  const cwd = realpathSync(mkdtempSync(join(tmpdir(), 'forge-acp-interop-')));
  const asked: PermissionRequest[] = [];
  const events: Array<{ kind: string; payload: Record<string, unknown> }> = [];
  let turn: TurnResult | undefined;
  const ctx: RunContext = {
    start: { t: 'start', sessionId: 's1', provider: 'acp', cwd, prompt, permissionMode: 'default', acp: { name: 'claude-agent-acp', command: process.execPath, args: [FX('claude-agent-acp-launcher.mjs')] } },
    signal: new AbortController().signal,
    emit: (kind, payload) => events.push({ kind, payload }),
    delta: () => undefined,
    providerSession: () => undefined,
    requestPermission: async (r) => { asked.push(r); return decide(r); },
    turnComplete: (t) => { turn = t; },
  };
  await createAcpAdapter().run(ctx);
  return { cwd, asked, events, turn: turn! };
}

describe.skipIf(!installed)('real third-party ACP agent (claude-agent-acp)', () => {
  it('negotiates protocol 1, opens a session, and its write waits for FORGE\'s decision', async () => {
    const r = await run('Add the note. CLAUDE:write', () => ({ behavior: 'allow' }));
    expect(r.turn.status).toBe('success');
    const started = r.events.find((e) => e.kind === 'session.started')!.payload.acp as Record<string, unknown>;
    expect(started.protocolVersion).toBe(1);
    expect(r.asked.map((a) => [a.tool, a.input.file_path])).toEqual([['Edit', join(r.cwd, 'FORGE_CLAUDE.md')]]);
    expect(existsSync(join(r.cwd, 'FORGE_CLAUDE.md'))).toBe(true);
  }, 120_000);

  it('a FORGE denial is honoured: nothing is written', async () => {
    const r = await run('Add the note. CLAUDE:write', () => ({ behavior: 'deny', message: 'no' }));
    expect(r.turn.permissionDenials).toBe(1);
    expect(existsSync(join(r.cwd, 'FORGE_CLAUDE.md'))).toBe(false);
  }, 120_000);

  it('a write outside the worktree reaches FORGE with the real outside path (so policy can deny it)', async () => {
    const r = await run('Write the file. CLAUDE:escape', () => ({ behavior: 'deny', message: 'outside' }));
    expect(r.asked.some((a) => String(a.input.file_path).endsWith('outside-claude.txt') && !String(a.input.file_path).startsWith(r.cwd + '/'))).toBe(true);
    expect(existsSync(join(r.cwd, '..', 'outside-claude.txt'))).toBe(false);
  }, 120_000);
});
