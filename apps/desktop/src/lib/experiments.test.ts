import { describe, expect, it } from 'vitest';
import { blocking, chartDomain, draftProblems, draftToSpec, formatChange, formatValue, progress, toPercent, type ExperimentDraft } from './experiments';
import type { ExperimentDetail, ExperimentReport } from './types';

const draft = (o: Partial<ExperimentDraft> = {}): ExperimentDraft => ({
  name: 'speed', objective: '', command: 'npm run bench', correctnessCommand: 'npm test', metricKind: 'duration', metricName: '', direction: 'lower',
  repetitions: 5, warmup: 1, timeoutSecs: 600, baselineRef: 'main', variants: [{ label: 'a', source: 'ref', gitRef: 'feature/a', missionId: '' }], ...o,
});

describe('experiments helpers', () => {
  it('formats durations and metrics for people', () => {
    expect(formatValue({ kind: 'duration' }, 132.4)).toBe('132 ms');
    expect(formatValue({ kind: 'duration' }, 1830)).toBe('1.83 s');
    expect(formatValue({ kind: 'duration' }, 61_000)).toBe('61.0 s');
    expect(formatValue({ kind: 'output', name: 'x', direction: 'higher' }, 1.23456)).toBe('1.235');
    expect(formatValue({ kind: 'duration' }, null)).toBe('—');
    expect(formatChange(-0.2667)).toBe('-27 %');
    expect(formatChange(0.051)).toBe('+5.1 %');
  });

  it('puts every revision on one padded axis', () => {
    const r = { variants: [{ stats: { min: 100, max: 120 } }, { stats: { min: 50, max: 60 } }, { stats: null }] } as unknown as ExperimentReport;
    const d = chartDomain(r)!;
    expect(d.lo).toBeLessThan(50);
    expect(d.hi).toBeGreaterThan(120);
    expect(toPercent(50, d)).toBeGreaterThan(0);
    expect(toPercent(999, d)).toBe(100);
    expect(chartDomain({ variants: [] } as unknown as ExperimentReport)).toBeNull();
    const flat = chartDomain({ variants: [{ stats: { min: 5, max: 5 } }] } as unknown as ExperimentReport)!;
    expect(flat.hi).toBeGreaterThan(flat.lo);
  });

  it('counts progress against what the definition asks for', () => {
    const d = { experiment: { correctnessCommand: 'x', warmup: 1, repetitions: 3 }, variants: [{}, {}], results: new Array(5) } as unknown as ExperimentDetail;
    expect(progress(d)).toEqual({ done: 5, total: 2 + 2 + 6 });
  });

  it('validates drafts like the core does', () => {
    expect(blocking(draftProblems(draft()))).toEqual([]);
    expect(blocking(draftProblems(draft({ name: ' ' })))).toHaveLength(1);
    expect(blocking(draftProblems(draft({ baselineRef: '--evil' })))).toHaveLength(1);
    expect(blocking(draftProblems(draft({ variants: [{ label: 'Baseline', source: 'ref', gitRef: 'x', missionId: '' }] })))).toHaveLength(1);
    expect(blocking(draftProblems(draft({ variants: [{ label: 'm', source: 'mission', gitRef: '', missionId: '' }] })))).toHaveLength(1);
    expect(blocking(draftProblems(draft({ variants: [] })))).toHaveLength(1);
    expect(blocking(draftProblems(draft({ command: 'a\nb' })))).toHaveLength(1);
    // Fewer than 3 runs is allowed but explained.
    const few = draftProblems(draft({ repetitions: 2 }));
    expect(blocking(few)).toEqual([]);
    expect(few.some((p) => p.includes('never name a winner'))).toBe(true);
  });

  it('builds the spec the core expects', () => {
    const s = draftToSpec(draft({ metricKind: 'output', metricName: 'ops', direction: 'higher', correctnessCommand: ' ', variants: [{ label: ' m ', source: 'mission', gitRef: 'ignored', missionId: 'id1' }] }));
    expect(s.metric).toEqual({ kind: 'output', name: 'ops', direction: 'higher' });
    expect(s.correctnessCommand).toBeNull();
    expect(s.variants).toEqual([{ label: 'm', gitRef: '', missionId: 'id1' }]);
  });
});
