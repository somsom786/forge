import { Command } from 'cmdk';
import { FolderGit2, FolderPlus, LayoutDashboard, ListChecks, Plus, Settings, Target } from 'lucide-react';
import { addProjectViaDialog } from '../lib/actions';
import { missionLabel } from '../lib/format';
import { useStore } from '../lib/store';

export function CommandPalette() {
  const open = useStore((s) => s.paletteOpen);
  const setOpen = useStore((s) => s.setPalette);
  const projects = useStore((s) => s.projects);
  const missions = useStore((s) => s.missions);
  const navigate = useStore((s) => s.navigate);
  const setCurrent = useStore((s) => s.setCurrentProject);
  const currentProjectId = useStore((s) => s.currentProjectId);
  const openNew = useStore((s) => s.openNewMission);

  const run = (fn: () => void) => () => {
    setOpen(false);
    fn();
  };
  const projectName = (id: string) => projects.find((p) => p.id === id)?.name ?? '';
  const list = Object.values(missions).sort((a, b) => b.updatedAt.localeCompare(a.updatedAt)).slice(0, 60);

  const item = 'flex h-8 cursor-pointer items-center gap-2.5 rounded-[5px] px-2.5 text-muted aria-selected:bg-accent-dim aria-selected:text-fg';

  return (
    <Command.Dialog
      open={open}
      onOpenChange={setOpen}
      label="Command palette"
      overlayClassName="fixed inset-0 z-50 bg-black/55"
      contentClassName="fixed left-1/2 top-[14vh] z-50 w-[620px] max-w-[92vw] -translate-x-1/2 overflow-hidden rounded-lg border border-line-strong bg-panel shadow-2xl shadow-black/60"
    >
      <Command.Input placeholder="Type a command, project or mission…" className="h-11 w-full border-b border-line bg-transparent px-4 text-[14px] outline-none placeholder:text-faint" />
      <Command.List className="max-h-[52vh] overflow-y-auto p-1.5">
        <Command.Empty className="px-3 py-6 text-center text-muted">Nothing matches.</Command.Empty>
        <Command.Group heading="Actions" className="text-[11px] uppercase tracking-wide text-faint [&_[cmdk-group-heading]]:px-2.5 [&_[cmdk-group-heading]]:py-1.5">
          <Command.Item className={item} value="new mission" disabled={projects.length === 0} onSelect={run(() => openNew(currentProjectId ?? undefined))}>
            <Plus className="size-4" aria-hidden /> New mission
          </Command.Item>
          <Command.Item className={item} value="add project" onSelect={run(() => void addProjectViaDialog())}>
            <FolderPlus className="size-4" aria-hidden /> Add project…
          </Command.Item>
        </Command.Group>
        <Command.Group heading="Navigate" className="text-[11px] uppercase tracking-wide text-faint [&_[cmdk-group-heading]]:px-2.5 [&_[cmdk-group-heading]]:py-1.5">
          <Command.Item className={item} value="go command center inbox" onSelect={run(() => navigate({ name: 'center' }))}>
            <LayoutDashboard className="size-4" aria-hidden /> Command Center
          </Command.Item>
          <Command.Item className={item} value="go projects" onSelect={run(() => navigate({ name: 'projects' }))}>
            <FolderGit2 className="size-4" aria-hidden /> Projects
          </Command.Item>
          <Command.Item className={item} value="go missions" onSelect={run(() => navigate({ name: 'missions' }))}>
            <ListChecks className="size-4" aria-hidden /> Missions
          </Command.Item>
          <Command.Item className={item} value="go settings" onSelect={run(() => navigate({ name: 'settings' }))}>
            <Settings className="size-4" aria-hidden /> Settings
          </Command.Item>
        </Command.Group>
        {projects.length > 0 && (
          <Command.Group heading="Switch project" className="text-[11px] uppercase tracking-wide text-faint [&_[cmdk-group-heading]]:px-2.5 [&_[cmdk-group-heading]]:py-1.5">
            {projects.map((p) => (
              <Command.Item key={p.id} className={item} value={`switch project ${p.name} ${p.rootPath}`} onSelect={run(() => setCurrent(p.id))}>
                <FolderGit2 className="size-4" aria-hidden />
                <span className="text-fg">{p.name}</span>
                {p.id === currentProjectId && <span className="text-[11px] text-accent">current</span>}
                <span className="mono ml-auto truncate text-[11px] text-faint">{p.rootPath}</span>
              </Command.Item>
            ))}
          </Command.Group>
        )}
        {list.length > 0 && (
          <Command.Group heading="Open mission" className="text-[11px] uppercase tracking-wide text-faint [&_[cmdk-group-heading]]:px-2.5 [&_[cmdk-group-heading]]:py-1.5">
            {list.map((m) => (
              <Command.Item key={m.id} className={item} value={`open mission ${missionLabel(m)} ${projectName(m.projectId)} ${m.state} ${m.id}`} onSelect={run(() => navigate({ name: 'mission', id: m.id }))}>
                <Target className="size-4 shrink-0" aria-hidden />
                <span className="truncate text-fg">{missionLabel(m)}</span>
                <span className="ml-auto shrink-0 text-[11px] text-faint">{projectName(m.projectId)} · {m.state.replace('_', ' ')}</span>
              </Command.Item>
            ))}
          </Command.Group>
        )}
      </Command.List>
    </Command.Dialog>
  );
}
