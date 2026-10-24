// DEV-ONLY: Experiments and mission-collision preview data. Imported only by devPreview.ts.
// Marker for the production-bundle check: FORGE_DEVPREVIEW_MARKER_7f3a91
/* eslint-disable @typescript-eslint/no-explicit-any */
import type { ConflictReport, Experiment, ExperimentDetail, ExperimentReport, ExperimentResult, ExperimentVariant } from './types';

const ago = (m: number) => new Date(Date.now() - m * 60_000).toISOString();
const sha = (s: string) => (s.repeat(40) + '0123456789abcdef').slice(0, 40);

const variants: ExperimentVariant[] = [
  { id: 'v0', experimentId: 'e1', position: 0, label: 'baseline', isBaseline: true, gitRef: 'main', commitSha: sha('9c1e') },
  { id: 'v1', experimentId: 'e1', position: 1, label: 'pooled client', isBaseline: false, gitRef: 'mission:m1', commitSha: sha('a3f2') },
  { id: 'v2', experimentId: 'e1', position: 2, label: 'cache headers', isBaseline: false, gitRef: 'feature/cache-headers', commitSha: sha('77b0') },
];
const measured: Record<string, number[]> = { v0: [182, 176, 191, 179, 185], v1: [131, 128, 136, 133, 129], v2: [171, 188, 166, 180, 175] };
const results: ExperimentResult[] = [];
let rid = 0;
const add = (variantId: string, phase: ExperimentResult['phase'], iteration: number, value: number | null, state: ExperimentResult['state'] = 'passed') =>
  results.push({ id: `r${++rid}`, experimentId: 'e1', variantId, attempt: 1, phase, iteration, state, exitCode: state === 'passed' ? 0 : 1, durationMs: 4200 + rid * 13, metricValue: value, worktreeChanged: false, outputTail: phase === 'correctness' ? '42 passing (3s)' : `bench: 200 requests\nFORGE_METRIC p50_ms=${value}`, logPath: null, startedAt: ago(30 - rid * 0.2), backend: 'trusted_local' });
for (const v of variants) add(v.id, 'correctness', 0, null);
for (const v of variants) add(v.id, 'warmup', 0, measured[v.id][0] + 9);
for (let i = 0; i < 5; i++) for (const v of variants) add(v.id, 'measure', i, measured[v.id][i]);

const stats = (xs: number[]) => {
  const s = [...xs].sort((a, b) => a - b);
  const med = s[Math.floor(s.length / 2)];
  const dev = s.map((x) => Math.abs(x - med)).sort((a, b) => a - b);
  return { n: s.length, median: med, min: s[0], max: s[s.length - 1], mad: dev[Math.floor(dev.length / 2)] };
};
const report: ExperimentReport = {
  metric: { kind: 'output', name: 'p50_ms', direction: 'lower' },
  direction: 'lower',
  minRunsForVerdict: 3,
  variants: [
    { variantId: 'v0', label: 'baseline', isBaseline: true, commit: sha('9c1e'), stats: stats(measured.v0), failedRuns: 0, correctness: true, worktreeChanged: false, changeVsBaseline: null, verdict: 'baseline', reasons: [] },
    { variantId: 'v1', label: 'pooled client', isBaseline: false, commit: sha('a3f2'), stats: stats(measured.v1), failedRuns: 0, correctness: true, worktreeChanged: false, changeVsBaseline: (131 - 182) / 182, verdict: 'better', reasons: [] },
    { variantId: 'v2', label: 'cache headers', isBaseline: false, commit: sha('77b0'), stats: stats(measured.v2), failedRuns: 0, correctness: true, worktreeChanged: false, changeVsBaseline: (175 - 182) / 182, verdict: 'no_clear_difference', reasons: ["its range of results overlaps the baseline's"] },
  ],
  winner: 'v1',
  conclusion: 'winner',
  explanation: '"pooled client" beat the baseline in every measured run (median change -28.0 %). Whether to keep it is your decision.',
};

const base = (o: Partial<Experiment> & Pick<Experiment, 'id' | 'name'>): Experiment => ({
  projectId: 'p1', objective: null, command: 'npm run bench -- --json', correctnessCommand: 'npm test', metric: { kind: 'output', name: 'p50_ms', direction: 'lower' },
  repetitions: 5, warmup: 1, timeoutSecs: 600, state: 'draft', attempt: 0, env: null, report: null, lastError: null, createdAt: ago(60), startedAt: null, finishedAt: null, ...o,
});
const experiments: Experiment[] = [
  base({ id: 'e1', name: 'HTTP client latency', objective: 'Lower p50 latency of the /orders endpoint without breaking the API tests', state: 'completed', attempt: 1, report, startedAt: ago(34), finishedAt: ago(28), env: { os: 'windows', arch: 'x86_64', cpus: 16, git: 'git version 2.55.0.windows.1', forge: '0.1.0' } }),
  base({ id: 'e2', name: 'Test suite wall-clock', command: 'npm test', correctnessCommand: null, metric: { kind: 'duration' }, repetitions: 2, state: 'draft' }),
];
const detail = (id: string): ExperimentDetail => {
  const e = experiments.find((x) => x.id === id)!;
  return { experiment: e, variants: id === 'e1' ? variants : variants.slice(0, 2).map((v) => ({ ...v, experimentId: id })), results: id === 'e1' ? results : [] };
};

const conflicts = (projectId: string): ConflictReport => ({
  projectId, generatedAt: new Date().toISOString(), skipped: [], missions: [],
  pairs: projectId === 'p1' ? [
    { a: 'm1', b: 'm2', overlappingFiles: ['src/http/client.ts'], prediction: { outcome: 'conflicts', conflictedFiles: ['src/http/client.ts'], error: null } },
    { a: 'm2', b: 'm4', overlappingFiles: ['src/log.ts'], prediction: { outcome: 'clean', conflictedFiles: [], error: null } },
  ] : [],
  drift: projectId === 'p1' ? [{ missionId: 'm4', baseRef: 'main', baseCommit: sha('9c1e'), baseHead: sha('aa11'), commitsBehind: 4, overlappingFiles: [], prediction: { outcome: 'clean', conflictedFiles: [], error: null } }] : [],
});

export function handleP6(cmd: string, args: any): Promise<any> | undefined {
  const done = (v: any) => Promise.resolve(v);
  switch (cmd) {
    case 'experiment_list': return done(experiments.filter((e) => e.projectId === args.projectId));
    case 'experiment_get': return done(detail(args.experimentId));
    case 'experiment_create': {
      const s = args.spec;
      const e = base({ id: `e${experiments.length + 1}`, name: s.name, objective: s.objective, command: s.command, correctnessCommand: s.correctnessCommand, metric: s.metric, repetitions: s.repetitions, warmup: s.warmup ?? 0, timeoutSecs: s.timeoutSecs, projectId: args.projectId, createdAt: new Date().toISOString() });
      experiments.unshift(e);
      return done(e);
    }
    case 'experiment_run': {
      const e = experiments.find((x) => x.id === args.experimentId)!;
      e.state = 'running';
      return new Promise((res) => setTimeout(() => {
        e.state = 'completed';
        e.attempt += 1;
        e.finishedAt = new Date().toISOString();
        e.report = { ...report, variants: report.variants.slice(0, 2), winner: null, conclusion: 'inconclusive', explanation: 'The results cannot name a winner: see the reasons on each revision.' };
        e.report.variants = e.report.variants.map((v) => (v.isBaseline ? { ...v, verdict: 'insufficient_runs', reasons: ['fewer than 3 measured runs'] } : { ...v, verdict: 'insufficient_runs', reasons: ["fewer than 3 measured runs on this side or the baseline's"] }));
        res(detail(e.id));
      }, 2500));
    }
    case 'experiment_cancel': return done(null);
    case 'mission_conflicts': return done(conflicts(args.projectId));
    case 'agents_overview': {
      const s = (id: string, missionId: string, title: string, provider: string, state: string, startMin: number, usage: any, endReason: string | null = null) =>
        ({ session: { id, missionId, provider, providerSessionId: null, state, pid: state === 'live' ? 18432 : null, startedAt: ago(startMin), endedAt: state === 'live' ? null : ago(startMin - 20), endReason, usage }, missionTitle: title, projectId: 'p1' });
      const sessions = [
        s('s1', 'm1', 'Add retry with exponential backoff to the HTTP client', 'claude', 'live', 41, null),
        s('s2', 'm2', 'Fix flaky date parsing test', 'claude', 'ended', 90, { inputTokens: 48210, outputTokens: 6120, cacheReadTokens: 210400, cacheCreationTokens: 9100, costUsdEstimate: 0.412, numTurns: 14 }, 'completed'),
        s('s9', 'm4', 'Refactor logger to structured output', 'codex', 'ended', 300, { inputTokens: 22100, outputTokens: 3400, cacheReadTokens: 80000, cacheCreationTokens: 0 }, 'completed'),
        s('s4', 'm4', 'Refactor logger to structured output', 'claude', 'lost', 420, null, 'agent process disappeared'),
      ];
      return done({ since: ago(60 * 24 * args.days), days: args.days, sessions, usage: [
        { provider: 'claude', sessions: 3, live: 1, inputTokens: 48210, outputTokens: 6120, cacheReadTokens: 210400, cacheCreationTokens: 9100, turns: 14, costUsdEstimate: 0.412, sessionsWithUsage: 1 },
        { provider: 'codex', sessions: 1, live: 0, inputTokens: 22100, outputTokens: 3400, cacheReadTokens: 80000, cacheCreationTokens: 0, turns: 0, costUsdEstimate: null, sessionsWithUsage: 1 },
      ] });
    }
    default: return undefined;
  }
}
