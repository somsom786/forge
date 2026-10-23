// Agent Client Protocol (ACP) adapter. FORGE is the ACP *client*; the agent is a program the user registered (for example an
// ACP bridge for a coding agent), spoken to over stdio with the official SDK (@agentclientprotocol/sdk).
//
// Control points FORGE owns here:
//   * every `session/request_permission` goes through the FORGE core (policy, then a human where required); FORGE only ever
//     answers with a *once* option, never "always", so the agent cannot be granted standing permission behind FORGE's back;
//   * file reads/writes the agent asks the client for (`fs/*`) are brokered: policy decides, and FORGE performs the write
//     itself after re-checking the real path is inside the worktree (links refused);
//   * no terminal capability is offered; MCP servers are only the ones the user authorized for the project.
// What FORGE cannot control: an agent that runs a tool *without* asking. ACP permission requests are agent-initiated; the
// worktree diff stays the authoritative record and is reviewed before anything is merged.

import { spawn, type ChildProcess } from 'node:child_process';
import { constants as fsConstants } from 'node:fs';
import { lstat, mkdir, open, readFile, realpath } from 'node:fs/promises';
import { dirname, isAbsolute, join, relative, resolve, sep } from 'node:path';
import { Readable, Writable } from 'node:stream';
import * as acp from '@agentclientprotocol/sdk';
import { MAX_FIELD_BYTES, type CapabilityMatrix, type McpServerConfig, type StartCommand } from '../protocol.js';
import { errMessage, truncateDeep } from '../util.js';
import type { TurnResult } from './claudeNormalize.js';
import type { PermissionDecision, ProviderAdapter, RunContext } from './types.js';

export const ACP_CAPABILITIES: CapabilityMatrix = {
  sessionStart: { supported: true },
  resume: { supported: false, note: 'Only if the agent advertises loadSession; negotiated per session.' },
  streaming: { supported: true },
  interrupt: { supported: true, note: 'session/cancel, then the agent process is stopped.' },
  toolEvents: { supported: true, note: 'As reported by the agent (tool_call / tool_call_update).' },
  fileChanges: { supported: true, note: 'Writes through FORGE (fs/write_text_file) and diffs the agent reports; the git diff is authoritative.' },
  approvals: { supported: true, note: 'Agent-initiated: FORGE decides every request_permission and every fs request, but cannot see a tool the agent runs without asking.' },
  usageReporting: { supported: false, note: 'ACP defines no usage reporting.' },
  modelSelection: { supported: false, note: 'The registered agent decides its model.' },
  worktreeIntegration: { supported: true, note: 'The session cwd is the FORGE worktree.' },
};

/** Keys that would put a mission on separately billed API credentials, whatever the agent. */
export const ACP_BILLING_ENV = [
  'ANTHROPIC_API_KEY', 'ANTHROPIC_AUTH_TOKEN', 'ANTHROPIC_BASE_URL', 'CLAUDE_CODE_USE_BEDROCK', 'CLAUDE_CODE_USE_VERTEX', 'CLAUDE_CODE_USE_FOUNDRY',
  'OPENAI_API_KEY', 'OPENAI_BASE_URL', 'CODEX_API_KEY', 'GEMINI_API_KEY', 'GOOGLE_API_KEY', 'GOOGLE_GENAI_USE_VERTEXAI',
];

/** Billing credentials and model-endpoint redirects, whatever their case (Windows env names are case-insensitive). */
const BILLING_PATTERN = /(^|_)(API_?KEY|AUTH_TOKEN)$|_BASE_URL$|^CLAUDE_CODE_USE_|^GOOGLE_GENAI_USE_VERTEXAI$/i;

export function acpEnv(base: NodeJS.ProcessEnv): Record<string, string | undefined> {
  const env: Record<string, string | undefined> = { ...base };
  const billing = new Set(ACP_BILLING_ENV.map((k) => k.toUpperCase()));
  for (const k of Object.keys(env)) {
    const up = k.toUpperCase();
    if (billing.has(up) || BILLING_PATTERN.test(k) || up.startsWith('FORGE_')) delete env[k];
  }
  return env;
}

/** FORGE's MCP configs (authorized, stored definitions) in ACP's session/new shape. */
export function toAcpMcpServers(servers: Record<string, McpServerConfig> | undefined, httpSupported: boolean): acp.McpServer[] {
  const out: acp.McpServer[] = [];
  for (const [name, s] of Object.entries(servers ?? {})) {
    if (s.type === 'stdio') out.push({ name, command: s.command, args: s.args ?? [], env: Object.entries(s.env ?? {}).map(([n, value]) => ({ name: n, value })) });
    else if (httpSupported) out.push({ type: 'http', name, url: s.url, headers: [] });
  }
  return out;
}

/**
 * Translate an ACP tool call into the request FORGE policy understands. Write-like kinds become one `Edit` per location
 * (policy checks each real path; a call without a location is denied by policy); `execute` becomes `Bash`.
 */
export function forgeRequests(tc: acp.ToolCallUpdate, cwd: string): Array<{ tool: string; input: Record<string, unknown> }> {
  const raw = (tc.rawInput && typeof tc.rawInput === 'object' ? tc.rawInput : {}) as Record<string, unknown>;
  const paths = [...new Set((tc.locations ?? []).map((l) => (isAbsolute(l.path) ? l.path : resolve(cwd, l.path))))];
  const base = { acpTitle: tc.title ?? null, acpKind: tc.kind ?? null };
  switch (tc.kind) {
    case 'edit':
    case 'delete':
    case 'move':
      return paths.length ? paths.map((p) => ({ tool: 'Edit', input: { ...base, file_path: p } })) : [{ tool: 'Edit', input: base }];
    case 'read':
    case 'search':
      // Without a location FORGE cannot check what is read: ask, and show the agent's own description of it.
      return paths.length ? paths.map((p) => ({ tool: 'Read', input: { ...base, file_path: p } })) : [{ tool: 'acp_read', input: { ...base, rawInput: raw } }];
    case 'execute': {
      const c = raw.command;
      const command = Array.isArray(c) && c.every((x) => typeof x === 'string') ? c.join(' ') : typeof c === 'string' ? c : null;
      // The title is the agent's prose, not the command: never let it stand in for what runs (policy reads `command`).
      if (command === null) return [{ tool: 'acp_execute', input: { ...base, rawInput: raw } }];
      return [{ tool: 'Bash', input: { ...base, command, rawInput: raw } }];
    }
    case 'fetch':
      return [{ tool: 'WebFetch', input: { ...base, url: raw.url ?? null } }];
    default:
      return [{ tool: `acp_${tc.kind ?? 'other'}`, input: { ...base, rawInput: raw } }];
  }
}

/** The option FORGE answers with: only ever a `*_once` option, or `cancelled` once the turn is being cancelled (ACP spec). */
export function chooseOption(options: acp.PermissionOption[], allow: boolean, cancelling = false): acp.RequestPermissionOutcome {
  if (cancelling) return { outcome: 'cancelled' };
  const want = allow ? ['allow_once'] : ['reject_once', 'reject_always'];
  for (const kind of want) {
    const o = options.find((x) => x.kind === kind);
    if (o) return { outcome: 'selected', optionId: o.optionId };
  }
  return { outcome: 'cancelled' };
}

/**
 * Resolve `p` for a file access FORGE performs itself: lexically inside the worktree, and no link anywhere on the way —
 * no symlink/junction ancestor (a directory swapped for a link after approval would redirect the write, possibly onto a
 * protected file) and no link or hard link as the file itself (a hard link would let a write change a file outside).
 */
export async function safeTarget(cwd: string, p: string): Promise<string> {
  if (!isAbsolute(p)) throw new Error('ACP requires absolute paths');
  const root = await realpath(cwd);
  const target = resolve(p);
  const rel = relative(root, target);
  if (rel === '' || isAbsolute(rel) || rel.split(sep)[0] === '..') throw new Error(`${p} is outside the mission worktree`);
  let cur = root;
  const parts = rel.split(sep);
  for (const [i, part] of parts.entries()) {
    cur = join(cur, part);
    const st = await lstat(cur).catch(() => null);
    if (!st) break; // the rest does not exist yet; it will be created as plain directories/file below the checked part
    if (st.isSymbolicLink()) throw new Error(`${p} goes through a link (${cur}); FORGE does not follow links`);
    if (i === parts.length - 1 && st.isFile() && st.nlink > 1) throw new Error(`${p} is a hard link to another file; FORGE does not write through it`);
  }
  return target;
}

/** Open for writing without following a final link (where the platform supports it), and re-check what was opened. */
async function writeNoFollow(target: string, content: string): Promise<void> {
  const flags = fsConstants.O_WRONLY | fsConstants.O_CREAT | fsConstants.O_TRUNC | (fsConstants.O_NOFOLLOW ?? 0);
  const fh = await open(target, flags, 0o644);
  try {
    const st = await fh.stat();
    if (!st.isFile() || st.nlink > 1) throw new Error(`${target} changed into a link while being written; nothing was written`);
    await fh.writeFile(content, 'utf8');
  } finally {
    await fh.close();
  }
}

/** Reads FORGE performs for the agent are bounded. */
const MAX_READ_BYTES = 4 * 1024 * 1024;

const zero = { inputTokens: 0, outputTokens: 0, cacheReadTokens: 0, cacheCreationTokens: 0 };

export interface AcpDeps {
  /** Tests may inject a process; production spawns `start.acp.command`. */
  spawnAgent?: (start: StartCommand) => ChildProcess;
}

export function createAcpAdapter(deps: AcpDeps = {}): ProviderAdapter {
  return {
    id: 'acp',
    capabilities: () => ACP_CAPABILITIES,
    run: (ctx) => runAcp(ctx, deps),
  };
}

async function runAcp(ctx: RunContext, deps: AcpDeps): Promise<void> {
  const s = ctx.start;
  let denials = 0;
  let closeClient = () => {};
  const finish = (r: Partial<TurnResult> & { status: TurnResult['status'] }) => {
    closeClient();
    ctx.turnComplete({ usage: zero, permissionDenials: denials, ...r });
  };
  if (!s.acp?.command) return finish({ status: 'error', message: 'no ACP agent was given for this mission' });

  let child: ChildProcess;
  try {
    child = deps.spawnAgent ? deps.spawnAgent(s) : spawn(s.acp.command, s.acp.args ?? [], { cwd: s.cwd, env: acpEnv(process.env), stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true });
  } catch (e) {
    return finish({ status: 'error', message: `could not start the ACP agent: ${errMessage(e)}` });
  }
  let stderrTail = '';
  child.stderr?.on('data', (d: Buffer) => { stderrTail = (stderrTail + d.toString('utf8')).slice(-4000); });
  const exited = new Promise<number | null>((res) => {
    child.once('exit', (code) => res(code));
    child.once('error', (e) => { stderrTail += `\n${errMessage(e)}`; res(null); });
  });

  // Paths the human approved through request_permission, consumed by the agent's next fs/write_text_file for that path.
  const granted = new Map<string, number>();
  const textIds = new Map<string, string>();
  let msgCounter = 0;
  let currentMsg = `acp-m${++msgCounter}`;
  let textSinceTool = false;

  // Every question FORGE asks gets its own id: the agent chooses its tool-call ids, and a reused id must never let an
  // earlier answer stand in for a new request.
  let askSeq = 0;
  // Once the turn has ended (or is ending), nothing more is granted, read or written for this agent.
  let closed = false;
  const ask = async (tool: string, input: Record<string, unknown>, idBase: string): Promise<PermissionDecision> => {
    if (closed) return { behavior: 'deny', message: 'the turn has ended' };
    const d = await ctx.requestPermission({ tool, input: truncateDeep(input, MAX_FIELD_BYTES), toolUseId: `${idBase}#${++askSeq}`, source: 'hook' });
    if (d.behavior !== 'allow') denials++;
    return d;
  };
  const refuse = (msg: string) => acp.RequestError.invalidParams(undefined, msg);
  closeClient = () => {
    closed = true;
    granted.clear();
  };

  const client: acp.Client = {
    async requestPermission(p) {
      if (closed || ctx.signal.aborted) return { outcome: { outcome: 'cancelled' } };
      const reqs = forgeRequests(p.toolCall, s.cwd);
      let allow = true;
      for (const r of reqs) {
        const d = await ask(r.tool, r.input, p.toolCall.toolCallId);
        if (d.behavior !== 'allow') { allow = false; break; }
      }
      const outcome = chooseOption(p.options, allow, closed || ctx.signal.aborted);
      if (allow && outcome.outcome === 'selected') {
        for (const r of reqs) if (r.tool === 'Edit' && typeof r.input.file_path === 'string') granted.set(resolve(r.input.file_path), (granted.get(resolve(r.input.file_path)) ?? 0) + 1);
      } else if (allow) {
        ctx.emit('status', { message: 'The agent offered no "allow once" option; FORGE never grants standing permission, so the call was refused.' });
      }
      return { outcome };
    },
    async sessionUpdate(n) {
      const u = n.update;
      switch (u.sessionUpdate) {
        case 'agent_message_chunk':
          if (u.content.type === 'text') { ctx.delta(currentMsg, u.content.text); textIds.set(currentMsg, (textIds.get(currentMsg) ?? '') + u.content.text); textSinceTool = true; }
          break;
        case 'agent_thought_chunk':
          if (u.content.type === 'text') ctx.emit('agent.thinking', { text: truncateDeep(u.content.text, MAX_FIELD_BYTES) });
          break;
        case 'tool_call':
          flushText();
          ctx.emit('tool.call', truncateDeep({ toolUseId: u.toolCallId, name: u.kind ?? 'other', summary: u.title, input: { kind: u.kind ?? null, locations: u.locations ?? [], rawInput: u.rawInput ?? null } }, MAX_FIELD_BYTES));
          break;
        case 'tool_call_update':
          if (u.status === 'completed' || u.status === 'failed') {
            ctx.emit('tool.result', truncateDeep({ toolUseId: u.toolCallId, name: u.kind ?? 'other', ok: u.status === 'completed', output: u.title ?? u.status }, MAX_FIELD_BYTES));
            for (const c of (u.content ?? []).slice(0, 200)) if (c.type === 'diff') ctx.emit('file.changed', { path: truncateDeep(c.path, MAX_FIELD_BYTES), tool: 'acp', toolUseId: u.toolCallId });
          }
          break;
        case 'plan':
          ctx.emit('status', truncateDeep({ message: 'plan', entries: u.entries.map((e) => ({ content: e.content, status: e.status })) }, MAX_FIELD_BYTES));
          break;
        default:
          break;
      }
    },
    async readTextFile(p) {
      const d = await ask('Read', { file_path: p.path, acpKind: 'fs/read_text_file' }, 'fs-read');
      if (d.behavior !== 'allow') throw refuse(d.message ?? 'denied by FORGE');
      // Policy allowed it (inside the worktree) or a human approved it (outside). Inside the worktree, links and hard links
      // are refused as for writes; an approved outside read is read as named.
      const inside = await safeTarget(s.cwd, p.path).then(() => true).catch((e: unknown) => (String(errMessage(e)).includes('outside the mission worktree') ? false : Promise.reject(refuse(errMessage(e)))));
      const st = await lstat(p.path).catch(() => null);
      if (!st?.isFile()) throw refuse(`${p.path} is not a regular file`);
      if (st.size > MAX_READ_BYTES) throw refuse(`${p.path} is larger than ${MAX_READ_BYTES} bytes; FORGE does not read it whole`);
      if (inside && st.nlink > 1) throw refuse(`${p.path} is a hard link`);
      const text = await readFile(p.path, 'utf8');
      if (p.line == null && p.limit == null) return { content: text };
      const lines = text.split('\n');
      const from = Math.max(0, (p.line ?? 1) - 1);
      return { content: lines.slice(from, p.limit == null ? undefined : from + p.limit).join('\n') };
    },
    async writeTextFile(p) {
      if (closed) throw refuse('the turn has ended');
      const key = resolve(p.path);
      const n = granted.get(key) ?? 0;
      if (n > 0) {
        if (n === 1) granted.delete(key); else granted.set(key, n - 1);
      } else {
        const d = await ask('Write', { file_path: p.path, content: p.content, acpKind: 'fs/write_text_file' }, 'fs-write');
        if (d.behavior !== 'allow') throw refuse(d.message ?? 'denied by FORGE');
      }
      // Re-checked at write time: a path that became (or goes through) a link, or left the worktree, since the decision.
      const target = await safeTarget(s.cwd, p.path).catch((e) => { throw refuse(errMessage(e)); });
      await mkdir(dirname(target), { recursive: true });
      await safeTarget(s.cwd, p.path).catch((e) => { throw refuse(errMessage(e)); }); // the directories just created are plain
      await writeNoFollow(target, p.content).catch((e) => { throw refuse(errMessage(e)); });
      ctx.emit('file.changed', { path: target, tool: 'fs/write_text_file' });
      return {};
    },
  };

  function flushText() {
    if (!textSinceTool) return;
    ctx.emit('agent.text', { messageId: currentMsg, text: textIds.get(currentMsg) ?? '' });
    textSinceTool = false;
    currentMsg = `acp-m${++msgCounter}`;
  }

  const stream = acp.ndJsonStream(Writable.toWeb(child.stdin!) as WritableStream<Uint8Array>, Readable.toWeb(child.stdout!) as ReadableStream<Uint8Array>);
  const conn = new acp.ClientSideConnection(() => client, stream);
  let sessionId: string | null = null;
  const onAbort = () => {
    if (sessionId) void conn.cancel({ sessionId }).catch(() => undefined);
    // An agent that ignores session/cancel does not keep running: stop it after a short grace period.
    setTimeout(() => { if (child.exitCode === null) child.kill('SIGKILL'); }, 3000).unref();
  };
  ctx.signal.addEventListener('abort', onAbort, { once: true });
  const died = exited.then((code) => { throw new Error(`the ACP agent exited (code ${code ?? 'unknown'})${stderrTail.trim() ? `: ${stderrTail.trim().slice(-500)}` : ''}`); });

  try {
    const init = await Promise.race([
      conn.initialize({ protocolVersion: acp.PROTOCOL_VERSION, clientCapabilities: { fs: { readTextFile: true, writeTextFile: true }, terminal: false }, clientInfo: { name: 'forge', version: '0.1.0' } }),
      died,
    ]);
    if (init.protocolVersion !== acp.PROTOCOL_VERSION) {
      throw new Error(`the agent speaks ACP protocol version ${init.protocolVersion}; FORGE supports ${acp.PROTOCOL_VERSION}`);
    }
    const caps = init.agentCapabilities ?? {};
    const mcpServers = toAcpMcpServers(s.mcpServers, caps.mcpCapabilities?.http === true);
    const dropped = Object.keys(s.mcpServers ?? {}).length - mcpServers.length;
    let resumed = false;
    if (s.resume && caps.loadSession) {
      await Promise.race([conn.loadSession({ sessionId: s.resume, cwd: s.cwd, mcpServers }), died]);
      sessionId = s.resume;
      resumed = true;
    } else {
      try {
        sessionId = (await Promise.race([conn.newSession({ cwd: s.cwd, mcpServers }), died])).sessionId;
      } catch (e) {
        if ((e as { code?: unknown })?.code === -32000) {
          throw new Error('the agent needs you to sign in. Do that in the agent itself, outside FORGE; FORGE never authenticates on your behalf');
        }
        throw e;
      }
    }
    ctx.providerSession(sessionId);
    ctx.emit('session.started', {
      providerSessionId: sessionId,
      model: s.acp.name ?? 'acp agent',
      credentialSource: 'agent-managed',
      cwd: s.cwd,
      acp: {
        protocolVersion: init.protocolVersion,
        agentInfo: init.agentInfo ?? null,
        loadSession: caps.loadSession === true,
        promptCapabilities: caps.promptCapabilities ?? {},
        mcpCapabilities: caps.mcpCapabilities ?? {},
        authMethods: (init.authMethods ?? []).map((m) => m.id),
        resumed,
        mcpServersGiven: mcpServers.map((m) => m.name),
        mcpServersDropped: dropped,
      },
    });
    if (s.resume && !resumed) ctx.emit('status', { message: 'The agent cannot load earlier sessions; this turn starts a new one.' });
    const res = await Promise.race([
      conn.prompt({ sessionId, prompt: [{ type: 'text', text: s.appendSystemPrompt ? `${s.appendSystemPrompt}\n\n${s.prompt}` : s.prompt }] }),
      died,
    ]);
    flushText();
    if (res.stopReason === 'cancelled' || ctx.signal.aborted) finish({ status: 'interrupted', message: 'Cancelled', providerSessionId: sessionId });
    else if (res.stopReason === 'end_turn') finish({ status: 'success', providerSessionId: sessionId });
    else finish({ status: 'error', message: `the agent stopped: ${res.stopReason}`, providerSessionId: sessionId ?? undefined });
  } catch (e) {
    flushText();
    // A closed connection usually means the agent died: say how, not just that the pipe closed.
    if (!ctx.signal.aborted && child.exitCode === null) await Promise.race([exited, new Promise((r) => setTimeout(r, 500))]);
    if (!ctx.signal.aborted && child.exitCode !== null && !/exited \(code/.test(errMessage(e))) {
      e = new Error(`the ACP agent exited (code ${child.exitCode})${stderrTail.trim() ? `: ${stderrTail.trim().slice(-500)}` : ''}`);
    }
    finish(ctx.signal.aborted ? { status: 'interrupted', message: 'Cancelled' } : { status: 'error', message: errMessage(e) });
  } finally {
    ctx.signal.removeEventListener('abort', onAbort);
    died.catch(() => undefined);
    child.stdin?.end();
    const t = setTimeout(() => child.kill(), 2000);
    await Promise.race([exited, new Promise((r) => setTimeout(r, 2500))]);
    clearTimeout(t);
    if (child.exitCode === null) child.kill('SIGKILL');
  }
}

export const acpAdapter = createAcpAdapter();
