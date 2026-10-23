import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { open as openFolder } from '@tauri-apps/plugin-dialog';
import { Copy, Plus, Search } from 'lucide-react';
import { api } from '../lib/api';
import { errMsg } from '../lib/format';
import { filterPages, NO_FILTERS, toggleIn, type PageFilters } from '../lib/knowledge';
import { useStore } from '../lib/store';
import type { KnowledgePage, KnowledgeStatus, LintIssue, PageStatus, ScanResult } from '../lib/types';
import { Modal } from '../components/Modal';
import { NewPageDialog, PAGE_KINDS } from '../components/NewPageDialog';
import { PageDetail, StateChips } from '../components/PageDetail';
import { SplitLayout } from '../components/Splitter';
import { Badge, Callout, EmptyState, PageHeader } from '../components/ui';

const STATES: PageStatus[] = ['verified', 'proposed', 'disputed', 'superseded', 'archived'];

export function KnowledgeScreen({ filter, pageId, newPage }: { filter?: 'proposed'; pageId?: string; newPage?: boolean }) {
  const projectId = useStore((s) => s.currentProjectId);
  const projects = useStore((s) => s.projects);
  const notify = useStore((s) => s.notify);
  const project = projects.find((p) => p.id === projectId);

  const [status, setStatus] = useState<KnowledgeStatus | null>(null);
  const [pages, setPages] = useState<KnowledgePage[]>([]);
  const [query, setQuery] = useState('');
  const [filters, setFilters] = useState<PageFilters>({ ...NO_FILTERS, states: new Set(filter ? [filter] : []) });
  const [selected, setSelected] = useState<string | null>(pageId ?? null);
  const [cursor, setCursor] = useState(0);
  const [scan, setScan] = useState<ScanResult | null>(null);
  const [lint, setLint] = useState<LintIssue[] | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [newOpen, setNewOpen] = useState(!!newPage);
  const [grant, setGrant] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const search = useRef<HTMLInputElement>(null);

  const tick = useStore((s) => (projectId ? s.knowledgeTick[projectId] ?? 0 : 0));
  const loadStatus = useCallback(async () => {
    if (!projectId) return;
    try { setStatus(await api.knowledgeStatus(projectId)); setLoadError(null); } catch (e) { setLoadError(errMsg(e)); }
  }, [projectId]);

  const loadPages = useCallback(async (q: string) => {
    if (!projectId) return;
    try { setPages(await api.knowledgeList(projectId, q.trim() || undefined)); } catch (e) { notify('error', errMsg(e)); }
  }, [projectId, notify]);

  const refresh = useCallback(() => { void loadStatus(); void loadPages(query); }, [loadStatus, loadPages, query]);

  useEffect(() => { void loadStatus(); }, [loadStatus, tick]);
  useEffect(() => {
    const t = window.setTimeout(() => void loadPages(query), query ? 250 : 0);
    return () => window.clearTimeout(t);
  }, [loadPages, query, tick]);

  const shown = useMemo(() => filterPages(pages, filters), [pages, filters]);
  const kinds = useMemo(() => Array.from(new Set([...PAGE_KINDS, ...pages.map((p) => p.kind)])).filter((k) => pages.some((p) => p.kind === k)), [pages]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement | null;
      const typing = !!t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.tagName === 'SELECT');
      if (e.key === '/' && !typing && !document.querySelector('[role="dialog"]')) { e.preventDefault(); search.current?.focus(); return; }
      if (typing || e.ctrlKey || e.metaKey || e.altKey || document.querySelector('[role="dialog"]')) return;
      if (e.key === 'j') { setCursor((c) => Math.min(shown.length - 1, c + 1)); }
      else if (e.key === 'k') { setCursor((c) => Math.max(0, c - 1)); }
      else if (e.key === 'Enter' && !(t && t.tagName === 'BUTTON')) { const p = shown[cursor]; if (p) setSelected(p.id); }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [shown, cursor]);

  const act = async (name: string, fn: () => Promise<void>) => {
    setBusy(name);
    try { await fn(); } catch (e) { notify('error', errMsg(e)); } finally { setBusy(null); }
  };

  const connect = () => act('connect', async () => {
    if (!projectId) return;
    const picked = await openFolder({ directory: true, multiple: false, title: 'Choose a notes folder or Obsidian vault' });
    const path = Array.isArray(picked) ? picked[0] : picked;
    if (!path) return;
    setStatus(await api.knowledgeVaultSet(projectId, path, false));
    notify('info', 'Vault connected read-only. Run "Scan now" to index it.');
    refresh();
  });

  if (!projectId || !project) {
    return <div className="flex h-full flex-col"><PageHeader title="Knowledge" /><EmptyState title="Choose a project">Knowledge belongs to a project. Add or select one first.</EmptyState></div>;
  }

  const counts = status?.counts ?? {};
  const vault = status?.vault ?? null;

  return (
    <div className="flex h-full min-h-0 flex-col">
      <PageHeader
        title="Knowledge"
        sub={`${project.name}: project memory. Only verified pages are attached to missions automatically.`}
        actions={<button className="btn btn-primary" onClick={() => setNewOpen(true)}><Plus className="size-3.5" aria-hidden /> New page</button>}
      />
      {loadError && <div className="p-3"><Callout tone="bad" title="Could not load knowledge status"><pre className="whitespace-pre-wrap text-[12px]">{loadError}</pre></Callout></div>}

      <div className="space-y-2 border-b border-line bg-panel px-5 py-2" aria-label="Knowledge status">
        <div className="flex flex-wrap items-center gap-x-4 gap-y-1">
          {STATES.map((s) => (
            <span key={s} className="text-[12px]"><Badge tone={s === 'verified' ? 'ok' : s === 'proposed' ? 'warn' : s === 'disputed' ? 'bad' : 'idle'}>{s}</Badge> <span className="mono">{counts[s] ?? 0}</span></span>
          ))}
          {status && status.missing > 0 && <span className="text-[12px] text-warn">{status.missing} page file{status.missing === 1 ? '' : 's'} missing</span>}
          {status && status.unfinishedOperations > 0 && <span className="text-[12px] text-warn">{status.unfinishedOperations} unfinished operation(s)</span>}
          <span className="ml-auto text-[12px] text-muted">Obsidian {status ? (status.obsidianInstalled ? 'installed' : 'not installed') : '…'}</span>
        </div>
        {status && (
          <div className="flex flex-wrap items-center gap-2 text-[12px]">
            <span className="text-faint">Managed folder</span>
            <span className="mono break-all">{status.managedRoot}</span>
            <button className="btn btn-sm btn-ghost" aria-label="Copy managed folder path" onClick={() => void navigator.clipboard.writeText(status.managedRoot).then(() => notify('info', 'Path copied'), (e) => notify('error', errMsg(e)))}><Copy className="size-3" aria-hidden /></button>
          </div>
        )}
        <div className="flex flex-wrap items-center gap-2 text-[12px]">
          {vault ? (
            <div className="flex flex-wrap items-center gap-2 rounded-md border border-line-strong bg-raised px-2 py-1">
              <span className="text-faint">Vault</span>
              <span className="mono break-all">{vault.path}</span>
              <Badge>{vault.isObsidianVault ? 'Obsidian vault' : 'plain folder'}</Badge>
              <Badge tone={vault.writable ? 'warn' : 'ok'}>{vault.writable ? 'write access' : 'read-only'}</Badge>
            </div>
          ) : <span className="text-muted">No vault connected.</span>}
          <button className="btn btn-sm" disabled={!!busy} onClick={() => void connect()}>Connect vault…</button>
          {vault && !vault.writable && <button className="btn btn-sm" disabled={!!busy} onClick={() => setGrant(true)}>Grant write access…</button>}
          {vault && <button className="btn btn-sm" disabled={!!busy} onClick={() => void act('disc', async () => { setStatus(await api.knowledgeVaultSet(projectId, null, false)); refresh(); notify('info', 'Vault disconnected. Its files were not touched.'); })}>Disconnect</button>}
          <span className="mx-1 h-4 w-px bg-line-strong" />
          <button className="btn btn-sm" disabled={!!busy} onClick={() => void act('scan', async () => { setScan(await api.knowledgeScan(projectId)); refresh(); })}>{busy === 'scan' ? 'Scanning…' : 'Scan now'}</button>
          <button className="btn btn-sm" disabled={!!busy} onClick={() => void act('lint', async () => setLint(await api.knowledgeLint(projectId)))}>Check quality</button>
          <button className="btn btn-sm" disabled={!!busy} onClick={() => void act('rebuild', async () => { const n = await api.knowledgeRebuildIndex(projectId); notify('info', `Index rebuilt (${n} pages)`); refresh(); })}>Rebuild index</button>
        </div>
        {scan && (
          <details className="text-[12px]" open>
            <summary className="cursor-pointer">Scan: {scan.scan.indexed} indexed, {scan.scan.unchanged} unchanged, {scan.scan.removed} removed · sources checked {scan.freshness.checked}, changed {scan.freshness.changed}, missing {scan.freshness.missing}</summary>
            {scan.scan.skipped.length > 0 && <div className="mt-1"><div className="text-faint">Skipped</div><ul className="m-0 list-disc pl-5">{scan.scan.skipped.map((s, i) => <li key={i} className="break-all">{s}</li>)}</ul></div>}
            {scan.scan.warnings.length > 0 && <div className="mt-1 text-warn"><div>Warnings</div><ul className="m-0 list-disc pl-5">{scan.scan.warnings.map((s, i) => <li key={i} className="break-all">{s}</li>)}</ul></div>}
            {scan.scan.skipped.length === 0 && scan.scan.warnings.length === 0 && <div className="text-muted">Nothing skipped, no warnings.</div>}
          </details>
        )}
      </div>

      {lint && <QualityPanel issues={lint} pages={pages} onClose={() => setLint(null)} onOpen={(id) => { setSelected(id); setLint(null); }} />}

      <div className="min-h-0 flex-1">
        <SplitLayout storageKey="knowledge.list" fixed="start" defaultSize={400} min={280} max={720}>
          <div className="flex min-h-0 flex-1 flex-col bg-panel/40">
            <div className="space-y-2 border-b border-line p-2">
              <label className="relative block">
                <span className="sr-only">Search knowledge</span>
                <Search className="pointer-events-none absolute left-2 top-1/2 size-3.5 -translate-y-1/2 text-faint" aria-hidden />
                <input ref={search} className="input !pl-7" placeholder="Search pages (press /)" value={query} onChange={(e) => setQuery(e.target.value)} />
              </label>
              <ChipRow label="State" options={STATES} sel={filters.states} onToggle={(v) => setFilters({ ...filters, states: toggleIn(filters.states, v) })} />
              <ChipRow label="Kind" options={kinds} sel={filters.kinds} onToggle={(v) => setFilters({ ...filters, kinds: toggleIn(filters.kinds, v) })} />
              <ChipRow label="Author" options={['user', 'agent', 'system']} sel={filters.authors} onToggle={(v) => setFilters({ ...filters, authors: toggleIn(filters.authors, v) })} />
              <ChipRow label="Root" options={['managed', 'vault']} sel={filters.roots} onToggle={(v) => setFilters({ ...filters, roots: toggleIn(filters.roots, v) })} />
            </div>
            <div className="min-h-0 flex-1 overflow-y-auto" role="listbox" aria-label="Pages">
              {shown.length === 0 && (
                <EmptyState title={pages.length === 0 ? 'No knowledge yet' : 'No pages match'}>
                  {pages.length === 0 ? 'Create a page, or save an agent message as knowledge. New pages are saved as proposed: they are only attached to missions after you verify them.' : 'Adjust the search or filters.'}
                </EmptyState>
              )}
              {shown.map((p, i) => (
                <button
                  key={p.id}
                  role="option"
                  aria-selected={selected === p.id}
                  onClick={() => { setSelected(p.id); setCursor(i); }}
                  className={`block w-full border-b border-line px-3 py-1.5 text-left ${selected === p.id ? 'bg-accent-dim' : i === cursor ? 'bg-hover' : 'hover:bg-hover'}`}
                >
                  <div className="truncate font-medium">{p.title}</div>
                  <div className="mt-0.5 flex flex-wrap items-center gap-1.5">
                    <StateChips page={p} />
                    <span className="text-[11px] text-muted">{p.kind}</span>
                    <span className="text-[11px] text-faint">{p.author}</span>
                    {p.rootKind === 'vault' && <span className="text-[11px] text-accent">in vault</span>}
                    {p.missing && <span className="text-[11px] text-bad">file missing</span>}
                  </div>
                </button>
              ))}
            </div>
          </div>
          <div className="flex min-h-0 flex-1 flex-col">
            {selected ? (
              <PageDetail
                key={selected}
                pageId={selected}
                pages={pages}
                obsidianInstalled={status?.obsidianInstalled ?? false}
                onOpenPage={setSelected}
                onChanged={refresh}
                onForgotten={() => { setSelected(null); refresh(); }}
              />
            ) : (
              <EmptyState title="Select a page">Choose a page on the left to see its content, provenance, review history and links.</EmptyState>
            )}
          </div>
        </SplitLayout>
      </div>

      <NewPageDialog open={newOpen} projectId={projectId} onClose={() => setNewOpen(false)} onCreated={(p) => { setSelected(p.id); refresh(); }} />
      <Modal open={grant} onClose={() => setGrant(false)} tone="bad" title="Grant write access to this vault?"
        footer={<><button className="btn" onClick={() => setGrant(false)}>Keep read-only</button><button className="btn btn-danger-solid" disabled={!!busy || !vault} onClick={() => void act('grant', async () => { if (vault) { setStatus(await api.knowledgeVaultSet(projectId, vault.path, true)); notify('info', 'Write access granted'); refresh(); } setGrant(false); })}>Grant write access</button></>}>
        <p className="m-0">FORGE may then modify notes in <span className="mono break-all">{vault?.path}</span>. Changes to a page always go through a revision you can review and apply; FORGE never silently overwrites a note.</p>
      </Modal>
    </div>
  );
}

function ChipRow({ label, options, sel, onToggle }: { label: string; options: string[]; sel: ReadonlySet<string>; onToggle: (v: string) => void }) {
  if (options.length === 0) return null;
  return (
    <div className="flex flex-wrap items-center gap-1" role="group" aria-label={`Filter by ${label.toLowerCase()}`}>
      <span className="w-[46px] text-[11px] text-faint">{label}</span>
      {options.map((o) => (
        <button key={o} aria-pressed={sel.has(o)} onClick={() => onToggle(o)} className={`rounded-full border px-2 py-0 text-[11px] ${sel.has(o) ? 'border-accent bg-accent-dim text-fg' : 'border-line-strong text-muted hover:text-fg'}`}>{o}</button>
      ))}
    </div>
  );
}

function QualityPanel({ issues, pages, onClose, onOpen }: { issues: LintIssue[]; pages: KnowledgePage[]; onClose: () => void; onOpen: (id: string) => void }) {
  const groups: ['problem' | 'warning' | 'info', string][] = [['problem', 'text-bad'], ['warning', 'text-warn'], ['info', 'text-muted']];
  return (
    <section aria-label="Quality" className="max-h-56 overflow-y-auto border-b border-line bg-panel px-5 py-2">
      <div className="mb-1 flex items-center"><h3 className="m-0 text-[11px] font-medium uppercase tracking-wide text-muted">Quality ({issues.length} finding{issues.length === 1 ? '' : 's'})</h3><button className="btn btn-sm btn-ghost ml-auto" onClick={onClose}>Close</button></div>
      {issues.length === 0 && <div className="text-ok">No issues found.</div>}
      {groups.map(([sev, color]) => {
        const list = issues.filter((i) => i.severity === sev);
        if (list.length === 0) return null;
        return (
          <div key={sev} className="mb-1.5">
            <div className={`text-[12px] font-medium ${color}`}>{sev} ({list.length})</div>
            <ul className="m-0 list-none p-0">
              {list.map((i, k) => (
                <li key={k} className="flex flex-wrap gap-2 text-[12px]">
                  <span className="text-faint">{i.kind}</span><span className="break-words">{i.message}</span>
                  {i.pageId && <button className="text-accent hover:underline" onClick={() => onOpen(i.pageId!)}>{pages.find((p) => p.id === i.pageId)?.title ?? 'Open page'}</button>}
                </li>
              ))}
            </ul>
          </div>
        );
      })}
    </section>
  );
}
