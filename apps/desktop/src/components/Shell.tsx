import type { ReactNode } from 'react';
import {
  Bot, BookOpen, FlaskConical, FolderGit2, LayoutDashboard, ListChecks, Plus, Search, Settings, Wrench, type LucideIcon,
} from 'lucide-react';
import { selectPendingApprovals, useStore, type Route } from '../lib/store';
import { Kbd } from './ui';

interface NavItem {
  key: string;
  label: string;
  icon: LucideIcon;
  route?: Route;
  tag?: string;
  match: (r: Route) => boolean;
}

const NAV: NavItem[] = [
  { key: 'center', label: 'Command Center', icon: LayoutDashboard, route: { name: 'center' }, match: (r) => r.name === 'center' },
  { key: 'projects', label: 'Projects', icon: FolderGit2, route: { name: 'projects' }, match: (r) => r.name === 'projects' },
  { key: 'missions', label: 'Missions', icon: ListChecks, route: { name: 'missions' }, match: (r) => r.name === 'missions' || r.name === 'mission' },
  { key: 'agents', label: 'Agents', icon: Bot, tag: 'Planned', match: () => false },
  { key: 'knowledge', label: 'Knowledge', icon: BookOpen, route: { name: 'knowledge' }, match: (r) => r.name === 'knowledge' },
  { key: 'harness', label: 'Harness Studio', icon: Wrench, route: { name: 'harness' }, match: (r) => r.name === 'harness' },
  { key: 'experiments', label: 'Experiments', icon: FlaskConical, tag: 'Phase 6', match: () => false },
  { key: 'settings', label: 'Settings', icon: Settings, route: { name: 'settings' }, match: (r) => r.name === 'settings' },
];

export function Shell({ children }: { children: ReactNode }) {
  const route = useStore((s) => s.route);
  const navigate = useStore((s) => s.navigate);
  const pending = useStore((s) => selectPendingApprovals(s.approvals).length);

  return (
    <div className="flex h-full min-h-0 flex-col">
      <TopBar />
      <div className="flex min-h-0 flex-1">
        <nav aria-label="Primary" className="flex w-[220px] shrink-0 flex-col gap-0.5 border-r border-line bg-panel px-2 py-2">
          {NAV.map((n) => {
            const Icon = n.icon;
            const disabled = !n.route;
            const active = n.match(route);
            const cls = `flex h-8 items-center gap-2.5 rounded-[5px] px-2.5 text-left transition-colors ${
              disabled
                ? 'cursor-not-allowed text-faint'
                : active
                  ? 'bg-accent-dim text-fg'
                  : 'text-muted hover:bg-hover hover:text-fg'
            }`;
            return (
              <button
                key={n.key}
                disabled={disabled}
                aria-disabled={disabled}
                aria-current={active ? 'page' : undefined}
                title={disabled ? `${n.label}: not built yet (${n.tag})` : undefined}
                onClick={() => n.route && navigate(n.route)}
                className={cls}
              >
                <Icon className={`size-4 shrink-0 ${active ? 'text-accent' : ''}`} aria-hidden />
                <span className="flex-1 truncate">{n.label}</span>
                {n.tag && <span className="rounded-sm border border-line-strong px-1 text-[10px] uppercase tracking-wide text-faint">{n.tag}</span>}
                {n.key === 'center' && pending > 0 && (
                  <span className="rounded-full bg-warn px-1.5 text-[11px] font-semibold text-black" aria-label={`${pending} approvals pending`}>
                    {pending}
                  </span>
                )}
              </button>
            );
          })}
        </nav>
        <main className="min-h-0 min-w-0 flex-1 overflow-hidden bg-bg">{children}</main>
      </div>
    </div>
  );
}

function TopBar() {
  const projects = useStore((s) => s.projects);
  const current = useStore((s) => s.currentProjectId);
  const setCurrent = useStore((s) => s.setCurrentProject);
  const setPalette = useStore((s) => s.setPalette);
  const openNew = useStore((s) => s.openNewMission);
  const appInfo = useStore((s) => s.appInfo);
  const pending = useStore((s) => selectPendingApprovals(s.approvals).length);
  const navigate = useStore((s) => s.navigate);
  const live = appInfo?.liveSessions;

  return (
    <header className="flex h-11 shrink-0 items-center gap-3 border-b border-line bg-panel px-3">
      <div className="flex items-center gap-2 pr-2">
        <span className="mono text-[13px] font-bold tracking-[0.18em] text-fg">FORGE</span>
      </div>
      <label className="flex items-center gap-2 text-muted">
        <span className="sr-only">Current project</span>
        <FolderGit2 className="size-4" aria-hidden />
        <select
          className="input !h-7 !w-[200px] !py-0"
          value={current ?? ''}
          onChange={(e) => setCurrent(e.target.value || null)}
          disabled={projects.length === 0}
          aria-label="Current project"
        >
          {projects.length === 0 && <option value="">No projects</option>}
          {projects.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}
            </option>
          ))}
        </select>
      </label>
      <button className="btn btn-sm" onClick={() => openNew(current ?? undefined)} disabled={projects.length === 0} title={projects.length === 0 ? 'Add a project first' : undefined}>
        <Plus className="size-3.5" aria-hidden /> New mission
      </button>
      <div className="flex-1" />
      <button
        className="btn btn-sm min-w-[230px] justify-between text-muted"
        onClick={() => setPalette(true)}
        aria-label="Open command palette"
      >
        <span className="flex items-center gap-2">
          <Search className="size-3.5" aria-hidden /> Search or run a command
        </span>
        <span className="flex gap-1">
          <Kbd>Ctrl</Kbd>
          <Kbd>K</Kbd>
        </span>
      </button>
      {pending > 0 && (
        <button className="btn btn-sm border-warn/40 text-warn" onClick={() => navigate({ name: 'center' })}>
          {pending} need{pending === 1 ? 's' : ''} you
        </button>
      )}
      <div className="flex items-center gap-2 text-muted" title="Agent processes currently attached, as reported by the core" aria-live="polite">
        <span className={`inline-block size-2 rounded-full ${live ? 'live-dot bg-run' : 'bg-line-strong'}`} aria-hidden />
        <span>{live === undefined ? 'Connecting…' : `${live} agent${live === 1 ? '' : 's'} running`}</span>
      </div>
    </header>
  );
}
