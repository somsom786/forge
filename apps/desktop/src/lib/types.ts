// Mirror of the Rust serde types in src-tauri (camelCase). This file is the contract between
// the UI and the FORGE core. Change it only together with the Rust side.

export type MissionState =
  | 'draft' | 'ready' | 'preparing' | 'running' | 'awaiting_approval' | 'awaiting_review'
  | 'approved' | 'merged' | 'rejected' | 'failed' | 'cancelled' | 'interrupted';

export type PermissionProfile = 'default' | 'accept_edits';
export type ProviderId = 'claude' | 'claude-local' | 'codex' | 'fake';
export type Risk = 'low' | 'medium' | 'high';

export interface Project {
  id: string;
  name: string;
  rootPath: string;
  defaultBranch: string;
  createdAt: string;
}

export interface Mission {
  id: string;
  projectId: string;
  title: string;
  objective: string;
  state: MissionState;
  provider: ProviderId;
  model: string | null;
  permissionProfile: PermissionProfile;
  baseRef: string | null;
  baseCommit: string | null;
  branch: string | null;
  worktreePath: string | null;
  providerSessionId: string | null;
  lastError: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface AgentSession {
  id: string;
  missionId: string;
  provider: ProviderId;
  providerSessionId: string | null;
  /** starting | live | ended | lost */
  state: 'starting' | 'live' | 'ended' | 'lost';
  pid: number | null;
  startedAt: string;
  endedAt: string | null;
  endReason: string | null;
  /** Provider-reported figures; costUsdEstimate is an estimate, not a bill. */
  usage: UsageReport | null;
}

export interface UsageReport {
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheCreationTokens: number;
  costUsdEstimate?: number;
  durationMs?: number;
  numTurns?: number;
}

export interface Transition {
  fromState: MissionState;
  toState: MissionState;
  reason: string | null;
  at: string;
}

export type ActivityKind =
  | 'session.started' | 'agent.text' | 'agent.thinking' | 'tool.call' | 'tool.result'
  | 'file.changed' | 'status' | 'provider.rate_limit' | 'provider.retry' | 'error'
  // core-originated
  | 'approval.requested' | 'approval.decided' | 'turn.completed';

export interface ActivityEvent {
  /** Database row id; the stable ordering key for display. */
  id: number;
  missionId: string;
  sessionId: string;
  seq: number;
  kind: ActivityKind;
  payload: Record<string, any>;
  at: string;
}

export type ApprovalState = 'pending' | 'approved' | 'denied' | 'auto_denied' | 'expired';

export interface Approval {
  id: string;
  missionId: string;
  sessionId: string;
  providerRequestId: string;
  tool: string;
  summary: string;
  detail: { tool: string; input: Record<string, any>; source?: string; blockedPath?: string | null; providerReason?: string | null };
  risk: Risk;
  warnings: string[];
  state: ApprovalState;
  decidedBy: string | null;
  decisionNote: string | null;
  createdAt: string;
  decidedAt: string | null;
}

export interface MissionDetail {
  mission: Mission;
  project: Project;
  sessions: AgentSession[];
  transitions: Transition[];
  approvals: Approval[];
  /** True only while an agent process is actually attached. */
  running: boolean;
}

export interface FileStatus {
  path: string;
  origPath: string | null;
  index: string;
  worktree: string;
  untracked: boolean;
  conflicted: boolean;
}

export interface RepoStatus {
  branch: string | null;
  head: string | null;
  upstream: string | null;
  ahead: number;
  behind: number;
  files: FileStatus[];
  staged: number;
  unstaged: number;
  untracked: number;
  conflicted: number;
  dirty: boolean;
}

export interface ChangedFile { path: string; added: number | null; removed: number | null; binary: boolean }
export interface WorktreeSafety { exists: boolean; dirtyFiles: number; unmergedCommits: number }
export interface MissionChanges { files: ChangedFile[]; safety: WorktreeSafety }
export interface MissionDiff { diff: string; truncated: boolean }

export interface Capability { supported: boolean; note?: string }
export interface CapabilityMatrix {
  sessionStart: Capability; resume: Capability; streaming: Capability; interrupt: Capability;
  toolEvents: Capability; fileChanges: Capability; approvals: Capability; usageReporting: Capability;
  modelSelection: Capability; worktreeIntegration: Capability;
}

export interface ProviderInfo {
  id: ProviderId;
  name: string;
  installed: boolean;
  path: string | null;
  version: string | null;
  signedIn: boolean | null;
  /** As reported by the CLI, e.g. "claude.ai" (subscription) or "chatgpt". */
  authMethod: string | null;
  plan: string | null;
  /** Selectable models when the provider can enumerate them (claude-local lists the local Ollama models). */
  models: string[];
  /** False means missions cannot use this provider in this build. */
  adapterAvailable: boolean;
  notes: string[];
  capabilities: CapabilityMatrix;
}

export interface AppInfo {
  version: string;
  home: string;
  gitVersion: string | null;
  nodeFound: boolean;
  engineFound: boolean;
  claudeFound: boolean;
  scriptedAgentEnabled: boolean;
  maxConcurrent: number;
  liveSessions: number;
}

export interface RecoveryReport { interruptedMissions: string[]; lostSessions: number; expiredApprovals: number }

export interface NewMissionArgs {
  projectId: string;
  objective: string;
  title?: string;
  provider: ProviderId;
  model?: string;
  permissionProfile?: PermissionProfile;
}

/** Pushed on the `forge://event` channel. State is always persisted before it is pushed. */
export type UiEvent =
  | { type: 'activity'; event: ActivityEvent }
  | { type: 'delta'; missionId: string; messageId: string | null; text: string }
  | { type: 'mission'; mission: Mission }
  | { type: 'approval'; approval: Approval }
  | { type: 'session'; session: AgentSession }
  | { type: 'verification'; run: VerificationRun }
  | { type: 'notice'; level: 'info' | 'warn' | 'error'; message: string };

// ---- Phase 2: contracts, verification, review, proof -----------------------------------------

export type CheckKind = 'test' | 'build' | 'lint' | 'typecheck' | 'security' | 'custom';

export interface CheckSpec {
  id: string;
  name: string;
  /** Shell command run in the mission worktree. Authored or confirmed by the user, never by the agent. */
  command: string;
  kind: CheckKind;
  timeoutSecs: number;
  /** A required check not passing on the current changes needs a recorded override reason to approve. */
  required: boolean;
}

export interface Criterion {
  id: string;
  text: string;
  /** Ids of checks whose passing result is evidence for this criterion. */
  verifiedBy: string[];
  /** Needs a human verdict in addition to any checks. */
  manual: boolean;
  /** Evaluated automatically against the contract's scope globs. */
  scopeRule: boolean;
}

export interface Contract {
  version: number;
  /** Globs relative to the repo root; a trailing "/" means everything beneath. Empty = unrestricted. */
  inScope: string[];
  outOfScope: string[];
  criteria: Criterion[];
  checks: CheckSpec[];
  securityConstraints: string[];
  reviewExpectations: string | null;
}

export type RunState = 'running' | 'passed' | 'failed' | 'error' | 'timeout' | 'cancelled';

export interface VerificationRun {
  id: string;
  missionId: string;
  checkId: string;
  name: string;
  command: string;
  kind: string;
  state: RunState;
  exitCode: number | null;
  startedAt: string;
  endedAt: string | null;
  durationMs: number | null;
  /** Identity of the changes this ran against. */
  fingerprint: string;
  headCommit: string | null;
  /** The check modified files in the worktree, so its result no longer matches the current changes. */
  mutated: boolean;
  outputTail: string;
  outputTruncated: boolean;
  logPath: string | null;
}

/** Derived from evidence only; an agent claiming "done" changes nothing. */
export type CriterionStatus =
  | { status: 'not_checked' }
  | { status: 'check_pending'; pending: string[] }
  | { status: 'verified_by_checks'; evidence: string[] }
  | { status: 'verified_manually'; note: string | null }
  | { status: 'verified_by_scope_check' }
  | { status: 'failed'; reason: string }
  | { status: 'inconclusive'; reason: string }
  | { status: 'requires_manual_review'; reason: string };

export interface ManualVerdict {
  criterionId: string;
  pass: boolean;
  note: string | null;
  fingerprint: string;
  decidedBy: string;
  at: string;
}

export interface CriterionReport {
  id: string;
  text: string;
  status: CriterionStatus;
  manual: boolean;
  scopeRule: boolean;
  verifiedBy: string[];
  manualVerdict: ManualVerdict | null;
}

export interface CheckReport {
  spec: CheckSpec;
  latest: VerificationRun | null;
  /** The latest run is about the changes as they are now. */
  fresh: boolean;
  runs: number;
}

export interface ScopeReport { outsideInScope: string[]; inOutOfScope: string[] }

export interface VerificationReport {
  fingerprint: string;
  headCommit: string | null;
  files: ChangedFile[];
  scope: ScopeReport;
  scopeConfigured: boolean;
  criteria: CriterionReport[];
  checks: CheckReport[];
  /** Required checks not passing on the current changes. */
  blockingChecks: string[];
  /** Everything not established, in plain words. If non-empty, approving needs an override reason. */
  unresolved: string[];
  /** Cannot be overridden: agent running, unanswered permission requests, a check in flight. */
  hardBlockers: string[];
  contract: Contract;
}

export type ReviewDecisionKind = 'approve' | 'reject' | 'request_changes';

export interface MergeOutcome {
  commit: string;
  branch: string;
  target: string;
  squashed: boolean;
  cleanedUp: boolean;
  notes: string[];
}

export interface ProofPack {
  id: string;
  missionId: string;
  createdAt: string;
  contentSha256: string;
  markdown: string;
  /** Local path of the exact diff the pack describes. */
  diffPath: string | null;
  content: Record<string, any>;
}
