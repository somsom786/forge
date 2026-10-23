import { useEffect, useMemo, useState } from 'react';
import { api } from '../lib/api';
import { errMsg } from '../lib/format';
import { proposalsToReview } from '../lib/knowledge';
import { ChevronDown, ChevronRight, FolderPlus, Plus } from 'lucide-react';
import { addProjectViaDialog, startMission } from '../lib/actions';
import { relTime, STATE_META } from '../lib/format';
import { buildInbox, INBOX_LABEL, sinceLastVisit, type InboxRow } from '../lib/inbox';
import { COLLISION_META, collisions, type Collision } from '../lib/conflicts';
import { useStore } from '../lib/store';
import type { Mission } from '../lib/types';
import { ApprovalCard } from '../components/ApprovalCard';
import { Badge, EmptyState, PageHeader, StateBadge, useNow } from '../components/ui';

export function CommandCenter() {
  const missions = useStore((s) => s.missions);
  const approvals = useStore((s) => s.approvals);
  const projects = useStore((s) => s.projects);
  const loaded = useStore((s) => s.loaded);
  const lastVisit = useStore((s) => s.lastVisit);
  const markVisited = useStore((s) => s.markVisited);
  const openNew = useStore((s) => s.openNewMission);
  const currentProjectId = useStore((s) => s.currentProjectId);
  const now = useNow();
  const [review, setReview] = useState<{ projectId: string; count: number }[]>([]);
  const notify = useStore((st) => st.notify);
  const projectKey = projects.map((p) => p.id).join(',');
  useEffect(() => {
    let off = false;
    (async () => {
      const out: { projectId: string; count: number }[] = [];
      for (const p of projects) {
        try {
          const n = proposalsToReview(await api.knowledgeList(p.id)).length;
          if (n > 0) out.push({ projectId: p.id, count: n });
        } catch (e) { notify('error', `Could not check knowledge for ${p.name}: ${errMsg(e)}`); }
      }
      if (!off) setReview(out);
    })();
    return () => { off = true; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projectKey]);

  const list = useMemo(() => Object.values(missions), [missions]);
  const coll = useCollisions(list);
  const rows = useMemo(() => buildInbox(list, Object.values(approvals)), [list, approvals]);
  const since = useMemo(() => sinceLastVisit(list, lastVisit), [list, lastVisit]);
  const projectName = (id: string) => projects.find((p) => p.id === id)?.name ?? 'unknown project';

  return (
    <div className="flex h-full min-h-0 flex-col">
      <PageHeader
        title="Command Center"
        sub="Engineering inbox: what needs you, in priority order. Everything here comes from the FORGE core."
        actions={
          <>
            <button className="btn" onClick={() => void addProjectViaDialog()}>
              <FolderPlus className="size-3.5" aria-hidden /> Add project…
            </button>
            <button className="btn btn-primary" disabled={projects.length === 0} onClick={() => openNew(currentProjectId ?? undefined)}>
              <Plus className="size-3.5" aria-hidden /> New mission
            </button>
          </>
        }
      />
      <div className="min-h-0 flex-1 overflow-y-auto">
        {lastVisit !== null && since.changed.length > 0 && (
          <div className="flex items-center gap-3 border-b border-line bg-panel px-5 py-2" aria-label="Since your last visit">
            <span className="text-muted">Since your last visit ({relTime(new Date(lastVisit).toISOString(), now)}):</span>
            <span>
              {since.changed.length} mission{since.changed.length === 1 ? '' : 's'} changed
            </span>
            <span className="flex flex-wrap gap-1.5">
              {Object.entries(since.byState).map(([state, n]) => (
                <Badge key={state} tone={STATE_META[state as keyof typeof STATE_META].tone}>
                  {n} {STATE_META[state as keyof typeof STATE_META].label.toLowerCase()}
                </Badge>
              ))}
            </span>
            <button className="btn btn-sm btn-ghost ml-auto" onClick={markVisited}>
              Mark as seen
            </button>
          </div>
        )}
        {!loaded && <div className="p-8 text-muted">Loading from the core…</div>}
        {loaded && projects.length === 0 && (
          <EmptyState
            title="No projects yet"
            actions={
              <button className="btn btn-primary" onClick={() => void addProjectViaDialog()}>
                <FolderPlus className="size-3.5" aria-hidden /> Add a project
              </button>
            }
          >
            Register a local git repository. FORGE runs each mission in its own worktree and branch, so your working copy is never touched.
          </EmptyState>
        )}
        {loaded && projects.length > 0 && rows.length === 0 && (
          <EmptyState
            title="Inbox is clear"
            actions={
              <button className="btn btn-primary" onClick={() => openNew(currentProjectId ?? undefined)}>
                <Plus className="size-3.5" aria-hidden /> New mission
              </button>
            }
          >
            Nothing is waiting on you: no pending approvals, failures, reviews or running missions.
          </EmptyState>
        )}
        {review.length > 0 && (
          <div aria-label="Knowledge to review">
            <div className="flex items-center gap-2 border-b border-line bg-panel px-5 py-1 text-[11px] font-medium uppercase tracking-wide text-muted"><span className="size-1.5 rounded-full bg-warn" aria-hidden />Knowledge to review</div>
            {review.map((r) => (
              <div key={r.projectId} className="flex items-center gap-3 border-b border-line px-5 py-2">
                <span className="w-[130px] shrink-0 truncate text-muted">{projectName(r.projectId)}</span>
                <span className="min-w-0 flex-1">{r.count} proposed page{r.count === 1 ? '' : 's'} from agents or the system need review. Nothing here is attached to missions until you verify it.</span>
                <button className="btn btn-sm btn-primary" onClick={() => { useStore.getState().setCurrentProject(r.projectId); useStore.getState().navigate({ name: 'knowledge', filter: 'proposed' }); }}>Review</button>
              </div>
            ))}
          </div>
        )}
        {coll.length > 0 && <CollisionList items={coll} title={(id) => missions[id]?.title ?? id} projectOf={(id) => projectName(missions[id]?.projectId ?? '')} />}
        {rows.length > 0 && (
          <ul className="m-0 list-none p-0" aria-label="Inbox">
            {rows.map((r, i) => (
              <InboxItem key={r.key} row={r} first={i === 0 || rows[i - 1].kind !== r.kind} projectName={projectName(r.mission.projectId)} now={now} />
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}

const KIND_TONE = { approval: 'warn', failed: 'bad', interrupted: 'warn', review: 'warn', running: 'run', ready: 'idle' } as const;

function InboxItem({ row, first, projectName, now }: { row: InboxRow; first: boolean; projectName: string; now: number }) {
  const navigate = useStore((s) => s.navigate);
  const [open, setOpen] = useState(row.kind === 'approval');
  const m = row.mission;
  const open_ = () => navigate({ name: 'mission', id: m.id });

  let action: React.ReactNode;
  switch (row.kind) {
    case 'approval':
      action = <button className="btn btn-sm" onClick={open_}>Open mission</button>;
      break;
    case 'failed':
      action = <button className="btn btn-sm btn-primary" onClick={open_}>See failure</button>;
      break;
    case 'interrupted':
      action = (
        <>
          <button className="btn btn-sm" onClick={() => void startMission(m.id)}>Resume</button>
          <button className="btn btn-sm btn-primary" onClick={open_}>Open</button>
        </>
      );
      break;
    case 'review':
      action = <button className="btn btn-sm btn-primary" onClick={open_}>Review changes</button>;
      break;
    case 'running':
      action = <button className="btn btn-sm" onClick={open_}>Open</button>;
      break;
    case 'ready':
      action = (
        <>
          <button className="btn btn-sm btn-primary" onClick={() => void startMission(m.id)}>Start</button>
          <button className="btn btn-sm" onClick={open_}>Open</button>
        </>
      );
  }

  return (
    <li>
      {first && (
        <div className="sticky top-0 z-[1] flex items-center gap-2 border-b border-line bg-panel px-5 py-1 text-[11px] font-medium uppercase tracking-wide text-muted">
          <span className={`size-1.5 rounded-full bg-current ${KIND_TONE[row.kind] === 'warn' ? 'text-warn' : KIND_TONE[row.kind] === 'bad' ? 'text-bad' : KIND_TONE[row.kind] === 'run' ? 'text-run' : 'text-faint'}`} aria-hidden />
          {INBOX_LABEL[row.kind]}
        </div>
      )}
      <div className="border-b border-line px-5 py-2 hover:bg-panel/60">
        <div className="flex items-center gap-3">
          {row.kind === 'approval' ? (
            <button className="btn btn-ghost btn-sm !px-1" onClick={() => setOpen(!open)} aria-expanded={open} aria-label={open ? 'Collapse approval' : 'Expand approval'}>
              {open ? <ChevronDown className="size-4" aria-hidden /> : <ChevronRight className="size-4" aria-hidden />}
            </button>
          ) : (
            <span className="w-6" />
          )}
          <span className="w-[130px] shrink-0 truncate text-muted" title={projectName}>{projectName}</span>
          <div className="min-w-0 flex-1">
            <div className="truncate font-medium">{m.title || m.id}</div>
            {row.kind === 'approval' && row.approval && (
              <div className="mono truncate text-[12px] text-muted">{row.approval.tool}: {row.approval.summary}</div>
            )}
            {row.kind === 'failed' && m.lastError && <div className="truncate text-[12px] text-bad">{m.lastError.split('\n')[0]}</div>}
            {row.kind === 'interrupted' && <div className="truncate text-[12px] text-warn">The agent process is gone. You can resume or discard.</div>}
          </div>
          <StateBadge state={m.state} />
          <span className="w-[72px] shrink-0 text-right text-[12px] text-muted">{relTime(row.at, now)}</span>
          <div className="flex w-[170px] shrink-0 justify-end gap-1.5">{action}</div>
        </div>
        {row.kind === 'approval' && row.approval && open && (
          <div className="ml-9 mt-2 max-w-3xl">
            <ApprovalCard approval={row.approval} />
          </div>
        )}
      </div>
    </li>
  );
}

const LANDED: Mission['state'][] = ['merged', 'rejected', 'cancelled'];

/** Collisions between the active missions of every project, recomputed (read-only, in the core) when missions change. */
function useCollisions(list: Mission[]): Collision[] {
  const notify = useStore((s) => s.notify);
  const [out, setOut] = useState<Collision[]>([]);
  const active = list.filter((m) => m.worktreePath && !LANDED.includes(m.state));
  const byProject = new Map<string, number>();
  for (const m of active) byProject.set(m.projectId, (byProject.get(m.projectId) ?? 0) + 1);
  // A change of any active mission (or the set of them) is a reason to look again.
  const key = active.map((m) => `${m.id}:${m.updatedAt}`).sort().join('|');
  useEffect(() => {
    let off = false;
    const t = window.setTimeout(async () => {
      const all: Collision[] = [];
      for (const pid of byProject.keys()) {
        try {
          const r = await api.missionConflicts(pid);
          const title = (id: string) => list.find((m) => m.id === id)?.title ?? id;
          all.push(...collisions(r, title));
        } catch (e) {
          notify('error', `Could not check missions for collisions: ${errMsg(e)}`);
        }
      }
      if (!off) setOut(all);
    }, 400);
    return () => { off = true; window.clearTimeout(t); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);
  return out;
}

function CollisionList({ items, title, projectOf }: { items: Collision[]; title: (id: string) => string; projectOf: (id: string) => string }) {
  const navigate = useStore((s) => s.navigate);
  const [open, setOpen] = useState<string | null>(null);
  return (
    <div aria-label="Collisions between missions">
      <div className="flex items-center gap-2 border-b border-line bg-panel px-5 py-1 text-[11px] font-medium uppercase tracking-wide text-muted">
        <span className={`size-1.5 rounded-full ${items.some((c) => COLLISION_META[c.kind].tone === 'bad') ? 'bg-bad' : 'bg-warn'}`} aria-hidden />Collisions between missions
        <span className="ml-auto normal-case tracking-normal text-faint">predicted by git, read-only; agents can change this any moment</span>
      </div>
      {items.map((c) => (
        <div key={c.key} className="border-b border-line px-5 py-2">
          <div className="flex items-center gap-3">
            <span className="w-[130px] shrink-0 truncate text-muted">{projectOf(c.missions[0])}</span>
            <Badge tone={COLLISION_META[c.kind].tone}>{COLLISION_META[c.kind].label}</Badge>
            <span className="min-w-0 flex-1">{c.text}</span>
            {c.files.length > 0 && (
              <button className="btn btn-sm btn-ghost" aria-expanded={open === c.key} onClick={() => setOpen(open === c.key ? null : c.key)}>
                {open === c.key ? 'Hide files' : `${c.files.length} file${c.files.length === 1 ? '' : 's'}`}
              </button>
            )}
            {c.missions.map((id) => (
              <button key={id} className="btn btn-sm" title={title(id)} onClick={() => navigate({ name: 'mission', id })}>
                Open {c.missions.length > 1 ? `"${title(id).slice(0, 18)}${title(id).length > 18 ? '…' : ''}"` : 'mission'}
              </button>
            ))}
          </div>
          {open === c.key && <ul className="mono m-0 mt-1 list-none pl-[142px] text-[12px] text-muted">{c.files.map((f) => <li key={f}>{f}</li>)}</ul>}
        </div>
      ))}
    </div>
  );
}
