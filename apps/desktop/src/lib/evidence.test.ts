import { describe, expect, it } from 'vitest';
import { severityLine, sortFindings } from './evidence';
import type { Evidence, Finding } from './types';

const f = (ruleId: string, severity: Finding['severity'], blocking: boolean): Finding => ({ ruleId, severity, title: '', location: null, package: null, fixedIn: null, blocking });

describe('evidence helpers', () => {
  it('summarises severities in order and omits empty ones', () => {
    const e = { bySeverity: { medium: 1, high: 2, info: 0 } } as unknown as Evidence;
    expect(severityLine(e)).toBe('2 high · 1 medium');
    expect(severityLine({ bySeverity: {} } as unknown as Evidence)).toBe('');
  });
  it('puts blocking findings first, then by severity (unknown ranks as serious)', () => {
    expect(sortFindings([f('a', 'low', false), f('b', 'medium', true), f('c', 'unknown', true), f('d', 'critical', true)]).map((x) => x.ruleId)).toEqual(['d', 'c', 'b', 'a']);
  });
});
