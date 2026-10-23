// Experiments: pure helpers for the Experiments screen (formatting, chart scale, progress, draft validation).
import type { Tone } from './format';
import type { Experiment, ExperimentDetail, ExperimentMetric, ExperimentReport, ExperimentVerdict, NewExperiment } from './types';

export const VERDICT_META: Record<ExperimentVerdict, { label: string; tone: Tone }> = {
  baseline: { label: 'Baseline', tone: 'idle' },
  better: { label: 'Better', tone: 'ok' },
  worse: { label: 'Worse', tone: 'bad' },
  no_clear_difference: { label: 'No clear difference', tone: 'idle' },
  insufficient_runs: { label: 'Too few runs', tone: 'warn' },
  invalid: { label: 'Invalid', tone: 'bad' },
};

export const EXPERIMENT_STATE_TONE: Record<Experiment['state'], Tone> = {
  draft: 'idle', running: 'run', completed: 'ok', failed: 'bad', cancelled: 'warn', interrupted: 'warn',
};

export function metricLabel(m: ExperimentMetric): string {
  return m.kind === 'duration' ? 'Wall-clock time (lower is better)' : `${m.name} (${m.direction} is better)`;
}

/** A measured value as a person reads it: durations in ms or s, other metrics with at most 3 decimals. */
export function formatValue(m: ExperimentMetric, v: number | null | undefined): string {
  if (v === null || v === undefined || !Number.isFinite(v)) return '—';
  if (m.kind === 'duration') return v >= 10_000 ? `${(v / 1000).toFixed(1)} s` : v >= 1000 ? `${(v / 1000).toFixed(2)} s` : `${Math.round(v)} ms`;
  return Number.isInteger(v) ? String(v) : String(Number(v.toFixed(3)));
}

export function formatChange(c: number | null | undefined): string {
  if (c === null || c === undefined || !Number.isFinite(c)) return '—';
  const pct = c * 100;
  return `${pct > 0 ? '+' : ''}${pct.toFixed(Math.abs(pct) < 10 ? 1 : 0)} %`;
}

/** Shared axis for the range chart: every revision's min..max, padded 5 % so the extremes are not on the edge. */
export function chartDomain(r: ExperimentReport): { lo: number; hi: number } | null {
  const vals = r.variants.flatMap((v) => (v.stats ? [v.stats.min, v.stats.max] : []));
  if (vals.length === 0) return null;
  let lo = Math.min(...vals);
  let hi = Math.max(...vals);
  if (lo === hi) {
    const pad = Math.abs(lo) * 0.05 || 1;
    return { lo: lo - pad, hi: hi + pad };
  }
  const pad = (hi - lo) * 0.05;
  lo -= pad;
  hi += pad;
  return { lo, hi };
}

export function toPercent(v: number, d: { lo: number; hi: number }): number {
  return Math.max(0, Math.min(100, ((v - d.lo) / (d.hi - d.lo)) * 100));
}

/** Runs recorded so far in the latest attempt against the runs the definition asks for. */
export function progress(d: ExperimentDetail): { done: number; total: number } {
  const e = d.experiment;
  const n = d.variants.length;
  const total = (e.correctnessCommand ? n : 0) + n * e.warmup + n * e.repetitions;
  return { done: Math.min(d.results.length, total), total };
}

export interface VariantDraft {
  label: string;
  source: 'ref' | 'mission';
  gitRef: string;
  missionId: string;
}

export interface ExperimentDraft {
  name: string;
  objective: string;
  command: string;
  correctnessCommand: string;
  metricKind: 'duration' | 'output';
  metricName: string;
  direction: 'lower' | 'higher';
  repetitions: number;
  warmup: number;
  timeoutSecs: number;
  baselineRef: string;
  variants: VariantDraft[];
}

const REF_BAD = /^-|\.\.|@|[\s~^:?*[\\]/;

/** The same rules the core enforces, checked before sending so the form can say what is wrong. */
export function draftProblems(d: ExperimentDraft): string[] {
  const p: string[] = [];
  if (!d.name.trim()) p.push('Give the experiment a name.');
  if (!d.command.trim()) p.push('Enter the command to measure.');
  if (/[\r\n]/.test(d.command) || /[\r\n]/.test(d.correctnessCommand)) p.push('Commands must be a single line.');
  if (d.metricKind === 'output' && !/^[A-Za-z0-9_.-]{1,64}$/.test(d.metricName)) p.push('A metric name is 1-64 letters, digits, "_", "-" or ".".');
  if (!(d.repetitions >= 1 && d.repetitions <= 100)) p.push('Repetitions must be between 1 and 100.');
  if (!(d.warmup >= 0 && d.warmup <= 10)) p.push('At most 10 warm-up runs.');
  if (!(d.timeoutSecs >= 1 && d.timeoutSecs <= 4 * 3600)) p.push('The timeout must be between 1 second and 4 hours.');
  if (!d.baselineRef.trim() || REF_BAD.test(d.baselineRef.trim())) p.push('The baseline must be a branch, tag or commit.');
  if (d.variants.length === 0) p.push('Add at least one variant to compare with the baseline.');
  if (d.variants.length > 8) p.push('At most 8 variants.');
  const seen = new Set(['baseline']);
  d.variants.forEach((v, i) => {
    const l = v.label.trim().toLowerCase();
    if (!l) p.push(`Variant ${i + 1} needs a label.`);
    else if (seen.has(l)) p.push(`Variant label "${v.label.trim()}" is used twice ("baseline" is reserved).`);
    seen.add(l);
    if (v.source === 'mission' && !v.missionId) p.push(`Variant ${i + 1}: choose a mission.`);
    if (v.source === 'ref' && (!v.gitRef.trim() || REF_BAD.test(v.gitRef.trim()))) p.push(`Variant ${i + 1}: enter a branch, tag or commit.`);
  });
  if (d.repetitions < 3) p.push('Note: with fewer than 3 repetitions FORGE will never name a winner.');
  return p;
}

/** Problems that block saving (the note about fewer than 3 runs does not). */
export function blocking(problems: string[]): string[] {
  return problems.filter((p) => !p.startsWith('Note:'));
}

export function draftToSpec(d: ExperimentDraft): NewExperiment {
  return {
    name: d.name.trim(),
    objective: d.objective.trim() || null,
    command: d.command.trim(),
    correctnessCommand: d.correctnessCommand.trim() || null,
    metric: d.metricKind === 'duration' ? { kind: 'duration' } : { kind: 'output', name: d.metricName.trim(), direction: d.direction },
    repetitions: d.repetitions,
    warmup: d.warmup,
    timeoutSecs: d.timeoutSecs,
    baselineRef: d.baselineRef.trim(),
    variants: d.variants.map((v) => (v.source === 'mission' ? { label: v.label.trim(), gitRef: '', missionId: v.missionId } : { label: v.label.trim(), gitRef: v.gitRef.trim(), missionId: null })),
  };
}
