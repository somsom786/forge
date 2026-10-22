import type { Options } from '@anthropic-ai/claude-agent-sdk';
import type { CapabilityMatrix, StartCommand } from '../protocol.js';
import { AsyncQueue, errMessage } from '../util.js';
import { ClaudeNormalizer, type TurnResult } from './claudeNormalize.js';
import type { PermissionDecision, ProviderAdapter, RunContext } from './types.js';

export const CLAUDE_CAPABILITIES: CapabilityMatrix = {
  sessionStart: { supported: true },
  resume: { supported: true, note: 'Via provider session id; resumes the transcript, not process state.' },
  streaming: { supported: true },
  interrupt: { supported: true },
  toolEvents: { supported: true },
  fileChanges: { supported: true, note: 'Derived from successful Write/Edit tool results; the git diff is authoritative.' },
  approvals: { supported: true, note: 'Every tool call is gated through a PreToolUse hook; canUseTool is a fail-safe fallback.' },
  usageReporting: { supported: true, note: 'Token counts and cost are provider estimates. Subscription utilisation is reported only when the provider emits rate-limit events.' },
  modelSelection: { supported: true },
  worktreeIntegration: { supported: true, note: 'FORGE creates and owns the worktree; the agent runs inside it.' },
};

/** Environment variables that would switch a mission onto separately billed API credentials. */
const BILLING_ENV = [
  'ANTHROPIC_API_KEY',
  'ANTHROPIC_AUTH_TOKEN',
  'CLAUDE_CODE_USE_BEDROCK',
  'CLAUDE_CODE_USE_VERTEX',
  'CLAUDE_CODE_USE_FOUNDRY',
];

/** Only loopback endpoints are ever accepted: a "local model" setting must not be able to ship prompts off-box. */
export function assertLoopbackEndpoint(url: string): URL {
  let u: URL;
  try {
    u = new URL(url);
  } catch {
    throw new Error(`invalid local endpoint: ${url}`);
  }
  const host = u.hostname.replace(/^\[/, '').replace(/\]$/, '');
  if (u.protocol !== 'http:' || !['127.0.0.1', 'localhost', '::1'].includes(host)) {
    throw new Error(`local endpoint must be http on loopback (127.0.0.1, localhost or ::1), got ${url}`);
  }
  return u;
}

export function sanitizedEnv(
  base: NodeJS.ProcessEnv,
  allowApiBilling: boolean,
  local?: { endpoint: string; model: string },
): Record<string, string | undefined> {
  const env: Record<string, string | undefined> = { ...base };
  if (!allowApiBilling || local) for (const k of BILLING_ENV) delete env[k];
  env.CLAUDE_AGENT_SDK_CLIENT_APP = 'forge/0.1';
  if (local) {
    const u = assertLoopbackEndpoint(local.endpoint);
    env.ANTHROPIC_BASE_URL = u.origin;
    env.ANTHROPIC_AUTH_TOKEN = 'ollama'; // placeholder the local server ignores; never a real credential
    // Every internal "small/fast", subagent and family-alias call must also stay on the local model.
    for (const k of ['ANTHROPIC_MODEL', 'ANTHROPIC_DEFAULT_OPUS_MODEL', 'ANTHROPIC_DEFAULT_SONNET_MODEL', 'ANTHROPIC_DEFAULT_HAIKU_MODEL', 'ANTHROPIC_SMALL_FAST_MODEL', 'CLAUDE_CODE_SUBAGENT_MODEL']) {
      env[k] = local.model;
    }
    env.CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC = '1';
  }
  return env;
}

/**
 * Neutralise every execution surface a repository can define for itself. Without this, a committed
 * `.claude/settings.json` (SessionStart/PreToolUse command hooks) and `.mcp.json` (stdio MCP servers) run code
 * the moment a session starts, with no prompt, because SDK sessions skip the interactive trust dialog.
 *
 * `settingSources: ['project']` is still used so CLAUDE.md, rules and skills load. Our own programmatic hooks are
 * unaffected by `allowManagedHooksOnly`. Both facts are verified empirically by
 * `scripts/lockdown-experiment.mjs` (control run vs locked run); re-run it when bumping the SDK.
 */
export const LOCKDOWN = {
  managedSettings: { allowManagedHooksOnly: true },
  strictMcpConfig: true,
  mcpServers: {},
} satisfies Pick<Options, 'managedSettings' | 'strictMcpConfig' | 'mcpServers'>;

export type Decide = (
  tool: string,
  input: Record<string, unknown>,
  toolUseId: string,
  source: 'hook' | 'can_use_tool',
  extra?: { blockedPath?: string; reason?: string; signal?: AbortSignal },
) => Promise<PermissionDecision>;

const DENY_ON_ERROR = (e: unknown): PermissionDecision => ({
  behavior: 'deny',
  message: `FORGE could not reach a decision (${errMessage(e)}); denying for safety`,
});

export function buildQueryOptions(
  s: StartCommand,
  parts: { abort: AbortController; decide: Decide; stderr: (d: string) => void; env?: NodeJS.ProcessEnv },
): Options {
  return {
    cwd: s.cwd,
    model: s.model,
    resume: s.resume,
    maxTurns: s.maxTurns,
    permissionMode: s.permissionMode,
    // Project settings only (CLAUDE.md, rules, skills). User-level settings are excluded so missions are
    // reproducible and a user's global allow-rules / bypass defaults cannot widen what FORGE has approved.
    settingSources: ['project'],
    ...LOCKDOWN,
    includePartialMessages: true,
    persistSession: true,
    executable: 'node',
    pathToClaudeCodeExecutable: s.claudePath,
    abortController: parts.abort,
    env: sanitizedEnv(
      parts.env ?? process.env,
      s.allowApiBilling === true,
      s.localEndpoint ? { endpoint: s.localEndpoint, model: s.model ?? '' } : undefined,
    ),
    stderr: parts.stderr,
    ...(s.appendSystemPrompt
      ? { systemPrompt: { type: 'preset' as const, preset: 'claude_code' as const, append: s.appendSystemPrompt } }
      : {}),
    hooks: {
      PreToolUse: [
        {
          // Approvals wait on a human; do not let the hook time out in the meantime.
          timeout: 24 * 3600,
          hooks: [
            async (hookInput, _toolUseId, { signal }) => {
              if (hookInput.hook_event_name !== 'PreToolUse') return {};
              let r: PermissionDecision;
              try {
                r = await parts.decide(
                  hookInput.tool_name,
                  (hookInput.tool_input ?? {}) as Record<string, unknown>,
                  hookInput.tool_use_id,
                  'hook',
                  { signal },
                );
              } catch (e) {
                r = DENY_ON_ERROR(e);
              }
              // Never return updatedInput: FORGE approves exactly what the agent asked for, nothing rewritten.
              return {
                hookSpecificOutput: {
                  hookEventName: 'PreToolUse' as const,
                  permissionDecision: r.behavior,
                  permissionDecisionReason: r.message ?? (r.behavior === 'allow' ? 'Approved via FORGE' : 'Denied via FORGE'),
                },
              };
            },
          ],
        },
      ],
    },
    // Fail-safe: reached only if a call falls through to the provider's own prompt.
    canUseTool: async (tool, toolInput, opts) => {
      let r: PermissionDecision;
      try {
        r = await parts.decide(tool, toolInput, opts.toolUseID, 'can_use_tool', {
          blockedPath: opts.blockedPath,
          reason: opts.decisionReason,
          signal: opts.signal,
        });
      } catch (e) {
        r = DENY_ON_ERROR(e);
      }
      return r.behavior === 'allow'
        ? { behavior: 'allow' as const, updatedInput: toolInput, toolUseID: opts.toolUseID }
        : { behavior: 'deny' as const, message: r.message ?? 'Denied by FORGE', toolUseID: opts.toolUseID };
    },
  };
}

const ZERO_USAGE = { inputTokens: 0, outputTokens: 0, cacheReadTokens: 0, cacheCreationTokens: 0 };

export const claudeAdapter: ProviderAdapter = {
  id: 'claude',
  capabilities: () => CLAUDE_CAPABILITIES,

  async run(ctx: RunContext): Promise<void> {
    const s = ctx.start;
    const sdk = await import('@anthropic-ai/claude-agent-sdk');
    const input = new AsyncQueue<import('@anthropic-ai/claude-agent-sdk').SDKUserMessage>();
    input.push({ type: 'user', message: { role: 'user', content: s.prompt }, parent_tool_use_id: null });

    const abort = new AbortController();
    if (s.localEndpoint) {
      if (!s.model) throw new Error('a local-model mission needs a model name');
      assertLoopbackEndpoint(s.localEndpoint);
    }
    const normalizer = new ClaudeNormalizer(
      { event: ctx.emit, delta: ctx.delta, providerSession: ctx.providerSession },
      s.localEndpoint ? { credentialSource: 'local-endpoint', endpoint: s.localEndpoint } : undefined,
    );

    const q = sdk.query({
      prompt: input,
      options: buildQueryOptions(s, {
        abort,
        stderr: (d) => process.stderr.write(`[claude] ${d}`),
        decide: (tool, toolInput, toolUseId, source, extra = {}) =>
          ctx.requestPermission({ tool, input: toolInput, toolUseId, source, ...extra }),
      }),
    });

    let result: TurnResult | null = null;
    let aborted = false;
    const onAbort = () => {
      aborted = true;
      // Ask nicely first so the provider can flush its transcript, then force.
      q.interrupt().catch(() => undefined);
      setTimeout(() => {
        abort.abort();
        try {
          q.close();
        } catch {
          /* already closed */
        }
      }, 2500).unref();
    };
    if (ctx.signal.aborted) onAbort();
    else ctx.signal.addEventListener('abort', onAbort, { once: true });

    try {
      for await (const msg of q) {
        const r = normalizer.handle(msg as unknown as Record<string, unknown>);
        if (r) {
          result = r;
          input.close(); // one mission turn per engine process; follow-ups resume the session
        }
      }
    } catch (e) {
      if (!aborted) {
        ctx.turnComplete({ status: 'error', message: errMessage(e), usage: ZERO_USAGE, permissionDenials: 0 });
        return;
      }
    } finally {
      input.close();
    }

    if (aborted) {
      ctx.turnComplete({
        status: 'interrupted',
        message: 'Cancelled by user',
        usage: result?.usage ?? ZERO_USAGE,
        permissionDenials: result?.permissionDenials ?? 0,
        providerSessionId: result?.providerSessionId,
      });
    } else if (result) {
      ctx.turnComplete(result);
    } else {
      ctx.turnComplete({ status: 'error', message: 'The provider ended the session without producing a result', usage: ZERO_USAGE, permissionDenials: 0 });
    }
  },
};
