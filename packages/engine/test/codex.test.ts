import { EventEmitter } from 'node:events';
import { chmodSync, existsSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { PassThrough } from 'node:stream';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { CODEX_DISABLED_FEATURES, UNGATED_ITEM_TYPES, codexServerArgs, createCodexAdapter, effectiveSettingsProblems, homeInside, innerCommand, projectCodexConfig, userAllowRules } from '../src/adapters/codex.js';
import { type CodexRpc, RpcError, sanitizedCodexEnv, spawnCodexServer, windowsTreeKill } from '../src/adapters/codexRpc.js';
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

/** What a well-behaved app-server reports as applied: exactly what was requested. */
const effective = (p: any) => ({ sandbox: { type: 'workspaceWrite', writableRoots: [], networkAccess: false }, approvalPolicy: 'untrusted', approvalsReviewer: 'user', cwd: p.cwd, modelProvider: p.modelProvider }); // eslint-disable-line @typescript-eslint/no-explicit-any

/** Every item and approval carries the thread and turn it belongs to. */
const S = { threadId: 'th1', turnId: 'tu1' };

const baseHandlers = (extra: Record<string, Handler> = {}): Record<string, Handler> => ({
  initialize: () => ({}),
  'account/read': () => ({ account: { type: 'chatgpt', email: null, planType: 'plus' } }),
  'config/read': () => ({ config: { mcp_servers: { evil: { command: 'node' }, node_repl: { command: 'x' } } } }),
  'thread/start': (p) => ({ thread: { id: 'th1' }, ...effective(p) }),
  'thread/resume': (p) => ({ thread: { id: 'th1' }, ...effective(p) }),
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
    p.push('item/started', { ...S, item: { type: 'fileChange', id: 'f1', status: 'inProgress', changes: [{ path: 'C:\\w\\a.txt', kind: { type: 'add' }, diff: 'hi' }] } });
    expect(await p.ask('item/fileChange/requestApproval', { ...S, itemId: 'f1' })).toEqual({ decision: 'accept' });
    p.push('item/started', { ...S, item: { type: 'fileChange', id: 'f2', status: 'inProgress', changes: [{ path: 'C:\\w\\a.txt', kind: { type: 'update' }, diff: 'x' }, { path: 'C:\\w\\.env', kind: { type: 'add' }, diff: 'S=1' }] } });
    expect(await p.ask('item/fileChange/requestApproval', { ...S, itemId: 'f2' })).toEqual({ decision: 'decline' });
    expect(c.asked.map((a) => [a.tool, a.input.file_path])).toEqual([['Write', 'C:\\w\\a.txt'], ['Edit', 'C:\\w\\a.txt'], ['Write', 'C:\\w\\.env']]);
    p.push('item/completed', { ...S, item: { type: 'fileChange', id: 'f1', status: 'completed', changes: [{ path: 'C:\\w\\a.txt', kind: { type: 'add' } }] } });
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
    expect(await p.ask('item/fileChange/requestApproval', { ...S, itemId: 'nope' })).toEqual({ decision: 'decline' });
    p.push('item/started', { ...S, item: { type: 'fileChange', id: 'g', changes: [{ path: 'C:\\w\\a', kind: { type: 'add' } }] } });
    expect(await p.ask('item/fileChange/requestApproval', { ...S, itemId: 'g', grantRoot: 'C:\\' })).toEqual({ decision: 'decline' });
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
    expect(await p.ask('item/commandExecution/requestApproval', { ...S, itemId: 'c1', command: cmd })).toEqual({ decision: 'decline' });
    expect(c.asked[0]!.tool).toBe(process.platform === 'win32' ? 'PowerShell' : 'Bash');
    expect(c.asked[0]!.input.command).toBe(cmd);
    expect(await p.ask('item/commandExecution/requestApproval', { ...S, itemId: 'c2', command: 'git status' })).toEqual({ decision: 'accept' });
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

  it('refuses API-key / custom-provider logins: only a ChatGPT login may run a mission', async () => {
    const p = peer(baseHandlers({ 'account/read': () => ({ account: { type: 'apiKey' } }) }));
    const c = context();
    await createCodexAdapter({ spawnServer: () => p.server }).run(c.ctx);
    expect(c.result().status).toBe('error');
    expect(c.result().message).toContain('not with a ChatGPT login');
    expect(p.calls.some((x) => x.method === 'turn/start' || x.method === 'thread/start')).toBe(false);
  });

  it('closes notify and live web search at the process level, and pins the provider', async () => {
    expect(codexServerArgs().join(' ')).toContain('-c notify=[]');
    expect(codexServerArgs().join(' ')).toContain('-c web_search="disabled"');
    const p = peer(baseHandlers());
    const c = context();
    const run = createCodexAdapter({ spawnServer: () => p.server }).run(c.ctx);
    await new Promise((r) => setTimeout(r, 20));
    p.push('turn/completed', completed());
    await run;
    expect(p.calls.find((x) => x.method === 'thread/start')!.params.modelProvider).toBe('openai');
  });

  it('user exec allow-rules and a Codex home inside the repo are detected', () => {
    const home = mkdtempSync(join(tmpdir(), 'forge-codexhome-'));
    expect(userAllowRules(home)).toEqual([]);
    mkdirSync(join(home, 'rules'));
    writeFileSync(join(home, 'rules', 'default.rules'), 'prefix_rule(pattern=["rg"], decision="allow")');
    writeFileSync(join(home, 'rules', 'other.rules'), 'prefix_rule(pattern=["rm"], decision="forbidden")');
    expect(userAllowRules(home)).toEqual(['default.rules (1 allow rule)']);
    expect(homeInside(join(home, 'repo'), join(home, 'repo', '.codex'))).toBe(true);
    expect(homeInside(join(home, 'repo'), join(home, 'elsewhere'))).toBe(false);
  });

  it('environment scrubbing is case-insensitive (Windows)', () => {
    expect(sanitizedCodexEnv({ openai_api_key: 'x', Codex_Api_Key: 'y', Path: 'p' })).toEqual({ Path: 'p' });
  });

  it('command approvals with distinct approvalIds get distinct toolUseIds even for the same item and command', async () => {
    const p = peer(baseHandlers());
    const c = context();
    const run = createCodexAdapter({ spawnServer: () => p.server }).run(c.ctx);
    await new Promise((r) => setTimeout(r, 20));
    const params = { itemId: 'same', command: 'git status' };
    await p.ask('item/commandExecution/requestApproval', { ...S, ...params, approvalId: 'a1' });
    await p.ask('item/commandExecution/requestApproval', { ...S, ...params, approvalId: 'a2' });
    expect(c.asked).toHaveLength(2);
    expect(c.asked[0]!.toolUseId).not.toBe(c.asked[1]!.toolUseId);
    p.push('turn/completed', completed());
    await run;
  });

  it('if the server dies while a permission request is pending, the run still ends in error and does not hang', async () => {
    const p = peer(baseHandlers());
    const c = context();
    c.ctx.requestPermission = async (r) => {
      c.asked.push(r);
      return new Promise<PermissionDecision>(() => {}); // never answered
    };
    const run = createCodexAdapter({ spawnServer: () => p.server }).run(c.ctx);
    await new Promise((r) => setTimeout(r, 20));
    void p.ask('item/commandExecution/requestApproval', { ...S, itemId: 'c1', command: 'git status', approvalId: 'x' });
    await new Promise((r) => setTimeout(r, 20));
    expect(c.asked).toHaveLength(1);
    p.die(3, 'crashed');
    await Promise.race([run, new Promise((_, rej) => setTimeout(() => rej(new Error('run hung')), 3000))]);
    expect(c.result().status).toBe('error');
    expect(c.result().message).toContain('exited unexpectedly');
  });

  it('an RPC request that gets no answer fails after the deadline', async () => {
    const server = spawnCodexServer(process.execPath, ['-e', 'setInterval(()=>{},1000)'], process.cwd(), process.env, 100);
    try {
      await expect(server.rpc.request('initialize', {})).rejects.toThrow(/did not answer initialize/);
    } finally {
      server.kill();
      await server.exited;
    }
  });
  // ---- effective settings (review 5, finding 8) -------------------------------------------------------------------------

  it('records the settings Codex actually applied, and refuses anything broader before a turn starts', async () => {
    const wt = join(tmpdir(), 'forge-wt');
    const broader: Array<[string, (p: any) => Record<string, unknown>]> = [ // eslint-disable-line @typescript-eslint/no-explicit-any
      ['dangerFullAccess', (p) => ({ ...effective(p), sandbox: { type: 'dangerFullAccess' } })],
      ['externalSandbox', (p) => ({ ...effective(p), sandbox: { type: 'externalSandbox', networkAccess: 'restricted' } })],
      ['network access', (p) => ({ ...effective(p), sandbox: { type: 'workspaceWrite', writableRoots: [], networkAccess: true } })],
      ['outside the worktree', (p) => ({ ...effective(p), sandbox: { type: 'workspaceWrite', writableRoots: [join(tmpdir(), 'elsewhere')], networkAccess: false } })],
      ['approval policy', (p) => ({ ...effective(p), approvalPolicy: 'never' })],
      ['approval policy', (p) => ({ ...effective(p), approvalPolicy: { granular: { rules: false, sandbox_approval: true, mcp_elicitations: false } } })],
      ['reviewed by', (p) => ({ ...effective(p), approvalsReviewer: 'auto_review' })],
      ['not the mission worktree', (p) => ({ ...effective(p), cwd: join(tmpdir(), 'other') })],
      ['model provider', (p) => ({ ...effective(p), modelProvider: 'azure' })],
      ['did not report the sandbox', (p) => ({ ...effective(p), sandbox: undefined })],
    ];
    for (const [why, report] of broader) {
      const p = peer(baseHandlers({ 'thread/start': (q) => ({ thread: { id: 'th1' }, ...report(q) }) }));
      const c = context({ cwd: wt });
      await createCodexAdapter({ spawnServer: () => p.server }).run(c.ctx);
      expect(c.result().status, why).toBe('error');
      expect(c.result().message, why).toContain(why);
      expect(p.calls.some((x) => x.method === 'turn/start'), why).toBe(false);
      const started = c.events.find((e) => e.kind === 'session.started')!.payload;
      expect(started.effectiveAccepted).toBe(false);
      expect(started.requested.sandbox).toBe('workspace-write');
    }
    // Narrower or equal is fine: a writable root inside the worktree, or read-only.
    expect(effectiveSettingsProblems({ ...effective({ cwd: wt, modelProvider: 'openai' }), sandbox: { type: 'workspaceWrite', writableRoots: [join(wt, 'sub')], networkAccess: false } }, { cwd: wt, modelProvider: 'openai' })).toEqual([]);
    expect(effectiveSettingsProblems({ ...effective({ cwd: wt, modelProvider: 'openai' }), sandbox: { type: 'readOnly', networkAccess: false } }, { cwd: wt, modelProvider: 'openai' })).toEqual([]);
    // The accepted case records the effective values, not the requested ones.
    const p = peer(baseHandlers());
    const c = context({ cwd: wt });
    const run = createCodexAdapter({ spawnServer: () => p.server }).run(c.ctx);
    await new Promise((r) => setTimeout(r, 20));
    p.push('turn/completed', completed());
    await run;
    const started = c.events.find((e) => e.kind === 'session.started')!.payload;
    expect(started.effectiveAccepted).toBe(true);
    expect(started.effective).toMatchObject({ sandbox: { type: 'workspaceWrite', networkAccess: false }, approvalPolicy: 'untrusted', approvalsReviewer: 'user', cwd: wt, modelProvider: 'openai' });
    expect(c.result().status).toBe('success');
  });

  it('a resumed thread is held to the same effective-settings check', async () => {
    const p = peer(baseHandlers({ 'thread/resume': (q) => ({ thread: { id: 'th1' }, ...effective(q), approvalPolicy: 'on-request' }) }));
    const c = context({ resume: 'old' });
    await createCodexAdapter({ spawnServer: () => p.server }).run(c.ctx);
    expect(c.result().status).toBe('error');
    expect(p.calls.some((x) => x.method === 'turn/start')).toBe(false);
  });

  // ---- approval scope (review 5, finding 6) -------------------------------------------------------------------------------

  it('approvals for another thread or turn, or for an item of another type, are declined without asking', async () => {
    const p = peer(baseHandlers());
    const c = context();
    const run = createCodexAdapter({ spawnServer: () => p.server }).run(c.ctx);
    await new Promise((r) => setTimeout(r, 20));
    expect(await p.ask('item/commandExecution/requestApproval', { threadId: 'other', turnId: 'tu1', itemId: 'c1', command: 'ls' })).toEqual({ decision: 'decline' });
    expect(await p.ask('item/commandExecution/requestApproval', { threadId: 'th1', turnId: 'old-turn', itemId: 'c1', command: 'ls' })).toEqual({ decision: 'decline' });
    expect(await p.ask('item/commandExecution/requestApproval', { itemId: 'c1', command: 'ls' })).toEqual({ decision: 'decline' }); // no scope at all
    p.push('item/started', { ...S, item: { type: 'commandExecution', id: 'c2', command: 'ls', cwd: 'C:\\w' } });
    expect(await p.ask('item/fileChange/requestApproval', { ...S, itemId: 'c2' })).toEqual({ decision: 'decline' });
    p.push('item/started', { threadId: 'th1', turnId: 'stale', item: { type: 'fileChange', id: 'f9', changes: [{ path: 'C:\\w\\x', kind: { type: 'add' } }] } });
    expect(await p.ask('item/fileChange/requestApproval', { ...S, itemId: 'f9' })).toEqual({ decision: 'decline' }); // never recorded
    expect(c.asked).toHaveLength(0);
    p.push('turn/completed', completed());
    await run;
    expect(c.result().permissionDenials).toBe(5);
  });

  it('an item re-announced with different content cannot be approved; replaced after approval aborts the session', async () => {
    let p = peer(baseHandlers());
    let c = context();
    let run = createCodexAdapter({ spawnServer: () => p.server }).run(c.ctx);
    await new Promise((r) => setTimeout(r, 20));
    p.push('item/started', { ...S, item: { type: 'fileChange', id: 'f1', changes: [{ path: 'C:\\w\\a.txt', kind: { type: 'add' }, diff: 'hi' }] } });
    p.push('item/started', { ...S, item: { type: 'fileChange', id: 'f1', changes: [{ path: 'C:\\w\\.git\\config', kind: { type: 'add' }, diff: 'x' }] } });
    expect(await p.ask('item/fileChange/requestApproval', { ...S, itemId: 'f1' })).toEqual({ decision: 'decline' });
    expect(c.asked).toHaveLength(0);
    p.push('turn/completed', completed());
    await run;

    // Replaced while the human was deciding: the decision covers only what they saw.
    p = peer(baseHandlers());
    c = context();
    let release!: () => void;
    const gate = new Promise<void>((r) => (release = r));
    c.ctx.requestPermission = async (r) => {
      c.asked.push(r);
      await gate;
      return { behavior: 'allow' };
    };
    run = createCodexAdapter({ spawnServer: () => p.server }).run(c.ctx);
    await new Promise((r) => setTimeout(r, 20));
    p.push('item/started', { ...S, item: { type: 'fileChange', id: 'f2', changes: [{ path: 'C:\\w\\a.txt', kind: { type: 'add' }, diff: 'hi' }] } });
    const answer = p.ask('item/fileChange/requestApproval', { ...S, itemId: 'f2' });
    await new Promise((r) => setTimeout(r, 10));
    p.push('item/started', { ...S, item: { type: 'fileChange', id: 'f2', changes: [{ path: 'C:\\w\\b.txt', kind: { type: 'add' }, diff: 'other' }] } });
    release();
    expect(await answer).toEqual({ decision: 'decline' });
    p.push('turn/completed', completed());
    await run;

    // Replaced after the approval was given: stop the session.
    p = peer(baseHandlers());
    c = context();
    run = createCodexAdapter({ spawnServer: () => p.server }).run(c.ctx);
    await new Promise((r) => setTimeout(r, 20));
    p.push('item/started', { ...S, item: { type: 'fileChange', id: 'f3', changes: [{ path: 'C:\\w\\a.txt', kind: { type: 'add' }, diff: 'hi' }] } });
    expect(await p.ask('item/fileChange/requestApproval', { ...S, itemId: 'f3' })).toEqual({ decision: 'accept' });
    p.push('item/started', { ...S, item: { type: 'fileChange', id: 'f3', changes: [{ path: 'C:\\w\\a.txt', kind: { type: 'add' }, diff: 'evil' }] } });
    await run;
    expect(c.result().status).toBe('error');
    expect(c.result().message).toContain('after FORGE approved it');
    expect(p.calls.some((x) => x.method === 'turn/interrupt')).toBe(true);
  });

  it('an applied file change touching a path FORGE did not approve aborts the session', async () => {
    for (const approve of [true, false]) {
      const p = peer(baseHandlers());
      const c = context();
      const run = createCodexAdapter({ spawnServer: () => p.server }).run(c.ctx);
      await new Promise((r) => setTimeout(r, 20));
      p.push('item/started', { ...S, item: { type: 'fileChange', id: 'f1', changes: [{ path: 'C:\\w\\a.txt', kind: { type: 'add' } }] } });
      if (approve) await p.ask('item/fileChange/requestApproval', { ...S, itemId: 'f1' });
      p.push('item/completed', { ...S, item: { type: 'fileChange', id: 'f1', status: 'completed', changes: [{ path: 'C:\\w\\a.txt', kind: { type: 'add' } }, { path: 'C:\\w\\b.txt', kind: { type: 'add' } }] } });
      await run;
      expect(c.result().status).toBe('error');
      expect(c.result().message).toContain(approve ? 'b.txt' : 'a.txt');
    }
  });

  it('an approval that arrives before the turn id is known waits for it, then is checked against it', async () => {
    let releaseTurn!: (v: unknown) => void;
    const p = peer(baseHandlers({ 'turn/start': () => new Promise((r) => (releaseTurn = r)) }));
    const c = context();
    const run = createCodexAdapter({ spawnServer: () => p.server }).run(c.ctx);
    await new Promise((r) => setTimeout(r, 20));
    const early = p.ask('item/commandExecution/requestApproval', { ...S, itemId: 'c1', command: 'ls' });
    await new Promise((r) => setTimeout(r, 10));
    expect(c.asked).toHaveLength(0);
    p.push('turn/started', { threadId: 'th1', turn: { id: 'tu1' } });
    expect(await early).toEqual({ decision: 'accept' });
    releaseTurn({ turn: { id: 'tu1' } });
    p.push('turn/completed', completed());
    await run;
  });

  // ---- items with no approval gate (review 5, finding 5) ------------------------------------------------------------------

  it('every item type without an approval gate is surfaced and aborts the session', async () => {
    for (const type of ['mcpToolCall', 'dynamicToolCall', 'webSearch', 'imageGeneration', 'collabAgentToolCall', 'subAgentActivity', 'hookPrompt']) {
      expect(UNGATED_ITEM_TYPES).toContain(type);
      const p = peer(baseHandlers());
      const c = context();
      const run = createCodexAdapter({ spawnServer: () => p.server }).run(c.ctx);
      await new Promise((r) => setTimeout(r, 20));
      p.push('item/started', { ...S, item: { type, id: `i-${type}`, server: 's', tool: 't', arguments: { q: 1 }, query: 'q', status: 'inProgress' } });
      await run;
      expect(c.result().status, type).toBe('error');
      expect(c.result().message, type).toContain(type);
      expect(c.events.some((e) => e.kind === 'tool.call' && e.payload.ungated === true), type).toBe(true);
      expect(p.calls.some((x) => x.method === 'turn/interrupt'), type).toBe(true);
    }
    // Also when only the completion is seen.
    const p = peer(baseHandlers());
    const c = context();
    const run = createCodexAdapter({ spawnServer: () => p.server }).run(c.ctx);
    await new Promise((r) => setTimeout(r, 20));
    p.push('item/completed', { ...S, item: { type: 'webSearch', id: 'w', query: 'q' } });
    await run;
    expect(c.result().status).toBe('error');
  });

  // ---- cleanup never runs anything from the worktree (review 5, finding 1) -----------------------------------------------

  /** A stand-in child process: enough of ChildProcess for spawnCodexServer. */
  function fakeChild(pid: number) {
    const child = Object.assign(new EventEmitter(), { pid, stdin: new PassThrough(), stdout: new PassThrough(), stderr: new PassThrough(), kill: () => true });
    return child;
  }

  it('on Windows, cleanup runs taskkill by absolute System32 path with a neutral cwd, never from the hostile worktree', () => {
    const wt = mkdtempSync(join(tmpdir(), 'forge-hostile-'));
    const marker = join(wt, 'SENTINEL_RAN');
    // A sentinel that would record that it ran, under every name a cwd/PATH lookup could pick.
    for (const name of ['taskkill.exe', 'taskkill', 'taskkill.cmd', 'taskkill.bat']) {
      writeFileSync(join(wt, name), `#!/bin/sh\necho ran > "${marker}"\n`);
      chmodSync(join(wt, name), 0o755);
    }
    const calls: Array<{ command: string; args: string[]; options: Record<string, unknown> }> = [];
    const spawn = (command: string, args: string[], options: Record<string, unknown>) => {
      calls.push({ command, args, options });
      return fakeChild(calls.length === 1 ? 4242 : 1) as never;
    };
    const server = spawnCodexServer('C:\\codex\\codex.exe', ['app-server'], wt, { PATH: `.;${wt}` }, 1000, { spawn, platform: 'win32', hostEnv: { SystemRoot: 'C:\\Windows' } });
    server.kill();
    expect(calls).toHaveLength(2);
    expect(calls[0]!.options.cwd).toBe(wt); // the server itself runs in the worktree
    const kill = calls[1]!;
    expect(kill.command).toBe('C:\\Windows\\System32\\taskkill.exe');
    expect(kill.args).toEqual(['/PID', '4242', '/T', '/F']);
    expect(kill.options.cwd).toBe('C:\\Windows');
    expect(kill.options.shell).toBeUndefined();
    expect(String(kill.options.cwd)).not.toContain(wt);
    expect(existsSync(marker)).toBe(false);
    // A relative or odd SystemRoot never makes the lookup relative.
    // (SystemRoot comes from FORGE's own environment, not the repository, so an absolute drive path is taken as given.)
    for (const bad of ['.', 'Windows', '..\\x', '\\\\server\\share', 'relative\\System', '']) expect(windowsTreeKill(1, { SystemRoot: bad }).command).toBe('C:\\Windows\\System32\\taskkill.exe');
    expect(windowsTreeKill(1, { SystemRoot: 'D:\\Win\\' }).command).toBe('D:\\Win\\System32\\taskkill.exe');
  });

  it('on this platform (real processes), cleanup of a server running in a hostile worktree executes nothing from it', async () => {
    if (process.platform === 'win32') return; // covered by the injected-spawn test above
    const wt = mkdtempSync(join(tmpdir(), 'forge-hostile-real-'));
    const marker = join(wt, 'SENTINEL_RAN');
    for (const name of ['taskkill.exe', 'taskkill', 'kill', 'pkill']) {
      writeFileSync(join(wt, name), `#!/bin/sh\necho ran > "${marker}"\n`);
      chmodSync(join(wt, name), 0o755);
    }
    const server = spawnCodexServer(process.execPath, ['-e', 'setInterval(()=>{},1000)'], wt, { ...process.env, PATH: `.:${wt}:${process.env.PATH}` }, 1000);
    server.kill();
    const { code } = await server.exited;
    expect(code).toBeNull(); // killed by signal
    await new Promise((r) => setTimeout(r, 50));
    expect(existsSync(marker)).toBe(false);
  });
});
