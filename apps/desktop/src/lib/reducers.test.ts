import { describe, expect, it } from 'vitest';
import {
  addNotice, applyDelta, clearStream, emptyData, mergeEvents, NULL_MESSAGE, reduceUiEvent, upsertApproval, upsertMission,
} from './reducers';
import { buildFeed, filterFeed } from './feed';
import type { ActivityEvent, Approval, Mission } from './types';

const ev = (id: number, kind: ActivityEvent['kind'] = 'agent.text', payload: Record<string, any> = {}, missionId = 'm1'): ActivityEvent => ({
  id, missionId, sessionId: 's1', seq: id, kind, payload, at: '2026-01-01T00:00:00Z',
});

const mission = (over: Partial<Mission> = {}): Mission => ({
  id: 'm1', projectId: 'p1', title: 't', objective: 'o', state: 'ready', provider: 'claude', model: null,
  permissionProfile: 'default', baseRef: null, baseCommit: null, branch: null, worktreePath: null,
  providerSessionId: null, lastError: null, createdAt: '2026-01-01T00:00:00Z', updatedAt: '2026-01-01T00:00:00Z', useMemory: true, memoryPins: [], ...over,
});

const approval = (over: Partial<Approval> = {}): Approval => ({
  id: 'a1', missionId: 'm1', sessionId: 's1', providerRequestId: 'r1', tool: 'Bash', summary: 'npm test',
  detail: { tool: 'Bash', input: {} }, risk: 'low', warnings: [], state: 'pending', decidedBy: null, decisionNote: null,
  createdAt: '2026-01-01T00:00:00Z', decidedAt: null, bindingSha256: null, policyRev: null, executesOn: null, expiresAt: null, ...over,
});

describe('mergeEvents', () => {
  it('dedupes by id', () => {
    const a = mergeEvents(undefined, [ev(1), ev(2)]);
    const b = mergeEvents(a, [ev(2), ev(3)]);
    expect(b.map((e) => e.id)).toEqual([1, 2, 3]);
  });
  it('orders by id even when delivered out of order', () => {
    const a = mergeEvents([], [ev(5), ev(3)]);
    expect(a.map((e) => e.id)).toEqual([3, 5]);
    const b = mergeEvents(a, [ev(4), ev(1)]);
    expect(b.map((e) => e.id)).toEqual([1, 3, 4, 5]);
  });
  it('returns the same reference when nothing is new', () => {
    const a = mergeEvents([], [ev(1)]);
    expect(mergeEvents(a, [ev(1)])).toBe(a);
  });
  it('merges history loaded after live events', () => {
    const live = mergeEvents([], [ev(10), ev(11)]);
    const merged = mergeEvents(live, [ev(8), ev(9), ev(10)]);
    expect(merged.map((e) => e.id)).toEqual([8, 9, 10, 11]);
  });
});

describe('streaming deltas', () => {
  it('accumulates text per messageId', () => {
    let s = applyDelta({}, 'm1', 'x', 'Hel');
    s = applyDelta(s, 'm1', 'x', 'lo');
    s = applyDelta(s, 'm1', 'y', 'other');
    expect(s.m1).toEqual({ x: 'Hello', y: 'other' });
  });
  it('keys unkeyed deltas separately', () => {
    const s = applyDelta({}, 'm1', null, 'hi');
    expect(s.m1[NULL_MESSAGE]).toBe('hi');
    expect(clearStream(s, 'm1', null)).toEqual({});
  });
  it('is replaced by the final agent.text with the same messageId', () => {
    let d = emptyData();
    d = reduceUiEvent(d, { type: 'delta', missionId: 'm1', messageId: 'x', text: 'partial' });
    d = reduceUiEvent(d, { type: 'delta', missionId: 'm1', messageId: 'z', text: 'keep' });
    d = reduceUiEvent(d, { type: 'activity', event: ev(1, 'agent.text', { messageId: 'x', text: 'partial done' }) });
    expect(d.streams.m1).toEqual({ z: 'keep' });
    expect(d.activity.m1).toHaveLength(1);
  });
  it('clears all streams for the mission on turn completion', () => {
    let d = emptyData();
    d = reduceUiEvent(d, { type: 'delta', missionId: 'm1', messageId: 'z', text: 'x' });
    d = reduceUiEvent(d, { type: 'activity', event: ev(2, 'turn.completed', { status: 'success' }) });
    expect(d.streams).toEqual({});
  });
  it('does not touch other missions', () => {
    let d = emptyData();
    d = reduceUiEvent(d, { type: 'delta', missionId: 'm2', messageId: 'x', text: 'a' });
    d = reduceUiEvent(d, { type: 'activity', event: ev(1, 'agent.text', { messageId: 'x' }, 'm1') });
    expect(d.streams.m2).toEqual({ x: 'a' });
  });
});

describe('upserts', () => {
  it('replaces a mission with a newer one and ignores stale pushes', () => {
    let ms = upsertMission({}, mission({ state: 'running', updatedAt: '2026-01-01T00:00:10Z' }));
    ms = upsertMission(ms, mission({ state: 'ready', updatedAt: '2026-01-01T00:00:05Z' }));
    expect(ms.m1.state).toBe('running');
    ms = upsertMission(ms, mission({ state: 'awaiting_review', updatedAt: '2026-01-01T00:00:20Z' }));
    expect(ms.m1.state).toBe('awaiting_review');
  });
  it('tracks approval state changes and never reverts a decision', () => {
    let as = upsertApproval({}, approval());
    expect(as.a1.state).toBe('pending');
    as = upsertApproval(as, approval({ state: 'approved', decidedBy: 'user', decidedAt: '2026-01-01T00:00:03Z' }));
    expect(as.a1.state).toBe('approved');
    as = upsertApproval(as, approval({ state: 'pending' }));
    expect(as.a1.state).toBe('approved');
  });
  it('records auto-denied approvals with their reason', () => {
    const d = reduceUiEvent(emptyData(), {
      type: 'approval', approval: approval({ state: 'auto_denied', decisionNote: 'Blocked: writes outside worktree' }),
    });
    expect(d.approvals.a1.state).toBe('auto_denied');
    expect(d.approvals.a1.decisionNote).toMatch(/outside worktree/);
  });
});

describe('notices', () => {
  it('adds notices with increasing ids and caps the list', () => {
    let s = { notices: [] as ReturnType<typeof addNotice>['notices'], noticeSeq: 0 };
    for (let i = 0; i < 10; i++) s = addNotice(s, 'error', `e${i}`);
    expect(s.notices).toHaveLength(6);
    expect(s.notices[5].id).toBe(10);
  });
  it('turns pushed notices into toasts', () => {
    const d = reduceUiEvent(emptyData(), { type: 'notice', level: 'warn', message: 'careful' });
    expect(d.notices[0]).toMatchObject({ level: 'warn', message: 'careful' });
  });
});

describe('buildFeed', () => {
  it('pairs tool.call with tool.result by toolUseId in one row', () => {
    const items = buildFeed([
      ev(1, 'tool.call', { toolUseId: 't1', name: 'Read' }),
      ev(2, 'agent.text', { text: 'x' }),
      ev(3, 'tool.result', { toolUseId: 't1', ok: true }),
    ]);
    expect(items).toHaveLength(2);
    expect(items[0]).toMatchObject({ type: 'tool' });
    const t = items[0] as Extract<typeof items[number], { type: 'tool' }>;
    expect(t.call?.id).toBe(1);
    expect(t.result?.id).toBe(3);
  });
  it('shows an unpaired result on its own and an unanswered call without result', () => {
    const items = buildFeed([ev(1, 'tool.call', { toolUseId: 'a' }), ev(2, 'tool.result', { toolUseId: 'zzz' })]);
    expect(items).toHaveLength(2);
    expect((items[0] as any).result).toBeNull();
    expect((items[1] as any).call).toBeNull();
  });
  it('filters by group', () => {
    const items = buildFeed([ev(1, 'agent.thinking'), ev(2, 'tool.call', { toolUseId: 'a' }), ev(3, 'agent.text')]);
    expect(filterFeed(items, new Set(['thinking', 'tools']))).toHaveLength(1);
    expect(filterFeed(items, new Set())).toHaveLength(3);
  });

  it('bumps per-mission and per-project refresh ticks for pushed proof and knowledge events', () => {
    let d = reduceUiEvent(emptyData(), { type: 'proofPack', missionId: 'm1' });
    d = reduceUiEvent(d, { type: 'proofPack', missionId: 'm1' });
    d = reduceUiEvent(d, { type: 'knowledge', projectId: 'p1' });
    expect(d.proofTick).toEqual({ m1: 2 });
    expect(d.knowledgeTick).toEqual({ p1: 1 });
  });
});
