import { describe, expect, it, vi } from 'vitest';
import { buildQueryOptions, LOCKDOWN } from '../src/adapters/claude.js';
import type { StartCommand } from '../src/protocol.js';

const start: StartCommand = {
  t: 'start', sessionId: 's', provider: 'claude', cwd: '/w', prompt: 'p', permissionMode: 'default', claudePath: '/bin/claude',
};

function opts(decide = vi.fn(async () => ({ behavior: 'allow' as const })), s: Partial<StartCommand> = {}, env: NodeJS.ProcessEnv = { PATH: 'x' }) {
  return { o: buildQueryOptions({ ...start, ...s }, { abort: new AbortController(), decide, stderr: () => undefined, env }), decide };
}

const hookOf = (o: ReturnType<typeof opts>['o']) => o.hooks!.PreToolUse![0]!.hooks[0]!;
const preToolUse = (name = 'Bash', input: Record<string, unknown> = { command: 'ls' }) =>
  ({ hook_event_name: 'PreToolUse', tool_name: name, tool_input: input, tool_use_id: 'tu1' }) as never;
const sig = { signal: new AbortController().signal };

describe('repo-defined execution surfaces are locked down', () => {
  it('always applies the lockdown and still loads project instructions', () => {
    const { o } = opts();
    expect(o.managedSettings).toEqual({ allowManagedHooksOnly: true });
    expect(o.strictMcpConfig).toBe(true);
    expect(o.mcpServers).toEqual({});
    expect(o.settingSources).toEqual(['project']); // CLAUDE.md etc. still load; user settings do not
    expect(LOCKDOWN.managedSettings.allowManagedHooksOnly).toBe(true);
  });

  it('never enables bypass or dangerous permission modes', () => {
    for (const mode of ['default', 'acceptEdits'] as const) {
      const { o } = opts(undefined, { permissionMode: mode });
      expect(o.permissionMode).toBe(mode);
      expect(JSON.stringify(o)).not.toMatch(/bypassPermissions|allowDangerouslySkipPermissions/);
    }
  });

  it('strips API-billing credentials from the provider environment', () => {
    const { o } = opts(undefined, {}, { PATH: 'x', ANTHROPIC_API_KEY: 'sk-ant-1', CLAUDE_CODE_USE_BEDROCK: '1' });
    expect(o.env!.ANTHROPIC_API_KEY).toBeUndefined();
    expect(o.env!.CLAUDE_CODE_USE_BEDROCK).toBeUndefined();
    expect(o.env!.PATH).toBe('x');
  });
});

describe('PreToolUse hook (the enforcement point)', () => {
  it('forwards the decision exactly and never rewrites the tool input', async () => {
    const decide = vi.fn(async () => ({ behavior: 'allow' as const }));
    const { o } = opts(decide);
    const out = (await hookOf(o)(preToolUse(), 'tu1', sig)) as any;
    expect(out.hookSpecificOutput.permissionDecision).toBe('allow');
    expect(out.hookSpecificOutput.updatedInput).toBeUndefined();
    expect(decide).toHaveBeenCalledWith('Bash', { command: 'ls' }, 'tu1', 'hook', { signal: sig.signal });
  });

  it('turns a denial into a deny with the reason', async () => {
    const { o } = opts(vi.fn(async () => ({ behavior: 'deny' as const, message: 'nope' })));
    const out = (await hookOf(o)(preToolUse(), 'tu1', sig)) as any;
    expect(out.hookSpecificOutput).toMatchObject({ permissionDecision: 'deny', permissionDecisionReason: 'nope' });
  });

  it('FAILS CLOSED when the decision path throws', async () => {
    const { o } = opts(vi.fn(async () => { throw new Error('core unreachable'); }));
    const out = (await hookOf(o)(preToolUse(), 'tu1', sig)) as any;
    expect(out.hookSpecificOutput.permissionDecision).toBe('deny');
    expect(out.hookSpecificOutput.permissionDecisionReason).toMatch(/core unreachable/);
  });

  it('ignores other hook events', async () => {
    const { o, decide } = opts();
    expect(await hookOf(o)({ hook_event_name: 'SessionStart' } as never, undefined, sig)).toEqual({});
    expect(decide).not.toHaveBeenCalled();
  });

  it('does not time out while a human is deciding', () => {
    const { o } = opts();
    expect(o.hooks!.PreToolUse![0]!.timeout).toBeGreaterThanOrEqual(3600);
  });
});

describe('canUseTool fallback', () => {
  const run = (o: ReturnType<typeof opts>['o']) =>
    o.canUseTool!('Edit', { file_path: 'a' }, { signal: new AbortController().signal, toolUseID: 'tu9' } as never);

  it('allows only what the core allows, echoing the original input', async () => {
    const { o } = opts();
    expect(await run(o)).toEqual({ behavior: 'allow', updatedInput: { file_path: 'a' }, toolUseID: 'tu9' });
  });

  it('denies on error', async () => {
    const { o } = opts(vi.fn(async () => { throw new Error('boom'); }));
    expect(await run(o)).toMatchObject({ behavior: 'deny', toolUseID: 'tu9' });
  });
});

describe('local model endpoint (claude-local)', () => {
  const local = { endpoint: 'http://127.0.0.1:11434', model: 'qwen3.5:9b' };
  const base = { PATH: 'x', ANTHROPIC_API_KEY: 'sk-real-key', ANTHROPIC_AUTH_TOKEN: 'real-token' };

  it('points every model alias and the base url at the local server and drops real credentials', async () => {
    const { sanitizedEnv } = await import('../src/adapters/claude.js');
    const e = sanitizedEnv(base, false, local);
    expect(e.ANTHROPIC_BASE_URL).toBe('http://127.0.0.1:11434');
    expect(e.ANTHROPIC_AUTH_TOKEN).toBe('ollama');
    expect(e.ANTHROPIC_API_KEY).toBeUndefined();
    for (const k of ['ANTHROPIC_MODEL', 'ANTHROPIC_DEFAULT_OPUS_MODEL', 'ANTHROPIC_DEFAULT_SONNET_MODEL', 'ANTHROPIC_DEFAULT_HAIKU_MODEL', 'ANTHROPIC_SMALL_FAST_MODEL', 'CLAUDE_CODE_SUBAGENT_MODEL']) {
      expect(e[k]).toBe('qwen3.5:9b');
    }
    expect(e.CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC).toBe('1');
  });

  it('strips real credentials even if API billing was (mis)configured as allowed', async () => {
    const { sanitizedEnv } = await import('../src/adapters/claude.js');
    expect(sanitizedEnv(base, true, local).ANTHROPIC_API_KEY).toBeUndefined();
  });

  it('refuses anything that is not plain-http loopback', async () => {
    const { assertLoopbackEndpoint, sanitizedEnv } = await import('../src/adapters/claude.js');
    for (const bad of ['https://api.anthropic.com', 'http://example.com:11434', 'http://192.168.1.5:11434', 'http://127.0.0.1.evil.com', 'ftp://localhost', 'not a url', 'http://0.0.0.0:11434']) {
      expect(() => assertLoopbackEndpoint(bad), bad).toThrow();
      expect(() => sanitizedEnv(base, false, { endpoint: bad, model: 'm' }), bad).toThrow();
    }
    for (const ok of ['http://127.0.0.1:11434', 'http://localhost:11434', 'http://[::1]:11434']) {
      expect(() => assertLoopbackEndpoint(ok), ok).not.toThrow();
    }
  });

  it('flows through buildQueryOptions', () => {
    const { o } = opts(undefined, { provider: 'claude-local', localEndpoint: local.endpoint, model: local.model });
    expect(o.env!.ANTHROPIC_BASE_URL).toBe(local.endpoint);
    expect(o.model).toBe('qwen3.5:9b');
  });
});
