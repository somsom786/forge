// Codex adapter: drives `codex app-server` (JSON-RPC over stdio) as one FORGE agent session.
//
// Safety model (see docs/SECURITY.md, "Codex"):
//   * Every command and file change Codex wants to make arrives as an approval request (approvalPolicy "untrusted") and is
//     decided by the FORGE core (policy first, then a human) — Codex's own answer is never "accept" on its own.
//   * Codex's sandbox (workspace-write) stays on as defence in depth.
//   * Repository- and user-defined MCP servers, plugins, apps and hooks are switched off for the session. A hostile repo's
//     `.codex/config.toml` MCP server RUNS even though Codex says project config is untrusted (verified), and an empty
//     `mcp_servers` table does not remove it — only a per-server `enabled:false` does. So the effective server list is read
//     with `config/read` and each one is disabled by name, and any server that still reports starting aborts the session.
//   * The child gets no API-key environment, so a mission can never silently bill an API.

import { existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import type { CapabilityMatrix, UsageReport } from '../protocol.js';
import { MAX_FIELD_BYTES } from '../protocol.js';
import { errMessage, truncate, truncateDeep } from '../util.js';
import type { TurnResult } from './claudeNormalize.js';
import { type CodexRpc, type CodexServer, RpcError, spawnCodexServer } from './codexRpc.js';
import type { ProviderAdapter, RunContext } from './types.js';

export const CODEX_CAPABILITIES: CapabilityMatrix = {
  sessionStart: { supported: true },
  resume: { supported: true, note: 'Resumes the Codex thread transcript, not process state.' },
  streaming: { supported: true },
  interrupt: { supported: true },
  toolEvents: { supported: true },
  fileChanges: { supported: true, note: 'Derived from Codex file-change items; the git diff is authoritative.' },
  approvals: { supported: true, note: 'Every command and file change is decided by FORGE; Codex runs with its own sandbox as well.' },
  usageReporting: { supported: true, note: 'Token counts only; no cost is reported for a ChatGPT login.' },
  modelSelection: { supported: true },
  worktreeIntegration: { supported: true },
};

/** Features that load code or tools from plugins, apps, hooks and other agents. Disabled for every mission session. */
export const CODEX_DISABLED_FEATURES = ['hooks', 'plugins', 'apps', 'computer_use', 'browser_use', 'browser_use_external', 'multi_agent', 'remote_plugin'];

export function codexServerArgs(): string[] {
  return ['app-server', ...CODEX_DISABLED_FEATURES.flatMap((f) => ['--disable', f])];
}

const isJson = (v: unknown): v is Record<string, any> => !!v && typeof v === 'object'; // eslint-disable-line @typescript-eslint/no-explicit-any

/** `"C:\...\powershell.exe" -Command "git status"` → `git status` (display only; policy sees the full command). */
export function innerCommand(command: string): string {
  const m = /^"?[^"\s]*(?:powershell|pwsh)(?:\.exe)?"?\s+(?:-\w+\s+)*-Command\s+(.+)$/is.exec(command.trim());
  if (m) return m[1]!.trim().replace(/^"(.*)"$/s, '$1');
  const b = /^"?[^"\s]*bash(?:\.exe)?"?\s+-l?c\s+(.+)$/is.exec(command.trim());
  if (b) return b[1]!.trim().replace(/^['"](.*)['"]$/s, '$1');
  return command;
}

/** Project-level Codex files that can start programs (MCP servers run even though Codex calls project config "untrusted"). */
const PROJECT_CODEX_FILES = ['.codex/config.toml', '.codex/hooks.json', '.codex/hooks.toml', '.codex/requirements.toml'];

/** Project Codex config in `cwd` or any parent up to and including the repository root. Empty when the folder is clean. */
export function projectCodexConfig(cwd: string): string[] {
  const found: string[] = [];
  let dir = cwd;
  for (let i = 0; i < 12; i++) {
    for (const f of PROJECT_CODEX_FILES) if (existsSync(join(dir, f))) found.push(join(dir, f));
    if (existsSync(join(dir, '.git'))) break; // the worktree/repository root
    const up = dirname(dir);
    if (up === dir) break;
    dir = up;
  }
  return found;
}

export interface CodexDeps {
  /** Tests inject a scripted peer; production spawns the real `codex app-server`. */
  spawnServer?: (ctx: RunContext) => CodexServer;
}

export function createCodexAdapter(deps: CodexDeps = {}): ProviderAdapter {
  return {
    id: 'codex',
    capabilities: () => CODEX_CAPABILITIES,
    async run(ctx) {
      // Fail closed before anything is started. `config/read` cannot be trusted to list what Codex will load from the project
      // (inside a git worktree it omits servers that `thread/start` then still starts), so the only safe rule is: no project
      // Codex config at all. Review or remove it, then run the mission again.
      const hostile = projectCodexConfig(ctx.start.cwd);
      if (hostile.length > 0) {
        const message = `This repository contains Codex project configuration (${hostile.map((h) => h.replace(ctx.start.cwd, '.')).join(', ')}). Codex starts MCP servers from it even when it calls the project untrusted, so FORGE will not run a Codex mission here. Review or remove the file, or use Claude for this repository.`;
        ctx.emit('status', { message, provider: 'codex', refused: true });
        ctx.turnComplete({ status: 'error', message, usage: { inputTokens: 0, outputTokens: 0, cacheReadTokens: 0, cacheCreationTokens: 0 }, permissionDenials: 0 });
        return;
      }
      const server = (deps.spawnServer ?? defaultSpawn)(ctx);
      try {
        await runSession(ctx, server);
      } finally {
        server.kill();
      }
    },
  };
}

function defaultSpawn(ctx: RunContext): CodexServer {
  const exe = ctx.start.codexPath;
  if (!exe) throw new Error('the Codex executable was not found; install Codex and sign in with `codex login`');
  return spawnCodexServer(exe, codexServerArgs(), ctx.start.cwd, process.env);
}

async function runSession(ctx: RunContext, server: CodexServer): Promise<void> {
  const rpc: CodexRpc = server.rpc;
  const { start } = ctx;
  const items = new Map<string, Record<string, any>>(); // eslint-disable-line @typescript-eslint/no-explicit-any
  let threadId = '';
  let turnId = '';
  let denials = 0;
  let usage: UsageReport = { inputTokens: 0, outputTokens: 0, cacheReadTokens: 0, cacheCreationTokens: 0 };
  let failure: string | undefined;
  let tripwire: string | undefined;
  let resolveTurn!: (r: { status: string; error?: string }) => void;
  const turnDone = new Promise<{ status: string; error?: string }>((r) => (resolveTurn = r));
  let turnFinished = false;

  const finishTurn = (r: { status: string; error?: string }) => {
    if (turnFinished) return;
    turnFinished = true;
    resolveTurn(r);
  };

  // If the server dies mid-turn, say so with its last words instead of hanging.
  void server.exited.then(({ code, stderrTail }) => {
    finishTurn({ status: 'failed', error: `Codex exited unexpectedly (code ${code})${stderrTail.trim() ? `: ${truncate(stderrTail.trim(), 800).text}` : ''}` });
  });

  const interrupt = () => {
    if (threadId && turnId) void rpc.request('turn/interrupt', { threadId, turnId }).catch(() => {});
  };
  ctx.signal.addEventListener('abort', interrupt, { once: true });

  // ---- notifications --------------------------------------------------------------------------------------------------
  rpc.onNotification((method, p) => {
    switch (method) {
      case 'item/agentMessage/delta':
        if (typeof p.delta === 'string') ctx.delta(typeof p.itemId === 'string' ? p.itemId : null, p.delta);
        break;
      case 'item/started':
        if (isJson(p.item)) onItemStarted(p.item);
        break;
      case 'item/completed':
        if (isJson(p.item)) onItemCompleted(p.item);
        break;
      case 'thread/tokenUsage/updated': {
        const t = p.tokenUsage?.total;
        if (isJson(t)) {
          usage = {
            inputTokens: Number(t.inputTokens ?? 0),
            outputTokens: Number(t.outputTokens ?? 0),
            cacheReadTokens: Number(t.cachedInputTokens ?? 0),
            cacheCreationTokens: Number(t.cacheWriteInputTokens ?? 0),
          };
        }
        break;
      }
      case 'error':
        if (p.willRetry) ctx.emit('provider.retry', { message: p.error?.message });
        else failure = String(p.error?.message ?? 'Codex reported an error');
        break;
      case 'warning':
      case 'configWarning':
        ctx.emit('status', { message: truncate(String(p.message ?? p.summary ?? 'warning'), MAX_FIELD_BYTES).text, provider: 'codex', warning: true });
        break;
      case 'mcpServer/startupStatus/updated':
        // Everything was disabled by name. Anything that still starts is a hole in that: stop before it can be used.
        if (p.status === 'starting' || p.status === 'ready') {
          tripwire = `the MCP server "${String(p.name)}" started although all MCP servers were disabled for this mission`;
          interrupt();
          finishTurn({ status: 'failed', error: tripwire });
        }
        break;
      case 'hook/started':
        tripwire = 'a Codex hook started although hooks were disabled for this mission';
        interrupt();
        finishTurn({ status: 'failed', error: tripwire });
        break;
      case 'turn/completed': {
        const t = p.turn ?? {};
        finishTurn({ status: String(t.status ?? 'completed'), error: t.error?.message ? String(t.error.message) : failure });
        break;
      }
    }
  });

  const toolName = (item: Record<string, any>): string => { // eslint-disable-line @typescript-eslint/no-explicit-any
    switch (item.type) {
      case 'commandExecution':
        return process.platform === 'win32' ? 'PowerShell' : 'Bash';
      case 'fileChange':
        return Array.isArray(item.changes) && item.changes.every((c: any) => c?.kind?.type === 'add') ? 'Write' : 'Edit'; // eslint-disable-line @typescript-eslint/no-explicit-any
      case 'mcpToolCall':
        return `mcp__${String(item.server)}__${String(item.tool)}`;
      case 'webSearch':
        return 'WebSearch';
      default:
        return String(item.tool ?? item.type);
    }
  };

  const onItemStarted = (item: Record<string, any>) => { // eslint-disable-line @typescript-eslint/no-explicit-any
    if (typeof item.id !== 'string') return;
    items.set(item.id, item);
    if (item.type === 'commandExecution') {
      const command = String(item.command ?? '');
      ctx.emit('tool.call', { toolUseId: item.id, name: toolName(item), summary: truncate(innerCommand(command), 300).text, input: truncateDeep({ command, cwd: item.cwd }, MAX_FIELD_BYTES) });
    } else if (item.type === 'fileChange') {
      const changes = Array.isArray(item.changes) ? item.changes : [];
      ctx.emit('tool.call', {
        toolUseId: item.id,
        name: toolName(item),
        summary: changes.map((c: any) => String(c?.path ?? '')).join(', '), // eslint-disable-line @typescript-eslint/no-explicit-any
        input: truncateDeep({ changes: changes.map((c: any) => ({ path: c?.path, kind: c?.kind?.type, diff: c?.diff })) }, MAX_FIELD_BYTES), // eslint-disable-line @typescript-eslint/no-explicit-any
      });
    } else if (item.type === 'mcpToolCall' || item.type === 'dynamicToolCall' || item.type === 'webSearch') {
      ctx.emit('tool.call', { toolUseId: item.id, name: toolName(item), summary: truncate(JSON.stringify(item.arguments ?? item.query ?? {}), 160).text, input: truncateDeep(item.arguments ?? {}, MAX_FIELD_BYTES) });
    }
  };

  const onItemCompleted = (item: Record<string, any>) => { // eslint-disable-line @typescript-eslint/no-explicit-any
    const id = String(item.id);
    switch (item.type) {
      case 'agentMessage':
        if (item.text) ctx.emit('agent.text', { messageId: id, text: truncate(String(item.text), 64 * 1024).text, phase: item.phase ?? undefined });
        break;
      case 'reasoning': {
        const text = (Array.isArray(item.summary) ? item.summary : []).join('\n').trim();
        if (text) ctx.emit('agent.thinking', { messageId: id, text: truncate(text, MAX_FIELD_BYTES).text });
        break;
      }
      case 'commandExecution': {
        const ok = item.status === 'completed' && (item.exitCode === 0 || item.exitCode == null);
        const out = truncate(String(item.aggregatedOutput ?? (item.status === 'declined' ? 'declined' : '')), MAX_FIELD_BYTES);
        ctx.emit('tool.result', { toolUseId: id, name: toolName(item), ok, output: out.text, truncated: out.truncated, exitCode: item.exitCode ?? undefined, status: item.status });
        break;
      }
      case 'fileChange': {
        const ok = item.status === 'completed';
        ctx.emit('tool.result', { toolUseId: id, name: toolName(item), ok, output: ok ? 'applied' : String(item.status ?? 'failed'), status: item.status });
        if (ok) {
          for (const c of Array.isArray(item.changes) ? item.changes : []) {
            if (typeof c?.path === 'string') ctx.emit('file.changed', { path: c.path, tool: toolName(item), toolUseId: id });
          }
        }
        break;
      }
      case 'mcpToolCall':
      case 'dynamicToolCall':
      case 'webSearch':
        ctx.emit('tool.result', { toolUseId: id, name: toolName(item), ok: item.status !== 'failed', output: truncate(JSON.stringify(item.result ?? item.error ?? ''), MAX_FIELD_BYTES).text });
        break;
    }
  };

  // ---- approvals: FORGE decides ---------------------------------------------------------------------------------------
  rpc.onRequest(async (method, p) => {
    if (method === 'item/commandExecution/requestApproval') {
      const item = items.get(String(p.itemId)) ?? {};
      const command = String(p.command ?? item.command ?? '');
      const net = isJson(p.networkApprovalContext) ? p.networkApprovalContext : null;
      const tool = net ? 'WebFetch' : toolName({ type: 'commandExecution' });
      const input = net ? { url: String(net.host ?? net.url ?? command) } : { command, cwd: p.cwd ?? item.cwd };
      const d = await ctx.requestPermission({ tool, input, toolUseId: String(p.itemId), source: 'can_use_tool', reason: typeof p.reason === 'string' ? p.reason : undefined, signal: ctx.signal });
      if (d.behavior === 'allow') return { decision: 'accept' };
      denials++;
      return { decision: 'decline' };
    }
    if (method === 'item/fileChange/requestApproval') {
      const item = items.get(String(p.itemId));
      const changes: any[] = Array.isArray(item?.changes) ? item!.changes : []; // eslint-disable-line @typescript-eslint/no-explicit-any
      // Without the file list the paths cannot be checked against policy, and a requested grant root widens writes beyond
      // what the user reviewed: both are refused.
      if (changes.length === 0 || p.grantRoot) {
        denials++;
        ctx.emit('status', { message: p.grantRoot ? 'Codex asked for write access beyond the worktree; declined' : 'Codex asked to change files FORGE could not identify; declined', provider: 'codex' });
        return { decision: 'decline' };
      }
      let n = 0;
      for (const c of changes) {
        const paths = [c?.path, c?.kind?.move_path].filter((x): x is string => typeof x === 'string');
        if (paths.length === 0) {
          denials++;
          return { decision: 'decline' };
        }
        for (const path of paths) {
          const tool = c?.kind?.type === 'add' ? 'Write' : 'Edit';
          const d = await ctx.requestPermission({ tool, input: { file_path: path, content: String(c?.diff ?? '') }, toolUseId: `${String(p.itemId)}:${n++}`, source: 'can_use_tool', reason: typeof p.reason === 'string' ? p.reason : undefined, signal: ctx.signal });
          if (d.behavior !== 'allow') {
            denials++;
            return { decision: 'decline' };
          }
        }
      }
      return { decision: 'accept' };
    }
    // Permission grants, user-input prompts, MCP elicitations, dynamic tools, token refresh, attestation, legacy approvals:
    // FORGE supports none of them in a mission. An error answer makes Codex treat the call as failed instead of waiting.
    ctx.emit('status', { message: `Codex sent a request FORGE does not support (${method}); refused`, provider: 'codex' });
    throw new RpcError(-32601, `FORGE does not support ${method} in a mission`);
  });

  // ---- session ----------------------------------------------------------------------------------------------------------
  const finish = (status: 'success' | 'error' | 'interrupted', message?: string): void => {
    ctx.turnComplete({ status, message, usage, permissionDenials: denials, providerSessionId: threadId || undefined } satisfies TurnResult);
  };

  try {
    await rpc.request('initialize', { clientInfo: { name: 'forge', title: 'FORGE', version: '0.1.0' }, capabilities: null });
    rpc.notify('initialized', {});

    // Effective MCP servers as Codex will see them from the worktree — including any the repository defines.
    const cfg = await rpc.request('config/read', { cwd: start.cwd, includeLayers: false });
    const mcpNames = Object.keys(isJson(cfg?.config?.mcp_servers) ? cfg.config.mcp_servers : {});
    const threadParams: Record<string, unknown> = {
      cwd: start.cwd,
      approvalPolicy: 'untrusted',
      sandbox: 'workspace-write',
      approvalsReviewer: 'user',
      ...(start.model ? { model: start.model } : {}),
      ...(start.appendSystemPrompt ? { developerInstructions: start.appendSystemPrompt } : {}),
      config: { mcp_servers: Object.fromEntries(mcpNames.map((n) => [n, { enabled: false }])) },
    };
    const th = start.resume
      ? await rpc.request('thread/resume', { threadId: start.resume, ...threadParams })
      : await rpc.request('thread/start', { ...threadParams, ephemeral: false });
    threadId = String(th?.thread?.id ?? th?.threadId ?? start.resume ?? '');
    if (!threadId) throw new Error('Codex did not return a thread id');
    ctx.providerSession(threadId);
    ctx.emit('session.started', {
      providerSessionId: threadId,
      model: th?.model ?? start.model,
      cwd: start.cwd,
      credentialSource: 'chatgpt-login',
      sandbox: 'workspace-write',
      approvalPolicy: 'untrusted',
      disabledMcpServers: mcpNames,
      disabledFeatures: CODEX_DISABLED_FEATURES,
    });
    if (tripwire) throw new Error(tripwire);

    if (ctx.signal.aborted) {
      finish('interrupted', 'Cancelled');
      return;
    }
    const turn = await rpc.request('turn/start', { threadId, input: [{ type: 'text', text: start.prompt, text_elements: [] }] });
    turnId = String(turn?.turn?.id ?? '');
    if (ctx.signal.aborted) interrupt();

    // A cancelled turn still has to report; give Codex a moment to wind down, then stop waiting.
    const result = await Promise.race([
      turnDone,
      new Promise<{ status: string; error?: string }>((resolve) => {
        ctx.signal.addEventListener('abort', () => setTimeout(() => resolve({ status: 'interrupted' }), 10_000).unref(), { once: true });
      }),
    ]);
    if (tripwire) return finish('error', tripwire);
    if (ctx.signal.aborted || result.status === 'interrupted') return finish('interrupted', 'Cancelled');
    if (result.status === 'completed') return finish('success');
    finish('error', result.error ?? failure ?? `Codex turn ${result.status}`);
  } catch (e) {
    if (ctx.signal.aborted) return finish('interrupted', 'Cancelled');
    finish('error', tripwire ?? errMessage(e));
  }
}

export const codexAdapter = createCodexAdapter();
