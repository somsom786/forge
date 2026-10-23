import { useCallback, useEffect, useMemo, useState } from 'react';
import * as Dialog from '@radix-ui/react-dialog';
import { ArrowLeft, FolderPlus, Plus, RefreshCw, Trash2 } from 'lucide-react';
import { api } from '../lib/api';
import { addProjectViaDialog, removeProject } from '../lib/actions';
import { errMsg } from '../lib/format';
import { useStore } from '../lib/store';
import type { Project, RepoStatus } from '../lib/types';
import { CodeMapPanel } from '../components/CodeMapPanel';
import { ExecutionBackendPanel } from '../components/ExecutionBackendPanel';
import { Badge, EmptyState, PageHeader } from '../components/ui';
import { MissionsTable } from './Missions';

type StatusEntry = { status?: RepoStatus; error?: string; loading: boolean };

function StatusCells({ s }: { s: StatusEntry | undefined }) {
  if (!s || s.loading) return <span className="text-muted">checking…</span>;
  if (s.error) return <span className="text-bad" title={s.error}>{s.error.split('\n')[0]}</span>;
  const st = s.status!;
  return (
    <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
      <span className="mono text-[12px]">{st.branch ?? '(detached)'}</span>
      {(st.ahead > 0 || st.behind > 0) && (
        <span className="mono text-[12px] text-muted" title="Ahead / behind upstream">↑{st.ahead} ↓{st.behind}</span>
      )}
      {st.dirty ? (
        <span className="flex gap-1.5">
          {st.staged > 0 && <Badge tone="ok">{st.staged} staged</Badge>}
          {st.unstaged > 0 && <Badge tone="warn">{st.unstaged} modified</Badge>}
          {st.untracked > 0 && <Badge tone="idle">{st.untracked} untracked</Badge>}
          {st.conflicted > 0 && <Badge tone="bad">{st.conflicted} conflicted</Badge>}
        </span>
      ) : (
        <Badge tone="ok">clean</Badge>
      )}
    </div>
  );
}

function useProjectStatuses(projects: Project[]) {
  const [map, setMap] = useState<Record<string, StatusEntry>>({});
  const load = useCallback(async () => {
    setMap((m) => Object.fromEntries(projects.map((p) => [p.id, { ...m[p.id], loading: true }])));
    await Promise.all(
      projects.map(async (p) => {
        try {
          const status = await api.projectStatus(p.id);
          setMap((m) => ({ ...m, [p.id]: { status, loading: false } }));
        } catch (e) {
          setMap((m) => ({ ...m, [p.id]: { error: errMsg(e), loading: false } }));
        }
      }),
    );
  }, [projects]);
  useEffect(() => {
    void load();
  }, [load]);
  return { map, reload: load };
}

export function ProjectsScreen({ projectId }: { projectId?: string }) {
  const projects = useStore((s) => s.projects);
  const project = projectId ? projects.find((p) => p.id === projectId) : undefined;
  if (project) return <ProjectView project={project} />;
  return <ProjectList />;
}

function ProjectList() {
  const projects = useStore((s) => s.projects);
  const navigate = useStore((s) => s.navigate);
  const setCurrent = useStore((s) => s.setCurrentProject);
  const openNew = useStore((s) => s.openNewMission);
  const missions = useStore((s) => s.missions);
  const { map, reload } = useProjectStatuses(projects);
  const [removing, setRemoving] = useState<Project | null>(null);
  const counts = useMemo(() => {
    const c: Record<string, number> = {};
    for (const m of Object.values(missions)) c[m.projectId] = (c[m.projectId] ?? 0) + 1;
    return c;
  }, [missions]);

  return (
    <div className="flex h-full min-h-0 flex-col">
      <PageHeader
        title="Projects"
        sub="Registered git repositories. Status is read live from each repository."
        actions={
          <>
            <button className="btn" onClick={() => void reload()} disabled={projects.length === 0}>
              <RefreshCw className="size-3.5" aria-hidden /> Refresh status
            </button>
            <button className="btn btn-primary" onClick={() => void addProjectViaDialog()}>
              <FolderPlus className="size-3.5" aria-hidden /> Add project…
            </button>
          </>
        }
      />
      <div className="min-h-0 flex-1 overflow-auto">
        {projects.length === 0 ? (
          <EmptyState
            title="No projects yet"
            actions={
              <button className="btn btn-primary" onClick={() => void addProjectViaDialog()}>
                <FolderPlus className="size-3.5" aria-hidden /> Add a project
              </button>
            }
          >
            Choose a local folder that is a git repository.
          </EmptyState>
        ) : (
          <table className="tbl" aria-label="Projects">
            <thead>
              <tr>
                <th>Project</th>
                <th>Default branch</th>
                <th>Git status</th>
                <th className="w-[80px]">Missions</th>
                <th className="w-[210px]" />
              </tr>
            </thead>
            <tbody>
              {projects.map((p) => (
                <tr key={p.id} className="hover:bg-panel">
                  <td>
                    <button className="text-left font-medium text-fg hover:text-accent hover:underline" onClick={() => navigate({ name: 'projects', projectId: p.id })}>
                      {p.name}
                    </button>
                    <div className="mono max-w-[420px] truncate text-[11.5px] text-muted" title={p.rootPath}>{p.rootPath}</div>
                  </td>
                  <td className="mono text-[12px]">{p.defaultBranch}</td>
                  <td><StatusCells s={map[p.id]} /></td>
                  <td className="text-muted">{counts[p.id] ?? 0}</td>
                  <td>
                    <div className="flex justify-end gap-1.5">
                      <button className="btn btn-sm" onClick={() => { setCurrent(p.id); openNew(p.id); }}>
                        <Plus className="size-3.5" aria-hidden /> New mission
                      </button>
                      <button className="btn btn-sm btn-danger" onClick={() => setRemoving(p)} aria-label={`Remove ${p.name}`}>
                        <Trash2 className="size-3.5" aria-hidden />
                      </button>
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
      <RemoveDialog project={removing} onClose={() => setRemoving(null)} />
    </div>
  );
}

function RemoveDialog({ project, onClose }: { project: Project | null; onClose: () => void }) {
  const [busy, setBusy] = useState(false);
  return (
    <Dialog.Root open={!!project} onOpenChange={(o) => !o && !busy && onClose()}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-50 bg-black/60" />
        <Dialog.Content className="fixed left-1/2 top-1/2 z-50 w-[480px] max-w-[94vw] -translate-x-1/2 -translate-y-1/2 rounded-lg border border-line-strong bg-panel shadow-2xl shadow-black/60" aria-describedby="rm-body">
          <div className="border-b border-line px-4 py-3">
            <Dialog.Title className="m-0 text-[14px] font-semibold">Remove project from FORGE?</Dialog.Title>
          </div>
          <div id="rm-body" className="space-y-2 p-4">
            <p className="m-0">
              <span className="font-medium">{project?.name}</span> will be archived: it disappears from FORGE's project list.
            </p>
            <p className="m-0 text-muted">Your repository folder on disk is not modified or deleted. Mission worktrees and branches already created are not removed by this action.</p>
          </div>
          <div className="flex justify-end gap-2 border-t border-line px-4 py-3">
            <button className="btn" onClick={onClose} disabled={busy}>Cancel</button>
            <button
              className="btn btn-danger-solid"
              disabled={busy}
              onClick={async () => {
                if (!project) return;
                setBusy(true);
                await removeProject(project);
                setBusy(false);
                onClose();
              }}
            >
              Remove project
            </button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

function ProjectView({ project }: { project: Project }) {
  const navigate = useStore((s) => s.navigate);
  const openNew = useStore((s) => s.openNewMission);
  const missions = useStore((s) => s.missions);
  const { map, reload } = useProjectStatuses(useMemo(() => [project], [project]));
  const [tab, setTab] = useState<'missions' | 'code'>('missions');
  const rows = useMemo(
    () => Object.values(missions).filter((m) => m.projectId === project.id).sort((a, b) => b.updatedAt.localeCompare(a.updatedAt)),
    [missions, project.id],
  );
  return (
    <div className="flex h-full min-h-0 flex-col">
      <PageHeader
        title={project.name}
        sub={<span className="mono text-[12px]">{project.rootPath}</span>}
        actions={
          <>
            <button className="btn" onClick={() => navigate({ name: 'projects' })}>
              <ArrowLeft className="size-3.5" aria-hidden /> All projects
            </button>
            <button className="btn" onClick={() => void reload()}>
              <RefreshCw className="size-3.5" aria-hidden /> Refresh
            </button>
            <button className="btn btn-primary" onClick={() => openNew(project.id)}>
              <Plus className="size-3.5" aria-hidden /> New mission
            </button>
          </>
        }
      />
      <div className="flex flex-wrap items-center gap-x-6 gap-y-1 border-b border-line px-5 py-2">
        <span className="text-muted">Default branch <span className="mono text-fg">{project.defaultBranch}</span></span>
        <StatusCells s={map[project.id]} />
        <span className="ml-auto flex gap-2"><ExecutionBackendPanel project={project} scope="agents" /><ExecutionBackendPanel project={project} /></span>
      </div>
      <div role="tablist" aria-label="Project views" className="flex gap-1 border-b border-line px-5">
        {([['missions', 'Missions'], ['code', 'Code map']] as const).map(([v, label]) => (
          <button key={v} role="tab" aria-selected={tab === v} onClick={() => setTab(v)} className={`h-8 px-3 ${tab === v ? 'border-b-2 border-accent text-fg' : 'text-muted hover:text-fg'}`}>{label}</button>
        ))}
      </div>
      <div role="tabpanel" className="min-h-0 flex-1 overflow-auto">
        {tab === 'missions'
          ? <MissionsTable missions={rows} showProject={false} emptyHint="No missions in this project yet." />
          : <CodeMapPanel key={project.id} project={project} />}
      </div>
    </div>
  );
}
