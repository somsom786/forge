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
  | { type: 'proofPack'; missionId: string }
  | { type: 'knowledge'; projectId: string }
  | { type: 'harness'; projectId: string }
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
  /** The latest run is about the changes as they are now, under the check's CURRENT definition, and did not modify files. */
  fresh: boolean;
  runs: number;
  /** Changed files that define or configure this check (its script, package.json, test configs, CI). A pass is weaker evidence when these changed. */
  touchedInputs: string[];
}

/** What the reviewer's screen showed. Approving is only valid for exactly this state. */
export interface Seen {
  fingerprint: string;
  contractSha256: string;
}

export interface ScopeReport { outsideInScope: string[]; inOutOfScope: string[] }

export interface VerificationReport {
  fingerprint: string;
  /** Revision hash of the contract this report was computed for. Pass it back (with `fingerprint`) as `Seen` when approving. */
  contractSha256: string;
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
  /** Hash of exactly `text`. Verify against THIS (not `page.currentSha256`) so what a person reads is what they verify. */
  textSha256: string;
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

// ---- Phase 4: Harness Studio -----------------------------------------------------------------------------------

export type HarnessKind = 'instructions' | 'settings' | 'rule' | 'skill' | 'subagent' | 'command' | 'mcp';

export interface HarnessItem {
  id: string;
  kind: HarnessKind;
  scope: 'project' | 'local' | 'user';
  path: string;
  /** Project-relative path with `/` separators; null for user-level items. */
  rel: string | null;
  name: string;
  description: string | null;
  bytes: number;
  sha256: string;
  /** FORGE may edit this (project scope, inside the allowlist). */
  editable: boolean;
  /** Does a FORGE mission read this file? (settings/hooks/MCP: no.) */
  loadedInMissions: boolean;
  warnings: string[];
  details: Record<string, unknown>;
}

export interface HookDef {
  /** `<settings item id>#<event>#<group>#<index>` */
  id: string;
  event: string;
  matcher: string | null;
  kind: string;
  command: string;
  timeoutSecs: number | null;
  sourceItem: string;
  scope: string;
  knownEvent: boolean;
  runsInForgeMissions: boolean;
  /** A plain command hook FORGE can show completely and dry-run faithfully (false: other hook types, or extra fields like `args`/`shell`). */
  dryRunSupported: boolean;
}

export interface McpServerDef {
  name: string;
  transport: string;
  command: string | null;
  url: string | null;
  sourceItem: string;
  scope: string;
  runsInForgeMissions: boolean;
}

export interface HarnessInventory {
  items: HarnessItem[];
  hooks: HookDef[];
  mcpServers: McpServerDef[];
  notes: string[];
  truncated: boolean;
}

export interface DoctorFinding {
  severity: 'problem' | 'warning' | 'info';
  kind: string;
  subject: string | null;
  message: string;
  suggestion: string | null;
}

export interface HarnessFile {
  rel: string;
  exists: boolean;
  text: string;
  /** sha256 of the exact bytes on disk; null when the file does not exist. Send back as `expectedSha256`. */
  sha256: string | null;
  bytes: number;
  editable: boolean;
}

export interface HookRisk { event: string; command: string; display: string; risks: string[] }

export interface HarnessPreview {
  rel: string;
  exists: boolean;
  beforeSha256: string | null;
  /** Hash of exactly the text that would be written; send back as `expectedAfterSha256`. */
  afterSha256: string;
  unchanged: boolean;
  /** Unified diff, 3 lines of context. */
  diff: string;
  added: number;
  removed: number;
  /** Reasons it cannot be saved. */
  problems: string[];
  warnings: string[];
  /** Hooks this change adds or modifies that run shell commands. Saving needs `acknowledgeRisks`. */
  risks: HookRisk[];
  /** The text that would be written (line endings matched to the existing file). Save THIS text. */
  text: string;
}

export interface HarnessChange {
  id: string;
  projectId: string;
  rel: string;
  kind: 'edit' | 'create' | 'rollback';
  beforeSha256: string | null;
  afterSha256: string | null;
  backupPath: string | null;
  rollsBackId: string | null;
  note: string | null;
  state: 'started' | 'applied' | 'failed';
  createdAt: string;
  finishedAt: string | null;
  /** Id of the later rollback that undid this change. */
  rolledBackBy: string | null;
}

/** Disk used by a project's Harness Studio backups. Protected backups (unresolved changes, the newest `keepPerFile` undo steps
 *  per file, and any change that can be rolled back right now) are never pruned. */
export interface HarnessBackupUsage {
  keepPerFile: number;
  files: number;
  bytes: number;
  protectedFiles: number;
  protectedBytes: number;
  prunableFiles: number;
  prunableBytes: number;
  /** Backup files no journal entry refers to (an interrupted save). */
  orphanFiles: number;
}

export interface HarnessPruneReport {
  removedFiles: number;
  removedBytes: number;
  after: HarnessBackupUsage;
}

export interface HarnessApplyArgs {
  projectId: string;
  rel: string;
  text: string;
  expectedSha256: string | null;
  expectedAfterSha256: string;
  acknowledgeRisks?: boolean;
  note?: string;
}

export interface RollbackPreview {
  rel: string;
  /** Present when the file changed after the edit: rollback is refused, and this says why. */
  conflict: string | null;
  removesFile: boolean;
  diff: string;
  added: number;
  removed: number;
}

export interface HookTemplateParam { key: string; label: string; default: string }
export interface HookTemplate {
  id: string;
  name: string;
  description: string;
  event: string;
  matcher: string | null;
  timeoutSecs: number;
  params: HookTemplateParam[];
  requires: string;
}

export interface HookSpec {
  event?: string;
  matcher?: string | null;
  command?: string;
  timeoutSecs?: number;
  templateId?: string;
  params?: Record<string, string>;
}

export interface HookDryRun {
  shell: string;
  exitCode: number | null;
  timedOut: boolean;
  stdout: string;
  stderr: string;
  /** What Claude Code would do with this outcome, in plain words. */
  interpretation: string;
  input: unknown;
  durationMs: number;
  /** Environment variables withheld because their names look like secrets. */
  scrubbedEnv: string[];
}

// ---- Experiments (Phase 6; backend only, no screen yet) ----

export type ExperimentMetric =
  /** Wall-clock duration of the command (ms); lower is better. */
  | { kind: 'duration' }
  /** A number the command prints as `FORGE_METRIC <name>=<number>` (the last such line counts). */
  | { kind: 'output'; name: string; direction: 'lower' | 'higher' };

export interface NewExperiment {
  name: string;
  objective?: string | null;
  command: string;
  /** Must exit 0 in a revision's worktree before its measurements count. */
  correctnessCommand?: string | null;
  metric: ExperimentMetric;
  repetitions: number;
  warmup?: number;
  timeoutSecs: number;
  baselineRef: string;
  /** Each variant is a ref (branch, tag, commit) or a mission whose current work FORGE snapshots when the experiment is created. */
  variants: { label: string; gitRef: string; missionId?: string | null }[];
}

export interface ExperimentStats {
  n: number;
  median: number;
  min: number;
  max: number;
  /** Median absolute deviation. */
  mad: number;
}

export type ExperimentVerdict = 'baseline' | 'better' | 'worse' | 'no_clear_difference' | 'insufficient_runs' | 'invalid';

export interface ExperimentVariantReport {
  variantId: string;
  label: string;
  isBaseline: boolean;
  commit: string;
  stats: ExperimentStats | null;
  failedRuns: number;
  correctness: boolean | null;
  worktreeChanged: boolean;
  /** Relative change of the median against the baseline (-0.27 = 27 % lower). */
  changeVsBaseline: number | null;
  verdict: ExperimentVerdict;
  reasons: string[];
}

export interface ExperimentReport {
  metric: ExperimentMetric;
  direction: 'lower' | 'higher';
  minRunsForVerdict: number;
  variants: ExperimentVariantReport[];
  /** Variant id; only set when a variant beat the baseline in every measured run with enough runs on both sides. */
  winner: string | null;
  conclusion: 'winner' | 'no_winner' | 'inconclusive';
  explanation: string;
}

export interface Experiment {
  id: string;
  projectId: string;
  name: string;
  objective: string | null;
  command: string;
  correctnessCommand: string | null;
  metric: ExperimentMetric;
  repetitions: number;
  warmup: number;
  timeoutSecs: number;
  state: 'draft' | 'running' | 'completed' | 'failed' | 'cancelled' | 'interrupted';
  attempt: number;
  env: Record<string, unknown> | null;
  report: ExperimentReport | null;
  lastError: string | null;
  createdAt: string;
  startedAt: string | null;
  finishedAt: string | null;
}

export interface ExperimentVariant {
  id: string;
  experimentId: string;
  position: number;
  label: string;
  isBaseline: boolean;
  gitRef: string;
  /** The exact commit measured by every attempt. */
  commitSha: string;
}

export interface ExperimentResult {
  id: string;
  experimentId: string;
  variantId: string;
  attempt: number;
  phase: 'correctness' | 'warmup' | 'measure';
  iteration: number;
  state: 'passed' | 'failed' | 'error' | 'timeout' | 'cancelled' | 'no_metric';
  exitCode: number | null;
  durationMs: number;
  metricValue: number | null;
  worktreeChanged: boolean;
  outputTail: string;
  logPath: string | null;
  startedAt: string;
}

export interface ExperimentDetail {
  experiment: Experiment;
  variants: ExperimentVariant[];
  /** Runs of the latest attempt. */
  results: ExperimentResult[];
}

// ---- Conflicts between missions (Phase 5; backend only, no screen yet) ----

export interface ConflictPrediction {
  outcome: 'clean' | 'conflicts' | 'unknown';
  conflictedFiles: string[];
  /** Why no prediction could be made (outcome 'unknown'). */
  error: string | null;
}

export interface MissionPairConflict {
  a: string;
  b: string;
  /** Paths both missions changed (relative to their own bases). */
  overlappingFiles: string[];
  /** Whether landing both would conflict, predicted with `git merge-tree` on their current states. */
  prediction: ConflictPrediction;
}

export interface MissionBaseDrift {
  missionId: string;
  baseRef: string;
  baseCommit: string;
  /** Where the base branch points now; null if it no longer exists. */
  baseHead: string | null;
  commitsBehind: number;
  overlappingFiles: string[];
  /** Merging the mission into the current base; null when the base has not moved. */
  prediction: ConflictPrediction | null;
}

export interface ConflictReport {
  projectId: string;
  generatedAt: string;
  missions: { missionId: string; title: string; state: string; baseRef: string; baseCommit: string; changedFiles: string[] }[];
  pairs: MissionPairConflict[];
  drift: MissionBaseDrift[];
  skipped: { missionId: string; reason: string }[];
}

// ---- Agents overview ----

export interface AgentSessionRow {
  session: AgentSession;
  missionTitle: string;
  projectId: string;
}

/** Provider-reported figures for sessions FORGE launched; not a bill, and nothing from sessions outside FORGE. */
export interface ProviderUsage {
  provider: string;
  sessions: number;
  live: number;
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  cacheCreationTokens: number;
  turns: number;
  costUsdEstimate: number | null;
  sessionsWithUsage: number;
}

export interface AgentsOverview {
  since: string;
  days: number;
  sessions: AgentSessionRow[];
  usage: ProviderUsage[];
}
