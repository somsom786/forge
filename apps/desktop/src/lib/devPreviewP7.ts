// DEV-ONLY: Code intelligence preview data (Code Map, symbol search). Imported only by devPreview.ts.
// Marker for the production-bundle check: FORGE_DEVPREVIEW_MARKER_7f3a91
/* eslint-disable @typescript-eslint/no-explicit-any */
import type { CodeMap, CodeSymbol, DirSummary, IndexedFile, RelevantCode, SymbolKind } from './types';

const S = (file: string, name: string, kind: SymbolKind, startLine: number, endLine: number, signature: string, container: string | null = null): CodeSymbol =>
  ({ file, name, kind, startLine, endLine, signature, container });

const symbols: CodeSymbol[] = [
  S('src/http/client.ts', 'HttpClient', 'class', 8, 61, 'export class HttpClient'),
  S('src/http/client.ts', 'request', 'method', 14, 30, 'async request(url: string, init: RequestInit = {}): Promise‹Response›', 'HttpClient'),
  S('src/http/client.ts', 'get', 'method', 32, 34, 'get(url: string)', 'HttpClient'),
  S('src/http/client.ts', 'HttpError', 'class', 63, 70, 'export class HttpError extends Error'),
  S('src/http/retry.ts', 'RetryOptions', 'interface', 3, 7, 'export interface RetryOptions'),
  S('src/http/retry.ts', 'withRetry', 'function', 9, 18, 'export async function withRetry‹T›(fn: () =› Promise‹T›, o: RetryOptions): Promise‹T›'),
  S('src/http/retry.ts', 'backoff', 'function', 20, 23, 'export function backoff(attempt: number): number'),
  S('src/http/retry.ts', 'MAX_ATTEMPTS', 'const', 1, 1, 'export const MAX_ATTEMPTS = 5;'),
  S('src/date/parse.ts', 'parseDate', 'function', 4, 19, 'export function parseDate(input: string): Date'),
  S('src/date/format.ts', 'formatDate', 'function', 3, 9, 'export function formatDate(date: Date): string'),
  S('src/log.ts', 'Logger', 'class', 5, 40, 'export class Logger'),
  S('src/log.ts', 'info', 'method', 12, 15, 'info(msg: string, fields?: Record‹string, unknown›)', 'Logger'),
  S('src/log.ts', 'LogLevel', 'type', 1, 1, "export type LogLevel = 'debug' | 'info' | 'warn' | 'error'"),
  S('src-tauri/src/main.rs', 'main', 'function', 3, 9, 'fn main()'),
  S('src-tauri/src/cache.rs', 'LruCache', 'struct', 4, 8, 'pub struct LruCache'),
  S('src-tauri/src/cache.rs', 'evict_oldest', 'method', 21, 30, 'fn evict_oldest(&mut self)', 'LruCache'),
  S('scripts/bench.py', 'run_bench', 'function', 6, 30, 'def run_bench(url, n=200)'),
  S('scripts/bench.py', 'DEFAULT_URL', 'const', 3, 3, "DEFAULT_URL = 'http://localhost:8080/orders'"),
];

const files: IndexedFile[] = [
  ...[...new Set(symbols.map((s) => s.file))].map((path) => ({
    path, language: path.endsWith('.rs') ? 'Rust' : path.endsWith('.py') ? 'Python' : 'TypeScript', bytes: 1800, symbols: symbols.filter((s) => s.file === path).length, parseErrors: false, skipped: null,
  })),
  { path: 'src/vendor/huge.generated.ts', language: 'TypeScript', bytes: 2_400_000, symbols: 0, parseErrors: false, skipped: 'larger than 512 KiB' },
  { path: 'README.md', language: null, bytes: 2100, symbols: 0, parseErrors: false, skipped: null },
  { path: 'package.json', language: null, bytes: 640, symbols: 0, parseErrors: false, skipped: null },
  { path: 'docs/http.md', language: null, bytes: 900, symbols: 0, parseErrors: false, skipped: null },
];

const parent = (p: string) => (p.includes('/') ? p.slice(0, p.lastIndexOf('/')) : '');

function map(): CodeMap {
  const dirs = new Map<string, DirSummary>();
  const dir = (p: string) => dirs.get(p) ?? (dirs.set(p, { path: p, files: [], totalFiles: 0, totalSymbols: 0, languages: {}, topSymbols: [] }), dirs.get(p)!);
  const languages: Record<string, number> = {};
  for (const f of files) {
    const lang = f.language ?? 'Other';
    languages[lang] = (languages[lang] ?? 0) + 1;
    dir(parent(f.path)).files.push(f);
    for (let d = parent(f.path); ; d = parent(d)) {
      const s = dir(d);
      s.totalFiles += 1;
      s.totalSymbols += f.symbols;
      s.languages[lang] = (s.languages[lang] ?? 0) + 1;
      if (d === '') break;
    }
  }
  for (const s of symbols.filter((x) => !x.container)) {
    const d = dirs.get(parent(s.file));
    if (d && d.topSymbols.length < 12) d.topSymbols.push({ name: s.name, kind: s.kind, file: s.file, line: s.startLine });
  }
  return {
    head: '9c1e9c1e9c1e9c1e9c1e9c1e9c1e9c1e9c1e9c1e', dirty: true, builtAt: new Date().toISOString(), builtMs: 412,
    filesListed: files.length, filesParsed: files.filter((f) => f.language && !f.skipped).length, symbolCount: symbols.length, languages,
    skipped: { tooLarge: 1, binary: 0, unreadable: 0, excludedDirs: 0, symlinks: 0, parseFailed: 0, overFileLimit: 0 },
    truncated: null, limits: { maxFileBytes: 512 * 1024, maxFiles: 20000 },
    dirs: [...dirs.values()].sort((a, b) => a.path.localeCompare(b.path)),
  };
}

function search(query: string, file: string | null, limit: number) {
  const q = query.trim().toLowerCase();
  return symbols
    .filter((s) => (!file || s.file === file) && (!q || s.name.toLowerCase().includes(q)))
    .map((s) => ({ symbol: s, score: !q ? 0 : s.name.toLowerCase() === q ? 100 : s.name.toLowerCase().startsWith(q) ? 60 : 30 }))
    .sort((a, b) => b.score - a.score || a.symbol.file.localeCompare(b.symbol.file) || a.symbol.startLine - b.symbol.startLine)
    .slice(0, limit);
}

/** The "Relevant code" section shown in context packages of the preview. */
export function previewRelevantCode(): RelevantCode {
  const pick = (name: string, reasons: string[], score: number) => {
    const s = symbols.find((x) => x.name === name)!;
    return { ...s, score, reasons };
  };
  return {
    items: [
      pick('withRetry', ['name matches "retry"', 'path matches "http"'], 4.6),
      pick('backoff', ['name matches "backoff"', 'path matches "retry"'], 4.25),
      pick('RetryOptions', ['name matches "retry"'], 2.75),
    ],
    omitted: 1, terms: ['retry', 'exponential', 'backoff', 'http', 'client'], budgetBytes: 2000, usedBytes: 388,
    indexHead: '9c1e9c1e9c1e9c1e9c1e9c1e9c1e9c1e9c1e9c1e', indexDirty: false, indexTruncated: null, note: null,
  };
}

export function handleP7(cmd: string, args: any): Promise<any> | undefined {
  switch (cmd) {
    case 'code_map': return new Promise((res) => setTimeout(() => res(map()), 300));
    case 'code_symbols': return Promise.resolve(search(args.query ?? '', args.file ?? null, args.limit ?? 100));
    default: return undefined;
  }
}
