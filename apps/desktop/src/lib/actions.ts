// Side-effecting operations. Every failure is surfaced as a toast; nothing is swallowed.
import { open } from '@tauri-apps/plugin-dialog';
import { api, subscribe } from './api';
import { errMsg } from './format';
import { useStore } from './store';
import type { Mission, NewMissionArgs, Project } from './types';

const st = () => useStore.getState();

/** Run an async call, toast the error string on failure and return undefined. */
export async function guarded<T>(p: Promise<T>, context?: string): Promise<T | undefined> {
  try {
    return await p;
  } catch (e) {
    st().notify('error', context ? `${context}: ${errMsg(e)}` : errMsg(e));
    return undefined;
  }
}

export async function refreshAppInfo() {
  const info = await guarded(api.appInfo(), 'Could not read app info');
  if (info) st().setAppInfo(info);
}

export async function refreshProviders() {
  const p = await guarded(api.providersDetect(), 'Provider detection failed');
  if (p) st().setProviders(p);
  return p;
}

export async function refreshProjects() {
  const p = await guarded(api.projectList(), 'Could not load projects');
  if (p) {
    st().setProjects(p);
    const cur = st().currentProjectId;
    if (p.length && (!cur || !p.some((x) => x.id === cur))) st().setCurrentProject(p[0].id);
  }
}

export async function refreshMissions() {
  const m = await guarded(api.missionList(), 'Could not load missions');
  if (m) st().setMissions(m);
}

export async function refreshApprovals() {
  const a = await guarded(api.approvalsPending(), 'Could not load approvals');
  if (a) st().setApprovals(a);
}

export async function loadAll() {
  await Promise.all([refreshProjects(), refreshMissions(), refreshApprovals(), refreshAppInfo(), refreshProviders()]);
  const rec = await guarded(api.recoveryReport());
  if (rec && rec.interruptedMissions.length > 0) {
    st().notify(
      'warn',
      `${rec.interruptedMissions.length} mission(s) were interrupted when FORGE last closed; ${rec.expiredApprovals} approval(s) expired.`,
    );
  }
  st().setLoaded();
}

let bridgeInstalled = false;
let infoTimer: ReturnType<typeof setTimeout> | null = null;

/** Installs the single forge://event listener, then loads fresh lists. Safe to call more than once. */
export async function startCore() {
  if (bridgeInstalled) return;
  bridgeInstalled = true;
  try {
    await subscribe((e) => {
      st().applyEvent(e);
      if (e.type === 'session' || e.type === 'mission') {
        if (infoTimer) clearTimeout(infoTimer);
        infoTimer = setTimeout(() => void refreshAppInfo(), 400);
      }
    });
  } catch (e) {
    st().notify('error', `Could not subscribe to core events: ${errMsg(e)}`);
  }
  await loadAll();
  window.setInterval(() => void refreshAppInfo(), 15000);
}

export async function addProjectViaDialog(): Promise<Project | undefined> {
  let picked: string | string[] | null;
  try {
    picked = await open({ directory: true, multiple: false, title: 'Choose a git repository' });
  } catch (e) {
    st().notify('error', `Could not open folder picker: ${errMsg(e)}`);
    return undefined;
  }
  const path = Array.isArray(picked) ? picked[0] : picked;
  if (!path) return undefined;
  const p = await guarded(api.projectAdd(path), 'Could not add project');
  if (p) {
    await refreshProjects();
    st().setCurrentProject(p.id);
    st().notify('info', `Added project ${p.name}`);
  }
  return p;
}

export async function removeProject(p: Project) {
  const ok = await guarded(api.projectRemove(p.id).then(() => true), 'Could not remove project');
  if (ok) {
    await refreshProjects();
    st().notify('info', `Archived project ${p.name}`);
  }
}

export async function createMission(args: NewMissionArgs, start: boolean): Promise<Mission | undefined> {
  const m = await guarded(api.missionCreate(args), 'Could not create mission');
  if (!m) return undefined;
  st().upsertMission(m);
  if (!start) return m;
  const started = await guarded(api.missionStart(m.id), 'Mission created but could not start');
  if (started) st().upsertMission(started);
  return started ?? m;
}

export async function startMission(id: string) {
  const m = await guarded(api.missionStart(id), 'Could not start mission');
  if (m) st().upsertMission(m);
  return m;
}

export async function followupMission(id: string, text: string) {
  const m = await guarded(api.missionFollowup(id, text), 'Could not send follow-up');
  if (m) st().upsertMission(m);
  return m;
}

export async function stopMission(id: string) {
  const ok = await guarded(api.missionStop(id).then(() => true), 'Could not stop mission');
  if (ok) st().notify('info', 'Stop requested');
  return ok;
}

export async function openInEditor(id: string) {
  await guarded(api.openWorktreeInEditor(id), 'Could not open editor');
}

export async function decideApproval(id: string, approved: boolean, note?: string) {
  const a = await guarded(api.approvalDecide(id, approved, note?.trim() ? note.trim() : undefined), 'Could not record decision');
  if (a) st().setApprovals([a]);
  return a;
}

/** Heuristic for the core's "this will permanently delete" rejection. */
export function isLossWarning(msg: string): boolean {
  return /permanent|uncommitted|unmerged|commit/i.test(msg);
}

export function beginDiscard(missionId: string) {
  st().setDiscard({ missionId, lossMessage: null, busy: false });
}

/** First attempt with confirmLoss=false; a loss warning is surfaced verbatim for explicit confirmation. */
export async function attemptDiscard() {
  const d = st().discard;
  if (!d) return;
  st().setDiscard({ ...d, busy: true });
  try {
    const m = await api.missionDiscard(d.missionId, false);
    st().upsertMission(m);
    st().setDiscard(null);
    st().notify('info', 'Mission discarded');
  } catch (e) {
    const msg = errMsg(e);
    if (isLossWarning(msg)) {
      st().setDiscard({ missionId: d.missionId, lossMessage: msg, busy: false });
    } else {
      st().setDiscard(null);
      st().notify('error', `Could not discard mission: ${msg}`);
    }
  }
}

export async function confirmDiscardLoss() {
  const d = st().discard;
  if (!d) return;
  st().setDiscard({ ...d, busy: true });
  try {
    const m = await api.missionDiscard(d.missionId, true);
    st().upsertMission(m);
    st().notify('info', 'Mission discarded');
  } catch (e) {
    st().notify('error', `Could not discard mission: ${errMsg(e)}`);
  } finally {
    st().setDiscard(null);
  }
}
