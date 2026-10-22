import type { ActivityKind, CapabilityMatrix, ProviderId, StartCommand } from '../protocol.js';
import type { TurnResult } from './claudeNormalize.js';

export interface PermissionRequest {
  tool: string;
  input: Record<string, unknown>;
  toolUseId: string;
  source: 'hook' | 'can_use_tool';
  blockedPath?: string;
  reason?: string;
}

export interface PermissionDecision {
  behavior: 'allow' | 'deny';
  message?: string;
}

/** What an adapter may do. Everything else (policy, persistence, UI) lives in the FORGE core. */
export interface RunContext {
  start: StartCommand;
  /** Aborts when the core sends `cancel` or the core's pipe closes. */
  signal: AbortSignal;
  emit(kind: ActivityKind, payload: Record<string, unknown>): void;
  delta(messageId: string | null, text: string): void;
  providerSession(id: string): void;
  /** Ask the core to decide a tool call (policy first, then a human where required). */
  requestPermission(req: PermissionRequest): Promise<PermissionDecision>;
  turnComplete(result: TurnResult): void;
}

export interface ProviderAdapter {
  readonly id: ProviderId;
  capabilities(): CapabilityMatrix;
  /** Run one turn to completion (or abort). Must not throw for ordinary provider failures: report via turnComplete. */
  run(ctx: RunContext): Promise<void>;
}
