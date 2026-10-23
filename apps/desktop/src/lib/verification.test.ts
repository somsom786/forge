import { describe, expect, it } from 'vitest';
import { approvalGate, linesToList, normalizeContract, slugId, statusLabel, statusTone, STATUS_KEYS, upsertRun } from './verification';
import { emptyData, reduceUiEvent } from './reducers';
import type { Contract, CriterionStatus, VerificationRun } from './types';

const run = (id: string, startedAt: string, state: VerificationRun['state'] = 'running'): VerificationRun => ({
  id, missionId: 'm1', checkId: 'c', name: 'c', command: 'x', kind: 'test', state, exitCode: null, startedAt, endedAt: null,
  durationMs: null, fingerprint: 'f', headCommit: null, mutated: false, outputTail: '', outputTruncated: false, logPath: null, backend: 'trusted_local',
});

describe('statusLabel', () => {
  const cases: [CriterionStatus, string][] = [
    [{ status: 'not_checked' }, 'Not checked'],
    [{ status: 'check_pending', pending: ['t'] }, 'Check pending'],
    [{ status: 'verified_by_checks', evidence: ['t1', 't2'] }, 'Verified by Unit, Lint'],
    [{ status: 'verified_manually', note: null }, 'Verified manually'],
    [{ status: 'verified_by_scope_check' }, 'Within scope'],
    [{ status: 'failed', reason: 'r' }, 'Failed'],
    [{ status: 'inconclusive', reason: 'r' }, 'Inconclusive'],
    [{ status: 'requires_manual_review', reason: 'r' }, 'Requires manual review'],
  ];
  it('maps all 8 statuses', () => {
    expect(cases).toHaveLength(STATUS_KEYS.length);
    const names = (id: string) => ({ t1: 'Unit', t2: 'Lint' })[id] ?? id;
    for (const [s, label] of cases) expect(statusLabel(s, names)).toBe(label);
  });
  it('is positive only for verified_* states', () => {
    const ok = cases.filter(([s]) => statusTone(s) === 'ok').map(([s]) => s.status);
    expect(ok).toEqual(['verified_by_checks', 'verified_manually', 'verified_by_scope_check']);
  });
});

describe('slugId', () => {
  it('slugifies and dedupes', () => {
    expect(slugId('Unit Tests!')).toBe('unit-tests');
    expect(slugId('Unit Tests', new Set(['unit-tests']))).toBe('unit-tests-2');
    expect(slugId('!!!')).toBe('item');
  });
});

describe('approvalGate', () => {
  it('needs no override when everything is established', () => {
    expect(approvalGate({ unresolved: [], hardBlockers: [] }, '')).toMatchObject({ canApprove: true, needsOverride: false });
  });
  it('requires an override reason of at least 8 chars when unresolved', () => {
    const r = { unresolved: ['Tests not run'], hardBlockers: [] };
    expect(approvalGate(r, 'short').canApprove).toBe(false);
    expect(approvalGate(r, '   1234567   ').canApprove).toBe(false);
    expect(approvalGate(r, 'known flake').canApprove).toBe(true);
  });
  it('hard blockers can never be overridden', () => {
    const g = approvalGate({ unresolved: [], hardBlockers: ['agent running'] }, 'a very long reason indeed');
    expect(g.canApprove).toBe(false);
    expect(g.blockers).toEqual(['agent running']);
  });
  it('is closed without a report', () => expect(approvalGate(null, 'long enough reason').canApprove).toBe(false));
});

describe('runs', () => {
  it('upserts by id and orders by start time', () => {
    let r = upsertRun(undefined, run('b', '2026-01-01T00:00:02Z'));
    r = upsertRun(r, run('a', '2026-01-01T00:00:01Z'));
    r = upsertRun(r, run('b', '2026-01-01T00:00:02Z', 'passed'));
    expect(r.map((x) => x.id)).toEqual(['a', 'b']);
    expect(r[1].state).toBe('passed');
  });
  it('is applied from the verification UiEvent and bumps a refresh tick', () => {
    let d = emptyData();
    d = reduceUiEvent(d, { type: 'verification', run: run('r1', '2026-01-01T00:00:01Z') });
    const t1 = d.verifyTick.m1;
    d = reduceUiEvent(d, { type: 'verification', run: run('r1', '2026-01-01T00:00:01Z', 'failed') });
    expect(d.runs.m1).toHaveLength(1);
    expect(d.runs.m1[0].state).toBe('failed');
    expect(d.verifyTick.m1).toBe(t1 + 1);
  });
});

describe('contract normalization', () => {
  it('generates stable deduped ids and drops dangling links and blank rows', () => {
    const c: Contract = {
      version: 1, inScope: [' src/ ', ''], outOfScope: [], securityConstraints: [], reviewExpectations: null,
      checks: [
        { id: 'new-check', name: 'Unit tests', command: 'npm test', kind: 'test', timeoutSecs: 60, required: true },
        { id: 'new-check-2', name: 'Unit tests', command: 'npm run t2', kind: 'test', timeoutSecs: 60, required: false },
        { id: '', name: '', command: 'x', kind: 'custom', timeoutSecs: 60, required: false },
      ],
      criteria: [
        { id: '', text: 'Retries work', verifiedBy: ['new-check', 'ghost'], manual: false, scopeRule: false },
        { id: '', text: '  ', verifiedBy: [], manual: false, scopeRule: false },
      ],
    };
    const n = normalizeContract(c);
    expect(n.checks.map((k) => k.id)).toEqual(['unit-tests', 'unit-tests-2']);
    expect(n.criteria).toHaveLength(1);
    expect(n.criteria[0]).toMatchObject({ id: 'retries-work', verifiedBy: ['unit-tests'] });
    expect(n.inScope).toEqual(['src/']);
  });
  it('splits glob lines', () => expect(linesToList('a\r\n\n b \n')).toEqual(['a', 'b']));
});
