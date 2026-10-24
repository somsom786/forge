// Code Map: turning the core's flat per-directory summaries into a tree, filtering it, and describing what was
// indexed (and what was not) in plain words.
import type { CodeIndexSkipped, CodeMap, DirSummary, RelevantCode, SymbolKind } from './types';

export interface DirNode {
  path: string;
  name: string;
  summary: DirSummary;
  children: DirNode[];
}

export function parentOf(path: string): string {
  const i = path.lastIndexOf('/');
  return i < 0 ? '' : path.slice(0, i);
}

export function baseName(path: string): string {
  return path.slice(path.lastIndexOf('/') + 1);
}

const emptyDir = (path: string): DirSummary => ({ path, files: [], totalFiles: 0, totalSymbols: 0, languages: {}, topSymbols: [] });

/** The directory tree rooted at '' (the repository root). Missing ancestors are filled in, so any list nests. */
export function buildTree(dirs: DirSummary[]): DirNode {
  const nodes = new Map<string, DirNode>();
  const node = (path: string): DirNode => {
    let n = nodes.get(path);
    if (!n) {
      n = { path, name: path === '' ? '/' : baseName(path), summary: emptyDir(path), children: [] };
      nodes.set(path, n);
      if (path !== '') node(parentOf(path)).children.push(n);
    }
    return n;
  };
  node('');
  for (const d of dirs) node(d.path).summary = d;
  for (const n of nodes.values()) n.children.sort((a, b) => a.name.localeCompare(b.name));
  return nodes.get('')!;
}

/**
 * Directories whose path, or one of whose direct files, contains `query` (case-insensitive), plus their ancestors so
 * the tree still reads top-down. Returns the set of directory paths to show; an empty query shows everything.
 */
export function visibleDirs(root: DirNode, query: string): Set<string> {
  const q = query.trim().toLowerCase();
  const out = new Set<string>();
  const walk = (n: DirNode): boolean => {
    const self = !q || n.path.toLowerCase().includes(q) || n.summary.files.some((f) => f.path.toLowerCase().includes(q));
    let any = false;
    for (const c of n.children) any = walk(c) || any;
    if (self || any) out.add(n.path);
    return self || any;
  };
  walk(root);
  return out;
}

/** "Rust 12 · TypeScript 4 · Other 3": most files first, unparsed files ("Other") last. */
export function languageSummary(languages: Record<string, number>): string {
  return Object.entries(languages)
    .sort(([a, x], [b, y]) => (a === 'Other' ? 1 : 0) - (b === 'Other' ? 1 : 0) || y - x || a.localeCompare(b))
    .map(([l, n]) => `${l} ${n}`)
    .join(' · ');
}

const SKIP_WORDS: [keyof CodeIndexSkipped, string][] = [
  ['tooLarge', 'too large to parse'],
  ['binary', 'binary'],
  ['unreadable', 'unreadable'],
  ['parseFailed', 'failed to parse in time'],
  ['symlinks', 'symbolic links (never followed)'],
  ['excludedDirs', 'in .git, node_modules or target (always excluded)'],
  ['overFileLimit', 'over the file-count limit'],
];

/** Non-zero skip counts in plain words, e.g. "2 too large to parse". */
export function skippedSummary(s: CodeIndexSkipped): string[] {
  return SKIP_WORDS.filter(([k]) => s[k] > 0).map(([k, w]) => `${s[k]} ${w}`);
}

/** Which tree the map describes. */
export function indexedAt(map: Pick<CodeMap, 'head' | 'dirty'>): string {
  if (!map.head) return 'working tree (no commits yet)';
  return `commit ${map.head.slice(0, 10)}${map.dirty ? ' + uncommitted changes' : ''}`;
}

export const KIND_SHORT: Record<SymbolKind, string> = {
  function: 'fn', method: 'method', struct: 'struct', class: 'class', enum: 'enum', trait: 'trait', interface: 'interface', type: 'type', const: 'const', module: 'mod',
};

export function kindShort(kind: string): string {
  return KIND_SHORT[kind as SymbolKind] ?? kind;
}

/** One line for a context package: never claims code was sent when it was not. */
export function relevantCodeSummary(rc: RelevantCode | undefined | null): string {
  if (!rc) return 'No code pointers (recorded before code intelligence existed).';
  if (rc.note) return rc.note;
  if (rc.items.length === 0) return rc.terms.length ? 'No symbol names matched the objective.' : 'The objective has no words to match against code.';
  const more = rc.omitted > 0 ? `; ${rc.omitted} more matched but did not fit` : '';
  return `${rc.items.length} code pointer${rc.items.length === 1 ? '' : 's'} included (${rc.usedBytes} of ${rc.budgetBytes} bytes${more})`;
}
