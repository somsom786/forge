import type { CapabilityMatrix } from '../protocol.js';
import { AsyncQueue, errMessage } from '../util.js';
import { ClaudeNormalizer, type TurnResult } from './claudeNormalize.js';
import type { ProviderAdapter, RunContext } from './types.js';

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

export function sanitizedEnv(base: NodeJS.ProcessEnv, allowApiBilling: boolean): Record<string, string | undefined> {
  const env: Record<string, string | undefined> = { ...base };
  if (!allowApiBilling) for (const k of BILLING_ENV) delete env[k];
  env.CLAUDE_AGENT_SDK_CLIENT_APP = 'forge/0.1';
  return env;
}

export const claudeAdapter: ProviderAdapter = {
  id: 'claude',
  capabilities: () => CLAUDE_CAPABILITIES,

  async run(ctx: RunContext): Promise<void> {
    const s = ctx.start;
    const sdk = await import('@anthropic-ai/claude-agent-sdk');
    const input = new AsyncQueue<import('@anthropic-ai/claude-agent-sdk').SDKUserMessage>();
    input.push({
      type: 'user',
      message: { role: 'user', content: s.prompt },
      parent_tool_use_id: null,
    });

    const abort = new AbortController();
    const normalizer = new ClaudeNormalizer({
      event: ctx.emit,
      delta: ctx.delta,
      providerSession: ctx.providerSession,
    });

    const decide = async (
      tool: string,
      toolInput: Record<string, unknown>,
      toolUseId: string,
      source: 'hook' | 'can_use_tool',
      extra: { blockedPath?: string; reason?: string } = {},
    ) => ctx.requestPermission({ tool, input: toolInput, toolUseId, source, ...extra });

    const q = sdk.query({
      prompt: input,
      options: {
        cwd: s.cwd,
        model: s.model,
        resume: s.resume,
        maxTurns: s.maxTurns,
        permissionMode: s.permissionMode,
        // Project settings only: CLAUDE.md, rules, skills and hooks that travel with the repo.
        // User-level settings are excluded so missions are reproducible and a user's global
        // allow-rules / bypass defaults cannot silently widen what FORGE has approved.
        settingSources: ['project'],
        includePartialMessages: true,
        persistSession: true,
        executable: 'node',
        pathToClaudeCodeExecutable: s.claudePath,
        abortController: abort,
        env: sanitizedEnv(process.env, s.allowApiBilling === true),
        stderr: (d: string) => process.stderr.write(`[claude] ${d}`),
        ...(s.appendSystemPrompt
          ? { systemPrompt: { type: 'preset' as const, preset: 'claude_code' as const, append: s.appendSystemPrompt } }
          : {}),
        hooks: {
          PreToolUse: [
            {
              // Approvals wait on a human; do not let the hook time out in the meantime.
              timeout: 24 * 3600,
              hooks: [
                async (hookInput) => {
                  if (hookInput.hook_event_name !== 'PreToolUse') return {};
                  const r = await decide(
                    hookInput.tool_name,
                    (hookInput.tool_input ?? {}) as Record<string, unknown>,
                    hookInput.tool_use_id,
                    'hook',
                  );
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
          const r = await decide(tool, toolInput, opts.toolUseID, 'can_use_tool', {
            blockedPath: opts.blockedPath,
            reason: opts.decisionReason,
          });
          return r.behavior === 'allow'
            ? { behavior: 'allow' as const, updatedInput: toolInput, toolUseID: opts.toolUseID }
            : { behavior: 'deny' as const, message: r.message ?? 'Denied by FORGE', toolUseID: opts.toolUseID };
        },
      },
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
        ctx.turnComplete({
          status: 'error',
          message: errMessage(e),
          usage: { inputTokens: 0, outputTokens: 0, cacheReadTokens: 0, cacheCreationTokens: 0 },
          permissionDenials: 0,
        });
        return;
      }
    } finally {
      input.close();
    }

    if (aborted) {
      ctx.turnComplete({
        status: 'interrupted',
        message: 'Cancelled by user',
        usage: result?.usage ?? { inputTokens: 0, outputTokens: 0, cacheReadTokens: 0, cacheCreationTokens: 0 },
        permissionDenials: result?.permissionDenials ?? 0,
        providerSessionId: result?.providerSessionId,
      });
    } else if (result) {
      ctx.turnComplete(result);
    } else {
      ctx.turnComplete({
        status: 'error',
        message: 'The provider ended the session without producing a result',
        usage: { inputTokens: 0, outputTokens: 0, cacheReadTokens: 0, cacheCreationTokens: 0 },
        permissionDenials: 0,
      });
    }
  },
};
