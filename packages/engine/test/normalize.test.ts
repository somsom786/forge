import { describe, expect, it } from 'vitest';
import { ClaudeNormalizer, summarizeTool } from '../src/adapters/claudeNormalize.js';
import { sanitizedEnv } from '../src/adapters/claude.js';
import { AsyncQueue, truncate, truncateDeep } from '../src/util.js';

function harness() {
  const events: Array<{ kind: string; payload: Record<string, any> }> = [];
  const deltas: string[] = [];
  const sessions: string[] = [];
  const n = new ClaudeNormalizer({
    event: (kind, payload) => events.push({ kind, payload }),
    delta: (_id, text) => deltas.push(text),
    providerSession: (id) => sessions.push(id),
  });
  return { n, events, deltas, sessions };
}

describe('ClaudeNormalizer', () => {
  it('maps init, text, tool call/result and derives file.changed only from successful edits', () => {
    const h = harness();
    h.n.handle({ type: 'system', subtype: 'init', session_id: 's1', model: 'm', claude_code_version: '2.1', cwd: '/w', permissionMode: 'default', apiKeySource: 'none', tools: ['Read', 'Edit'], mcp_servers: [] });
    h.n.handle({
      type: 'assistant', session_id: 's1', parent_tool_use_id: null,
      message: { id: 'msg1', content: [{ type: 'text', text: 'Let me edit.' }, { type: 'tool_use', id: 't1', name: 'Edit', input: { file_path: '/w/a.ts', old_string: 'a', new_string: 'b' } }, { type: 'tool_use', id: 't2', name: 'Edit', input: { file_path: '/w/b.ts' } }] },
    });
    h.n.handle({ type: 'user', session_id: 's1', parent_tool_use_id: null, message: { content: [{ type: 'tool_result', tool_use_id: 't1', content: 'ok' }, { type: 'tool_result', tool_use_id: 't2', content: 'denied', is_error: true }] } });

    const kinds = h.events.map((e) => e.kind);
    expect(kinds).toEqual(['session.started', 'agent.text', 'tool.call', 'tool.call', 'tool.result', 'file.changed', 'tool.result']);
    expect(h.events[0]!.payload.credentialSource).toBe('none');
    const changed = h.events.filter((e) => e.kind === 'file.changed');
    expect(changed).toHaveLength(1);
    expect(changed[0]!.payload.path).toBe('/w/a.ts');
    expect(h.sessions).toEqual(['s1']);
  });

  it('streams text deltas only for the main agent', () => {
    const h = harness();
    h.n.handle({ type: 'stream_event', session_id: 's', parent_tool_use_id: null, event: { type: 'message_start', message: { id: 'm' } } });
    h.n.handle({ type: 'stream_event', session_id: 's', parent_tool_use_id: null, event: { type: 'content_block_delta', delta: { type: 'text_delta', text: 'Hel' } } });
    h.n.handle({ type: 'stream_event', session_id: 's', parent_tool_use_id: 'sub', event: { type: 'content_block_delta', delta: { type: 'text_delta', text: 'SUBAGENT' } } });
    h.n.handle({ type: 'stream_event', session_id: 's', parent_tool_use_id: null, event: { type: 'content_block_delta', delta: { type: 'input_json_delta', partial_json: '{' } } });
    expect(h.deltas).toEqual(['Hel']);
  });

  it('turns a success result into usage with estimates labelled as such', () => {
    const h = harness();
    const r = h.n.handle({ type: 'result', subtype: 'success', is_error: false, session_id: 's', result: 'done', total_cost_usd: 0.0123, duration_ms: 4200, num_turns: 3, usage: { input_tokens: 10, output_tokens: 20, cache_read_input_tokens: 5, cache_creation_input_tokens: 2 }, permission_denials: [{}] });
    expect(r).toMatchObject({ status: 'success', permissionDenials: 1, providerSessionId: 's' });
    expect(r!.usage).toEqual({ inputTokens: 10, outputTokens: 20, cacheReadTokens: 5, cacheCreationTokens: 2, costUsdEstimate: 0.0123, durationMs: 4200, numTurns: 3 });
  });

  it('never reports a cost for a local-model session', () => {
    const events: any[] = [];
    const n = new ClaudeNormalizer({ event: (k, p) => events.push({ k, p }), delta: () => undefined, providerSession: () => undefined }, { credentialSource: 'local-endpoint', endpoint: 'http://127.0.0.1:11434' });
    n.handle({ type: 'system', subtype: 'init', session_id: 's', apiKeySource: 'ANTHROPIC_AUTH_TOKEN', tools: [] });
    const r = n.handle({ type: 'result', subtype: 'success', is_error: false, total_cost_usd: 0.0772, usage: { input_tokens: 5, output_tokens: 6 } });
    expect(r!.usage.costUsdEstimate).toBeUndefined();
    expect(r!.usage.inputTokens).toBe(5);
    expect(events[0].p.credentialSource).toBe('local-endpoint');
  });

  it('treats error subtypes and is_error successes as errors with a message', () => {
    const h = harness();
    expect(h.n.handle({ type: 'result', subtype: 'error_max_turns', is_error: true, errors: ['too many turns'], usage: {} })).toMatchObject({ status: 'error', message: 'too many turns' });
    expect(h.n.handle({ type: 'result', subtype: 'success', is_error: true, result: 'API Error: 529', usage: {} })).toMatchObject({ status: 'error', message: 'API Error: 529' });
  });

  it('reports provider rate-limit utilisation without inventing fields', () => {
    const h = harness();
    h.n.handle({ type: 'rate_limit_event', session_id: 's', rate_limit_info: { status: 'allowed_warning', rateLimitType: 'five_hour', utilization: 0.83, resetsAt: 1790000000 } });
    expect(h.events[0]).toMatchObject({ kind: 'provider.rate_limit', payload: { status: 'allowed_warning', rateLimitType: 'five_hour', utilization: 0.83 } });
  });

  it('survives malformed and unknown messages', () => {
    const h = harness();
    expect(h.n.handle(null as any)).toBeNull();
    expect(h.n.handle({ type: 'who_knows' })).toBeNull();
    expect(h.n.handle({ type: 'assistant', message: { content: 'not an array' } })).toBeNull();
    expect(h.n.handle({ type: 'user', message: { content: [{ type: 'tool_result', tool_use_id: 'unknown', content: [{ type: 'text', text: 'x' }, { type: 'image' }] }] } })).toBeNull();
    expect(h.events.find((e) => e.kind === 'tool.result')!.payload.output).toBe('x\n[image]');
  });

  it('bounds large tool output and inputs', () => {
    const h = harness();
    h.n.handle({ type: 'assistant', message: { id: 'm', content: [{ type: 'tool_use', id: 't', name: 'Write', input: { file_path: '/w/x', content: 'A'.repeat(50_000) } }] } });
    h.n.handle({ type: 'user', message: { content: [{ type: 'tool_result', tool_use_id: 't', content: 'B'.repeat(50_000) }] } });
    const call = h.events.find((e) => e.kind === 'tool.call')!;
    expect(String(call.payload.input.content).length).toBeLessThan(5000);
    const res = h.events.find((e) => e.kind === 'tool.result')!;
    expect(res.payload.truncated).toBe(true);
  });
});

describe('summarizeTool', () => {
  it('gives a one-line human summary', () => {
    expect(summarizeTool('Bash', { command: 'npm test' })).toBe('npm test');
    expect(summarizeTool('Grep', { pattern: 'foo', path: 'src' })).toBe('foo in src');
    expect(summarizeTool('Weird', { a: 1 })).toBe('{"a":1}');
  });
});

describe('sanitizedEnv (no hidden API billing)', () => {
  const base = { PATH: '/bin', ANTHROPIC_API_KEY: 'sk-ant-x', ANTHROPIC_AUTH_TOKEN: 't', CLAUDE_CODE_USE_BEDROCK: '1', HOME: '/h' };
  it('strips billing credentials by default', () => {
    const e = sanitizedEnv(base, false);
    expect(e.ANTHROPIC_API_KEY).toBeUndefined();
    expect(e.ANTHROPIC_AUTH_TOKEN).toBeUndefined();
    expect(e.CLAUDE_CODE_USE_BEDROCK).toBeUndefined();
    expect(e.PATH).toBe('/bin');
    expect(base.ANTHROPIC_API_KEY).toBe('sk-ant-x'); // input untouched
  });
  it('keeps them only when explicitly allowed', () => {
    expect(sanitizedEnv(base, true).ANTHROPIC_API_KEY).toBe('sk-ant-x');
  });
});

describe('util', () => {
  it('truncates on UTF-8 boundaries', () => {
    const t = truncate('é'.repeat(10), 5);
    expect(t.truncated).toBe(true);
    expect(t.text.startsWith('éé')).toBe(true);
    expect(t.text.includes('�')).toBe(false);
  });
  it('truncateDeep bounds nested strings and arrays', () => {
    const v = truncateDeep({ a: ['x'.repeat(100)], b: { c: 'y'.repeat(100) } }, 10);
    expect(v.a[0]!.length).toBeLessThan(60);
  });
  it('AsyncQueue delivers in order and ends on close', async () => {
    const q = new AsyncQueue<number>();
    q.push(1);
    const it = q[Symbol.asyncIterator]();
    expect((await it.next()).value).toBe(1);
    const p = it.next();
    q.push(2);
    expect((await p).value).toBe(2);
    q.close();
    expect((await it.next()).done).toBe(true);
    q.push(3); // ignored after close
    expect((await it.next()).done).toBe(true);
  });
});
