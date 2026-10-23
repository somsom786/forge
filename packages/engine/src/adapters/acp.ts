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
import { lstat, mkdir, readFile, realpath, writeFile } from 'node:fs/promises';
import { dirname, isAbsolute, relative, resolve, sep } from 'node:path';
import { Readable, Writable } from 'node:stream';
import * as acp from '@agentclientprotocol/sdk';
import type { CapabilityMatrix, McpServerConfig, StartCommand } from '../protocol.js';
import { errMessage } from '../util.js';
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

export function acpEnv(base: NodeJS.ProcessEnv): Record<string, string | undefined> {
  const env: Record<string, string | undefined> = { ...base };
  for (const k of ACP_BILLING_ENV) delete env[k];
  for (const k of Object.keys(env)) if (k.startsWith('FORGE_')) delete env[k];
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
      return paths.length ? paths.map((p) => ({ tool: 'Read', input: { ...base, file_path: p } })) : [{ tool: 'Read', input: base }];
    case 'execute': {
      const c = raw.command;
      const command = Array.isArray(c) ? c.map(String).join(' ') : typeof c === 'string' ? c : String(tc.title ?? '');
      return [{ tool: 'Bash', input: { ...base, command } }];
    }
    case 'fetch':
      return [{ tool: 'WebFetch', input: { ...base, url: raw.url ?? null } }];
    default:
      return [{ tool: `acp_${tc.kind ?? 'other'}`, input: { ...base, rawInput: raw } }];
  }
}

/** The option FORGE answers with: only ever a `*_once` option. */
export function chooseOption(options: acp.PermissionOption[], allow: boolean): acp.RequestPermissionOutcome {
  const want = allow ? ['allow_once'] : ['reject_once', 'reject_always'];
  for (const kind of want) {
    const o = options.find((x) => x.kind === kind);
    if (o) return { outcome: 'selected', optionId: o.optionId };
  }
  return { outcome: 'cancelled' };
}

/** Resolve `p` for a FORGE-performed write: inside the worktree by real path, never through a link. */
export async function safeTarget(cwd: string, p: string): Promise<string> {
  if (!isAbsolute(p)) throw new Error('ACP requires absolute paths');
  const root = await realpath(cwd);
  const target = resolve(p);
  const st = await lstat(target).catch(() => null);
  if (st?.isSymbolicLink()) throw new Error(`${p} is a link; FORGE does not write through links`);
  // The deepest existing ancestor decides where the write really lands.
  let dir = dirname(target);
  for (;;) {
    const real = await realpath(dir).catch(() => null);
    if (real) {
      const rest = relative(dir, target);
      const finalPath = resolve(real, rest);
      const rel = relative(root, finalPath);
      if (rel === '' || rel.startsWith('..') || isAbsolute(rel) || rel.split(sep)[0] === '..') throw new Error(`${p} is outside the mission worktree`);
      return finalPath;
    }
    const up = dirname(dir);
    if (up === dir) throw new Error(`${p} has no existing parent`);
    dir = up;
  }
}

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
  const finish = (r: Partial<TurnResult> & { status: TurnResult['status'] }) =>
    ctx.turnComplete({ usage: zero, permissionDenials: denials, ...r });
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

  const ask = async (tool: string, input: Record<string, unknown>, toolUseId: string): Promise<PermissionDecision> => {
    const d = await ctx.requestPermission({ tool, input, toolUseId, source: 'hook' });
    if (d.behavior !== 'allow') denials++;
    return d;
  };

  const client: acp.Client = {
    async requestPermission(p) {
      const reqs = forgeRequests(p.toolCall, s.cwd);
      let allow = true;
      for (const [i, r] of reqs.entries()) {
        const d = await ask(r.tool, r.input, `${p.toolCall.toolCallId}${reqs.length > 1 ? `#${i}` : ''}`);
        if (d.behavior !== 'allow') { allow = false; break; }
      }
      const outcome = chooseOption(p.options, allow);
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
          if (u.content.type === 'text') ctx.emit('agent.thinking', { text: u.content.text });
          break;
        case 'tool_call':
          flushText();
          ctx.emit('tool.call', { toolUseId: u.toolCallId, name: u.kind ?? 'other', summary: u.title, input: { kind: u.kind ?? null, locations: u.locations ?? [], rawInput: u.rawInput ?? null } });
          break;
        case 'tool_call_update':
          if (u.status === 'completed' || u.status === 'failed') {
            ctx.emit('tool.result', { toolUseId: u.toolCallId, name: u.kind ?? 'other', ok: u.status === 'completed', output: u.title ?? u.status });
            for (const c of u.content ?? []) if (c.type === 'diff') ctx.emit('file.changed', { path: c.path, tool: 'acp', toolUseId: u.toolCallId });
          }
          break;
        case 'plan':
          ctx.emit('status', { message: 'plan', entries: u.entries.map((e) => ({ content: e.content, status: e.status })) });
          break;
        default:
          break;
      }
    },
    async readTextFile(p) {
      const d = await ask('Read', { file_path: p.path, acpKind: 'fs/read_text_file' }, `fs-read-${Date.now()}`);
      if (d.behavior !== 'allow') throw acp.RequestError.invalidParams(undefined, d.message ?? 'denied by FORGE');
      const text = await readFile(p.path, 'utf8');
      if (p.line == null && p.limit == null) return { content: text };
      const lines = text.split('\n');
      const from = Math.max(0, (p.line ?? 1) - 1);
      return { content: lines.slice(from, p.limit == null ? undefined : from + p.limit).join('\n') };
    },
    async writeTextFile(p) {
      const key = resolve(p.path);
      const n = granted.get(key) ?? 0;
      if (n > 0) {
        if (n === 1) granted.delete(key); else granted.set(key, n - 1);
      } else {
        const d = await ask('Write', { file_path: p.path, content: p.content, acpKind: 'fs/write_text_file' }, `fs-write-${Date.now()}`);
        if (d.behavior !== 'allow') throw acp.RequestError.invalidParams(undefined, d.message ?? 'denied by FORGE');
      }
      // Re-checked at write time: a path that became a link or left the worktree since the decision is refused.
      const target = await safeTarget(s.cwd, p.path).catch((e) => { throw acp.RequestError.invalidParams(undefined, errMessage(e)); });
      await mkdir(dirname(target), { recursive: true });
      await writeFile(target, p.content, 'utf8');
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
  const onAbort = () => { if (sessionId) void conn.cancel({ sessionId }).catch(() => undefined); };
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
