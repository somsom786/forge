// Pure translation of Claude Agent SDK messages into FORGE's provider-neutral activity
// vocabulary. Kept free of I/O so it can be tested against recorded message shapes.
// Input is typed loosely on purpose: the SDK union is large and evolves; we read only
// the fields we need and ignore the rest.

import type { ActivityKind, TurnStatus, UsageReport } from '../protocol.js';
import { MAX_FIELD_BYTES } from '../protocol.js';
import { truncate, truncateDeep } from '../util.js';

export interface Sink {
  event(kind: ActivityKind, payload: Record<string, unknown>): void;
  delta(messageId: string | null, text: string): void;
  providerSession(id: string): void;
}

export interface TurnResult {
  status: TurnStatus;
  message?: string;
  usage: UsageReport;
  permissionDenials: number;
  providerSessionId?: string;
}

const FILE_TOOLS = new Set(['Write', 'Edit', 'MultiEdit', 'NotebookEdit']);

type Json = Record<string, any>; // eslint-disable-line @typescript-eslint/no-explicit-any

export function summarizeTool(name: string, input: Json): string {
  const pick = (k: string) => (typeof input?.[k] === 'string' ? (input[k] as string) : '');
  switch (name) {
    case 'Bash':
    case 'PowerShell':
      return truncate(pick('command'), 300).text;
    case 'Read':
    case 'Write':
    case 'Edit':
    case 'MultiEdit':
      return pick('file_path');
    case 'NotebookEdit':
      return pick('notebook_path');
    case 'Grep':
      return `${pick('pattern')}${pick('path') ? ` in ${pick('path')}` : ''}`;
    case 'Glob':
      return pick('pattern');
    case 'WebFetch':
      return pick('url');
    case 'WebSearch':
      return pick('query');
    default:
      return truncate(JSON.stringify(input ?? {}), 160).text;
  }
}

function flattenToolResult(content: unknown): string {
  if (typeof content === 'string') return content;
  if (Array.isArray(content)) {
    return content
      .map((b) => (b && typeof b === 'object' && 'text' in b ? String((b as Json).text) : `[${(b as Json)?.type ?? 'block'}]`))
      .join('\n');
  }
  return content == null ? '' : JSON.stringify(content);
}

export class ClaudeNormalizer {
  private pending = new Map<string, { name: string; input: Json }>();
  private streamingMessageId: string | null = null;
  private lastSessionId: string | undefined;

  constructor(
    private readonly sink: Sink,
    private readonly override?: { credentialSource: string; endpoint?: string },
  ) {}

  /** Handle one SDK message. Returns a TurnResult when the message is the turn's `result`. */
  handle(msg: Json): TurnResult | null {
    if (!msg || typeof msg !== 'object') return null;
    if (typeof msg.session_id === 'string' && msg.session_id !== this.lastSessionId) {
      this.lastSessionId = msg.session_id;
      this.sink.providerSession(msg.session_id);
    }
    const parent = typeof msg.parent_tool_use_id === 'string' ? msg.parent_tool_use_id : undefined;

    switch (msg.type) {
      case 'system':
        this.handleSystem(msg);
        return null;
      case 'stream_event':
        this.handleStream(msg);
        return null;
      case 'assistant':
        this.handleAssistant(msg, parent);
        return null;
      case 'user':
        this.handleUser(msg, parent);
        return null;
      case 'rate_limit_event': {
        const i = msg.rate_limit_info ?? {};
        this.sink.event('provider.rate_limit', {
          status: i.status,
          rateLimitType: i.rateLimitType,
          utilization: i.utilization,
          resetsAt: i.resetsAt,
          isUsingOverage: i.isUsingOverage,
          overageStatus: i.overageStatus,
        });
        return null;
      }
      case 'result':
        return this.handleResult(msg);
      default:
        return null;
    }
  }

  private handleSystem(msg: Json) {
    if (msg.subtype === 'init') {
      this.sink.event('session.started', {
        providerSessionId: msg.session_id,
        model: msg.model,
        claudeCodeVersion: msg.claude_code_version,
        cwd: msg.cwd,
        permissionMode: msg.permissionMode,
        // 'none' means OAuth / subscription login; anything else indicates API-key billing.
        credentialSource: this.override?.credentialSource ?? msg.apiKeySource,
        localEndpoint: this.override?.endpoint,
        toolCount: Array.isArray(msg.tools) ? msg.tools.length : undefined,
        tools: Array.isArray(msg.tools) ? msg.tools : undefined,
        skills: Array.isArray(msg.skills) ? msg.skills : undefined,
        mcpServers: Array.isArray(msg.mcp_servers) ? msg.mcp_servers.map((s: Json) => ({ name: s.name, status: s.status })) : undefined,
      });
    } else if (msg.subtype === 'api_retry') {
      this.sink.event('provider.retry', {
        attempt: msg.attempt,
        maxRetries: msg.max_retries,
        delayMs: msg.retry_delay_ms,
        status: msg.error_status,
      });
    } else if (msg.subtype === 'compact_boundary') {
      this.sink.event('status', { message: 'Conversation context was compacted by the provider', compaction: true });
    }
  }

  private handleStream(msg: Json) {
    const ev = msg.event;
    if (!ev || msg.parent_tool_use_id) return; // subagent stream is not shown live
    if (ev.type === 'message_start') this.streamingMessageId = ev.message?.id ?? null;
    else if (ev.type === 'content_block_delta' && ev.delta?.type === 'text_delta' && typeof ev.delta.text === 'string') {
      this.sink.delta(this.streamingMessageId, ev.delta.text);
    }
  }

  private handleAssistant(msg: Json, parent: string | undefined) {
    const m = msg.message ?? {};
    const messageId: string | null = m.id ?? null;
    for (const block of Array.isArray(m.content) ? m.content : []) {
      if (block.type === 'text' && block.text) {
        this.sink.event('agent.text', { messageId, text: truncate(String(block.text), 64 * 1024).text, parentToolUseId: parent });
      } else if (block.type === 'thinking' && block.thinking) {
        this.sink.event('agent.thinking', { messageId, text: truncate(String(block.thinking), MAX_FIELD_BYTES).text, parentToolUseId: parent });
      } else if (block.type === 'tool_use') {
        const input: Json = block.input && typeof block.input === 'object' ? block.input : {};
        this.pending.set(block.id, { name: block.name, input });
        this.sink.event('tool.call', {
          toolUseId: block.id,
          name: block.name,
          summary: summarizeTool(block.name, input),
          input: truncateDeep(input, MAX_FIELD_BYTES),
          parentToolUseId: parent,
        });
      }
    }
  }

  private handleUser(msg: Json, parent: string | undefined) {
    const content = msg.message?.content;
    if (!Array.isArray(content)) return;
    for (const block of content) {
      if (block.type !== 'tool_result') continue;
      const id: string = block.tool_use_id;
      const call = this.pending.get(id);
      this.pending.delete(id);
      const ok = !block.is_error;
      const out = truncate(flattenToolResult(block.content), MAX_FIELD_BYTES);
      this.sink.event('tool.result', {
        toolUseId: id,
        name: call?.name,
        ok,
        output: out.text,
        truncated: out.truncated,
        parentToolUseId: parent,
      });
      if (ok && call && FILE_TOOLS.has(call.name)) {
        const path = String(call.input.file_path ?? call.input.notebook_path ?? '');
        if (path) this.sink.event('file.changed', { path, tool: call.name, toolUseId: id });
      }
    }
  }

  private handleResult(msg: Json): TurnResult {
    const u = msg.usage ?? {};
    const usage: UsageReport = {
      inputTokens: Number(u.input_tokens ?? 0),
      outputTokens: Number(u.output_tokens ?? 0),
      cacheReadTokens: Number(u.cache_read_input_tokens ?? 0),
      cacheCreationTokens: Number(u.cache_creation_input_tokens ?? 0),
      // A local model has no price: Claude Code's price-table figure for it would be invented, so never report one.
      costUsdEstimate: !this.override && typeof msg.total_cost_usd === 'number' ? msg.total_cost_usd : undefined,
      durationMs: typeof msg.duration_ms === 'number' ? msg.duration_ms : undefined,
      numTurns: typeof msg.num_turns === 'number' ? msg.num_turns : undefined,
    };
    const ok = msg.subtype === 'success' && !msg.is_error;
    const message = ok
      ? undefined
      : typeof msg.result === 'string' && msg.result
        ? msg.result
        : Array.isArray(msg.errors) && msg.errors.length
          ? msg.errors.join('; ')
          : String(msg.subtype ?? 'error');
    return {
      status: ok ? 'success' : 'error',
      message,
      usage,
      permissionDenials: Array.isArray(msg.permission_denials) ? msg.permission_denials.length : 0,
      providerSessionId: msg.session_id,
    };
  }
}
