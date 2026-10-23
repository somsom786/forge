import { mkdtempSync, readFileSync, existsSync, realpathSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { acpEnv, chooseOption, createAcpAdapter, forgeRequests, toAcpMcpServers } from '../src/adapters/acp.js';
import type { PermissionDecision, PermissionRequest, RunContext } from '../src/adapters/types.js';
import type { TurnResult } from '../src/adapters/claudeNormalize.js';
import type { StartCommand } from '../src/protocol.js';

const FIXTURE = fileURLToPath(new URL('./fixtures/acp-agent.mjs', import.meta.url));

interface Run {
  asked: PermissionRequest[];
  events: Array<{ kind: string; payload: Record<string, unknown> }>;
  text: string;
  turn: TurnResult;
  result: Record<string, unknown>;
}

/** Runs the real adapter against the scripted ACP agent; `decide` plays the FORGE core. */
async function run(prompt: string, decide: (r: PermissionRequest) => PermissionDecision, opts: { args?: string[]; mcp?: StartCommand['mcpServers']; abortAfterMs?: number } = {}): Promise<Run & { cwd: string }> {
  const cwd = realpathSync(mkdtempSync(join(tmpdir(), 'forge-acp-wt-')));
  const abort = new AbortController();
  const out: Run = { asked: [], events: [], text: '', turn: undefined as unknown as TurnResult, result: {} };
  const start: StartCommand = { t: 'start', sessionId: 's1234567', provider: 'acp', cwd, prompt, permissionMode: 'default', appendSystemPrompt: 'You are working on a FORGE mission.', mcpServers: opts.mcp, acp: { name: 'scripted', command: process.execPath, args: [FIXTURE, ...(opts.args ?? [])] } };
  const ctx: RunContext = {
    start,
    signal: abort.signal,
    emit: (kind, payload) => out.events.push({ kind, payload }),
    delta: (_m, t) => { out.text += t; },
    providerSession: () => undefined,
    requestPermission: async (r) => { out.asked.push(r); return decide(r); },
    turnComplete: (t) => { out.turn = t; },
  };
  if (opts.abortAfterMs) setTimeout(() => abort.abort(), opts.abortAfterMs);
  await createAcpAdapter().run(ctx);
  const m = /RESULT (\{.*\})/.exec(out.text);
  out.result = m ? JSON.parse(m[1]!) : {};
  return { ...out, cwd };
}

const allowAll = (): PermissionDecision => ({ behavior: 'allow' });
const denyAll = (): PermissionDecision => ({ behavior: 'deny', message: 'no' });

describe('ACP adapter against a scripted agent (official SDK on both sides)', () => {
  it('negotiates, reports capabilities and only offers fs (no terminal); prepends mission guidance; passes authorized MCP servers', async () => {
    const r = await run('ACP:report', allowAll, { mcp: { docs: { type: 'stdio', command: 'docs-mcp', args: [] }, remote: { type: 'http', url: 'https://x.invalid/mcp' } } });
    expect(r.turn.status).toBe('success');
    expect(r.result.clientCaps).toMatchObject({ fs: { readTextFile: true, writeTextFile: true }, terminal: false });
    expect(r.result.mcp).toEqual(['docs']); // the agent said it cannot do http MCP, so that one is not offered
    expect(r.result.promptHasGuidance).toBe(true);
    const started = r.events.find((e) => e.kind === 'session.started')!.payload.acp as Record<string, unknown>;
    expect(started).toMatchObject({ protocolVersion: 1, agentInfo: { name: 'scripted-acp' }, loadSession: false, mcpServersGiven: ['docs'], mcpServersDropped: 1 });
  });

  it('an approved edit is answered with allow_once and the write is performed by FORGE without a second prompt', async () => {
    const r = await run('ACP:edit', allowAll);
    expect(r.result).toMatchObject({ edit: 'ao', write: 'ok' });
    expect(r.asked.map((a) => [a.tool, a.input.file_path])).toEqual([['Edit', join(r.cwd, 'FORGE_ACP.md')]]);
    expect(readFileSync(join(r.cwd, 'FORGE_ACP.md'), 'utf8')).toBe('# written through FORGE\n');
    expect(r.events.some((e) => e.kind === 'file.changed')).toBe(true);
    expect(r.turn).toMatchObject({ status: 'success', permissionDenials: 0 });
  });

  it('a denied edit is answered with reject_once and nothing is written', async () => {
    const r = await run('ACP:edit', denyAll);
    expect(r.result.edit).toBe('ro');
    expect(existsSync(join(r.cwd, 'FORGE_ACP.md'))).toBe(false);
    expect(r.turn.permissionDenials).toBe(1);
  });

  it('a write the agent did not ask about first goes through FORGE as a Write request', async () => {
    const denied = await run('ACP:fswrite', denyAll);
    expect(denied.asked.map((a) => a.tool)).toEqual(['Write']);
    expect(String(denied.result.write)).toMatch(/^error: .*no/);
    expect(existsSync(join(denied.cwd, 'FORGE_FS.md'))).toBe(false);
    const allowed = await run('ACP:fswrite', allowAll);
    expect(readFileSync(join(allowed.cwd, 'FORGE_FS.md'), 'utf8')).toBe('direct\n');
  });

  it('never grants standing permission: an agent offering only "always" options is refused even when FORGE allows', async () => {
    const r = await run('ACP:always', allowAll);
    expect(r.result.edit).toBe('cancelled');
    expect(r.events.some((e) => e.kind === 'status' && String(e.payload.message).includes('allow once'))).toBe(true);
  });

  it('re-checks the real path at write time: a target swapped for a link after approval is not written through', async (t) => {
    const r = await run('ACP:link', allowAll);
    expect(r.result.edit).toBe('ao');
    // Creating a symlink needs a privilege on some Windows setups; then this attack cannot be staged here.
    if (r.result.symlinkError) t.skip();
    expect(String(r.result.write)).toMatch(/link/);
    expect(readFileSync(String(r.result.victim), 'utf8')).toBe('original\n');
  });

  it('maps tool kinds onto FORGE policy requests', async () => {
    const r = await run('ACP:exec ACP:escape ACP:read', denyAll);
    const tools = r.asked.map((a) => [a.tool, a.input.command ?? a.input.file_path]);
    expect(tools).toContainEqual(['Bash', 'git push origin main']);
    expect(tools).toContainEqual(['Edit', join(r.cwd, '..', 'outside-acp.txt')]);
    expect(tools).toContainEqual(['Read', join(r.cwd, '..', 'secret-acp.txt')]);
    expect(String(r.result.read)).toMatch(/^error/);
  });

  it('cancel sends session/cancel and the turn ends interrupted', async () => {
    const r = await run('ACP:hang', allowAll, { abortAfterMs: 400 });
    expect(r.turn.status).toBe('interrupted');
  });

  it('refuses an agent speaking another protocol version, and never authenticates on the user\'s behalf', async () => {
    const v = await run('ACP:report', allowAll, { args: ['--proto=99'] });
    expect(v.turn).toMatchObject({ status: 'error' });
    expect(v.turn.message).toMatch(/protocol version 99/);
    const a = await run('ACP:report', allowAll, { args: ['--auth'] });
    expect(a.turn.message).toMatch(/sign in.*outside FORGE/);
  });

  it('reports a missing or crashing agent as an error', async () => {
    const cwd = mkdtempSync(join(tmpdir(), 'forge-acp-x-'));
    let turn: TurnResult | undefined;
    const ctx = { start: { t: 'start', sessionId: 's', provider: 'acp', cwd, prompt: 'x', permissionMode: 'default', acp: { name: 'x', command: process.execPath, args: ['-e', 'process.exit(3)'] } }, signal: new AbortController().signal, emit: () => undefined, delta: () => undefined, providerSession: () => undefined, requestPermission: async () => ({ behavior: 'deny' }), turnComplete: (t: TurnResult) => { turn = t; } } as RunContext;
    await createAcpAdapter().run(ctx);
    expect(turn).toMatchObject({ status: 'error' });
    expect(turn!.message).toMatch(/exited \(code 3\)/);
  });
});

describe('ACP helpers', () => {
  it('only once options are chosen', () => {
    const o = [{ optionId: 'a', name: 'A', kind: 'allow_always' as const }, { optionId: 'b', name: 'B', kind: 'allow_once' as const }, { optionId: 'c', name: 'C', kind: 'reject_always' as const }];
    expect(chooseOption(o, true)).toEqual({ outcome: 'selected', optionId: 'b' });
    expect(chooseOption(o, false)).toEqual({ outcome: 'selected', optionId: 'c' });
    expect(chooseOption([o[0]!], true)).toEqual({ outcome: 'cancelled' });
  });
  it('write kinds without a location carry no path (policy denies them); multi-location moves check each path', () => {
    expect(forgeRequests({ toolCallId: 't', kind: 'delete' }, '/w')).toEqual([{ tool: 'Edit', input: { acpTitle: null, acpKind: 'delete' } }]);
    expect(forgeRequests({ toolCallId: 't', kind: 'move', locations: [{ path: '/w/a' }, { path: 'b' }] }, '/w').map((r) => r.input.file_path)).toEqual(['/w/a', '/w/b']);
    expect(forgeRequests({ toolCallId: 't', kind: 'switch_mode' }, '/w')[0]!.tool).toBe('acp_switch_mode');
  });
  it('strips billing credentials and FORGE internals from the agent environment', () => {
    const env = acpEnv({ PATH: 'p', OPENAI_API_KEY: 'k', ANTHROPIC_API_KEY: 'k', GEMINI_API_KEY: 'k', FORGE_ALLOW_FAKE_PROVIDER: '1' });
    expect(env).toEqual({ PATH: 'p' });
  });
  it('converts MCP configs to the ACP shape', () => {
    expect(toAcpMcpServers({ d: { type: 'stdio', command: 'c', args: ['a'], env: { T: 'v' } } }, false)).toEqual([{ name: 'd', command: 'c', args: ['a'], env: [{ name: 'T', value: 'v' }] }]);
  });
});
