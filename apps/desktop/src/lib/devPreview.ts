// DEV-ONLY visual preview bridge. Imported dynamically from main.tsx behind
// `import.meta.env.DEV && !__TAURI_INTERNALS__`, so it never ships in production bundles.
// Marker for the production-bundle check: FORGE_DEVPREVIEW_MARKER_7f3a91
/* eslint-disable @typescript-eslint/no-explicit-any */
import { handleP2 } from './devPreviewP2';
import { handleP3 } from './devPreviewP3';
import { handleP4 } from './devPreviewP4';
import { handleP6 } from './devPreviewP6';
import type {
  ActivityEvent, AgentSession, Approval, CapabilityMatrix, Mission, MissionDetail, Project, ProviderInfo, RepoStatus,
} from './types';

const w = window as any;
const iso = (minsAgo: number) => new Date(Date.now() - minsAgo * 60_000).toISOString();

// ---- banner ---------------------------------------------------------------------------------------
const banner = document.createElement('div');
banner.textContent = 'PREVIEW DATA — not connected to the FORGE core';
banner.setAttribute('role', 'status');
banner.style.cssText =
  'position:fixed;top:0;left:0;right:0;height:24px;z-index:9999;display:flex;align-items:center;justify-content:center;' +
  'background:#5c3b0a;color:#ffe2b0;font:600 12px "Segoe UI",system-ui,sans-serif;letter-spacing:.06em;border-bottom:1px solid #8a5a14';
document.body.appendChild(banner);
document.documentElement.style.setProperty('--banner-h', '24px');

// ---- dataset --------------------------------------------------------------------------------------
const projects: Project[] = [
  { id: 'p1', name: 'atlas-api', rootPath: 'C:\\dev\\atlas-api', defaultBranch: 'main', createdAt: iso(60 * 24 * 12) , executionBackend: 'trusted_local' },
  { id: 'p2', name: 'docs-site', rootPath: 'C:\\dev\\docs-site', defaultBranch: 'master', createdAt: iso(60 * 24 * 5) , executionBackend: 'trusted_local' },
];

const mk = (o: Partial<Mission> & Pick<Mission, 'id' | 'projectId' | 'title' | 'state'>): Mission => ({
  objective: o.title, provider: 'claude', model: null, permissionProfile: 'default', baseRef: 'main',
  baseCommit: '9c1e4f7a2b3d5e6f708192a3b4c5d6e7f8091a2b', branch: `forge/${o.id}`, worktreePath: `C:\\Users\\dev\\.forge\\wt\\${o.id}`,
  providerSessionId: null, lastError: null, createdAt: iso(300), updatedAt: iso(5), useMemory: true, memoryPins: [], ...o,
});

const missions: Mission[] = [
  mk({ id: 'm1', projectId: 'p1', title: 'Add retry with exponential backoff to the HTTP client', state: 'awaiting_approval', model: 'claude-sonnet-4-5', updatedAt: iso(1),
    objective: 'Add retry with exponential backoff and jitter to src/http/client.ts. Retry only idempotent requests, cap at 5 attempts, and add unit tests.' }),
  mk({ id: 'm2', projectId: 'p1', title: 'Fix flaky date parsing test', state: 'awaiting_review', updatedAt: iso(22), permissionProfile: 'accept_edits' }),
  mk({ id: 'm3', projectId: 'p1', title: 'Migrate config loader to zod schemas', state: 'failed', updatedAt: iso(140),
    lastError: 'Agent turn ended with error_max_turns\n\nThe provider stopped after 40 turns without completing the task.\nLast tool call: Bash `npm run typecheck` (exit 2)' }),
  mk({ id: 'm4', projectId: 'p1', title: 'Refactor logger to structured output', state: 'interrupted', updatedAt: iso(300) }),
  mk({ id: 'm5', projectId: 'p2', title: 'Document the public plugin API', state: 'ready', updatedAt: iso(12), worktreePath: null, branch: null, baseCommit: null }),
  mk({ id: 'm6', projectId: 'p2', title: 'Bump docusaurus to latest minor', state: 'merged', updatedAt: iso(60 * 30) }),
  mk({ id: 'm7', projectId: 'p2', title: 'Rewrite search indexing script', state: 'running', updatedAt: iso(48) }),
];
const runningIds = new Set(['m1']);

const sess = (id: string, missionId: string, state: AgentSession['state'], startMin: number, endMin: number | null, extra: Partial<AgentSession> = {}): AgentSession => ({
  id, missionId, provider: 'claude', providerSessionId: `ps-${id}`, state, pid: state === 'live' ? 18432 : null,
  startedAt: iso(startMin), endedAt: endMin === null ? null : iso(endMin), endReason: null, usage: null, ...extra,
});
const sessions: AgentSession[] = [
  sess('s1', 'm1', 'live', 41, null),
  sess('s2', 'm2', 'ended', 90, 25, { endReason: 'completed', usage: { inputTokens: 48210, outputTokens: 6120, cacheReadTokens: 210400, cacheCreationTokens: 9100, costUsdEstimate: 0.412, durationMs: 3_540_000, numTurns: 14 } }),
  sess('s3', 'm3', 'ended', 200, 142, { endReason: 'error_max_turns' }),
  sess('s4', 'm4', 'lost', 420, null, { endReason: 'agent process disappeared' }),
  sess('s7', 'm7', 'lost', 120, null),
];

const approvals: Approval[] = [
  {
    id: 'a1', missionId: 'm1', sessionId: 's1', providerRequestId: 'r1', tool: 'Bash', summary: 'npm test -- --runInBand src/http',
    detail: { tool: 'Bash', input: { command: 'npm test -- --runInBand src/http', description: 'Run the HTTP client unit tests', timeout: 120000 }, source: 'can_use_tool' },
    risk: 'medium', warnings: ['Runs a project script: package.json scripts can execute arbitrary code.', 'Command is not on the allow-list for this profile.'],
    state: 'pending', decidedBy: null, decisionNote: null, createdAt: iso(1), decidedAt: null, bindingSha256: null, policyRev: null, executesOn: null, expiresAt: null,
  },
  {
    id: 'a0', missionId: 'm1', sessionId: 's1', providerRequestId: 'r0', tool: 'Bash', summary: 'git push --force origin HEAD',
    detail: { tool: 'Bash', input: { command: 'git push --force origin HEAD' }, source: 'hook' },
    risk: 'high', warnings: ['Pushes to a remote.'], state: 'auto_denied', decidedBy: 'policy', decisionNote: 'Blocked by policy: pushing to remotes is not permitted inside a mission worktree.',
    createdAt: iso(18), decidedAt: iso(18), bindingSha256: null, policyRev: null, executesOn: null, expiresAt: null,
  },
  {
    id: 'a2', missionId: 'm1', sessionId: 's1', providerRequestId: 'r2', tool: 'Edit', summary: 'src/http/client.ts',
    detail: { tool: 'Edit', input: { file_path: 'src/http/client.ts', old_string: 'return fetch(url, init);', new_string: 'return withRetry(() => fetch(url, init), opts.retry);' } },
    risk: 'low', warnings: [], state: 'approved', decidedBy: 'user', decisionNote: null, createdAt: iso(30), decidedAt: iso(29), bindingSha256: null, policyRev: null, executesOn: null, expiresAt: null,
  },
  {
    id: 'a3', missionId: 'm7', sessionId: 's7', providerRequestId: 'r3', tool: 'Bash', summary: 'node scripts/build-index.mjs',
    detail: { tool: 'Bash', input: { command: 'node scripts/build-index.mjs' } },
    risk: 'low', warnings: [], state: 'pending', decidedBy: null, decisionNote: null, createdAt: iso(50), decidedAt: null, bindingSha256: null, policyRev: null, executesOn: null, expiresAt: null,
  },
];

let evId = 0;
const events: Record<string, ActivityEvent[]> = {};
const ev = (missionId: string, sessionId: string, minsAgo: number, kind: ActivityEvent['kind'], payload: Record<string, any>): ActivityEvent => {
  const e: ActivityEvent = { id: ++evId, missionId, sessionId, seq: evId, kind, payload, at: iso(minsAgo) };
  (events[missionId] ??= []).push(e);
  return e;
};

ev('m1', 's1', 41, 'session.started', { providerSessionId: 'ps-s1', model: 'claude-sonnet-4-5', claudeCodeVersion: '2.1.4', credentialSource: 'none', cwd: missions[0].worktreePath });
ev('m1', 's1', 40, 'agent.text', { messageId: 'x1', text: 'I will start by reading the existing HTTP client and its tests to see how requests are issued.' });
ev('m1', 's1', 40, 'agent.thinking', { messageId: 'x1', text: 'The client wraps fetch directly. Retries must only apply to idempotent methods (GET, HEAD, PUT, DELETE, OPTIONS) and honor Retry-After.' });
ev('m1', 's1', 39, 'tool.call', { toolUseId: 't1', name: 'Read', summary: 'src/http/client.ts', input: { file_path: 'src/http/client.ts' } });
ev('m1', 's1', 39, 'tool.result', { toolUseId: 't1', name: 'Read', ok: true, output: "export async function request(url: string, init: RequestInit = {}) {\n  const res = await fetch(url, init);\n  if (!res.ok) throw new HttpError(res.status);\n  return res;\n}\n" });
ev('m1', 's1', 38, 'tool.call', { toolUseId: 't2', name: 'Grep', summary: 'request( in src/', input: { pattern: 'request\\(', path: 'src' } });
ev('m1', 's1', 38, 'tool.result', { toolUseId: 't2', name: 'Grep', ok: true, output: 'src/http/client.ts:1\nsrc/api/users.ts:14\nsrc/api/orders.ts:22\n' });
ev('m1', 's1', 33, 'agent.text', { messageId: 'x2', text: 'Plan:\n1. Add a `withRetry` helper in `src/http/retry.ts`.\n2. Use it from `request` for idempotent methods only.\n\n```ts\nexport async function withRetry<T>(fn: () => Promise<T>, o: RetryOptions): Promise<T> {\n  for (let attempt = 1; ; attempt++) {\n    try { return await fn(); } catch (e) { if (attempt >= o.max) throw e; await sleep(backoff(attempt)); }\n  }\n}\n```' });
ev('m1', 's1', 30, 'approval.requested', { approvalId: 'a2', tool: 'Edit', summary: 'src/http/client.ts', risk: 'low' });
ev('m1', 's1', 29, 'approval.decided', { approvalId: 'a2', tool: 'Edit', decision: 'approved', note: null });
ev('m1', 's1', 29, 'tool.call', { toolUseId: 't3', name: 'Edit', summary: 'src/http/client.ts', input: { file_path: 'src/http/client.ts', old_string: 'return fetch(url, init);', new_string: 'return withRetry(() => fetch(url, init), opts.retry);' } });
ev('m1', 's1', 29, 'tool.result', { toolUseId: 't3', name: 'Edit', ok: true, output: 'The file src/http/client.ts has been updated.' });
ev('m1', 's1', 29, 'file.changed', { path: 'src/http/client.ts', tool: 'Edit', toolUseId: 't3' });
ev('m1', 's1', 22, 'provider.rate_limit', { status: 'allowed_warning', rateLimitType: 'five_hour', utilization: 0.74, resetsAt: Math.floor((Date.now() + 90 * 60_000) / 1000) });
ev('m1', 's1', 18, 'approval.requested', { approvalId: 'a0', tool: 'Bash', summary: 'git push --force origin HEAD', risk: 'high' });
ev('m1', 's1', 18, 'approval.decided', { approvalId: 'a0', tool: 'Bash', decision: 'auto_denied', note: 'Blocked by policy: pushing to remotes is not permitted inside a mission worktree.' });
ev('m1', 's1', 17, 'tool.call', { toolUseId: 't4', name: 'Bash', summary: 'git push --force origin HEAD', input: { command: 'git push --force origin HEAD' } });
ev('m1', 's1', 17, 'tool.result', { toolUseId: 't4', name: 'Bash', ok: false, output: 'denied: Blocked by policy', truncated: false });
ev('m1', 's1', 16, 'provider.retry', { attempt: 1, maxRetries: 5, delayMs: 2000, status: 529 });
ev('m1', 's1', 2, 'agent.text', { messageId: 'x3', text: 'The helper and the call site are in place. I need to run the unit tests to validate the retry behavior.' });
ev('m1', 's1', 1, 'approval.requested', { approvalId: 'a1', tool: 'Bash', summary: 'npm test -- --runInBand src/http', risk: 'medium' });

ev('m2', 's2', 90, 'session.started', { providerSessionId: 'ps-s2', model: 'claude-sonnet-4-5', claudeCodeVersion: '2.1.4', credentialSource: 'none' });
ev('m2', 's2', 88, 'agent.text', { messageId: 'y1', text: 'The test fails when run in a timezone ahead of UTC because parseDate uses local midnight. I will make the parser UTC-based.' });
ev('m2', 's2', 80, 'tool.call', { toolUseId: 'u1', name: 'Edit', summary: 'src/date/parse.ts', input: { file_path: 'src/date/parse.ts' } });
ev('m2', 's2', 80, 'tool.result', { toolUseId: 'u1', name: 'Edit', ok: true, output: 'updated' });
ev('m2', 's2', 80, 'file.changed', { path: 'src/date/parse.ts', tool: 'Edit', toolUseId: 'u1' });
ev('m2', 's2', 26, 'turn.completed', { status: 'success', message: 'Fixed the flaky test by normalizing to UTC.', usage: sessions[1].usage, permissionDenials: [] });

ev('m3', 's3', 200, 'session.started', { providerSessionId: 'ps-s3', model: 'claude-sonnet-4-5', claudeCodeVersion: '2.1.4', credentialSource: 'ANTHROPIC_API_KEY' });
ev('m3', 's3', 143, 'error', { message: 'error_max_turns: the provider stopped after 40 turns' });
ev('m3', 's3', 142, 'turn.completed', { status: 'error_max_turns', message: 'Turn limit reached', usage: null, permissionDenials: [] });

const transitions: Record<string, MissionDetail['transitions']> = {
  m1: [
    { fromState: 'draft', toState: 'ready', reason: 'created', at: iso(45) },
    { fromState: 'ready', toState: 'preparing', reason: 'worktree created from main @ 9c1e4f7a', at: iso(42) },
    { fromState: 'preparing', toState: 'running', reason: 'agent session started', at: iso(41) },
    { fromState: 'running', toState: 'awaiting_approval', reason: 'Bash needs approval', at: iso(1) },
  ],
};

const DIFF = `diff --git a/src/date/parse.ts b/src/date/parse.ts
index 3a1f2c4..8b7d9e0 100644
--- a/src/date/parse.ts
+++ b/src/date/parse.ts
@@ -1,12 +1,14 @@
 import { InvalidDateError } from './errors';

-export function parseDate(input: string): Date {
-  const [y, m, d] = input.split('-').map(Number);
-  const date = new Date(y, m - 1, d);
+/** Parses an ISO calendar date (YYYY-MM-DD) as UTC midnight, independent of the host timezone. */
+export function parseDate(input: string): Date {
+  const [y, m, d] = input.split('-').map(Number);
+  const date = new Date(Date.UTC(y, m - 1, d));
   if (Number.isNaN(date.getTime())) {
     throw new InvalidDateError(input);
   }
   return date;
 }
@@ -40,6 +42,7 @@ export function formatDate(date: Date): string {
   const y = date.getUTCFullYear();
   const m = String(date.getUTCMonth() + 1).padStart(2, '0');
   const d = String(date.getUTCDate()).padStart(2, '0');
+  // Always UTC so parse/format round-trip.
   return \`\${y}-\${m}-\${d}\`;
 }
diff --git a/src/date/parse.test.ts b/src/date/parse.test.ts
index 11aa22b..33cc44d 100644
--- a/src/date/parse.test.ts
+++ b/src/date/parse.test.ts
@@ -3,8 +3,12 @@ import { parseDate, formatDate } from './parse';

 describe('parseDate', () => {
-  it('parses a date', () => {
-    expect(parseDate('2024-03-09').getDate()).toBe(9);
-  });
+  it('parses a date as UTC midnight', () => {
+    expect(parseDate('2024-03-09').toISOString()).toBe('2024-03-09T00:00:00.000Z');
+  });
+
+  it('round-trips through formatDate in any timezone', () => {
+    expect(formatDate(parseDate('2024-12-31'))).toBe('2024-12-31');
+  });
 });
diff --git a/docs/dates.md b/docs/dates.md
new file mode 100644
index 0000000..5d0e1f2
--- /dev/null
+++ b/docs/dates.md
@@ -0,0 +1,3 @@
+# Dates
+
+All calendar dates are handled as UTC midnight.
diff --git a/src/util/old-name.ts b/src/util/new-name.ts
similarity index 94%
rename from src/util/old-name.ts
rename to src/util/new-name.ts
index 7e7e7e7..8f8f8f8 100644
--- a/src/util/old-name.ts
+++ b/src/util/new-name.ts
@@ -1 +1 @@
-export const NAME = 'old';
+export const NAME = 'new';
\\ No newline at end of file
diff --git a/assets/logo.png b/assets/logo.png
index 1234567..89abcde 100644
Binary files a/assets/logo.png and b/assets/logo.png differ
`;

const changesFor = (id: string) =>
  id === 'm2' || id === 'm1'
    ? {
        files: [
          { path: 'src/date/parse.ts', added: 4, removed: 3, binary: false },
          { path: 'src/date/parse.test.ts', added: 6, removed: 3, binary: false },
          { path: 'docs/dates.md', added: 3, removed: 0, binary: false },
          { path: 'src/util/new-name.ts', added: 1, removed: 1, binary: false },
          { path: 'assets/logo.png', added: null, removed: null, binary: true },
        ],
        safety: { exists: true, dirtyFiles: 5, unmergedCommits: id === 'm2' ? 1 : 0 },
      }
    : { files: [], safety: { exists: !!missions.find((m) => m.id === id)?.worktreePath, dirtyFiles: 0, unmergedCommits: 0 } };

const cap = (supported: boolean, note?: string) => ({ supported, note });
const claudeCaps: CapabilityMatrix = {
  sessionStart: cap(true), resume: cap(true), streaming: cap(true), interrupt: cap(true), toolEvents: cap(true), fileChanges: cap(true, 'Derived from file-editing tool results'),
  approvals: cap(true), usageReporting: cap(true, 'Estimates reported by the CLI'), modelSelection: cap(true), worktreeIntegration: cap(true),
};
const codexCaps: CapabilityMatrix = {
  sessionStart: cap(false, 'No adapter yet'), resume: cap(false), streaming: cap(false), interrupt: cap(false), toolEvents: cap(false), fileChanges: cap(false),
  approvals: cap(false), usageReporting: cap(false), modelSelection: cap(false), worktreeIntegration: cap(true),
};
const providers: ProviderInfo[] = [
  { id: 'claude', name: 'Claude Code', installed: true, path: 'C:\\Users\\dev\\AppData\\Roaming\\npm\\claude.cmd', version: '2.1.4', signedIn: true, authMethod: 'claude.ai', plan: 'max', adapterAvailable: true, notes: [], models: [], capabilities: claudeCaps },
  { id: 'claude-local', name: 'Claude Code (local model)', installed: true, path: 'C:\Users\dev\AppData\Roaming\npm\claude.cmd', version: '2.1.4', signedIn: null, authMethod: null, plan: null, adapterAvailable: true, notes: ['Runs the Claude Code harness against a local Ollama model on loopback. Models need a context window of at least 16k tokens; forge-* tags are prepared for this.'], models: ['forge-qwen2.5-coder-32k', 'qwen2.5-coder:7b'], capabilities: claudeCaps },
  { id: 'codex', name: 'Codex', installed: true, path: 'C:\\Users\\dev\\AppData\\Roaming\\npm\\codex.cmd', version: '0.40.0', signedIn: true, authMethod: 'chatgpt', plan: null, adapterAvailable: false, notes: ['Detected, but FORGE has no Codex adapter in this build.'], models: [], capabilities: codexCaps },
];

const statusFor = (id: string): RepoStatus =>
  id === 'p1'
    ? { branch: 'main', head: '9c1e4f7', upstream: 'origin/main', ahead: 0, behind: 2, files: [], staged: 0, unstaged: 3, untracked: 1, conflicted: 0, dirty: true }
    : { branch: 'master', head: '41b7aa2', upstream: 'origin/master', ahead: 1, behind: 0, files: [], staged: 0, unstaged: 0, untracked: 0, conflicted: 0, dirty: false };

// ---- bridge ---------------------------------------------------------------------------------------
const handlers = new Map<number, (e: any) => void>();
let nextCb = 1;
const subs: { event: string; handler: number }[] = [];
const emit = (payload: any) => {
  for (const s of subs) if (s.event === 'forge://event') handlers.get(s.handler)?.({ event: s.event, id: s.handler, payload });
};

const err = (m: string) => Promise.reject(m);
const find = (id: string) => missions.find((m) => m.id === id);

async function invoke(cmd: string, args: any = {}): Promise<any> {
  const p2 = handleP2(cmd, args, { iso, find, changesFor, isRunning: (id) => runningIds.has(id), emit, err });
  if (p2) return p2;
  const p3 = handleP3(cmd, args, { iso, find, err });
  if (p3) return p3;
  const p4 = handleP4(cmd, args);
  if (p4) return p4;
  const p6 = handleP6(cmd, args);
  if (p6) return p6;
  switch (cmd) {
    case 'plugin:event|listen':
      subs.push({ event: args.event, handler: args.handler });
      return subs.length;
    case 'plugin:event|unlisten':
      return null;
    case 'plugin:dialog|open':
      return 'C:\\dev\\new-repo';
    case 'app_info':
      return { version: '0.1.0', home: 'C:\\Users\\dev\\.forge', gitVersion: 'git version 2.47.1.windows.1', nodeFound: true, engineFound: true, claudeFound: true, scriptedAgentEnabled: false, maxConcurrent: 3, liveSessions: runningIds.size };
    case 'recovery_report':
      return { interruptedMissions: ['m4'], lostSessions: 2, expiredApprovals: 0 };
    case 'providers_detect':
      return providers;
    case 'project_list':
      return projects;
    case 'project_add': {
      const name = String(args.path).split(/[\\/]/).pop() ?? 'repo';
      const p: Project = { id: `p${projects.length + 1}`, name, rootPath: args.path, defaultBranch: 'main', createdAt: new Date().toISOString(), executionBackend: 'trusted_local' };
      projects.push(p);
      return p;
    }
    case 'project_remove': {
      const i = projects.findIndex((p) => p.id === args.projectId);
      if (i >= 0) projects.splice(i, 1);
      return null;
    }
    case 'project_status':
      return statusFor(args.projectId);
    case 'mission_list':
      return args.projectId ? missions.filter((m) => m.projectId === args.projectId) : missions;
    case 'mission_get': {
      const m = find(args.missionId);
      if (!m) return err(`mission not found: ${args.missionId}`);
      const detail: MissionDetail = {
        mission: m, project: projects.find((p) => p.id === m.projectId)!, sessions: sessions.filter((s) => s.missionId === m.id),
        transitions: transitions[m.id] ?? [{ fromState: 'draft', toState: 'ready', reason: 'created', at: m.createdAt }, ...(m.state === 'ready' ? [] : [{ fromState: 'ready' as const, toState: m.state, reason: null, at: m.updatedAt }])],
        approvals: approvals.filter((a) => a.missionId === m.id), running: runningIds.has(m.id),
      };
      return detail;
    }
    case 'mission_events':
      return (events[args.missionId] ?? []).filter((e) => e.id > (args.afterId ?? 0)).slice(0, args.limit ?? 500);
    case 'mission_changes':
      return changesFor(args.missionId);
    case 'mission_diff':
      return changesFor(args.missionId).files.length ? { diff: DIFF, truncated: args.missionId === 'm1' } : { diff: '', truncated: false };
    case 'approvals_pending':
      return approvals.filter((a) => a.state === 'pending' && (!args.missionId || a.missionId === args.missionId));
    case 'approval_decide': {
      const a = approvals.find((x) => x.id === args.approvalId);
      if (!a) return err('approval not found');
      a.state = args.approved ? 'approved' : 'denied';
      a.decidedBy = 'user';
      a.decisionNote = args.note;
      a.decidedAt = new Date().toISOString();
      emit({ type: 'approval', approval: { ...a } });
      const m = find(a.missionId);
      if (m && !approvals.some((x) => x.missionId === m.id && x.state === 'pending')) {
        m.state = 'running';
        m.updatedAt = new Date().toISOString();
        emit({ type: 'mission', mission: { ...m } });
      }
      emit({ type: 'activity', event: ev(a.missionId, a.sessionId, 0, 'approval.decided', { approvalId: a.id, tool: a.tool, decision: a.state, note: a.decisionNote }) });
      return { ...a };
    }
    case 'mission_create': {
      const a = args.args;
      const m = mk({ id: `m${missions.length + 1}`, projectId: a.projectId, title: a.title || String(a.objective).slice(0, 60), objective: a.objective, state: 'ready', provider: a.provider, model: a.model ?? null, permissionProfile: a.permissionProfile ?? 'default', worktreePath: null, branch: null, baseCommit: null, updatedAt: new Date().toISOString() });
      missions.push(m);
      return m;
    }
    case 'mission_start': {
      const m = find(args.missionId);
      if (!m) return err('mission not found');
      m.state = 'running';
      m.updatedAt = new Date().toISOString();
      return { ...m };
    }
    case 'mission_discard': {
      const m = find(args.missionId);
      if (!m) return err('mission not found');
      if (!args.confirmLoss && changesFor(m.id).safety.dirtyFiles > 0) {
        return err(`This will permanently delete 5 uncommitted file changes${changesFor(m.id).safety.unmergedCommits ? ' and 1 unmerged commit' : ''} in the mission worktree. This cannot be undone.`);
      }
      m.state = 'cancelled';
      m.updatedAt = new Date().toISOString();
      return { ...m };
    }
    case 'mission_followup':
    case 'mission_stop':
    case 'open_worktree_in_editor':
      return find(args.missionId) ?? null;
    default:
      return err(`preview bridge: unhandled command ${cmd}`);
  }
}

w.__TAURI_INTERNALS__ = {
  invoke,
  transformCallback(cb: (e: any) => void) {
    const id = nextCb++;
    handlers.set(id, cb);
    return id;
  },
  unregisterCallback(id: number) {
    handlers.delete(id);
  },
  convertFileSrc: (p: string) => p,
  metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main', windowLabel: 'main' } },
};
w.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
