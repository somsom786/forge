// Pure helpers for the Phase 2 verification / review UI.
import type {
  Seen,
  CheckKind, CheckSpec, Contract, Criterion, CriterionStatus, MergeOutcome, VerificationReport, VerificationRun,
} from './types';
import type { Tone } from './format';

export const OVERRIDE_MIN = 8;

export const STATUS_KEYS = [
  'not_checked', 'check_pending', 'verified_by_checks', 'verified_manually', 'verified_by_scope_check', 'failed', 'inconclusive',
  'requires_manual_review',
] as const;

/** Exact wording required by the core's semantics. `names` resolves check ids to display names. */
export function statusLabel(s: CriterionStatus, names: (id: string) => string = (x) => x): string {
  switch (s.status) {
    case 'not_checked':
      return 'Not checked';
    case 'check_pending':
      return 'Check pending';
    case 'verified_by_checks':
      return `Verified by ${s.evidence.length ? s.evidence.map(names).join(', ') : 'checks'}`;
    case 'verified_manually':
      return 'Verified manually';
    case 'verified_by_scope_check':
      return 'Within scope';
    case 'failed':
      return 'Failed';
    case 'inconclusive':
      return 'Inconclusive';
    case 'requires_manual_review':
      return 'Requires manual review';
  }
}

/** Only the three verified_* states are ever positive. */
export function statusTone(s: CriterionStatus): Tone {
  switch (s.status) {
    case 'verified_by_checks':
    case 'verified_manually':
    case 'verified_by_scope_check':
      return 'ok';
    case 'failed':
      return 'bad';
    case 'check_pending':
      return 'run';
    case 'inconclusive':
    case 'requires_manual_review':
      return 'warn';
    default:
      return 'idle';
  }
}

export function statusDetail(s: CriterionStatus): string | null {
  switch (s.status) {
    case 'failed':
    case 'inconclusive':
    case 'requires_manual_review':
      return s.reason;
    case 'check_pending':
      return s.pending.length ? `Waiting on: ${s.pending.join(', ')}` : null;
    case 'verified_manually':
      return s.note;
    default:
      return null;
  }
}

export function slugId(text: string, taken: ReadonlySet<string> = new Set()): string {
  const base =
    text
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-+|-+$/g, '')
      .slice(0, 40)
      .replace(/-+$/g, '') || 'item';
  if (!taken.has(base)) return base;
  for (let i = 2; ; i++) {
    const c = `${base}-${i}`;
    if (!taken.has(c)) return c;
  }
}

export interface ApprovalGate {
  /** Approve button usable at all. */
  canApprove: boolean;
  /** An override reason must be typed. */
  needsOverride: boolean;
  overrideValid: boolean;
  blockers: string[];
}

export function approvalGate(report: Pick<VerificationReport, 'unresolved' | 'hardBlockers'> | null, overrideReason: string): ApprovalGate {
  if (!report) return { canApprove: false, needsOverride: false, overrideValid: false, blockers: ['Verification report not loaded yet.'] };
  const needsOverride = report.unresolved.length > 0;
  const overrideValid = overrideReason.trim().length >= OVERRIDE_MIN;
  const blockers = report.hardBlockers;
  return {
    canApprove: blockers.length === 0 && (!needsOverride || overrideValid),
    needsOverride,
    overrideValid,
    blockers,
  };
}

/** Insert or replace a run (by id), keeping newest-started last. */
export function upsertRun(runs: VerificationRun[] | undefined, run: VerificationRun): VerificationRun[] {
  const base = runs ?? [];
  const i = base.findIndex((r) => r.id === run.id);
  const next = i >= 0 ? base.map((r, j) => (j === i ? run : r)) : [...base, run];
  return next.sort((a, b) => a.startedAt.localeCompare(b.startedAt) || a.id.localeCompare(b.id));
}

export function linesToList(text: string): string[] {
  return text.split(/\r?\n/).map((l) => l.trim()).filter(Boolean);
}

export function listToLines(list: string[]): string {
  return list.join('\n');
}

/** Draft rows keep a local key so ids can be generated from names at save time. */
export function normalizeContract(c: Contract): Contract {
  const takenChecks = new Set<string>();
  const renamed = new Map<string, string>();
  const checks: CheckSpec[] = c.checks
    .filter((k) => k.name.trim() && k.command.trim())
    .map((k) => {
      const temp = !k.id || /^new-check(-\d+)?$/.test(k.id);
      const id = !temp && !takenChecks.has(k.id) ? k.id : slugId(k.name, takenChecks);
      takenChecks.add(id);
      if (id !== k.id) renamed.set(k.id, id);
      return { ...k, id, name: k.name.trim(), command: k.command.trim() };
    });
  const known = new Set(checks.map((k) => k.id));
  const takenCrit = new Set<string>();
  const criteria: Criterion[] = c.criteria
    .filter((k) => k.text.trim())
    .map((k) => {
      const id = k.id && !takenCrit.has(k.id) ? k.id : slugId(k.text, takenCrit);
      takenCrit.add(id);
      return { ...k, id, text: k.text.trim(), verifiedBy: k.verifiedBy.map((v) => renamed.get(v) ?? v).filter((v) => known.has(v)) };
    });
  return { ...c, inScope: c.inScope.map((s) => s.trim()).filter(Boolean), outOfScope: c.outOfScope.map((s) => s.trim()).filter(Boolean), checks, criteria };
}

export function contractIsEmpty(c: Contract): boolean {
  return c.inScope.length === 0 && c.outOfScope.length === 0 && c.criteria.length === 0 && c.checks.length === 0;
}

export const CHECK_KINDS: CheckKind[] = ['test', 'build', 'lint', 'typecheck', 'security', 'custom'];

export function isMergeOutcomeNoteworthy(o: MergeOutcome): boolean {
  return o.notes.length > 0;
}

/** True for a check result that is no longer about the current changes. */
export function isStale(fresh: boolean, run: VerificationRun | null): boolean {
  return !!run && !fresh;
}

export function summarizeRun(r: VerificationRun): string {
  const parts: string[] = [r.state];
  if (r.exitCode !== null) parts.push(`exit ${r.exitCode}`);
  return parts.join(', ');
}

/** The `Seen` token for approving: taken from the report the reviewer is looking at, never a fresh fetch. */
export function seenFrom(report: Pick<VerificationReport, 'fingerprint' | 'contractSha256'> | null): Seen | null {
  return report ? { fingerprint: report.fingerprint, contractSha256: report.contractSha256 } : null;
}

/** The core refuses decisions about state the reviewer did not see. */
export function isStaleReviewRefusal(message: string): boolean {
  return /not the ones you (reviewed|were looking at)/i.test(message);
}

export const STALE_REVIEW_NOTICE = 'The changes or success criteria changed since you opened this review. The report has been reloaded: review the new state, then decide again.';

export interface ApproveOutcome {
  /** Keep the dialog (and its typed text) open. */
  keepDialog: boolean;
  /** Reload the verification report. */
  reload: boolean;
  /** Text to show persistently in the dialog; always the core message verbatim. */
  message: string | null;
}

export function classifyApproveError(message: string): ApproveOutcome {
  return { keepDialog: true, reload: isStaleReviewRefusal(message), message };
}

/** Text for the amber chip on a check whose defining files were changed by the agent. */
export function touchedInputsChip(touched: string[]): { text: string; tooltip: string } | null {
  if (touched.length === 0) return null;
  return {
    text: `inputs changed: ${touched.join(', ')}`,
    tooltip: 'A passing result is weaker evidence: the agent edited files that define or configure this check.',
  };
}

/** Blocking core errors (git filters, merge refusals) that must stay visible in the panel, not only as a toast. */
export function isPersistentBlock(message: string): boolean {
  return /^this repository configures external git programs|^merging would overwrite|^the main checkout moved|^the merge produced a different tree/i.test(message.trim());
}
