// Collisions between missions: turns the core's ConflictReport into a short, ranked list a person can act on.
import type { ConflictReport } from './types';

export type CollisionKind = 'conflict' | 'drift_conflict' | 'overlap' | 'drift' | 'unknown';

export interface Collision {
  key: string;
  kind: CollisionKind;
  /** Mission ids involved (one for drift, two for a pair). */
  missions: string[];
  files: string[];
  text: string;
}

const RANK: CollisionKind[] = ['conflict', 'drift_conflict', 'unknown', 'overlap', 'drift'];

export const COLLISION_META: Record<CollisionKind, { label: string; tone: 'bad' | 'warn' | 'idle' }> = {
  conflict: { label: 'Will conflict', tone: 'bad' },
  drift_conflict: { label: 'Conflicts with base', tone: 'bad' },
  unknown: { label: 'Could not predict', tone: 'warn' },
  overlap: { label: 'Same files', tone: 'warn' },
  drift: { label: 'Base moved', tone: 'idle' },
};

const plural = (n: number, w: string) => `${n} ${w}${n === 1 ? '' : 's'}`;

/** Ranked collisions: predicted conflicts first, then unpredictable pairs, then plain overlaps and base drift. Pairs that
 *  merge cleanly and touch different files are not collisions and are left out. */
export function collisions(r: ConflictReport, title: (id: string) => string): Collision[] {
  const out: Collision[] = [];
  for (const p of r.pairs) {
    const pair = `"${title(p.a)}" and "${title(p.b)}"`;
    if (p.prediction.outcome === 'conflicts') {
      out.push({ key: `p-${p.a}-${p.b}`, kind: 'conflict', missions: [p.a, p.b], files: p.prediction.conflictedFiles, text: `${pair} would conflict in ${plural(p.prediction.conflictedFiles.length || 1, 'file')} when both land.` });
    } else if (p.prediction.outcome === 'unknown') {
      out.push({ key: `p-${p.a}-${p.b}`, kind: 'unknown', missions: [p.a, p.b], files: p.overlappingFiles, text: `FORGE could not predict whether ${pair} conflict: ${p.prediction.error ?? 'unknown error'}` });
    } else if (p.overlappingFiles.length > 0) {
      out.push({ key: `p-${p.a}-${p.b}`, kind: 'overlap', missions: [p.a, p.b], files: p.overlappingFiles, text: `${pair} both change ${plural(p.overlappingFiles.length, 'file')}; git can merge them, but check they agree.` });
    }
  }
  for (const d of r.drift) {
    if (d.baseHead === null) {
      out.push({ key: `d-${d.missionId}`, kind: 'unknown', missions: [d.missionId], files: [], text: `The base branch "${d.baseRef}" of "${title(d.missionId)}" no longer exists.` });
    } else if (d.prediction?.outcome === 'conflicts') {
      out.push({ key: `d-${d.missionId}`, kind: 'drift_conflict', missions: [d.missionId], files: d.prediction.conflictedFiles, text: `"${d.baseRef}" moved ${plural(d.commitsBehind, 'commit')} since "${title(d.missionId)}" started, and the mission now conflicts with it.` });
    } else if (d.commitsBehind > 0) {
      out.push({ key: `d-${d.missionId}`, kind: 'drift', missions: [d.missionId], files: d.overlappingFiles, text: `"${d.baseRef}" moved ${plural(d.commitsBehind, 'commit')} since "${title(d.missionId)}" started${d.overlappingFiles.length ? `, touching ${plural(d.overlappingFiles.length, 'file')} it also changed` : ''}; it still merges cleanly.` });
    }
  }
  return out.sort((a, b) => RANK.indexOf(a.kind) - RANK.indexOf(b.kind));
}

/** Collisions that involve one mission (for its workspace). */
export function collisionsFor(all: Collision[], missionId: string): Collision[] {
  return all.filter((c) => c.missions.includes(missionId));
}
