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

import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { dirname, join, relative, isAbsolute, resolve as resolvePath } from 'node:path';
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

/** Config overrides that close command-running and network surfaces Codex has outside the approval flow. */
export const CODEX_CONFIG_OVERRIDES = ['notify=[]', 'web_search="disabled"'];

export function codexServerArgs(): string[] {
  return ['app-server', ...CODEX_DISABLED_FEATURES.flatMap((f) => ['--disable', f]), ...CODEX_CONFIG_OVERRIDES.flatMap((c) => ['-c', c])];
}

/** The Codex home Codex will use (it holds the user's login and exec rules). */
export function effectiveCodexHome(env: NodeJS.ProcessEnv = process.env): string {
  return env.CODEX_HOME && env.CODEX_HOME.trim() ? env.CODEX_HOME : join(homedir(), '.codex');
}

/** Exec-policy files in the Codex home that ALLOW commands without a prompt. They run commands outside FORGE's approval flow
 *  (and outside Codex's sandbox), so a mission refuses to start while any exist, unless the user opts in explicitly. */
export function userAllowRules(home: string): string[] {
  const dir = join(home, 'rules');
  const found: string[] = [];
  try {
    for (const f of readdirSync(dir)) {
      if (!f.endsWith('.rules')) continue;
      const text = readFileSync(join(dir, f), 'utf8');
      const n = (text.match(/decision\s*=\s*["']allow["']/g) ?? []).length;
      if (n > 0) found.push(`${f} (${n} allow rule${n === 1 ? '' : 's'})`);
    }
  } catch {
    /* no rules directory */
  }
  return found;
}

/** A Codex home inside the repository would let the repository supply the login, config and rules. */
export function homeInside(cwd: string, home: string): boolean {
  const rel = relative(resolvePath(cwd), resolvePath(home));
  return rel === '' || (!rel.startsWith('..') && !isAbsolute(rel));
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

const samePath = (a: string, b: string): boolean => {
  const norm = (p: string) => resolvePath(p).replace(/[\\/]+$/, '');
  return process.platform === 'win32' ? norm(a).toLowerCase() === norm(b).toLowerCase() : norm(a) === norm(b);
};

const insideOrSame = (root: string, p: string): boolean => {
  if (samePath(root, p)) return true;
  const rel = relative(resolvePath(root), resolvePath(p));
  return rel !== '' && !rel.startsWith('..') && !isAbsolute(rel);
};

/** What `thread/start` / `thread/resume` reported as actually in effect for the thread. */
export interface EffectiveThreadSettings {
  sandbox: unknown;
  approvalPolicy: unknown;
  approvalsReviewer: unknown;
  cwd: unknown;
  modelProvider: unknown;
}

/** Codex answers `thread/start` with the settings it actually applied, after merging the user's config, profiles and
 *  requirements. The request asked for workspace-write / untrusted / user; anything broader (full access, an external
 *  sandbox, network, writable roots outside the worktree, a weaker approval policy, an automatic reviewer, another cwd or
 *  provider) is refused before a turn starts. Missing fields fail closed. Returns the reasons; empty means acceptable. */
export function effectiveSettingsProblems(eff: EffectiveThreadSettings, expected: { cwd: string; modelProvider: string }): string[] {
  const problems: string[] = [];
  const sb = isJson(eff.sandbox) ? eff.sandbox : null;
  if (!sb) problems.push('Codex did not report the sandbox it applied');
  else if (sb.type === 'workspaceWrite') {
    if (sb.networkAccess !== false && sb.networkAccess !== undefined) problems.push(`the sandbox allows network access (${JSON.stringify(sb.networkAccess)})`);
    const roots = Array.isArray(sb.writableRoots) ? sb.writableRoots : sb.writableRoots === undefined ? [] : null;
    if (roots === null) problems.push('the sandbox reported unreadable writable roots');
    else {
      const outside = roots.filter((r: unknown) => typeof r !== 'string' || !insideOrSame(expected.cwd, r));
      if (outside.length > 0) problems.push(`the sandbox can write outside the worktree (${outside.map((r: unknown) => String(r)).join(', ')})`);
    }
  } else if (sb.type === 'readOnly') {
    if (sb.networkAccess !== false && sb.networkAccess !== undefined) problems.push(`the read-only sandbox allows network access (${JSON.stringify(sb.networkAccess)})`);
  } else problems.push(`the sandbox is "${String(sb.type)}", broader than workspace-write`);
  if (eff.approvalPolicy !== 'untrusted') problems.push(`the approval policy is ${JSON.stringify(eff.approvalPolicy)}, not "untrusted"`);
  if (eff.approvalsReviewer !== 'user') problems.push(`approvals are reviewed by ${JSON.stringify(eff.approvalsReviewer)}, not routed to FORGE ("user")`);
  if (typeof eff.cwd !== 'string' || !samePath(eff.cwd, expected.cwd)) problems.push(`the thread runs in ${JSON.stringify(eff.cwd)}, not the mission worktree`);
  if (eff.modelProvider !== expected.modelProvider) problems.push(`the model provider is ${JSON.stringify(eff.modelProvider)}, not "${expected.modelProvider}"`);
  return problems;
}

/** Item types that do something without passing through an approval request FORGE answers. Their prerequisites (MCP,
 *  plugins, apps, web search, multi-agent, hooks) are disabled for the session; if one appears anyway it is shown to the
 *  user and the session is aborted. */
export const UNGATED_ITEM_TYPES = ['mcpToolCall', 'dynamicToolCall', 'webSearch', 'imageGeneration', 'collabAgentToolCall', 'subAgentActivity', 'hookPrompt'];

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
      if (!deps.spawnServer) {
        const home = effectiveCodexHome();
        const rules = userAllowRules(home);
        const refuse = (message: string) => {
          ctx.emit('status', { message, provider: 'codex', refused: true });
          ctx.turnComplete({ status: 'error', message, usage: { inputTokens: 0, outputTokens: 0, cacheReadTokens: 0, cacheCreationTokens: 0 }, permissionDenials: 0 });
        };
        if (homeInside(ctx.start.cwd, home)) return refuse(`The Codex home (${home}) is inside the repository, which would let the repository supply Codex's login, configuration and rules. FORGE will not run a Codex mission.`);
        if (rules.length > 0 && process.env.FORGE_CODEX_ALLOW_USER_RULES !== '1') {
          return refuse(
            `Your Codex exec rules (${join(home, 'rules')}: ${rules.join(', ')}) allow commands to run without asking, which would bypass FORGE's approvals. FORGE will not run a Codex mission until those allow rules are removed or moved (set FORGE_CODEX_ALLOW_USER_RULES=1 to accept the bypass knowingly).`,
          );
        }
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
  // Items whose content changed after they were first announced: approvals for them are declined.
  const conflicted = new Set<string>();
  // File-change items FORGE approved, with the exact paths that were approved.
  const approvedPaths = new Map<string, Set<string>>();
  let threadId = '';
  let turnId = '';
  let resolveTurnKnown!: () => void;
  const turnKnown = new Promise<void>((r) => (resolveTurnKnown = r));
  const setTurnId = (id: string) => {
    if (!id || turnId) return;
    turnId = id;
    resolveTurnKnown();
  };
  let denials = 0;
  let callbackSeq = 0;
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

  const trip = (why: string) => {
    if (tripwire) return;
    tripwire = why;
    ctx.emit('status', { message: `Codex session aborted: ${why}`, provider: 'codex', tripwire: true });
    interrupt();
    server.kill();
    finishTurn({ status: 'failed', error: why });
  };

  /** Notifications and approvals must belong to this session's thread and current turn. */
  const outOfScope = (p: Record<string, any>): string | null => { // eslint-disable-line @typescript-eslint/no-explicit-any
    if (p.threadId !== threadId) return `thread ${JSON.stringify(p.threadId)} is not this session's thread`;
    if (p.turnId !== turnId) return `turn ${JSON.stringify(p.turnId)} is not the current turn`;
    return null;
  };

  // ---- notifications --------------------------------------------------------------------------------------------------
  rpc.onNotification((method, p) => {
    switch (method) {
      case 'item/agentMessage/delta':
        if (typeof p.delta === 'string') ctx.delta(typeof p.itemId === 'string' ? p.itemId : null, p.delta);
        break;
      case 'turn/started':
        if (p.threadId === threadId && isJson(p.turn) && typeof p.turn.id === 'string') setTurnId(p.turn.id);
        break;
      case 'item/started':
        if (isJson(p.item)) onItemStarted(p.item, p);
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
        if (p.status === 'starting' || p.status === 'ready') trip(`the MCP server "${String(p.name)}" started although all MCP servers were disabled for this mission`);
        break;
      case 'hook/started':
        trip('a Codex hook started although hooks were disabled for this mission');
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

  /** The parts of an item an approval is about. A second announcement of the same id with different content is a conflict. */
  const itemIdentity = (item: Record<string, any>) => // eslint-disable-line @typescript-eslint/no-explicit-any
    JSON.stringify([item.type, item.command ?? null, item.cwd ?? null, Array.isArray(item.changes) ? item.changes.map((c: any) => [c?.path, c?.kind?.type, c?.kind?.move_path ?? null, c?.diff ?? null]) : null]); // eslint-disable-line @typescript-eslint/no-explicit-any
  const itemTurn = new Map<string, string>();

  const onItemStarted = (item: Record<string, any>, scope: Record<string, any>) => { // eslint-disable-line @typescript-eslint/no-explicit-any
    if (typeof item.id !== 'string') return;
    if (!turnId && scope.threadId === threadId && typeof scope.turnId === 'string') setTurnId(scope.turnId);
    const bad = outOfScope(scope);
    if (bad) {
      // Not recorded, so no approval can refer to it; an ungated action is still a reason to stop.
      ctx.emit('status', { message: `Codex announced item ${item.id} outside this session (${bad}); ignored`, provider: 'codex', warning: true });
      if (UNGATED_ITEM_TYPES.includes(String(item.type))) trip(`Codex started a ${String(item.type)} item, which FORGE cannot gate, outside this session's turn`);
      return;
    }
    const prev = items.get(item.id);
    if (prev) {
      if (itemIdentity(prev) !== itemIdentity(item)) {
        conflicted.add(item.id);
        if (approvedPaths.has(item.id)) trip(`Codex replaced the content of item ${item.id} after FORGE approved it`);
        else ctx.emit('status', { message: `Codex re-announced item ${item.id} with different content; any approval for it will be declined`, provider: 'codex', warning: true });
      }
      return; // the first announcement stays authoritative
    }
    items.set(item.id, item);
    itemTurn.set(item.id, String(scope.turnId));
    if (UNGATED_ITEM_TYPES.includes(String(item.type))) {
      ctx.emit('tool.call', { toolUseId: item.id, name: toolName(item), summary: truncate(JSON.stringify(item.arguments ?? item.query ?? item.tool ?? item.type), 160).text, input: truncateDeep(item.arguments ?? {}, MAX_FIELD_BYTES), ungated: true });
      trip(`Codex started a ${String(item.type)} item, which does not pass through FORGE approval (its prerequisites were disabled for this mission)`);
      return;
    }
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
          // An applied file change must be one FORGE approved, touching only the approved paths.
          const approved = approvedPaths.get(id);
          const touched = (Array.isArray(item.changes) ? item.changes : []).flatMap((c: any) => [c?.path, c?.kind?.move_path]).filter((x: unknown): x is string => typeof x === 'string'); // eslint-disable-line @typescript-eslint/no-explicit-any
          const unapproved = approved ? touched.filter((t: string) => !approved.has(t)) : touched;
          if (!approved || unapproved.length > 0) trip(`Codex applied a file change FORGE did not approve (${unapproved.join(', ') || id})`);
          for (const c of Array.isArray(item.changes) ? item.changes : []) {
            if (typeof c?.path === 'string') ctx.emit('file.changed', { path: c.path, tool: toolName(item), toolUseId: id });
          }
        }
        break;
      }
      default:
        if (UNGATED_ITEM_TYPES.includes(String(item.type))) {
          ctx.emit('tool.result', { toolUseId: id, name: toolName(item), ok: item.status !== 'failed', output: truncate(JSON.stringify(item.result ?? item.error ?? ''), MAX_FIELD_BYTES).text, ungated: true });
          trip(`Codex ran a ${String(item.type)} item, which does not pass through FORGE approval`);
        }
        break;
    }
  };

  // ---- approvals: FORGE decides ---------------------------------------------------------------------------------------
  const declineOutOfScope = async (p: Record<string, any>): Promise<string | null> => { // eslint-disable-line @typescript-eslint/no-explicit-any
    // Approvals only arrive during a turn; wait until its id is known (or the turn is over) before comparing.
    if (!turnId) await Promise.race([turnKnown, turnDone]);
    const bad = outOfScope(p);
    const itemId = String(p.itemId);
    const why = bad ?? (conflicted.has(itemId) ? `item ${itemId} was re-announced with different content` : itemTurn.has(itemId) && itemTurn.get(itemId) !== p.turnId ? `item ${itemId} belongs to another turn` : null);
    if (!why) return null;
    denials++;
    ctx.emit('status', { message: `Codex asked for an approval FORGE cannot attribute (${why}); declined`, provider: 'codex', warning: true });
    return why;
  };

  rpc.onRequest(async (method, p) => {
    if (method === 'item/commandExecution/requestApproval' || method === 'item/fileChange/requestApproval') {
      if (await declineOutOfScope(p)) return { decision: 'decline' };
      const known = items.get(String(p.itemId));
      const expectType = method === 'item/fileChange/requestApproval' ? 'fileChange' : 'commandExecution';
      if (known && known.type !== expectType) {
        denials++;
        ctx.emit('status', { message: `Codex asked to approve item ${String(p.itemId)} as ${expectType}, but it was announced as ${String(known.type)}; declined`, provider: 'codex', warning: true });
        return { decision: 'decline' };
      }
    }
    if (method === 'item/commandExecution/requestApproval') {
      const item = items.get(String(p.itemId)) ?? {};
      const command = String(p.command ?? item.command ?? '');
      const net = isJson(p.networkApprovalContext) ? p.networkApprovalContext : null;
      const tool = net ? 'WebFetch' : toolName({ type: 'commandExecution' });
      const input = net ? { url: String(net.host ?? net.url ?? command) } : { command, cwd: p.cwd ?? item.cwd };
      const cb = String(p.approvalId ?? `${p.itemId}:${++callbackSeq}`);
      const d = await ctx.requestPermission({ tool, input, toolUseId: `${String(p.itemId)}:${cb}`, source: 'can_use_tool', reason: typeof p.reason === 'string' ? p.reason : undefined, signal: ctx.signal });
      if (d.behavior === 'allow' && !conflicted.has(String(p.itemId))) return { decision: 'accept' };
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
      const approved = new Set<string>();
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
          approved.add(path);
        }
      }
      // The item may have been replaced while a human was deciding: the approval covers only what they saw.
      if (conflicted.has(String(p.itemId))) {
        denials++;
        return { decision: 'decline' };
      }
      approvedPaths.set(String(p.itemId), approved);
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

    // Never bill an API key or a custom provider: only the user's ChatGPT login is acceptable (token-free metadata only).
    const acct = await rpc.request('account/read', { refreshToken: false });
    const acctType = String(acct?.account?.type ?? 'none');
    if (acctType !== 'chatgpt') {
      throw new Error(`Codex is signed in as "${acctType}", not with a ChatGPT login; FORGE only runs Codex missions on the ChatGPT subscription so nothing is billed to an API key. Run \`codex login\`.`);
    }

    // Effective MCP servers as Codex will see them from the worktree — including any the repository defines.
    const cfg = await rpc.request('config/read', { cwd: start.cwd, includeLayers: false });
    const mcpNames = Object.keys(isJson(cfg?.config?.mcp_servers) ? cfg.config.mcp_servers : {});
    const threadParams: Record<string, unknown> = {
      cwd: start.cwd,
      approvalPolicy: 'untrusted',
      sandbox: 'workspace-write',
      approvalsReviewer: 'user',
      modelProvider: start.localEndpoint ? 'ollama' : 'openai',
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
    // Record what Codex says it applied, not what was asked for, and refuse anything broader before a turn can start.
    const effective: EffectiveThreadSettings = { sandbox: th?.sandbox, approvalPolicy: th?.approvalPolicy, approvalsReviewer: th?.approvalsReviewer, cwd: th?.cwd, modelProvider: th?.modelProvider };
    const problems = effectiveSettingsProblems(effective, { cwd: start.cwd, modelProvider: threadParams.modelProvider as string });
    ctx.emit('session.started', {
      providerSessionId: threadId,
      model: th?.model ?? start.model,
      cwd: start.cwd,
      credentialSource: 'chatgpt-login',
      requested: { sandbox: 'workspace-write', approvalPolicy: 'untrusted', approvalsReviewer: 'user' },
      effective: truncateDeep(effective, MAX_FIELD_BYTES),
      effectiveAccepted: problems.length === 0,
      disabledMcpServers: mcpNames,
      disabledFeatures: CODEX_DISABLED_FEATURES,
    });
    if (tripwire) throw new Error(tripwire);
    if (problems.length > 0) {
      const message = `Codex applied settings broader than FORGE allows for a mission: ${problems.join('; ')}. Check your Codex config (sandbox_mode, sandbox_workspace_write, approval_policy, profiles, requirements) and run the mission again.`;
      ctx.emit('status', { message, provider: 'codex', refused: true });
      server.kill();
      return finish('error', message);
    }

    if (ctx.signal.aborted) {
      finish('interrupted', 'Cancelled');
      return;
    }
    const turn = await rpc.request('turn/start', { threadId, input: [{ type: 'text', text: start.prompt, text_elements: [] }] });
    setTurnId(String(turn?.turn?.id ?? ''));
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
