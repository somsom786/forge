// DEV-ONLY: Phase 4 (Harness Studio) preview data. Imported only by devPreview.ts.
// Marker for the production-bundle check: FORGE_DEVPREVIEW_MARKER_7f3a91
/* eslint-disable @typescript-eslint/no-explicit-any */
import type {
  McpAuthorization, McpDefinition,
  DoctorFinding, HarnessChange, HarnessFile, HarnessInventory, HarnessItem, HarnessPreview, HookDef, HookDryRun, HookTemplate, RollbackPreview,
} from './types';

const H = (s: string) => (s + 'abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789').slice(0, 64);
const ago = (m: number) => new Date(Date.now() - m * 60_000).toISOString();

const files: Record<string, string> = {
  'CLAUDE.md': '# Project instructions\n\n- Run `npm test` before finishing.\n- Prefer small, reviewable commits.\n',
  '.claude/settings.json': '{\n  "hooks": {\n    "PreToolUse": [\n      { "matcher": "Bash", "hooks": [ { "type": "command", "command": "echo checking", "timeout": 10 } ] }\n    ]\n  }\n}\n',
  '.claude/rules/testing.md': '# Testing\n\nEvery bug fix needs a regression test.\n',
};

const item = (o: Partial<HarnessItem> & Pick<HarnessItem, 'id' | 'kind' | 'name'>): HarnessItem => ({
  scope: 'project', path: `C:\\dev\\atlas-api\\${o.rel ?? o.name}`, rel: o.name, description: null, bytes: 200, sha256: H(o.id), editable: true,
  loadedInMissions: true, warnings: [], details: {}, ...o,
});

let changes: HarnessChange[] = [
  { id: 'c1', projectId: 'p1', rel: 'CLAUDE.md', kind: 'edit', beforeSha256: H('a'), afterSha256: H('b'), backupPath: null, rollsBackId: null, note: 'Add testing guidance', state: 'applied', createdAt: ago(300), finishedAt: ago(300), rolledBackBy: null },
  { id: 'c2', projectId: 'p1', rel: '.claude/rules/testing.md', kind: 'create', beforeSha256: null, afterSha256: H('c'), backupPath: null, rollsBackId: null, note: null, state: 'applied', createdAt: ago(100), finishedAt: ago(100), rolledBackBy: null },
];

const settings = item({ id: 'i2', kind: 'settings', name: '.claude/settings.json', rel: '.claude/settings.json', loadedInMissions: false, warnings: ['Contains a hook that runs a shell command.'] });
const hooks = (): HookDef[] => [
  { id: 'i2#PreToolUse#0#0', event: 'PreToolUse', matcher: 'Bash', kind: 'command', command: 'echo checking', timeoutSecs: 10, sourceItem: 'i2', scope: 'project', knownEvent: true, runsInForgeMissions: false, dryRunSupported: true },
  { id: 'i2#Stop#0#0', event: 'Stop', matcher: null, kind: 'prompt', command: 'Summarise what changed', timeoutSecs: null, sourceItem: 'i2', scope: 'project', knownEvent: true, runsInForgeMissions: false, dryRunSupported: false },
];

const inventory = (): HarnessInventory => ({
  items: [
    item({ id: 'i1', kind: 'instructions', name: 'CLAUDE.md', rel: 'CLAUDE.md', description: 'Project instructions' }),
    settings,
    item({ id: 'i3', kind: 'rule', name: 'testing', rel: '.claude/rules/testing.md' }),
    item({ id: 'i4', kind: 'instructions', name: 'CLAUDE.md', scope: 'user', rel: null, path: 'C:\\Users\\dev\\.claude\\CLAUDE.md', editable: false, loadedInMissions: false }),
    item({ id: 'i5', kind: 'mcp', name: '.mcp.json', rel: '.mcp.json', loadedInMissions: false }),
  ],
  hooks: hooks(),
  mcpServers: [{ name: 'docs', transport: 'stdio', command: 'npx docs-mcp', url: null, sourceItem: 'i5', scope: 'project', runsInForgeMissions: false }],
  notes: ['FORGE missions read CLAUDE.md and rules from the project. Settings and hooks are not loaded by FORGE missions; a .mcp.json server runs only after you authorize it.', 'User-level files are shown for reference and never edited by FORGE.'],
  truncated: false,
});

const doctor = (): DoctorFinding[] => [
  { severity: 'warning', kind: 'changed_since_review', subject: 'i2', message: '.claude/settings.json changed since you last reviewed it.', suggestion: 'Read the diff, then mark it reviewed.' },
  { severity: 'info', kind: 'no_agents_md', subject: null, message: 'No AGENTS.md found.', suggestion: 'Create one if other agents work in this repo.' },
];

const diffOf = (rel: string, before: string, after: string) => {
  const a = before.split('\n'); const b = after.split('\n');
  return `--- a/${rel}\n+++ b/${rel}\n@@ -1,${a.length} +1,${b.length} @@\n` + a.map((l) => `-${l}`).join('\n') + '\n' + b.map((l) => `+${l}`).join('\n') + '\n';
};

function preview(rel: string, text: string, extra: Partial<HarnessPreview> = {}): HarnessPreview {
  const before = files[rel];
  let problems: string[] = [];
  if (rel.endsWith('.json')) { try { JSON.parse(text); } catch (e) { problems = [`Invalid JSON: ${(e as Error).message}`]; } }
  return {
    rel, exists: before !== undefined, beforeSha256: before !== undefined ? H(rel) : null, afterSha256: H(text.length + rel), unchanged: before === text,
    diff: diffOf(rel, before ?? '', text), added: text.split('\n').length, removed: (before ?? '').split('\n').length, problems, warnings: [], risks: [], text, ...extra,
  };
}

const mcpDef: McpDefinition = { name: 'docs', transport: 'stdio', command: 'npx', args: ['docs-mcp'], url: null, envNames: [] };
let mcpAuth: McpAuthorization | null = null;

const templates: HookTemplate[] = [
  { id: 'block-rm', name: 'Block destructive rm', description: 'Denies Bash commands that contain rm -rf.', event: 'PreToolUse', matcher: 'Bash', timeoutSecs: 10, requires: 'bash, grep', params: [{ key: 'pattern', label: 'Pattern to block', default: 'rm -rf' }] },
];

export function handleP4(cmd: string, args: any): Promise<any> | undefined {
  const done = (v: any) => Promise.resolve(v);
  switch (cmd) {
    case 'execution_backends': {
      const p = (level: string, detail: string) => ({ level, detail });
      const caps = (l: string, d: string) => ({ operatingSystems: ['linux'], filesystemIsolation: p(l, d), processIsolation: p(l, d), networkControl: p(l, d), resourceLimits: p(l === 'enforced' ? 'partial' : l === 'none' && d.startsWith('Not') ? 'none' : 'partial', l === 'none' && d.startsWith('Not') ? d : 'Time limit and output caps only.'), workspaceMapping: p(d.startsWith('Not') ? 'none' : 'enforced', d.startsWith('Not') ? d : 'Runs in the worktree.'), credentialExposure: p(l, d), persistentState: p(l, d), interactiveTerminal: false, concurrentInstances: true });
      return done([
        { id: 'trusted_local', label: 'Trusted local (not isolated)', implemented: true, available: true, unavailableReason: null, capabilities: caps('none', 'Runs as you: everything your account can reach.') },
        { id: 'linux_namespaces', label: 'Isolated: Linux namespaces (bubblewrap)', implemented: true, available: false, unavailableReason: 'Linux only. On Windows this backend is the planned inner layer of the isolated-WSL backend.', capabilities: caps('enforced', 'Enforced by namespaces.') },
        { id: 'windows_sandbox', label: 'Isolated: Windows Sandbox', implemented: false, available: false, unavailableReason: 'Not implemented in this build. See docs/EXECUTION.md for the design.', capabilities: caps('none', 'Not implemented.') },
        { id: 'isolated_wsl', label: 'Isolated: dedicated WSL distribution', implemented: false, available: false, unavailableReason: 'Not implemented in this build. See docs/EXECUTION.md for the design.', capabilities: caps('none', 'Not implemented.') },
      ].map((b) => ({ ...b, agentCapabilities: b.id === 'linux_namespaces' ? { ...b.capabilities, networkControl: p('none', 'Network shared with the host for provider connectivity.'), credentialExposure: p('partial', 'Provider login and transcripts are reachable; environment is restricted.') } : b.capabilities })));
    }
    case 'project_set_execution_backend': return Promise.reject('"Isolated: Windows Sandbox" cannot be used on this machine: Not implemented in this build.');
    case 'storage_usage': return done({ databaseBytes: 48_300_000, events: 182_455, runFiles: 412, runBytes: 96_500_000, proofFiles: 37, proofBytes: 1_900_000, experimentBytes: 12_400_000, worktrees: 6, untrackedWorktrees: ['C:\\Users\\dev\\.forge\\worktrees\\atlas-api\\old-spike-3f2a'] });
    case 'storage_cleanup_preview': return done({ experiments: [], skippedExperiments: [], olderThanDays: args.olderThanDays, missions: [{ id: 'm6', title: 'Bump docusaurus to latest minor', state: 'merged', updatedAt: '2026-08-01T10:00:00.000Z', events: 1840, files: ['r1.log', 'r2.log'], bytes: 2_300_000 }], orphanFiles: ['r9.stdout'], events: 1840, files: 3, bytes: 2_450_000, token: 'preview' });
    case 'storage_cleanup_apply': return done({ eventsDeleted: 1840, filesDeleted: 3, bytesFreed: 2_450_000, failures: [] });
    case 'storage_compact': return done({ pagesBefore: 11792, pagesAfter: 8200, freePagesBefore: 3592 });
    case 'project_set_agent_backend': return Promise.reject('"Isolated: Linux namespaces (bubblewrap)" cannot be used for agents on this machine: Linux only.');
    case 'harness_inventory': { const inv = inventory(); inv.mcpServers = inv.mcpServers.map((s) => ({ ...s, runsInForgeMissions: mcpAuth !== null && s.name === mcpAuth.name })); return done(inv); }
    case 'mcp_preview': return done({ definition: mcpDef, definitionSha256: 'a3f1c0de'.repeat(8), summary: ['Starts the program `npx` on this computer, with your privileges, whenever a Claude mission in this project starts — before any tool call is approved. It is not sandboxed.', 'It is fetched by a package runner: unless the arguments pin an exact version, a different version can run next time.', 'FORGE passes it no extra environment variables.', 'Its tools can only be used with your approval, one call at a time; what they return is treated as untrusted data.'], authorized: mcpAuth });
    case 'mcp_authorize': mcpAuth = { id: 'ma1', projectId: args.projectId, name: args.name, definition: mcpDef, definitionSha256: args.definitionSha256, toolAllowlist: args.toolAllowlist, authorizedAt: new Date().toISOString(), revokedAt: null }; return done(mcpAuth);
    case 'mcp_revoke': { const a = { ...mcpAuth!, revokedAt: new Date().toISOString() }; mcpAuth = null; return done(a); }
    case 'mcp_authorizations': return done(mcpAuth ? [{ authorization: mcpAuth, changedSinceAuthorization: false, summary: [] }] : []);
    case 'harness_doctor': return done(doctor());
    case 'harness_read': { const t = files[args.rel]; return done({ rel: args.rel, exists: t !== undefined, text: t ?? '', sha256: t !== undefined ? H(args.rel) : null, bytes: (t ?? '').length, editable: true } as HarnessFile); }
    case 'harness_preview': return done(preview(args.rel, args.text));
    case 'harness_apply': {
      if (args.args.text.includes('CHANGED')) return Promise.reject('the file changed on disk since you opened it; nothing was written');
      files[args.args.rel] = args.args.text;
      const c: HarnessChange = { id: `c${changes.length + 1}`, projectId: args.args.projectId, rel: args.args.rel, kind: 'edit', beforeSha256: args.args.expectedSha256, afterSha256: args.args.expectedAfterSha256, backupPath: null, rollsBackId: null, note: args.args.note ?? null, state: 'applied', createdAt: new Date().toISOString(), finishedAt: new Date().toISOString(), rolledBackBy: null };
      changes = [c, ...changes];
      return done(c);
    }
    case 'harness_history': return done(changes.filter((c) => !args.rel || c.rel === args.rel));
    case 'harness_rollback_preview': {
      const c = changes.find((x) => x.id === args.changeId)!;
      return done({ rel: c.rel, conflict: c.id === 'c1' ? 'The file changed after this edit (current sha256 differs).' : null, removesFile: c.kind === 'create', diff: diffOf(c.rel, files[c.rel] ?? '', 'previous\n'), added: 1, removed: 2 } as RollbackPreview);
    }
    case 'harness_rollback': return done(changes[0]);
    case 'harness_mark_reviewed': return done(null);
    case 'harness_hook_templates': return done(templates);
    case 'harness_hook_preview_add': {
      const cmdText = args.spec.command ?? 'grep -q "rm -rf" && exit 2';
      const text = '{\n  "hooks": {\n    "PreToolUse": [\n      { "matcher": "Bash", "hooks": [ { "type": "command", "command": ' + JSON.stringify(cmdText) + ' } ] }\n    ]\n  }\n}\n';
      return done(preview('.claude/settings.json', text, { risks: [{ event: args.spec.event || 'PreToolUse', command: cmdText, display: cmdText, risks: ['This command runs on your machine every time the event fires, with your privileges.', 'Review it before saving.'] }, { event: 'settings.statusLine', command: 'node status.js', display: 'node status.js<U+200B>', risks: ['This command runs on your machine whenever the status line refreshes.'] }] }));
    }
    case 'harness_hook_preview_remove': return done(preview('.claude/settings.json', '{\n  "hooks": {}\n}\n'));
    case 'harness_hook_dry_run': if (String(args.command).includes('nobash')) return Promise.reject('no bash was found to run the hook in; install Git for Windows'); return done({ shell: 'bash', exitCode: 0, timedOut: false, stdout: 'checking\n', stderr: '', interpretation: 'Exit code 0: Claude Code would allow the tool call to proceed.', input: { hook_event_name: args.event, tool_name: 'Bash', tool_input: { command: 'ls' } }, durationMs: 18, scrubbedEnv: ['GITHUB_TOKEN', 'AWS_SECRET_ACCESS_KEY'] } as HookDryRun);
    default: return undefined;
  }
}
