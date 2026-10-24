import { useCallback, useEffect, useMemo, useState } from 'react';
import { ChevronDown, ChevronRight, FileCode2, Folder, RefreshCw, Search } from 'lucide-react';
import { api } from '../lib/api';
import { baseName, buildTree, indexedAt, kindShort, languageSummary, skippedSummary, visibleDirs, type DirNode } from '../lib/codemap';
import { errMsg, formatDuration } from '../lib/format';
import type { CodeMap, IndexedFile, Project, SymbolHit } from '../lib/types';
import { Badge, Callout } from './ui';

/** Directory tree → files → symbols for a project's main checkout, from FORGE's tree-sitter index. Read-only. */
export function CodeMapPanel({ project }: { project: Project }) {
  const [map, setMap] = useState<CodeMap | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [dir, setDir] = useState('');
  const [file, setFile] = useState<string | null>(null);
  const [filter, setFilter] = useState('');
  const [query, setQuery] = useState('');
  const [hits, setHits] = useState<SymbolHit[] | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try { setMap(await api.codeMap(project.id)); } catch (e) { setError(errMsg(e)); } finally { setLoading(false); }
  }, [project.id]);
  useEffect(() => { void load(); }, [load]);

  // Symbol search across the repository, debounced.
  useEffect(() => {
    const q = query.trim();
    if (!q) { setHits(null); return; }
    let off = false;
    const t = setTimeout(() => { api.codeSymbols(project.id, q, null, 100).then((h) => !off && setHits(h), (e) => !off && setError(errMsg(e))); }, 200);
    return () => { off = true; clearTimeout(t); };
  }, [project.id, query]);

  const tree = useMemo(() => (map ? buildTree(map.dirs) : null), [map]);
  const visible = useMemo(() => (tree ? visibleDirs(tree, filter) : new Set<string>()), [tree, filter]);
  const current = map?.dirs.find((d) => d.path === dir);
  const open = (path: string) => { setDir(path.includes('/') ? path.slice(0, path.lastIndexOf('/')) : ''); setFile(path); };

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex flex-wrap items-center gap-x-4 gap-y-1 border-b border-line px-5 py-2 text-[12px] text-muted">
        {map ? (
          <>
            <span>Indexed <span className="mono text-fg">{indexedAt(map)}</span></span>
            <span>{map.filesListed.toLocaleString()} files · {map.filesParsed.toLocaleString()} parsed · {map.symbolCount.toLocaleString()} symbols</span>
            <span>{languageSummary(map.languages)}</span>
            <span>built in {formatDuration(map.builtMs)}</span>
          </>
        ) : <span>{loading ? 'Indexing the repository…' : ''}</span>}
        <span className="flex-1" />
        <label className="relative">
          <Search className="pointer-events-none absolute left-2 top-1/2 size-3.5 -translate-y-1/2 text-faint" aria-hidden />
          <input className="input !h-7 !w-[240px] !pl-7" placeholder="Search symbols…" aria-label="Search symbols" value={query} onChange={(e) => setQuery(e.target.value)} />
        </label>
        <button className="btn btn-sm" onClick={() => void load()} disabled={loading} title="Re-read the repository (rebuilt only if something changed)">
          <RefreshCw className="size-3.5" aria-hidden /> Refresh
        </button>
      </div>
      {error && <div className="px-5 pt-2"><Callout tone="bad" title="Code map unavailable">{error}</Callout></div>}
      {map?.truncated && <div className="px-5 pt-2"><Callout tone="warn" title="Partial index">{map.truncated}</Callout></div>}
      {map && skippedSummary(map.skipped).length > 0 && (
        <div className="px-5 pt-1 text-[11.5px] text-muted">Not parsed: {skippedSummary(map.skipped).join('; ')}. Files over {Math.round(map.limits.maxFileBytes / 1024)} KiB are skipped.</div>
      )}

      {hits !== null ? (
        <SearchResults hits={hits} query={query} onOpen={(p) => { open(p); setQuery(''); }} />
      ) : tree && (
        <div className="grid min-h-0 flex-1 grid-cols-[minmax(200px,280px)_minmax(240px,1fr)_minmax(280px,1.3fr)]">
          <nav aria-label="Directories" className="min-h-0 overflow-auto border-r border-line p-2">
            <input className="input mb-2 !h-7" placeholder="Filter paths…" aria-label="Filter paths" value={filter} onChange={(e) => setFilter(e.target.value)} />
            {visible.size === 0 ? <div className="text-muted">No path matches.</div> : <DirTree node={tree} depth={0} selected={dir} visible={visible} forceOpen={!!filter.trim()} onSelect={(p) => { setDir(p); setFile(null); }} />}
          </nav>
          <section aria-label="Files" className="min-h-0 overflow-auto border-r border-line p-3">
            <h3 className="m-0 mb-1 mono text-[12.5px]">{dir || '/'}</h3>
            {current ? (
              <>
                <div className="mb-2 text-[11.5px] text-muted">{current.totalFiles} files · {current.totalSymbols} symbols below · {languageSummary(current.languages)}</div>
                {current.files.length === 0 ? <div className="text-muted">No files directly in this directory.</div> : (
                  <ul className="m-0 list-none p-0">
                    {current.files.map((f) => <FileRow key={f.path} f={f} selected={file === f.path} onSelect={() => setFile(f.path)} />)}
                  </ul>
                )}
                {current.topSymbols.length > 0 && (
                  <>
                    <h4 className="m-0 mb-1 mt-3 text-[11px] font-medium uppercase tracking-wide text-muted">Top-level symbols here</h4>
                    <ul className="m-0 list-none p-0">
                      {current.topSymbols.map((s) => (
                        <li key={`${s.file}:${s.line}:${s.name}`}>
                          <button className="flex w-full items-baseline gap-2 py-0.5 text-left hover:text-accent" onClick={() => setFile(s.file)}>
                            <Badge>{kindShort(s.kind)}</Badge><span className="mono">{s.name}</span><span className="text-[11px] text-faint">{baseName(s.file)}:{s.line}</span>
                          </button>
                        </li>
                      ))}
                    </ul>
                  </>
                )}
              </>
            ) : <div className="text-muted">Nothing indexed here.</div>}
          </section>
          <section aria-label="Symbols" className="min-h-0 overflow-auto p-3">
            {file ? <FileSymbols projectId={project.id} file={file} indexKey={map?.builtAt ?? ''} /> : <div className="text-muted">Choose a file to list its symbols.</div>}
          </section>
        </div>
      )}
      <div className="border-t border-line px-5 py-1.5 text-[11px] text-faint">
        From the project's main checkout (git ls-files, respecting .gitignore), parsed with tree-sitter for Rust, TypeScript/TSX, JavaScript and Python. Nothing in the repository is executed. Mission worktrees are not shown here.
      </div>
    </div>
  );
}

function DirTree({ node, depth, selected, visible, forceOpen, onSelect }: { node: DirNode; depth: number; selected: string; visible: Set<string>; forceOpen: boolean; onSelect: (p: string) => void }) {
  const [open, setOpen] = useState(depth < 1);
  const shown = node.children.filter((c) => visible.has(c.path));
  const expanded = open || forceOpen;
  return (
    <div>
      <div className={`flex items-center gap-1 rounded px-1 py-0.5 ${selected === node.path ? 'bg-raised text-fg' : 'text-muted hover:text-fg'}`} style={{ paddingLeft: depth * 12 + 4 }}>
        {shown.length > 0 ? (
          <button aria-label={expanded ? `Collapse ${node.name}` : `Expand ${node.name}`} aria-expanded={expanded} onClick={() => setOpen(!expanded)} className="text-faint hover:text-fg">
            {expanded ? <ChevronDown className="size-3.5" aria-hidden /> : <ChevronRight className="size-3.5" aria-hidden />}
          </button>
        ) : <span className="inline-block w-3.5" />}
        <button className="flex min-w-0 flex-1 items-center gap-1.5 text-left" onClick={() => onSelect(node.path)}>
          <Folder className="size-3.5 shrink-0" aria-hidden /><span className="truncate">{node.name}</span>
          <span className="ml-auto shrink-0 text-[11px] text-faint">{node.summary.totalSymbols}</span>
        </button>
      </div>
      {expanded && shown.map((c) => <DirTree key={c.path} node={c} depth={depth + 1} selected={selected} visible={visible} forceOpen={forceOpen} onSelect={onSelect} />)}
    </div>
  );
}

function FileRow({ f, selected, onSelect }: { f: IndexedFile; selected: boolean; onSelect: () => void }) {
  return (
    <li>
      <button className={`flex w-full items-center gap-2 rounded px-1 py-0.5 text-left ${selected ? 'bg-raised text-fg' : 'hover:text-accent'}`} onClick={onSelect} title={f.skipped ?? undefined}>
        <FileCode2 className="size-3.5 shrink-0 text-faint" aria-hidden />
        <span className="mono min-w-0 truncate">{baseName(f.path)}</span>
        <span className="ml-auto flex shrink-0 items-center gap-1.5 text-[11px] text-faint">
          {f.skipped ? <Badge tone="warn">not parsed</Badge> : f.language ? `${f.symbols} sym` : 'not parsed (language)'}
          {f.parseErrors && <Badge tone="warn" title="Parsed with syntax errors; symbols come from the parts that parsed">errors</Badge>}
        </span>
      </button>
    </li>
  );
}

function FileSymbols({ projectId, file, indexKey }: { projectId: string; file: string; indexKey: string }) {
  const [syms, setSyms] = useState<SymbolHit[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let off = false;
    setSyms(null);
    setError(null);
    api.codeSymbols(projectId, '', file, 500).then((h) => !off && setSyms(h), (e) => !off && setError(errMsg(e)));
    return () => { off = true; };
  }, [projectId, file, indexKey]);
  return (
    <>
      <h3 className="m-0 mb-2 mono break-all text-[12.5px]">{file}</h3>
      {error && <div className="text-bad">{error}</div>}
      {syms === null ? (!error && <div className="text-muted">Loading…</div>) : syms.length === 0 ? <div className="text-muted">No symbols (not a parsed language, skipped, or no declarations).</div> : (
        <ul className="m-0 list-none space-y-1 p-0">
          {syms.map(({ symbol: s }) => (
            <li key={`${s.startLine}:${s.name}`} className={s.container ? 'pl-4' : ''}>
              <div className="flex items-baseline gap-2">
                <Badge>{kindShort(s.kind)}</Badge>
                <span className="mono font-medium">{s.name}</span>
                {s.container && <span className="text-[11px] text-faint">in {s.container}</span>}
                <span className="ml-auto text-[11px] text-faint">L{s.startLine}{s.endLine > s.startLine ? `–${s.endLine}` : ''}</span>
              </div>
              <div className="mono truncate text-[11.5px] text-muted" title={s.signature}>{s.signature}</div>
            </li>
          ))}
        </ul>
      )}
    </>
  );
}

function SearchResults({ hits, query, onOpen }: { hits: SymbolHit[]; query: string; onOpen: (file: string) => void }) {
  return (
    <section aria-label="Symbol search results" className="min-h-0 flex-1 overflow-auto p-3">
      <div className="mb-2 text-[12px] text-muted">{hits.length === 0 ? `No symbol named like “${query.trim()}”.` : `${hits.length}${hits.length === 100 ? '+' : ''} symbols`}</div>
      <ul className="m-0 list-none space-y-1 p-0">
        {hits.map(({ symbol: s }) => (
          <li key={`${s.file}:${s.startLine}:${s.name}`}>
            <button className="w-full text-left hover:text-accent" onClick={() => onOpen(s.file)}>
              <div className="flex items-baseline gap-2">
                <Badge>{kindShort(s.kind)}</Badge><span className="mono font-medium">{s.name}</span>
                {s.container && <span className="text-[11px] text-faint">in {s.container}</span>}
                <span className="mono ml-auto text-[11px] text-faint">{s.file}:{s.startLine}</span>
              </div>
              <div className="mono truncate text-[11.5px] text-muted">{s.signature}</div>
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}
