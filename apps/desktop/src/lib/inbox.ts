// Engineering Inbox: prioritized rows derived only from real missions and approvals.
import type { Approval, Mission, MissionState } from './types';

export type InboxKind = 'approval' | 'failed' | 'interrupted' | 'review' | 'running' | 'ready';

export interface InboxRow {
  key: string;
  kind: InboxKind;
  mission: Mission;
  approval?: Approval;
  /** ISO time the row became relevant. */
  at: string;
}

const ORDER: InboxKind[] = ['approval', 'failed', 'interrupted', 'review', 'running', 'ready'];

export const INBOX_LABEL: Record<InboxKind, string> = {
  approval: 'Needs you',
  failed: 'Failed',
  interrupted: 'Interrupted',
  review: 'Awaiting review',
  running: 'Running',
  ready: 'Ready to start',
};

function kindFor(state: MissionState): InboxKind | null {
  switch (state) {
    case 'failed':
      return 'failed';
    case 'interrupted':
      return 'interrupted';
    case 'awaiting_review':
      return 'review';
    case 'preparing':
    case 'running':
    case 'awaiting_approval':
      return 'running';
    case 'ready':
      return 'ready';
    default:
      return null;
  }
}

export function buildInbox(missions: Mission[], approvals: Approval[]): InboxRow[] {
  const byId = new Map(missions.map((m) => [m.id, m]));
  const rows: InboxRow[] = [];
  for (const a of approvals) {
    if (a.state !== 'pending') continue;
    const m = byId.get(a.missionId);
    if (m) rows.push({ key: `a-${a.id}`, kind: 'approval', mission: m, approval: a, at: a.createdAt });
  }
  for (const m of missions) {
    const k = kindFor(m.state);
    if (k) rows.push({ key: `m-${m.id}`, kind: k, mission: m, at: m.updatedAt });
  }
  // Oldest waiting approvals first (they block an agent); everything else most recent first.
  return rows.sort((x, y) => {
    const d = ORDER.indexOf(x.kind) - ORDER.indexOf(y.kind);
    if (d !== 0) return d;
    return x.kind === 'approval' ? x.at.localeCompare(y.at) : y.at.localeCompare(x.at);
  });
}

export interface SinceSummary {
  changed: Mission[];
  byState: Partial<Record<MissionState, number>>;
}

export function sinceLastVisit(missions: Mission[], lastVisit: number | null): SinceSummary {
  if (lastVisit === null) return { changed: [], byState: {} };
  const changed = missions.filter((m) => Date.parse(m.updatedAt) > lastVisit);
  const byState: SinceSummary['byState'] = {};
  for (const m of changed) byState[m.state] = (byState[m.state] ?? 0) + 1;
  return { changed, byState };
}
