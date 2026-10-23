import { mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { CODEX_DISABLED_FEATURES, codexServerArgs, createCodexAdapter, innerCommand, projectCodexConfig } from '../src/adapters/codex.js';
import { type CodexRpc, RpcError, sanitizedCodexEnv } from '../src/adapters/codexRpc.js';
import type { PermissionDecision, PermissionRequest, RunContext } from '../src/adapters/types.js';
import type { StartCommand } from '../src/protocol.js';

type Handler = (params: any) => unknown; // eslint-disable-line @typescript-eslint/no-explicit-any

/** A scripted stand-in for `codex app-server`: answers requests, and can push notifications / server requests. */
function peer(handlers: Record<string, Handler>) {
  let notif: (m: string, p: any) => void = () => {}; // eslint-disable-line @typescript-eslint/no-explicit-any
  let req: (m: string, p: any) => Promise<unknown> = async () => ({}); // eslint-disable-line @typescript-eslint/no-explicit-any
  const calls: Array<{ method: string; params: any }> = []; // eslint-disable-line @typescript-eslint/no-explicit-any
  let exit!: (v: { code: number | null; stderrTail: string }) => void;
  const exited = new Promise<{ code: number | null; stderrTail: string }>((r) => (exit = r));
  const rpc: CodexRpc = {
    request: async (method, params) => {
      calls.push({ method, params });
      const h = handlers[method];
      if (!h) throw new RpcError(-32601, `no handler for ${method}`);
      return h(params) as never;
    },
    notify: () => {},
    onNotification: (h) => (notif = h),
    onRequest: (h) => (req = h),
  };
  return { server: { rpc, exited, kill: () => exit({ code: 0, stderrTail: '' }) }, push: (m: string, p: unknown) => notif(m, p), ask: (m: string, p: unknown) => req(m, p), calls, die: (code: number, tail = '') => exit({ code, stderrTail: tail }) };
}

function context(start: Partial<StartCommand> = {}, decide: (r: PermissionRequest) => PermissionDecision = () => ({ behavior: 'allow' })) {
  const events: Array<{ kind: string; payload: any }> = []; // eslint-disable-line @typescript-eslint/no-explicit-any
  const asked: PermissionRequest[] = [];
  const abort = new AbortController();
  let result: any; // eslint-disable-line @typescript-eslint/no-explicit-any
  const ctx: RunContext = {
    start: { t: 'start', sessionId: 's1', provider: 'codex', cwd: 'C:\\w', prompt: 'do it', permissionMode: 'default', ...start } as StartCommand,
    signal: abort.signal,
    emit: (kind, payload) => events.push({ kind, payload }),
    delta: () => {},
    providerSession: () => {},
    requestPermission: async (r) => {
      asked.push(r);
      return decide(r);
    },
    turnComplete: (r) => (result = r),
  };
  return { ctx, events, asked, abort, result: () => result };
}

const baseHandlers = (extra: Record<string, Handler> = {}): Record<string, Handler> => ({
  initialize: () => ({}),
  'config/read': () => ({ config: { mcp_servers: { evil: { command: 'node' }, node_repl: { command: 'x' } } } }),
  'thread/start': () => ({ thread: { id: 'th1' } }),
  'thread/resume': () => ({ thread: { id: 'th1' } }),
  'turn/start': () => ({ turn: { id: 'tu1' } }),
  'turn/interrupt': () => ({}),
  ...extra,
});

const completed = (status = 'completed', error?: string) => ({ turn: { id: 'tu1', status, error: error ? { message: error } : null } });

describe('codex adapter', () => {
  it('disables every effective MCP server by name (including repo-defined ones), hooks and plugins, and asks for approval of everything', async () => {
    const p = peer(baseHandlers());
    const c = context({ appendSystemPrompt: 'guidance' });
    const run = createCodexAdapter({ spawnServer: () => p.server }).run(c.ctx);
    await new Promise((r) => setTimeout(r, 20));
    p.push('turn/completed', completed());
    await run;
    const t = p.calls.find((x) => x.method === 'thread/start')!.params;
    expect(t.config.mcp_servers).toEqual({ evil: { enabled: false }, node_repl: { enabled: false } });
    expect(t.approvalPolicy).toBe('untrusted');
    expect(t.sandbox).toBe('workspace-write');
    expect(t.developerInstructions).toBe('guidance');
    expect(p.calls.find((x) => x.method === 'config/read')!.params.cwd).toBe('C:\\w');
    expect(codexServerArgs().join(' ')).toContain('--disable hooks');
    for (const f of ['hooks', 'plugins', 'apps']) expect(CODEX_DISABLED_FEATURES).toContain(f);
    expect(c.result().status).toBe('success');
  });

  it('a file change is approved per path through FORGE policy; one denial declines the whole change', async () => {
    const p = peer(baseHandlers());
    const c = context({}, (r) => (String(r.input.file_path).endsWith('.env') ? { behavior: 'deny' } : { behavior: 'allow' }));
    const run = createCodexAdapter({ spawnServer: () => p.server }).run(c.ctx);
    await new Promise((r) => setTimeout(r, 20));
    p.push('item/started', { item: { type: 'fileChange', id: 'f1', status: 'inProgress', changes: [{ path: 'C:\\w\\a.txt', kind: { type: 'add' }, diff: 'hi' }] } });
    expect(await p.ask('item/fileChange/requestApproval', { itemId: 'f1' })).toEqual({ decision: 'accept' });
    p.push('item/started', { item: { type: 'fileChange', id: 'f2', status: 'inProgress', changes: [{ path: 'C:\\w\\a.txt', kind: { type: 'update' }, diff: 'x' }, { path: 'C:\\w\\.env', kind: { type: 'add' }, diff: 'S=1' }] } });
    expect(await p.ask('item/fileChange/requestApproval', { itemId: 'f2' })).toEqual({ decision: 'decline' });
    expect(c.asked.map((a) => [a.tool, a.input.file_path])).toEqual([['Write', 'C:\\w\\a.txt'], ['Edit', 'C:\\w\\a.txt'], ['Write', 'C:\\w\\.env']]);
    p.push('item/completed', { item: { type: 'fileChange', id: 'f1', status: 'completed', changes: [{ path: 'C:\\w\\a.txt', kind: { type: 'add' } }] } });
    p.push('turn/completed', completed());
    await run;
    expect(c.events.some((e) => e.kind === 'file.changed' && e.payload.path === 'C:\\w\\a.txt')).toBe(true);
    expect(c.result().permissionDenials).toBe(1);
  });

  it('unknown files, a grant root, and unsupported requests are refused rather than left hanging', async () => {
    const p = peer(baseHandlers());
    const c = context();
    const run = createCodexAdapter({ spawnServer: () => p.server }).run(c.ctx);
    await new Promise((r) => setTimeout(r, 20));
    expect(await p.ask('item/fileChange/requestApproval', { itemId: 'nope' })).toEqual({ decision: 'decline' });
    p.push('item/started', { item: { type: 'fileChange', id: 'g', changes: [{ path: 'C:\\w\\a', kind: { type: 'add' } }] } });
    expect(await p.ask('item/fileChange/requestApproval', { itemId: 'g', grantRoot: 'C:\\' })).toEqual({ decision: 'decline' });
    await expect(p.ask('item/permissions/requestApproval', {})).rejects.toBeInstanceOf(RpcError);
    expect(c.asked).toHaveLength(0);
    p.push('turn/completed', completed());
    await run;
  });

  it('shell commands go to policy as PowerShell/Bash with the full command; denial declines', async () => {
    const p = peer(baseHandlers());
    const c = context({}, (r) => (String(r.input.command).includes('git push') ? { behavior: 'deny' } : { behavior: 'allow' }));
    const run = createCodexAdapter({ spawnServer: () => p.server }).run(c.ctx);
    await new Promise((r) => setTimeout(r, 20));
    const cmd = '"C:\\\\WINDOWS\\\\System32\\\\WindowsPowerShell\\\\v1.0\\\\powershell.exe" -Command "git push origin main"';
    expect(await p.ask('item/commandExecution/requestApproval', { itemId: 'c1', command: cmd })).toEqual({ decision: 'decline' });
    expect(c.asked[0]!.tool).toBe(process.platform === 'win32' ? 'PowerShell' : 'Bash');
    expect(c.asked[0]!.input.command).toBe(cmd);
    expect(await p.ask('item/commandExecution/requestApproval', { itemId: 'c2', command: 'git status' })).toEqual({ decision: 'accept' });
    p.push('turn/completed', completed());
    await run;
    expect(innerCommand(cmd)).toBe('git push origin main');
  });

  it('an MCP server or hook that starts anyway aborts the session (tripwire)', async () => {
    const p = peer(baseHandlers());
    const c = context();
    const run = createCodexAdapter({ spawnServer: () => p.server }).run(c.ctx);
    await new Promise((r) => setTimeout(r, 20));
    p.push('mcpServer/startupStatus/updated', { name: 'sneaky', status: 'starting' });
    await run;
    expect(c.result().status).toBe('error');
    expect(c.result().message).toContain('sneaky');
    expect(p.calls.some((x) => x.method === 'turn/interrupt')).toBe(true);
  });

  it('cancel interrupts the turn; a crash reports its last words; failures carry the message; usage is reported', async () => {
    let p = peer(baseHandlers());
    let c = context();
    let run = createCodexAdapter({ spawnServer: () => p.server }).run(c.ctx);
    await new Promise((r) => setTimeout(r, 20));
    c.abort.abort();
    await new Promise((r) => setTimeout(r, 20));
    p.push('turn/completed', completed('interrupted'));
    await run;
    expect(c.result().status).toBe('interrupted');
    expect(p.calls.some((x) => x.method === 'turn/interrupt' && x.params.turnId === 'tu1')).toBe(true);

    p = peer(baseHandlers());
    c = context();
    run = createCodexAdapter({ spawnServer: () => p.server }).run(c.ctx);
    await new Promise((r) => setTimeout(r, 20));
    p.die(9, 'boom: not logged in');
    await run;
    expect(c.result().status).toBe('error');
    expect(c.result().message).toContain('not logged in');

    p = peer(baseHandlers());
    c = context();
    run = createCodexAdapter({ spawnServer: () => p.server }).run(c.ctx);
    await new Promise((r) => setTimeout(r, 20));
    p.push('thread/tokenUsage/updated', { tokenUsage: { total: { inputTokens: 100, cachedInputTokens: 40, outputTokens: 7 } } });
    p.push('turn/completed', completed('failed', 'usage limit reached'));
    await run;
    expect(c.result()).toMatchObject({ status: 'error', message: 'usage limit reached', providerSessionId: 'th1' });
    expect(c.result().usage).toMatchObject({ inputTokens: 100, cacheReadTokens: 40, outputTokens: 7 });
  });

  it('resume uses thread/resume with the stored thread id', async () => {
    const p = peer(baseHandlers());
    const c = context({ resume: 'old-thread' });
    const run = createCodexAdapter({ spawnServer: () => p.server }).run(c.ctx);
    await new Promise((r) => setTimeout(r, 20));
    p.push('turn/completed', completed());
    await run;
    expect(p.calls.find((x) => x.method === 'thread/resume')!.params.threadId).toBe('old-thread');
    expect(p.calls.some((x) => x.method === 'thread/start')).toBe(false);
  });

  it('API-billing environment never reaches the Codex process', () => {
    const env = sanitizedCodexEnv({ OPENAI_API_KEY: 'sk-x', CODEX_API_KEY: 'y', OPENAI_BASE_URL: 'http://evil', PATH: 'p' });
    expect(env).toEqual({ PATH: 'p' });
  });

  it('refuses to start at all when the worktree carries project Codex config (it would run MCP servers)', async () => {
    const root = mkdtempSync(join(tmpdir(), 'forge-codex-'));
    mkdirSync(join(root, '.git'));
    mkdirSync(join(root, '.codex'));
    writeFileSync(join(root, '.codex', 'config.toml'), ['[mcp_servers.evil]', 'command="node"', ''].join(String.fromCharCode(10)));
    const sub = join(root, 'pkg');
    mkdirSync(sub);
    expect(projectCodexConfig(root)).toHaveLength(1);
    expect(projectCodexConfig(sub)).toHaveLength(1); // found from a subfolder too
    let spawned = false;
    const c = context({ cwd: root });
    await createCodexAdapter({ spawnServer: () => { spawned = true; return peer(baseHandlers()).server; } }).run(c.ctx);
    expect(spawned).toBe(false);
    expect(c.result().status).toBe('error');
    expect(c.result().message).toContain('will not run a Codex mission');
    const clean = mkdtempSync(join(tmpdir(), 'forge-codex-clean-'));
    mkdirSync(join(clean, '.git'));
    expect(projectCodexConfig(clean)).toEqual([]);
  });
});
