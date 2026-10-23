// Typed wrappers over the Tauri command surface. Errors from the core arrive as plain strings.

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type {
  ActivityEvent, AppInfo, Approval, Mission, MissionChanges, MissionDetail, MissionDiff, NewMissionArgs,
  Project, ProviderInfo, RecoveryReport, RepoStatus, UiEvent,
  ContextPackage, ContextPackageRow, KnowledgePage, KnowledgePageDetail, KnowledgeRevision, KnowledgeStatus, LintIssue, NewKnowledgeArgs, ScanResult,
  AcpAgent, McpAuthorization, McpAuthorizationStatus, McpPreview,
  DoctorFinding, HarnessApplyArgs, HarnessBackupUsage, HarnessChange, HarnessFile, HarnessInventory, HarnessPreview, HarnessPruneReport, HookDryRun, HookSpec, HookTemplate, RollbackPreview,
  CheckSpec, Contract, MergeOutcome, Seen, ProofPack, ReviewDecisionKind, VerificationReport, VerificationRun, WorktreeSafety,
  Experiment, ExperimentDetail, NewExperiment,
  ConflictReport,
  ExecutionBackendId, ExecutionBackendInfo,
  AgentsOverview,
  CodeMap, SymbolHit,
} from './types';

export const api = {
  appInfo: () => invoke<AppInfo>('app_info'),
  recoveryReport: () => invoke<RecoveryReport>('recovery_report'),
  providersDetect: () => invoke<ProviderInfo[]>('providers_detect'),

  projectAdd: (path: string) => invoke<Project>('project_add', { path }),
  projectList: () => invoke<Project[]>('project_list'),
  projectRemove: (projectId: string) => invoke<void>('project_remove', { projectId }),
  projectStatus: (projectId: string) => invoke<RepoStatus>('project_status', { projectId }),

  missionCreate: (args: NewMissionArgs) => invoke<Mission>('mission_create', { args }),
  missionStart: (missionId: string) => invoke<Mission>('mission_start', { missionId }),
  missionFollowup: (missionId: string, text: string) => invoke<Mission>('mission_followup', { missionId, text }),
  missionStop: (missionId: string) => invoke<void>('mission_stop', { missionId }),
  missionDiscard: (missionId: string, confirmLoss: boolean) => invoke<Mission>('mission_discard', { missionId, confirmLoss }),
  missionList: (projectId?: string) => invoke<Mission[]>('mission_list', { projectId: projectId ?? null }),
  missionGet: (missionId: string) => invoke<MissionDetail>('mission_get', { missionId }),
  missionEvents: (missionId: string, afterId?: number, limit?: number) =>
    invoke<ActivityEvent[]>('mission_events', { missionId, afterId: afterId ?? null, limit: limit ?? null }),
  missionChanges: (missionId: string) => invoke<MissionChanges>('mission_changes', { missionId }),
  missionDiff: (missionId: string) => invoke<MissionDiff>('mission_diff', { missionId }),
  openWorktreeInEditor: (missionId: string) => invoke<void>('open_worktree_in_editor', { missionId }),

  contractGet: (missionId: string) => invoke<Contract>('contract_get', { missionId }),
  contractSet: (missionId: string, contract: Contract) => invoke<Mission>('contract_set', { missionId, contract }),
  checksSuggest: (projectId: string) => invoke<CheckSpec[]>('checks_suggest', { projectId }),
  verificationReport: (missionId: string) => invoke<VerificationReport>('verification_report', { missionId }),
  verificationRuns: (missionId: string) => invoke<VerificationRun[]>('verification_runs', { missionId }),
  checkStart: (missionId: string, checkId: string) => invoke<VerificationRun>('check_start', { missionId, checkId }),
  checkCancel: (missionId: string) => invoke<void>('check_cancel', { missionId }),
  /** `expectedFingerprint` = the report's `fingerprint` the reviewer was looking at; refused if the changes moved. */
  verdictSet: (missionId: string, criterionId: string, pass: boolean, expectedFingerprint: string, note?: string) =>
    invoke<void>('verdict_set', { missionId, criterionId, pass, note: note ?? null, expectedFingerprint }),
  /** Approving REQUIRES `seen` (the report's fingerprint + contractSha256 the reviewer looked at); the core refuses if either moved. */
  reviewSubmit: (missionId: string, decision: ReviewDecisionKind, note?: string, overrideReason?: string, seen?: Seen) =>
    invoke<Mission>('review_submit', { missionId, decision, note: note ?? null, overrideReason: overrideReason ?? null, seen: seen ?? null }),
  missionMerge: (missionId: string, squash: boolean) => invoke<MergeOutcome>('mission_merge', { missionId, squash }),
  missionCleanup: (missionId: string, confirmLoss: boolean) => invoke<WorktreeSafety>('mission_cleanup', { missionId, confirmLoss }),
  proofpackGenerate: (missionId: string) => invoke<ProofPack>('proofpack_generate', { missionId }),
  proofpackList: (missionId: string) => invoke<ProofPack[]>('proofpack_list', { missionId }),

  knowledgeStatus: (projectId: string) => invoke<KnowledgeStatus>('knowledge_status', { projectId }),
  knowledgeScan: (projectId: string) => invoke<ScanResult>('knowledge_scan', { projectId }),
  knowledgeRebuildIndex: (projectId: string) => invoke<number>('knowledge_rebuild_index', { projectId }),
  knowledgeLint: (projectId: string) => invoke<LintIssue[]>('knowledge_lint', { projectId }),
  /** Connect (path) or disconnect (null) an Obsidian vault folder. Existing vaults are read-only until `write` is granted. */
  knowledgeVaultSet: (projectId: string, path: string | null, write: boolean) => invoke<KnowledgeStatus>('knowledge_vault_set', { projectId, path, write }),
  knowledgeList: (projectId: string, query?: string) => invoke<KnowledgePage[]>('knowledge_list', { projectId, query: query ?? null }),
  knowledgeGet: (pageId: string) => invoke<KnowledgePageDetail>('knowledge_get', { pageId }),
  knowledgeCreate: (projectId: string, args: NewKnowledgeArgs) => invoke<KnowledgePage>('knowledge_create', { projectId, args }),
  /** Save an agent message (activity event id) as a PROPOSAL sourced to that exact event. */
  knowledgeSaveFromEvent: (missionId: string, eventId: number, title: string, kind: string) =>
    invoke<KnowledgePage>('knowledge_save_from_event', { missionId, eventId, title, kind }),
  knowledgeProposeRevision: (pageId: string, text: string, note?: string) => invoke<KnowledgeRevision>('knowledge_propose_revision', { pageId, text, note: note ?? null }),
  knowledgeApplyRevision: (revisionId: string) => invoke<KnowledgePage>('knowledge_apply_revision', { revisionId }),
  knowledgeRejectRevision: (revisionId: string) => invoke<void>('knowledge_reject_revision', { revisionId }),
  /** The only way a page becomes verified. `expectedSha256` must be the page's `currentSha256` the reviewer was looking at. */
  knowledgeVerify: (pageId: string, expectedSha256: string, note?: string) => invoke<KnowledgePage>('knowledge_verify', { pageId, expectedSha256, note: note ?? null }),
  knowledgeSetStatus: (pageId: string, action: 'dispute' | 'archive' | 'restore', note?: string) => invoke<KnowledgePage>('knowledge_set_status', { pageId, action, note: note ?? null }),
  knowledgeSupersede: (oldId: string, newId: string, note?: string) => invoke<KnowledgePage>('knowledge_supersede', { oldId, newId, note: note ?? null }),
  /** Deletes the page file (when writable), its index rows and the text it left in past context snapshots. */
  knowledgeForget: (pageId: string) => invoke<void>('knowledge_forget', { pageId }),
  knowledgeOpenInObsidian: (pageId: string) => invoke<void>('knowledge_open_in_obsidian', { pageId }),
  contextPreview: (projectId: string, objective: string, hints: string[], pins: string[]) => invoke<ContextPackage>('context_preview', { projectId, objective, hints, pins }),
  contextForMission: (missionId: string) => invoke<ContextPackageRow[]>('context_for_mission', { missionId }),
  missionMemorySet: (missionId: string, useMemory: boolean, pins: string[]) => invoke<Mission>('mission_memory_set', { missionId, useMemory, pins }),

  // ---- Harness Studio (Phase 4). Every write is previewed, compare-and-swap, journaled and backed up by the core. ----
  harnessInventory: (projectId: string) => invoke<HarnessInventory>('harness_inventory', { projectId }),
  harnessDoctor: (projectId: string) => invoke<DoctorFinding[]>('harness_doctor', { projectId }),
  harnessRead: (projectId: string, rel: string) => invoke<HarnessFile>('harness_read', { projectId, rel }),
  harnessPreview: (projectId: string, rel: string, text: string) => invoke<HarnessPreview>('harness_preview', { projectId, rel, text }),
  /** Save `preview.text` (not the raw editor text) with `expectedSha256 = file.sha256` and `expectedAfterSha256 = preview.afterSha256`. */
  harnessApply: (args: HarnessApplyArgs) => invoke<HarnessChange>('harness_apply', { args }),
  harnessHistory: (projectId: string, rel?: string) => invoke<HarnessChange[]>('harness_history', { projectId, rel: rel ?? null }),
  harnessRollbackPreview: (changeId: string) => invoke<RollbackPreview>('harness_rollback_preview', { changeId }),
  harnessRollback: (changeId: string) => invoke<HarnessChange>('harness_rollback', { changeId }),
  harnessMarkReviewed: (projectId: string, rel: string) => invoke<void>('harness_mark_reviewed', { projectId, rel }),
  harnessBackupUsage: (projectId: string, keepPerFile?: number) => invoke<HarnessBackupUsage>('harness_backup_usage', { projectId, keepPerFile: keepPerFile ?? null }),
  /** Removes backups that are not protected; a pruned change can no longer be rolled back. */
  harnessPruneBackups: (projectId: string, keepPerFile: number) => invoke<HarnessPruneReport>('harness_prune_backups', { projectId, keepPerFile }),
  harnessHookTemplates: () => invoke<HookTemplate[]>('harness_hook_templates'),
  harnessHookPreviewAdd: (projectId: string, spec: HookSpec) => invoke<HarnessPreview>('harness_hook_preview_add', { projectId, spec: { event: '', ...spec } }),
  harnessHookPreviewRemove: (projectId: string, hookId: string) => invoke<HarnessPreview>('harness_hook_preview_remove', { projectId, hookId }),
  /** Executes the command on this machine with a synthetic event. Only ever call after the user confirmed the exact command. */
  harnessHookDryRun: (projectId: string, event: string, command: string) => invoke<HookDryRun>('harness_hook_dry_run', { projectId, event, command }),

  // ---- Experiments (Phase 6). Running executes the user's command against repository code; explicit user action only. ----
  experimentCreate: (projectId: string, spec: NewExperiment) => invoke<Experiment>('experiment_create', { projectId, spec }),
  experimentList: (projectId: string) => invoke<Experiment[]>('experiment_list', { projectId }),
  experimentGet: (experimentId: string) => invoke<ExperimentDetail>('experiment_get', { experimentId }),
  /** Resolves when the attempt ends (it can take minutes). */
  experimentRun: (experimentId: string) => invoke<ExperimentDetail>('experiment_run', { experimentId }),
  experimentCancel: (experimentId: string) => invoke<void>('experiment_cancel', { experimentId }),
  /** Read-only: overlap, predicted textual conflicts and base drift across the project's active missions. */
  agentsOverview: (days: number) => invoke<AgentsOverview>('agents_overview', { days }),
  missionConflicts: (projectId: string) => invoke<ConflictReport>('mission_conflicts', { projectId }),

  // ---- MCP authorization: nothing a repository declares runs until the user authorizes that exact definition. ----
  mcpPreview: (projectId: string, name: string) => invoke<McpPreview>('mcp_preview', { projectId, name }),
  /** `definitionSha256` from the preview; refused if `.mcp.json` changed since. */
  mcpAuthorize: (projectId: string, name: string, definitionSha256: string, toolAllowlist: string[]) =>
    invoke<McpAuthorization>('mcp_authorize', { projectId, name, definitionSha256, toolAllowlist }),
  mcpRevoke: (authorizationId: string) => invoke<McpAuthorization>('mcp_revoke', { authorizationId }),
  mcpAuthorizations: (projectId: string) => invoke<McpAuthorizationStatus[]>('mcp_authorizations', { projectId }),

  acpAgents: () => invoke<AcpAgent[]>('acp_agents'),
  /** Registers a program FORGE will start for ACP missions; the command is resolved to an absolute path. */
  acpAgentRegister: (name: string, command: string, args: string[]) => invoke<AcpAgent>('acp_agent_register', { name, command, args }),
  acpAgentRemove: (id: string) => invoke<AcpAgent>('acp_agent_remove', { id }),

  executionBackends: () => invoke<ExecutionBackendInfo[]>('execution_backends'),
  /** Refused for a backend that cannot run on this machine; never downgraded. */
  projectSetExecutionBackend: (projectId: string, backend: ExecutionBackendId) => invoke<Project>('project_set_execution_backend', { projectId, backend }),

  /** Built on demand from the main checkout (tree-sitter, parse only); cached per HEAD + working-tree state. */
  codeMap: (projectId: string) => invoke<CodeMap>('code_map', { projectId }),
  /** Symbols by name; with `file` and an empty query, every symbol of that file in line order. */
  codeSymbols: (projectId: string, query: string, file: string | null, limit: number) => invoke<SymbolHit[]>('code_symbols', { projectId, query, file, limit }),

  approvalsPending: (missionId?: string) => invoke<Approval[]>('approvals_pending', { missionId: missionId ?? null }),
  /** `binding` is the card's `bindingSha256`: the core refuses the decision if the request is not that one. */
  approvalDecide: (approvalId: string, approved: boolean, note?: string, binding?: string | null) =>
    invoke<Approval>('approval_decide', { approvalId, approved, note: note ?? null, binding: binding ?? null }),
};

export function subscribe(handler: (e: UiEvent) => void): Promise<UnlistenFn> {
  return listen<UiEvent>('forge://event', (e) => handler(e.payload));
}
