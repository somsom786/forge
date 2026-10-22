// Typed wrappers over the Tauri command surface. Errors from the core arrive as plain strings.

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type {
  ActivityEvent, AppInfo, Approval, Mission, MissionChanges, MissionDetail, MissionDiff, NewMissionArgs,
  Project, ProviderInfo, RecoveryReport, RepoStatus, UiEvent,
  CheckSpec, Contract, MergeOutcome, ProofPack, ReviewDecisionKind, VerificationReport, VerificationRun, WorktreeSafety,
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
  verdictSet: (missionId: string, criterionId: string, pass: boolean, note?: string) =>
    invoke<void>('verdict_set', { missionId, criterionId, pass, note: note ?? null }),
  reviewSubmit: (missionId: string, decision: ReviewDecisionKind, note?: string, overrideReason?: string) =>
    invoke<Mission>('review_submit', { missionId, decision, note: note ?? null, overrideReason: overrideReason ?? null }),
  missionMerge: (missionId: string, squash: boolean) => invoke<MergeOutcome>('mission_merge', { missionId, squash }),
  missionCleanup: (missionId: string, confirmLoss: boolean) => invoke<WorktreeSafety>('mission_cleanup', { missionId, confirmLoss }),
  proofpackGenerate: (missionId: string) => invoke<ProofPack>('proofpack_generate', { missionId }),
  proofpackList: (missionId: string) => invoke<ProofPack[]>('proofpack_list', { missionId }),

  approvalsPending: (missionId?: string) => invoke<Approval[]>('approvals_pending', { missionId: missionId ?? null }),
  approvalDecide: (approvalId: string, approved: boolean, note?: string) =>
    invoke<Approval>('approval_decide', { approvalId, approved, note: note ?? null }),
};

export function subscribe(handler: (e: UiEvent) => void): Promise<UnlistenFn> {
  return listen<UiEvent>('forge://event', (e) => handler(e.payload));
}
