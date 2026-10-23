import { create } from 'zustand';
import type { ActivityEvent, AppInfo, Approval, Mission, MissionDetail, Project, ProviderInfo, UiEvent } from './types';
import {
  addNotice, dismissNotice, emptyData, mergeEvents, reduceUiEvent, upsertApprovals, upsertMission, upsertMissions,
  upsertSession, type DataState,
} from './reducers';
import { readStore, writeStore } from './storage';

export type Route =
  | { name: 'center' }
  | { name: 'projects'; projectId?: string }
  | { name: 'missions' }
  | { name: 'mission'; id: string }
  | { name: 'knowledge'; filter?: 'proposed'; pageId?: string; newPage?: boolean }
  | { name: 'settings' };

export interface DiscardState {
  missionId: string;
  /** Exact core message when it warned about permanent loss; null while asking the first confirmation. */
  lossMessage: string | null;
  busy: boolean;
}

export interface Store extends DataState {
  loaded: boolean;
  projects: Project[];
  providers: ProviderInfo[] | null;
  appInfo: AppInfo | null;
  currentProjectId: string | null;
  route: Route;
  newMission: { open: boolean; projectId?: string };
  paletteOpen: boolean;
  discard: DiscardState | null;
  /** Epoch ms of the previous visit, read once at startup. */
  lastVisit: number | null;

  navigate: (r: Route) => void;
  setCurrentProject: (id: string | null) => void;
  openNewMission: (projectId?: string) => void;
  closeNewMission: () => void;
  setPalette: (open: boolean) => void;
  setDiscard: (d: DiscardState | null) => void;
  applyEvent: (e: UiEvent) => void;
  notify: (level: 'info' | 'warn' | 'error', message: string, link?: { label: string; pageId: string }) => void;
  dismiss: (id: number) => void;
  setProjects: (p: Project[]) => void;
  setMissions: (m: Mission[]) => void;
  upsertMission: (m: Mission) => void;
  setApprovals: (a: Approval[]) => void;
  ingestDetail: (d: MissionDetail) => void;
  ingestEvents: (missionId: string, e: ActivityEvent[]) => void;
  setProviders: (p: ProviderInfo[]) => void;
  setAppInfo: (a: AppInfo) => void;
  setLoaded: () => void;
  markVisited: () => void;
}

const CURRENT_PROJECT_KEY = 'forge.currentProject';
const LAST_VISIT_KEY = 'forge.lastVisit';

function readLastVisit(): number | null {
  const v = readStore(LAST_VISIT_KEY);
  const n = v ? Number(v) : NaN;
  return Number.isFinite(n) ? n : null;
}

export const useStore = create<Store>((set) => ({
  ...emptyData(),
  loaded: false,
  projects: [],
  providers: null,
  appInfo: null,
  currentProjectId: readStore(CURRENT_PROJECT_KEY),
  route: { name: 'center' },
  newMission: { open: false },
  paletteOpen: false,
  discard: null,
  lastVisit: readLastVisit(),

  navigate: (route) => set({ route }),
  setCurrentProject: (id) => {
    if (id) writeStore(CURRENT_PROJECT_KEY, id);
    set({ currentProjectId: id });
  },
  openNewMission: (projectId) => set({ newMission: { open: true, projectId } }),
  closeNewMission: () => set({ newMission: { open: false } }),
  setPalette: (paletteOpen) => set({ paletteOpen }),
  setDiscard: (discard) => set({ discard }),
  applyEvent: (e) => set((s) => reduceUiEvent(s, e)),
  notify: (level, message, link) => set((s) => addNotice(s, level, message, link)),
  dismiss: (id) => set((s) => ({ notices: dismissNotice(s.notices, id) })),
  setProjects: (projects) => set({ projects }),
  setMissions: (list) => set((s) => ({ missions: upsertMissions(s.missions, list) })),
  upsertMission: (m) => set((s) => ({ missions: upsertMission(s.missions, m) })),
  setApprovals: (list) => set((s) => ({ approvals: upsertApprovals(s.approvals, list) })),
  ingestDetail: (d) =>
    set((s) => {
      let sessions = s.sessions;
      for (const x of d.sessions) sessions = upsertSession(sessions, x);
      return {
        missions: upsertMission(s.missions, d.mission),
        approvals: upsertApprovals(s.approvals, d.approvals),
        sessions,
      };
    }),
  ingestEvents: (missionId, events) =>
    set((s) => {
      const merged = mergeEvents(s.activity[missionId], events);
      return merged === s.activity[missionId] ? s : { activity: { ...s.activity, [missionId]: merged } };
    }),
  setProviders: (providers) => set({ providers }),
  setAppInfo: (appInfo) => set({ appInfo }),
  setLoaded: () => set({ loaded: true }),
  markVisited: () => {
    const now = Date.now();
    writeStore(LAST_VISIT_KEY, String(now));
    set({ lastVisit: now });
  },
}));

export function selectPendingApprovals(approvals: Record<string, Approval>): Approval[] {
  return Object.values(approvals)
    .filter((a) => a.state === 'pending')
    .sort((a, b) => a.createdAt.localeCompare(b.createdAt));
}
