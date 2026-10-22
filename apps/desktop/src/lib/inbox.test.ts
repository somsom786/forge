import { describe, expect, it } from 'vitest';
import { buildInbox, sinceLastVisit } from './inbox';
import type { Approval, Mission } from './types';

const m = (id: string, state: Mission['state'], updatedAt = '2026-01-01T00:00:00Z'): Mission => ({
  id, projectId: 'p', title: id, objective: '', state, provider: 'claude', model: null, permissionProfile: 'default',
  baseRef: null, baseCommit: null, branch: null, worktreePath: null, providerSessionId: null, lastError: null,
  createdAt: updatedAt, updatedAt,
});
const ap = (id: string, missionId: string, state: Approval['state'], createdAt: string): Approval => ({
  id, missionId, sessionId: 's', providerRequestId: 'r', tool: 'Bash', summary: '', detail: { tool: 'Bash', input: {} },
  risk: 'low', warnings: [], state, decidedBy: null, decisionNote: null, createdAt, decidedAt: null,
});

describe('buildInbox', () => {
  it('orders needs-you, failed, interrupted, review, running, ready and skips closed missions', () => {
    const rows = buildInbox(
      [m('ready', 'ready'), m('run', 'running'), m('rev', 'awaiting_review'), m('int', 'interrupted'), m('fail', 'failed'), m('done', 'merged'), m('wait', 'awaiting_approval')],
      [ap('a1', 'wait', 'pending', '2026-01-01T00:00:00Z'), ap('a2', 'wait', 'approved', '2026-01-01T00:00:00Z')],
    );
    expect(rows.map((r) => r.kind)).toEqual(['approval', 'failed', 'interrupted', 'review', 'running', 'running', 'ready']);
    expect(rows.some((r) => r.mission.id === 'done')).toBe(false);
  });
  it('lists oldest pending approvals first', () => {
    const rows = buildInbox([m('x', 'awaiting_approval')], [ap('late', 'x', 'pending', '2026-01-02T00:00:00Z'), ap('early', 'x', 'pending', '2026-01-01T00:00:00Z')]);
    expect(rows.filter((r) => r.kind === 'approval').map((r) => r.approval?.id)).toEqual(['early', 'late']);
  });
});

describe('sinceLastVisit', () => {
  it('counts only missions updated after the last visit', () => {
    const t = Date.parse('2026-01-02T00:00:00Z');
    const s = sinceLastVisit([m('a', 'failed', '2026-01-03T00:00:00Z'), m('b', 'ready', '2026-01-01T00:00:00Z')], t);
    expect(s.changed.map((x) => x.id)).toEqual(['a']);
    expect(s.byState).toEqual({ failed: 1 });
  });
  it('is empty without a stored visit', () => {
    expect(sinceLastVisit([m('a', 'failed')], null).changed).toEqual([]);
  });
});
