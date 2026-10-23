// DEV-ONLY: Phase 3 preview data (knowledge + context). Imported only by devPreview.ts.
// Marker for the production-bundle check: FORGE_DEVPREVIEW_MARKER_7f3a91
/* eslint-disable @typescript-eslint/no-explicit-any */
import type {
  ContextItem, ContextPackage, ContextPackageRow, KnowledgePage, KnowledgePageDetail, KnowledgeSource, KnowledgeStatus, LintIssue, Mission,
} from './types';

export interface P3Ctx {
  iso: (minsAgo: number) => string;
  find: (id: string) => Mission | undefined;
  err: (m: string) => Promise<never>;
}

let c: P3Ctx;
const SHA = (n: string) => (n + '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef').slice(0, 64);

function page(o: Partial<KnowledgePage> & Pick<KnowledgePage, 'id' | 'title'>): KnowledgePage {
  return {
    projectId: 'p1', rootKind: 'managed', relPath: `${o.id}.md`, slug: o.id, kind: 'decision', scope: 'project', author: 'user', origin: 'forge',
    status: 'proposed', effectiveState: 'proposed', modifiedSinceVerified: false, verifiedSha256: null, verifiedAt: null, verifiedNote: null,
    currentSha256: SHA(o.id), topic: null, supersedesId: null, supersededById: null, files: [], tags: [], warnings: [], missing: false,
    createdAt: c.iso(5000), updatedAt: c.iso(300), indexedAt: c.iso(10), ...o,
  };
}

let pages: KnowledgePage[] | null = null;
const texts: Record<string, string> = {};
function all(): KnowledgePage[] {
  if (pages) return pages;
  pages = [
    page({ id: 'k1', title: 'HTTP client retries only idempotent requests', kind: 'decision', status: 'verified', effectiveState: 'verified', verifiedAt: c.iso(2000), verifiedNote: 'Matches src/http/client.ts', verifiedSha256: SHA('k1'), files: ['src/http/client.ts'], tags: ['http', 'reliability'], topic: 'http' }),
    page({ id: 'k2', title: 'Date parsing is UTC-only', kind: 'convention', status: 'verified', effectiveState: 'proposed', modifiedSinceVerified: true, verifiedAt: c.iso(4000), files: ['src/date/parse.ts'] }),
    page({ id: 'k3', title: 'Config loader falls back to defaults silently', kind: 'failure_mode', author: 'agent', files: ['src/config/load.ts'], warnings: ['Unknown frontmatter key "confidence" ignored'] }),
    page({ id: 'k4', title: 'Release process (from team vault)', kind: 'note', rootKind: 'vault', relPath: 'Engineering/Release process.md', origin: 'imported', author: 'user' }),
    page({ id: 'k5', title: 'Old logging approach', kind: 'architecture', status: 'archived', effectiveState: 'archived' }),
  ];
  texts.k1 = '---\ntitle: HTTP client retries only idempotent requests\nkind: decision\n---\n# Decision\n\nThe HTTP client retries **only idempotent** methods (GET, HEAD, PUT, DELETE, OPTIONS).\n\n- Max 5 attempts\n- Exponential backoff with jitter\n\nSee [[Date parsing is UTC-only]] and `src/http/client.ts`.\n';
  texts.k2 = '---\ntitle: Date parsing is UTC-only\nstate: verified\n---\n# Convention\n\nAll calendar dates are parsed as UTC midnight. (Edited after verification: added the note below.)\n\nNever use local-time constructors.\n';
  texts.k3 = '---\ntitle: Config loader falls back to defaults silently\nauthor: agent\n---\nThe loader returns defaults when the file is missing and logs nothing.\n';
  texts.k4 = '# Release process\n\n1. Tag main\n2. Run the pipeline\n';
  texts.k5 = '# Old logging approach\n\nSuperseded by structured logging.\n';
  return pages;
}

const status = (): KnowledgeStatus => ({
  managedRoot: 'C:\\Users\\dev\\.forge\\knowledge\\p1', vault: { path: 'C:\\Users\\dev\\Documents\\TeamVault', isObsidianVault: true, writable: false },
  obsidianInstalled: false, counts: counts(), total: all().length, missing: 0, unfinishedOperations: 0,
});
function counts() {
  const o: Record<string, number> = {};
  for (const p of all()) o[p.effectiveState] = (o[p.effectiveState] ?? 0) + 1;
  return o;
}

function detail(id: string): KnowledgePageDetail {
  const p = all().find((x) => x.id === id)!;
  const sources: KnowledgeSource[] = id === 'k1' ? [
    { id: 's1', projectId: 'p1', kind: 'repo_file', locator: 'src/http/client.ts', gitBlob: 'abc', rawSha256: SHA('r'), normSha256: SHA('n'), snapshot: 'export async function request(url, init) {\n  return withRetry(() => fetch(url, init));\n}', extractor: 'repo_file@1', state: 'current', capturedAt: c.iso(2100), checkedAt: c.iso(5) },
    { id: 's2', projectId: 'p1', kind: 'repo_file', locator: 'src/http/retry.ts', gitBlob: null, rawSha256: SHA('r2'), normSha256: SHA('n2'), snapshot: 'export function backoff(attempt) { /* redacted */ }', extractor: 'repo_file@1', state: 'changed', capturedAt: c.iso(2100), checkedAt: c.iso(5) },
  ] : [];
  return {
    page: p, text: texts[id] ?? '', writable: p.rootKind === 'managed', sources,
    links: id === 'k1' ? [{ kind: 'wikilink', target: 'Date parsing is UTC-only', toPage: 'k2', ambiguous: false }, { kind: 'wikilink', target: 'Release', toPage: null, ambiguous: true }, { kind: 'file', target: 'src/http/client.ts', toPage: null, ambiguous: false }] : [],
    backlinks: id === 'k2' ? [all()[0]] : [],
    reviews: p.verifiedAt ? [{ action: 'verify', revisionSha256: SHA(id), reviewer: 'user', note: p.verifiedNote, at: p.verifiedAt }] : [],
    revisions: id === 'k2' ? [
      { id: 'r1', pageId: 'k2', baseSha256: p.currentSha256, text: texts.k2.replace('Never use local-time constructors.', 'Never use local-time constructors (new Date(y, m, d)).'), author: 'agent', note: 'Clarify the rule', state: 'proposed', createdAt: c.iso(30), decidedAt: null },
      { id: 'r0', pageId: 'k2', baseSha256: SHA('old'), text: 'older text', author: 'agent', note: null, state: 'conflict', createdAt: c.iso(900), decidedAt: c.iso(800) },
    ] : [],
  };
}

const item = (o: Partial<ContextItem> & Pick<ContextItem, 'pageId' | 'title'>): ContextItem => ({
  kind: 'decision', trust: 'verified', author: 'user', origin: 'forge', verifiedAt: c.iso(2000), verifiedNote: 'Matches src/http/client.ts', sha256: SHA('i'),
  excerpt: 'The HTTP client retries only idempotent methods (GET, HEAD, PUT, DELETE, OPTIONS).\nMax 5 attempts, exponential backoff with jitter.', truncated: false,
  score: 0, reasons: ['mentions src/http/client.ts, which is in scope'], sources: [{ kind: 'repo_file', locator: 'src/http/client.ts', state: 'current' }, { kind: 'repo_file', locator: 'src/http/retry.ts', state: 'changed' }], stale: true, files: [], ...o,
});

const pkg = (): ContextPackage => ({
  items: [item({ pageId: 'k1', title: all()[0].title })],
  omitted: [
    { pageId: 'k3', title: all()[2].title, reason: 'not verified by a person' },
    { pageId: 'k2', title: all()[1].title, reason: 'edited since it was verified' },
    { pageId: 'k4', title: all()[3].title, reason: 'over budget' },
  ],
  query: 'retry', charBudget: 6000, sha256: SHA('pkg'), pathHints: ['src/'],
  text: '## Verified project knowledge\n\n### HTTP client retries only idempotent requests (decision, verified)\nThe HTTP client retries only idempotent methods (GET, HEAD, PUT, DELETE, OPTIONS).\nMax 5 attempts, exponential backoff with jitter.\n',
});

const lint = (): LintIssue[] => [
  { severity: 'problem', kind: 'missing_file', pageId: 'k5', message: 'The page file no longer exists on disk.' },
  { severity: 'warning', kind: 'stale_source', pageId: 'k1', message: 'Source src/http/retry.ts changed since this page was written.' },
  { severity: 'info', kind: 'orphan', pageId: 'k3', message: 'No other page links here.' },
];

export function handleP3(cmd: string, args: any, ctx: P3Ctx): Promise<any> | undefined {
  c = ctx;
  const done = (v: any) => Promise.resolve(v);
  switch (cmd) {
    case 'knowledge_status': return done(status());
    case 'knowledge_scan': return done({ scan: { indexed: 2, unchanged: 3, removed: 0, changedPages: ['k4'], skipped: ['Archive/huge.md: larger than the size limit'], warnings: ['Engineering/Release process.md: no frontmatter'] }, freshness: { checked: 4, changed: 1, missing: 0 } });
    case 'knowledge_rebuild_index': return done(all().length);
    case 'knowledge_lint': return done(lint());
    case 'knowledge_vault_set': { const s = status(); if (args.path === null) s.vault = null; else s.vault = { path: args.path, isObsidianVault: true, writable: !!args.write }; return done(s); }
    case 'knowledge_list': { const q = String(args.query ?? '').toLowerCase(); return done(all().filter((p) => !q || p.title.toLowerCase().includes(q))); }
    case 'knowledge_get': return done(detail(args.pageId));
    case 'knowledge_create': {
      const p = page({ id: `k${all().length + 1}`, title: args.args.title, kind: args.args.kind });
      all().push(p); texts[p.id] = args.args.body; return done(p);
    }
    case 'knowledge_save_from_event': {
      const p = page({ id: `k${all().length + 1}`, title: args.title, kind: args.kind, author: 'agent' });
      all().push(p); texts[p.id] = 'Saved from an agent message.'; return done(p);
    }
    case 'knowledge_propose_revision': return done({ id: 'rn', pageId: args.pageId, baseSha256: '', text: args.text, author: 'user', note: args.note, state: 'proposed', createdAt: new Date().toISOString(), decidedAt: null });
    case 'knowledge_apply_revision': return done(all()[1]);
    case 'knowledge_reject_revision': return done(null);
    case 'knowledge_verify': {
      const p = all().find((x) => x.id === args.pageId)!;
      if (p.currentSha256 !== args.expectedSha256) return ctx.err('page changed since you opened it');
      Object.assign(p, { status: 'verified', effectiveState: 'verified', modifiedSinceVerified: false, verifiedAt: new Date().toISOString(), verifiedNote: args.note });
      return done(p);
    }
    case 'knowledge_set_status': {
      const p = all().find((x) => x.id === args.pageId)!;
      const s = args.action === 'dispute' ? 'disputed' : args.action === 'archive' ? 'archived' : 'proposed';
      Object.assign(p, { status: s, effectiveState: s, modifiedSinceVerified: false });
      return done(p);
    }
    case 'knowledge_supersede': { const p = all().find((x) => x.id === args.oldId)!; Object.assign(p, { status: 'superseded', effectiveState: 'superseded', supersededById: args.newId }); return done(p); }
    case 'knowledge_forget': { pages = all().filter((x) => x.id !== args.pageId); return done(null); }
    case 'knowledge_open_in_obsidian': return ctx.err('Obsidian is not installed');
    case 'context_preview': return done(pkg());
    case 'context_for_mission': {
      if (args.missionId !== 'm2') return done([]);
      const p = pkg();
      const row: ContextPackageRow = { id: 'cp1', missionId: 'm2', createdAt: ctx.iso(90), query: p.query, params: { budgetChars: p.charBudget, pathHints: p.pathHints }, items: p.items, omitted: p.omitted, injectedText: p.text, injectedSha256: p.sha256, charBudget: p.charBudget };
      return done([row]);
    }
    case 'mission_memory_set': { const m = ctx.find(args.missionId)!; m.useMemory = args.useMemory; m.memoryPins = args.pins; m.updatedAt = new Date().toISOString(); return done({ ...m }); }
    default: return undefined;
  }
}
