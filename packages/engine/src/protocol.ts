// Wire protocol between the FORGE core (Rust) and one engine process.
// One engine process hosts exactly one agent session. Transport: newline-delimited JSON
// on stdin (commands) / stdout (events). stderr is diagnostics only and is never parsed.

export type ProviderId = 'claude' | 'claude-local' | 'codex' | 'acp' | 'fake';

/** Honest per-provider capability matrix. `false` means the control must not be shown. */
export interface Capability {
  supported: boolean;
  note?: string;
}

export interface CapabilityMatrix {
  sessionStart: Capability;
  resume: Capability;
  streaming: Capability;
  interrupt: Capability;
  toolEvents: Capability;
  fileChanges: Capability;
  approvals: Capability;
  usageReporting: Capability;
  modelSelection: Capability;
  worktreeIntegration: Capability;
}

export type EnginePermissionMode = 'default' | 'acceptEdits';

export interface StartCommand {
  t: 'start';
  sessionId: string;
  provider: ProviderId;
  cwd: string;
  prompt: string;
  model?: string;
  permissionMode: EnginePermissionMode;
  /** Provider session id to resume. */
  resume?: string;
  /** Absolute path to the user's installed Claude Code executable. */
  claudePath?: string;
  /** Absolute path to the user's installed Codex executable (a real binary, not an npm shim). */
  codexPath?: string;
  /** Extra guidance appended to the provider's system prompt (mission contract, memory). */
  appendSystemPrompt?: string;
  maxTurns?: number;
  /** `claude-local`: loopback Anthropic-compatible endpoint (e.g. Ollama). Never a remote URL. */
  localEndpoint?: string;
  /** When false (default) ANTHROPIC_API_KEY & co are stripped so a mission can never silently bill the API. */
  allowApiBilling?: boolean;
  /**
   * MCP servers the user authorized for this project, built by the core from the stored (reviewed) definitions. The
   * repository's own `.mcp.json` is never read by the provider (`strictMcpConfig`). Absent or `{}`: no MCP servers.
   */
  mcpServers?: Record<string, McpServerConfig>;
  /** `acp`: the registered ACP agent (absolute command path pinned when the user registered it). */
  acp?: { name: string; command: string; args?: string[] };
}

/** The SDK's MCP server shapes FORGE produces (stdio or remote http). Env values are resolved by the core. */
export type McpServerConfig =
  | { type: 'stdio'; command: string; args?: string[]; env?: Record<string, string> }
  | { type: 'http'; url: string };

export interface PermissionResultCommand {
  t: 'permission_result';
  requestId: string;
  behavior: 'allow' | 'deny';
  message?: string;
}

export interface CancelCommand {
  t: 'cancel';
}

export interface CloseCommand {
  t: 'close';
}

export type Command = StartCommand | PermissionResultCommand | CancelCommand | CloseCommand;

export type ActivityKind =
  | 'session.started'
  | 'agent.text'
  | 'agent.thinking'
  | 'tool.call'
  | 'tool.result'
  | 'file.changed'
  | 'status'
  | 'provider.rate_limit'
  | 'provider.retry'
  | 'error';

export interface UsageReport {
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheCreationTokens: number;
  /** Provider-estimated, not a billing statement. */
  costUsdEstimate?: number;
  durationMs?: number;
  numTurns?: number;
}

export type TurnStatus = 'success' | 'error' | 'interrupted';

export type EngineEvent =
  | { t: 'ready'; engine: string; provider: ProviderId; capabilities: CapabilityMatrix; pid: number }
  | { t: 'event'; seq: number; kind: ActivityKind; payload: Record<string, unknown> }
  | { t: 'delta'; messageId: string | null; text: string }
  | { t: 'provider_session'; providerSessionId: string }
  | {
      t: 'permission';
      requestId: string;
      tool: string;
      input: Record<string, unknown>;
      toolUseId: string;
      source: 'hook' | 'can_use_tool';
      blockedPath?: string;
      reason?: string;
    }
  | { t: 'permission_cancelled'; requestId: string }
  | {
      t: 'turn_complete';
      status: TurnStatus;
      message?: string;
      providerSessionId?: string;
      usage?: UsageReport;
      permissionDenials?: number;
    }
  | { t: 'fatal'; message: string };

export const MAX_FIELD_BYTES = 4096;
