import { describe, expect, it } from 'vitest';
import { collisions, collisionsFor } from './conflicts';
import type { ConflictReport } from './types';

const clean = { outcome: 'clean' as const, conflictedFiles: [], error: null };
const report: ConflictReport = {
  projectId: 'p', generatedAt: 't', missions: [], skipped: [],
  pairs: [
    { a: 'a', b: 'b', overlappingFiles: ['src/x.ts'], prediction: { outcome: 'conflicts', conflictedFiles: ['src/x.ts'], error: null } },
    { a: 'a', b: 'c', overlappingFiles: ['README.md'], prediction: clean },
    { a: 'b', b: 'c', overlappingFiles: [], prediction: clean },
  ],
  drift: [
    { missionId: 'a', baseRef: 'main', baseCommit: '1', baseHead: '1', commitsBehind: 0, overlappingFiles: [], prediction: null },
    { missionId: 'b', baseRef: 'main', baseCommit: '1', baseHead: '2', commitsBehind: 3, overlappingFiles: [], prediction: clean },
    { missionId: 'c', baseRef: 'main', baseCommit: '1', baseHead: '2', commitsBehind: 1, overlappingFiles: ['README.md'], prediction: { outcome: 'conflicts', conflictedFiles: ['README.md'], error: null } },
  ],
};

describe('collisions', () => {
  it('ranks predicted conflicts first and leaves out pairs that do not collide', () => {
    const c = collisions(report, (id) => id.toUpperCase());
    expect(c.map((x) => x.kind)).toEqual(['conflict', 'drift_conflict', 'overlap', 'drift']);
    expect(c[0].text).toContain('"A" and "B" would conflict in 1 file');
    expect(c.some((x) => x.key === 'p-b-c')).toBe(false);
    expect(c.find((x) => x.kind === 'drift')!.text).toContain('moved 3 commits');
    expect(collisionsFor(c, 'c').map((x) => x.kind)).toEqual(['drift_conflict', 'overlap']);
  });

  it('says when a prediction was impossible instead of guessing', () => {
    const r: ConflictReport = { ...report, pairs: [{ a: 'a', b: 'b', overlappingFiles: [], prediction: { outcome: 'unknown', conflictedFiles: [], error: 'boom' } }], drift: [{ missionId: 'a', baseRef: 'gone', baseCommit: '1', baseHead: null, commitsBehind: 0, overlappingFiles: [], prediction: null }] };
    const c = collisions(r, (id) => id);
    expect(c.map((x) => x.kind)).toEqual(['unknown', 'unknown']);
    expect(c[0].text).toContain('boom');
    expect(c[1].text).toContain('no longer exists');
  });
});
