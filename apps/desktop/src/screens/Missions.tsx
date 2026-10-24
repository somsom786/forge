import { useEffect, useMemo, useRef, useState } from 'react';
import { ArrowDown, ArrowUp, Plus, Search } from 'lucide-react';
import { providerLabel, relTime, STATE_META } from '../lib/format';
import { useStore } from '../lib/store';
import type { Mission, MissionState } from '../lib/types';
import { EmptyState, PageHeader, StateBadge, useNow } from '../components/ui';

/** The core returns at most this many missions per listing (store::list_missions). */
const MISSION_LIST_LIMIT = 500;

/** Dense mission table with j/k + Enter keyboard navigation. */
export function MissionsTable({ missions, showProject = true, emptyHint }: { missions: Mission[]; showProject?: boolean; emptyHint?: string }) {
  const projects = useStore((s) => s.projects);
  const navigate = useStore((s) => s.navigate);
  const now = useNow();
  const [sel, setSel] = useState(0);
  const body = useRef<HTMLTableSectionElement>(null);
  const selClamped = Math.min(sel, Math.max(0, missions.length - 1));

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement | null;
      if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.tagName === 'SELECT' || t.isContentEditable)) return;
      if (e.ctrlKey || e.metaKey || e.altKey) return;
      if (document.querySelector('[role="dialog"]')) return;
      if (e.key === 'j' || e.key === 'ArrowDown') {
        e.preventDefault();
        setSel((s) => Math.min(missions.length - 1, Math.min(s, missions.length - 1) + 1));
      } else if (e.key === 'k' || e.key === 'ArrowUp') {
        e.preventDefault();
        setSel((s) => Math.max(0, Math.min(s, missions.length - 1) - 1));
      } else if (e.key === 'Enter') {
        const m = missions[selClamped];
        // Let focused buttons/links handle their own Enter.
        if (m && !(t && (t.tagName === 'BUTTON' || t.tagName === 'A'))) {
          e.preventDefault();
          navigate({ name: 'mission', id: m.id });
        }
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [missions, selClamped, navigate]);

  useEffect(() => {
    body.current?.querySelector<HTMLElement>(`[data-row="${selClamped}"]`)?.scrollIntoView({ block: 'nearest' });
  }, [selClamped]);

  if (missions.length === 0) return <EmptyState title="No missions">{emptyHint ?? 'No missions match.'}</EmptyState>;

  return (
    <table className="tbl" aria-label="Missions">
      <thead>
        <tr>
          <th>Title</th>
          {showProject && <th className="w-[150px]">Project</th>}
          <th className="w-[140px]">State</th>
          <th className="w-[140px]">Provider</th>
          <th className="w-[170px]">Branch</th>
          <th className="w-[90px]">Updated</th>
        </tr>
      </thead>
      <tbody ref={body}>
        {missions.map((m, i) => (
          <tr
            key={m.id}
            data-row={i}
            aria-selected={i === selClamped}
            className={`cursor-pointer ${i === selClamped ? 'bg-accent-dim/70' : 'hover:bg-panel'}`}
            onClick={() => {
              setSel(i);
              navigate({ name: 'mission', id: m.id });
            }}
          >
            <td className="max-w-0 truncate font-medium" title={m.objective}>{m.title || m.id}</td>
            {showProject && <td className="truncate text-muted">{projects.find((p) => p.id === m.projectId)?.name ?? '—'}</td>}
            <td><StateBadge state={m.state} /></td>
            <td className="truncate text-muted">{providerLabel(m.provider)}{m.model ? ` · ${m.model}` : ''}</td>
            <td className="mono max-w-0 truncate text-[12px] text-muted" title={m.branch ?? ''}>{m.branch ?? '—'}</td>
            <td className="text-muted">{relTime(m.updatedAt, now)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

export function MissionsScreen() {
  const missions = useStore((s) => s.missions);
  const projects = useStore((s) => s.projects);
  const openNew = useStore((s) => s.openNewMission);
  const currentProjectId = useStore((s) => s.currentProjectId);
  const [projectFilter, setProjectFilter] = useState('');
  const [stateFilter, setStateFilter] = useState<'' | MissionState>('');
  const [query, setQuery] = useState('');
  const [desc, setDesc] = useState(true);

  const rows = useMemo(() => {
    const q = query.trim().toLowerCase();
    return Object.values(missions)
      .filter((m) => (!projectFilter || m.projectId === projectFilter) && (!stateFilter || m.state === stateFilter) && (!q || m.title.toLowerCase().includes(q)))
      .sort((a, b) => (desc ? b.updatedAt.localeCompare(a.updatedAt) : a.updatedAt.localeCompare(b.updatedAt)));
  }, [missions, projectFilter, stateFilter, query, desc]);

  return (
    <div className="flex h-full min-h-0 flex-col">
      <PageHeader
        title="Missions"
        sub={`${rows.length} of ${Object.keys(missions).length} shown${Object.keys(missions).length >= MISSION_LIST_LIMIT ? ` (only the ${MISSION_LIST_LIMIT} most recently updated missions are loaded)` : ''}. Keys: j / k to move, Enter to open.`}
        actions={
          <button className="btn btn-primary" disabled={projects.length === 0} onClick={() => openNew(currentProjectId ?? undefined)}>
            <Plus className="size-3.5" aria-hidden /> New mission
          </button>
        }
      />
      <div className="flex flex-wrap items-center gap-2 border-b border-line px-5 py-2">
        <label className="relative">
          <span className="sr-only">Search missions by title</span>
          <Search className="pointer-events-none absolute left-2 top-1/2 size-3.5 -translate-y-1/2 text-faint" aria-hidden />
          <input className="input !w-[260px] !pl-7" placeholder="Search by title" value={query} onChange={(e) => setQuery(e.target.value)} />
        </label>
        <select className="input !w-[170px]" aria-label="Filter by project" value={projectFilter} onChange={(e) => setProjectFilter(e.target.value)}>
          <option value="">All projects</option>
          {projects.map((p) => (
            <option key={p.id} value={p.id}>{p.name}</option>
          ))}
        </select>
        <select className="input !w-[170px]" aria-label="Filter by state" value={stateFilter} onChange={(e) => setStateFilter(e.target.value as '' | MissionState)}>
          <option value="">All states</option>
          {(Object.keys(STATE_META) as MissionState[]).map((s) => (
            <option key={s} value={s}>{STATE_META[s].label}</option>
          ))}
        </select>
        <button className="btn" onClick={() => setDesc(!desc)} aria-label={`Sort by updated time, currently ${desc ? 'newest' : 'oldest'} first`}>
          {desc ? <ArrowDown className="size-3.5" aria-hidden /> : <ArrowUp className="size-3.5" aria-hidden />} Updated
        </button>
      </div>
      <div className="min-h-0 flex-1 overflow-auto" tabIndex={0} role="region" aria-label="Mission list">
        <MissionsTable missions={rows} emptyHint={Object.keys(missions).length === 0 ? 'Create your first mission to get started.' : 'No missions match the current filters.'} />
      </div>
    </div>
  );
}
