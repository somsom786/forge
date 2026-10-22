import { useEffect } from 'react';
import { startCore } from './lib/actions';
import { writeStore } from './lib/storage';
import { useStore } from './lib/store';
import { CommandPalette } from './components/CommandPalette';
import { DiscardDialog } from './components/DiscardDialog';
import { NewMissionDialog } from './components/NewMissionDialog';
import { Shell } from './components/Shell';
import { Toasts } from './components/ui';
import { CommandCenter } from './screens/CommandCenter';
import { MissionWorkspace } from './screens/MissionWorkspace';
import { MissionsScreen } from './screens/Missions';
import { ProjectsScreen } from './screens/Projects';
import { SettingsScreen } from './screens/Settings';

function Screen() {
  const route = useStore((s) => s.route);
  switch (route.name) {
    case 'center':
      return <CommandCenter />;
    case 'projects':
      return <ProjectsScreen projectId={route.projectId} />;
    case 'missions':
      return <MissionsScreen />;
    case 'mission':
      return <MissionWorkspace key={route.id} id={route.id} />;
    case 'settings':
      return <SettingsScreen />;
  }
}

export default function App() {
  useEffect(() => {
    void startCore();
    const persist = () => writeStore('forge.lastVisit', String(Date.now()));
    const onVis = () => document.visibilityState === 'hidden' && persist();
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        const s = useStore.getState();
        s.setPalette(!s.paletteOpen);
      }
    };
    window.addEventListener('beforeunload', persist);
    document.addEventListener('visibilitychange', onVis);
    window.addEventListener('keydown', onKey);
    return () => {
      window.removeEventListener('beforeunload', persist);
      document.removeEventListener('visibilitychange', onVis);
      window.removeEventListener('keydown', onKey);
    };
  }, []);

  return (
    <>
      <Shell>
        <Screen />
      </Shell>
      <CommandPalette />
      <NewMissionDialog />
      <DiscardDialog />
      <Toasts />
    </>
  );
}
