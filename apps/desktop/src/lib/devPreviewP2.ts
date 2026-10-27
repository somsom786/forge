// DEV-ONLY: Phase 2 preview data (contract, runs, report, proof packs). Imported only by devPreview.ts.
// Marker for the production-bundle check: FORGE_DEVPREVIEW_MARKER_7f3a91
/* eslint-disable @typescript-eslint/no-explicit-any */
import type {
  ChangedFile, CheckReport, Contract, CriterionReport, Mission, ProofPack, UiEvent, VerificationReport, VerificationRun,
} from './types';

export interface P2Ctx {
  iso: (minsAgo: number) => string;
  find: (id: string) => Mission | undefined;
  changesFor: (id: string) => { files: ChangedFile[] };
  isRunning: (id: string) => boolean;
  emit: (e: UiEvent) => void;
  err: (m: string) => Promise<never>;
}

const emptyContract = (): Contract => ({ version: 1, inScope: [], outOfScope: [], criteria: [], checks: [], securityConstraints: [], reviewExpectations: null });

const contracts: Record<string, Contract> = {
  m2: {
    version: 3, inScope: ['src/', 'tests/'], outOfScope: ['package-lock.json'], securityConstraints: [], reviewExpectations: null,
    checks: [
      { id: 'unit', name: 'Unit tests', command: 'npm test', kind: 'test', timeoutSecs: 300, required: true },
      { id: 'lint', name: 'Lint', command: 'npm run lint', kind: 'lint', timeoutSecs: 120, required: false },
      { id: 'typecheck', name: 'Typecheck', command: 'npm run typecheck', kind: 'typecheck', timeoutSecs: 180, required: true },
      { id: 'build', name: 'Build', command: 'npm run build', kind: 'build', timeoutSecs: 300, required: false },
      { id: 'deps', name: 'Dependency vulnerabilities', command: 'osv-scanner scan source --format json -r .', kind: 'security', timeoutSecs: 600, required: true, adapter: 'osv-scanner' },
    ],
    criteria: [
      { id: 'tz', text: 'Date parsing gives the same result in every timezone', verifiedBy: ['unit'], manual: false, scopeRule: false },
      { id: 'lint-clean', text: 'No new lint errors are introduced', verifiedBy: ['lint'], manual: false, scopeRule: false },
      { id: 'api', text: 'The public parseDate signature is unchanged', verifiedBy: ['typecheck'], manual: false, scopeRule: false },
      { id: 'docs', text: 'The new docs page reads correctly to a newcomer', verifiedBy: [], manual: true, scopeRule: false },
      { id: 'scope', text: 'No unrelated changes', verifiedBy: [], manual: false, scopeRule: true },
    ],
  },
};

const CUR = 'f3a9c01b77d2';
const CSHA = 'c0ffee11aa22bb33cc44dd55ee66ff77';
let ctxRef: P2Ctx;

function vrun(checkId: string, name: string, command: string, state: VerificationRun['state'], exitCode: number | null, fingerprint: string, out: string, extra: Partial<VerificationRun> = {}): VerificationRun {
  return {
    id: `vr-${checkId}-${fingerprint}`, missionId: 'm2', checkId, name, command, kind: 'test', state, exitCode,
    startedAt: ctxRef.iso(20), endedAt: ctxRef.iso(19), durationMs: 41_000, fingerprint, headCommit: null, mutated: false,
    outputTail: out, outputTruncated: false, logPath: null, backend: 'trusted_local', ...extra,
  };
}

let runs: VerificationRun[] | null = null;
function allRuns(): VerificationRun[] {
  runs ??= [
    vrun('unit', 'Unit tests', 'npm test', 'passed', 0, CUR, ' PASS  src/date/parse.test.ts\n  parseDate\n    ok parses a date as UTC midnight\n    ok round-trips through formatDate\n\nTests: 2 passed, 2 total\n'),
    vrun('lint', 'Lint', 'npm run lint', 'passed', 0, 'old00000aaaa', 'eslint . --max-warnings 0\n(no problems)\n', { startedAt: ctxRef.iso(80), endedAt: ctxRef.iso(79) }),
    vrun('typecheck', 'Typecheck', 'npm run typecheck', 'failed', 2, CUR, "src/util/new-name.ts(3,7): error TS2322: Type 'string' is not assignable to type 'number'.\n\nFound 1 error.\n", { mutated: true }),
    vrun('deps', 'Dependency vulnerabilities', 'osv-scanner scan source --format json -r .', 'failed', 1, CUR, 'Scanning dir .\nScanned package-lock.json file and found 412 packages\n', {
      evidence: {
        adapter: 'osv-scanner', outcome: 'findings', summary: '3 blocking finding(s)', totalFindings: 3, blockingFindings: 3, bySeverity: { high: 1, medium: 1, unknown: 1 }, findingsTruncated: false,
        findings: [
          { ruleId: 'GHSA-35jh-r3h4-6jhm', severity: 'high', title: 'Command Injection in lodash', location: 'package-lock.json', package: 'lodash@4.17.20', fixedIn: null, blocking: true },
          { ruleId: 'OSV-2026-1', severity: 'unknown', title: 'No severity published', location: 'package-lock.json', package: 'mystery@0.0.1', fixedIn: null, blocking: true },
          { ruleId: 'GHSA-29mw-wpgm-hmr9', severity: 'medium', title: 'ReDoS in lodash', location: 'package-lock.json', package: 'lodash@4.17.20', fixedIn: null, blocking: true },
        ],
      },
    }),
  ];
  return runs;
}

const verdicts: Record<string, any> = {};

function reportFor(id: string): VerificationReport {
  const contract = contracts[id] ?? emptyContract();
  const files = ctxRef.changesFor(id).files;
  if (id !== 'm2') {
    return {
      fingerprint: 'none', contractSha256: CSHA, headCommit: null, files, agentInstructionFiles: [], scope: { outsideInScope: [], inOutOfScope: [] }, scopeConfigured: false,
      criteria: [], checks: [], blockingChecks: [], unresolved: [], contract,
      hardBlockers: ctxRef.isRunning(id) ? ['The agent is still running.'] : [],
    };
  }
  const latest = (cid: string) => [...allRuns()].reverse().find((r) => r.checkId === cid) ?? null;
  const checks: CheckReport[] = contract.checks.map((spec) => {
    const l = latest(spec.id);
    return { spec, latest: l, fresh: !!l && l.fingerprint === CUR && !l.mutated, runs: allRuns().filter((r) => r.checkId === spec.id).length, touchedInputs: spec.id === 'typecheck' ? ['package.json', 'tsconfig.json'] : [] };
  });
  const v = verdicts.docs ?? null;
  const c = contract.criteria;
  const criteria: CriterionReport[] = [
    { id: 'tz', text: c[0].text, manual: false, scopeRule: false, verifiedBy: ['unit'], manualVerdict: null, status: { status: 'verified_by_checks', evidence: ['unit'] } },
    { id: 'lint-clean', text: c[1].text, manual: false, scopeRule: false, verifiedBy: ['lint'], manualVerdict: null, status: { status: 'inconclusive', reason: 'Lint last ran against earlier changes.' } },
    { id: 'api', text: c[2].text, manual: false, scopeRule: false, verifiedBy: ['typecheck'], manualVerdict: null, status: { status: 'failed', reason: 'Typecheck exited with code 2 on the current changes.' } },
    {
      id: 'docs', text: c[3].text, manual: true, scopeRule: false, verifiedBy: [], manualVerdict: v,
      status: v ? { status: 'verified_manually', note: v.note } : { status: 'requires_manual_review', reason: 'A person must judge this; no check can.' },
    },
    { id: 'scope', text: c[4].text, manual: false, scopeRule: true, verifiedBy: [], manualVerdict: null, status: { status: 'failed', reason: '1 changed file is outside the in-scope paths.' } },
  ];
  const unresolved = [
    'Required check "Typecheck" fails on the current changes.',
    'Check "Lint" ran against earlier changes (re-run to refresh).',
    ...(v ? [] : ['Criterion "The new docs page reads correctly to a newcomer" needs a manual verdict.']),
    '1 changed file is outside the in-scope paths: docs/dates.md',
  ];
  return {
    fingerprint: CUR + 'e1', contractSha256: CSHA, headCommit: null, files, agentInstructionFiles: [], scope: { outsideInScope: ['docs/dates.md'], inOutOfScope: [] }, scopeConfigured: true,
    criteria, checks, blockingChecks: ['typecheck'], unresolved, hardBlockers: [], contract,
  };
}

let packs: ProofPack[] | null = null;
function allPacks(): ProofPack[] {
  packs ??= [
    {
      id: 'pp1', missionId: 'm2', createdAt: ctxRef.iso(10), contentSha256: '9b1f3c5d7e2a4c6e8091a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f7',
      diffPath: 'C:\\Users\\dev\\.forge\\proof\\m2\\pp1.diff', content: {},
      markdown:
        '# Proof Pack: Fix flaky date parsing test\n\nA Proof Pack records evidence; it is not a guarantee of correctness.\n\n## Criteria\n\n| Criterion | Status |\n| --- | --- |\n| Date parsing gives the same result in every timezone | Verified by Unit tests |\n| The public parseDate signature is unchanged | Failed |\n| The new docs page reads correctly | Requires manual review |\n\n## Not established\n\n- Required check "Typecheck" fails on the current changes.\n- 1 changed file is outside the in-scope paths: docs/dates.md\n\n```\nnpm test: passed (exit 0)\n```\n',
    },
  ];
  return packs;
}

/** Returns undefined when the command is not a Phase 2 command. */
export function handleP2(cmd: string, args: any, ctx: P2Ctx): Promise<any> | undefined {
  ctxRef = ctx;
  const done = (v: any) => Promise.resolve(v);
  switch (cmd) {
    case 'contract_get':
      return done(contracts[args.missionId] ?? emptyContract());
    case 'contract_set': {
      contracts[args.missionId] = args.contract;
      const m = ctx.find(args.missionId)!;
      m.updatedAt = new Date().toISOString();
      return done({ ...m });
    }
    case 'checks_suggest':
      return done([
        { id: 'tests', name: 'Tests', command: 'npm test', kind: 'test', timeoutSecs: 300, required: true },
        { id: 'typecheck', name: 'Typecheck', command: 'npm run typecheck', kind: 'typecheck', timeoutSecs: 180, required: false },
        { id: 'lint', name: 'Lint', command: 'npm run lint', kind: 'lint', timeoutSecs: 120, required: false },
      ]);
    case 'verification_adapters':
      return done([
        { id: 'gitleaks', label: 'Secret scan (gitleaks)', command: 'gitleaks dir . --redact --no-banner --report-format json --report-path -', kind: 'security' },
        { id: 'osv-scanner', label: 'Dependency vulnerabilities (OSV-Scanner)', command: 'osv-scanner scan source --format json -r .', kind: 'security' },
        { id: 'trivy', label: 'Vulnerabilities, secrets, misconfiguration (Trivy)', command: 'trivy fs --quiet --format json --scanners vuln,secret,misconfig .', kind: 'security' },
        { id: 'playwright', label: 'End-to-end tests (Playwright)', command: 'npx playwright test --reporter=json', kind: 'test' },
      ]);
    case 'verification_report':
      return done(reportFor(args.missionId));
    case 'verification_runs':
      return done(allRuns().filter((r) => r.missionId === args.missionId));
    case 'check_start': {
      const spec = contracts[args.missionId]?.checks.find((c) => c.id === args.checkId);
      if (!spec) return ctx.err('unknown check');
      const r = vrun(spec.id, spec.name, spec.command, 'running', null, CUR, '', { id: `vr-live-${Date.now()}`, startedAt: new Date().toISOString(), endedAt: null, durationMs: null });
      allRuns().push(r);
      ctx.emit({ type: 'verification', run: { ...r } });
      setTimeout(() => {
        Object.assign(r, { state: 'passed', exitCode: 0, endedAt: new Date().toISOString(), durationMs: 1500, outputTail: 'preview run finished\n' });
        ctx.emit({ type: 'verification', run: { ...r } });
      }, 1500);
      return done({ ...r });
    }
    case 'check_cancel':
      return done(null);
    case 'verdict_set':
      verdicts[args.criterionId] = { criterionId: args.criterionId, pass: args.pass, note: args.note, fingerprint: CUR, decidedBy: 'user', at: new Date().toISOString() };
      ctx.emit({ type: 'verification', run: { ...allRuns()[0] } });
      return done(null);
    case 'review_submit': {
      const m = ctx.find(args.missionId)!;
      if (args.decision === 'approve' && (!args.seen || args.seen.fingerprint !== CUR + 'e1' || args.seen.contractSha256 !== CSHA)) {
        return ctx.err('the changes (or the success criteria) are not the ones you reviewed; review them again');
      }
      m.state = args.decision === 'approve' ? 'approved' : args.decision === 'reject' ? 'rejected' : 'running';
      m.updatedAt = new Date().toISOString();
      return done({ ...m });
    }
    case 'mission_merge': {
      const m = ctx.find(args.missionId)!;
      m.state = 'merged';
      m.updatedAt = new Date().toISOString();
      return done({ commit: 'a1b2c3d4e5f60718293a4b5c6d7e8f9012345678', branch: m.branch, target: 'main', squashed: !!args.squash, cleanedUp: false, notes: ['Worktree left in place; clean it up when you no longer need it.'] });
    }
    case 'mission_cleanup':
      if (!args.confirmLoss) return ctx.err('This will permanently delete 1 unmerged commit in the mission worktree. This cannot be undone.');
      return done({ exists: false, dirtyFiles: 0, unmergedCommits: 0 });
    case 'proofpack_list':
      return done(allPacks().filter((p) => p.missionId === args.missionId));
    case 'proofpack_generate': {
      const p: ProofPack = { ...allPacks()[0], id: `pp${allPacks().length + 1}`, createdAt: new Date().toISOString(), contentSha256: Math.random().toString(16).slice(2).padEnd(64, '0') };
      allPacks().push(p);
      return done(p);
    }
    default:
      return undefined;
  }
}
