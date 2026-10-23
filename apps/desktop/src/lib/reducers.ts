// Pure state reducers. The zustand store only wires these to set(); all logic is testable here.
import type { ActivityEvent, AgentSession, Approval, Mission, UiEvent, VerificationRun } from './types';
import { upsertRun } from './verification';

export interface Notice {
  id: number;
  level: 'info' | 'warn' | 'error';
  message: string;
  /** Optional in-app link, e.g. to a knowledge page. */
  link?: { label: string; pageId: string };
}

export interface DataState {
  missions: Record<string, Mission>;
  approvals: Record<string, Approval>;
  sessions: Record<string, AgentSession>;
  /** Per mission, deduplicated by event id and ordered by id. */
  activity: Record<string, ActivityEvent[]>;
  /** Transient streaming text per mission, keyed by messageId (NULL_MESSAGE for unkeyed deltas). */
  streams: Record<string, Record<string, string>>;
  /** Verification runs pushed by the core, per mission. */
  runs: Record<string, VerificationRun[]>;
  /** Bumped on every verification event so views can refresh their report. */
  verifyTick: Record<string, number>;
  /** Bumped when the core seals a Proof Pack for a mission. */
  proofTick: Record<string, number>;
  /** Bumped when knowledge changes in a project, so lists and counts refetch. */
  knowledgeTick: Record<string, number>;
  notices: Notice[];
  noticeSeq: number;
}

export const NULL_MESSAGE = '__unkeyed__';
const MAX_NOTICES = 6;

export function emptyData(): DataState {
  return { missions: {}, approvals: {}, sessions: {}, activity: {}, streams: {}, runs: {}, verifyTick: {}, proofTick: {}, knowledgeTick: {}, notices: [], noticeSeq: 0 };
}

/** Merge events into an ordered list, dropping duplicate ids. Returns the same array if nothing changed. */
export function mergeEvents(existing: ActivityEvent[] | undefined, incoming: ActivityEvent[]): ActivityEvent[] {
  const base = existing ?? [];
  if (incoming.length === 0) return base;
  const seen = new Set<number>();
  for (const e of base) seen.add(e.id);
  const fresh: ActivityEvent[] = [];
  for (const e of incoming) {
    if (!seen.has(e.id)) {
      seen.add(e.id);
      fresh.push(e);
    }
  }
  if (fresh.length === 0) return base;
  // Fast path: strictly appending in order.
  const lastId = base.length ? base[base.length - 1].id : -Infinity;
  const sorted = [...fresh].sort((a, b) => a.id - b.id);
  if (sorted[0].id > lastId) return base.concat(sorted);
  return [...base, ...sorted].sort((a, b) => a.id - b.id);
}

export function upsertMission(missions: Record<string, Mission>, m: Mission): Record<string, Mission> {
  const cur = missions[m.id];
  if (cur && cur.updatedAt > m.updatedAt) return missions; // stale push
  return { ...missions, [m.id]: m };
}

export function upsertMissions(missions: Record<string, Mission>, list: Mission[]): Record<string, Mission> {
  let out = missions;
  for (const m of list) out = upsertMission(out, m);
  return out;
}

/** A decided approval is never reverted to pending by a stale snapshot. */
export function upsertApproval(approvals: Record<string, Approval>, a: Approval): Record<string, Approval> {
  const cur = approvals[a.id];
  if (cur && cur.state !== 'pending' && a.state === 'pending') return approvals;
  return { ...approvals, [a.id]: a };
}

export function upsertApprovals(approvals: Record<string, Approval>, list: Approval[]): Record<string, Approval> {
  let out = approvals;
  for (const a of list) out = upsertApproval(out, a);
  return out;
}

export function upsertSession(sessions: Record<string, AgentSession>, s: AgentSession): Record<string, AgentSession> {
  return { ...sessions, [s.id]: s };
}

export function applyDelta(
  streams: DataState['streams'],
  missionId: string,
  messageId: string | null,
  text: string,
): DataState['streams'] {
  const key = messageId ?? NULL_MESSAGE;
  const forMission = streams[missionId] ?? {};
  return { ...streams, [missionId]: { ...forMission, [key]: (forMission[key] ?? '') + text } };
}

export function clearStream(streams: DataState['streams'], missionId: string, messageId: string | null): DataState['streams'] {
  const key = messageId ?? NULL_MESSAGE;
  const forMission = streams[missionId];
  if (!forMission || !(key in forMission)) return streams;
  const rest = { ...forMission };
  delete rest[key];
  const out = { ...streams };
  if (Object.keys(rest).length) out[missionId] = rest;
  else delete out[missionId];
  return out;
}

export function clearMissionStreams(streams: DataState['streams'], missionId: string): DataState['streams'] {
  if (!(missionId in streams)) return streams;
  const out = { ...streams };
  delete out[missionId];
  return out;
}

export function addNotice(s: Pick<DataState, 'notices' | 'noticeSeq'>, level: Notice['level'], message: string, link?: Notice['link']) {
  const id = s.noticeSeq + 1;
  const notices = [...s.notices, { id, level, message, link }].slice(-MAX_NOTICES);
  return { notices, noticeSeq: id };
}

export function dismissNotice(notices: Notice[], id: number): Notice[] {
  return notices.filter((n) => n.id !== id);
}

/** Apply one pushed event to the data slice. */
export function reduceUiEvent(s: DataState, e: UiEvent): DataState {
  switch (e.type) {
    case 'activity': {
      const ev = e.event;
      const activity = { ...s.activity, [ev.missionId]: mergeEvents(s.activity[ev.missionId], [ev]) };
      let streams = s.streams;
      if (ev.kind === 'agent.text') {
        streams = clearStream(streams, ev.missionId, (ev.payload?.messageId as string | undefined) ?? null);
      } else if (ev.kind === 'turn.completed' || ev.kind === 'error') {
        streams = clearMissionStreams(streams, ev.missionId);
      }
      return { ...s, activity, streams };
    }
    case 'delta':
      return { ...s, streams: applyDelta(s.streams, e.missionId, e.messageId, e.text) };
    case 'mission':
      return { ...s, missions: upsertMission(s.missions, e.mission) };
    case 'approval':
      return { ...s, approvals: upsertApproval(s.approvals, e.approval) };
    case 'session':
      return { ...s, sessions: upsertSession(s.sessions, e.session) };
    case 'verification':
      return {
        ...s,
        runs: { ...s.runs, [e.run.missionId]: upsertRun(s.runs[e.run.missionId], e.run) },
        verifyTick: { ...s.verifyTick, [e.run.missionId]: (s.verifyTick[e.run.missionId] ?? 0) + 1 },
      };
    case 'proofPack':
      return { ...s, proofTick: { ...s.proofTick, [e.missionId]: (s.proofTick[e.missionId] ?? 0) + 1 } };
    case 'knowledge':
      return { ...s, knowledgeTick: { ...s.knowledgeTick, [e.projectId]: (s.knowledgeTick[e.projectId] ?? 0) + 1 } };
    case 'notice':
      return { ...s, ...addNotice(s, e.level, e.message) };
    default:
      return s;
  }
}
