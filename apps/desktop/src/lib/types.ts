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
  /** Attach verified project knowledge to the first prompt (default true). */
  useMemory: boolean;
  /** Page ids the user explicitly pinned, even if unverified. */
  memoryPins: string[];
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

// ---- Phase 3: knowledge (memory) -----------------------------------------------------------------
//
// TRUST MODEL (enforced in Rust, tested): authority lives in FORGE's review records, never in page files.
//  * `status` is what a person last decided; `effectiveState` is what is true NOW: a page edited after verification is
//    `proposed` again (`modifiedSinceVerified`). Only `effectiveState === 'verified'` pages are injected automatically.
//  * Nothing written inside a page (frontmatter `state:`, `author:`) can grant trust.
//  * Verification is bound to the exact content hash (`currentSha256`) the reviewer saw.

export type PageStatus = 'proposed' | 'verified' | 'disputed' | 'superseded' | 'archived';
export type PageKind = 'architecture' | 'component' | 'decision' | 'convention' | 'incident' | 'failure_mode' | 'solution' | 'debt' | 'experiment' | 'mission_record' | 'note';

export interface KnowledgePage {
  id: string;
  projectId: string;
  /** `managed` = FORGE-owned folder; `vault` = a connected Obsidian vault folder. */
  rootKind: 'managed' | 'vault';
  relPath: string;
  slug: string;
  title: string;
  kind: PageKind | string;
  scope: 'project' | 'global';
  /** How the page entered FORGE: who wrote it. */
  author: 'user' | 'agent' | 'system';
  origin: 'forge' | 'imported';
  /** What a human last decided. */
  status: PageStatus;
  /** What is true right now. */
  effectiveState: PageStatus;
  modifiedSinceVerified: boolean;
  verifiedSha256: string | null;
  verifiedAt: string | null;
  verifiedNote: string | null;
  /** Hash of the page file as last read; pass this to knowledgeVerify as `expectedSha256`. */
  currentSha256: string;
  topic: string | null;
  supersedesId: string | null;
  supersededById: string | null;
  files: string[];
  tags: string[];
  warnings: string[];
  missing: boolean;
  createdAt: string;
  updatedAt: string;
  indexedAt: string;
}

export interface KnowledgeLink { kind: 'wikilink' | 'file'; target: string; toPage: string | null; ambiguous: boolean }
export interface KnowledgeSource {
  id: string;
  projectId: string;
  kind: 'repo_file' | 'commit' | 'mission' | 'check_run' | 'user_note';
  locator: string;
  gitBlob: string | null;
  rawSha256: string | null;
  normSha256: string | null;
  snapshot: string | null;
  extractor: string;
  /** Whether the source still matches what was captured. */
  state: 'current' | 'changed' | 'missing' | 'unknown';
  capturedAt: string;
  checkedAt: string | null;
}
export interface KnowledgeReview { action: string; revisionSha256: string | null; reviewer: string; note: string | null; at: string }
export interface KnowledgeRevision {
  id: string;
  pageId: string;
  baseSha256: string;
  /** Full proposed file text (frontmatter + body). */
  text: string;
  author: string;
  note: string | null;
  state: 'proposed' | 'applied' | 'rejected' | 'conflict';
  createdAt: string;
  decidedAt: string | null;
}

export interface KnowledgePageDetail {
  page: KnowledgePage;
  /** The file exactly as on disk. */
  text: string;
  links: KnowledgeLink[];
  backlinks: KnowledgePage[];
  sources: KnowledgeSource[];
  reviews: KnowledgeReview[];
  revisions: KnowledgeRevision[];
  /** False for pages in a read-only vault: revisions can be proposed but not applied. */
  writable: boolean;
}

export interface VaultInfo { path: string; isObsidianVault: boolean; writable: boolean }
export interface KnowledgeStatus {
  managedRoot: string;
  vault: VaultInfo | null;
  obsidianInstalled: boolean;
  /** Counts by effective state. */
  counts: Partial<Record<PageStatus, number>>;
  total: number;
  missing: number;
  unfinishedOperations: number;
}

export interface ScanReport { indexed: number; unchanged: number; removed: number; changedPages: string[]; skipped: string[]; warnings: string[] }
export interface FreshnessReport { checked: number; changed: number; missing: number }
export interface ScanResult { scan: ScanReport; freshness: FreshnessReport }

export interface LintIssue { severity: 'info' | 'warning' | 'problem'; kind: string; pageId: string | null; message: string }

export interface SourceArg { kind: KnowledgeSource['kind']; locator: string; eventSeq?: number }
export interface NewKnowledgeArgs {
  title: string;
  kind: PageKind | string;
  body: string;
  files?: string[];
  tags?: string[];
  topic?: string;
  sources?: SourceArg[];
}

export interface ContextSourceRef { kind: string; locator: string; state: string }
export interface ContextItem {
  pageId: string;
  title: string;
  kind: string;
  /** verified = a person verified this exact revision; pinned_unverified = the user chose to include it anyway. */
  trust: 'verified' | 'pinned_unverified';
  author: string;
  origin: string;
  verifiedAt: string | null;
  verifiedNote: string | null;
  sha256: string;
  excerpt: string;
  truncated: boolean;
  score: number;
  /** Plain-words reasons it was included. */
  reasons: string[];
  sources: ContextSourceRef[];
  /** A source it was written from has changed or disappeared since. */
  stale: boolean;
  files: string[];
}
export interface ContextOmitted { pageId: string; title: string; reason: string }
export interface ContextPackage {
  items: ContextItem[];
  omitted: ContextOmitted[];
  query: string;
  charBudget: number;
  /** Exactly what is appended to the agent's first prompt ('' when nothing qualified). */
  text: string;
  sha256: string;
  pathHints: string[];
}
/** The package a mission actually received (persisted at its first turn). */
export interface ContextPackageRow {
  id: string;
  missionId: string;
  createdAt: string;
  query: string;
  params: { budgetChars?: number; pathHints?: string[] };
  items: ContextItem[];
  omitted: ContextOmitted[];
  injectedText: string;
  injectedSha256: string;
  charBudget: number;
}
