import type { Evidence, EvidenceOutcome, Finding, FindingSeverity } from './types';

export const OUTCOME_LABEL: Record<EvidenceOutcome, string> = {
  passed: 'no blocking findings',
  findings: 'blocking findings',
  tool_missing: 'tool not installed',
  tool_error: 'report unreadable',
  incomplete: 'no evidence (run did not finish)',
};

export const OUTCOME_TONE: Record<EvidenceOutcome, 'ok' | 'bad' | 'warn' | 'idle'> = {
  passed: 'ok',
  findings: 'bad',
  tool_missing: 'warn',
  tool_error: 'warn',
  incomplete: 'idle',
};

const ORDER: FindingSeverity[] = ['critical', 'high', 'unknown', 'medium', 'low', 'info'];

export const SEVERITY_TONE: Record<FindingSeverity, 'bad' | 'warn' | 'idle'> = {
  critical: 'bad', high: 'bad', unknown: 'warn', medium: 'warn', low: 'idle', info: 'idle',
};

/** "2 high · 1 medium" in severity order; empty when there are no findings. */
export function severityLine(e: Evidence): string {
  return ORDER.filter((s) => (e.bySeverity[s] ?? 0) > 0).map((s) => `${e.bySeverity[s]} ${s}`).join(' · ');
}

/** Blocking first, then by severity. */
export function sortFindings(fs: Finding[]): Finding[] {
  return [...fs].sort((a, b) => Number(b.blocking) - Number(a.blocking) || ORDER.indexOf(a.severity) - ORDER.indexOf(b.severity));
}
