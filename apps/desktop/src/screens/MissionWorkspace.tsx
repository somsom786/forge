import { useEffect, useMemo, useRef, useState } from 'react';
import * as Tabs from '@radix-ui/react-tabs';
import { ArrowLeft, ExternalLink, Play, RotateCcw, Send, Square, Trash2 } from 'lucide-react';
import { api } from '../lib/api';
import {
  beginDiscard, followupMission, openInEditor, startMission, stopMission,
} from '../lib/actions';
import { errMsg, formatDuration, isActiveState, providerLabel, shortSha } from '../lib/format';
import { useStore } from '../lib/store';
import type { ActivityEvent, Mission, MissionDetail } from '../lib/types';
import { ActivityFeed } from '../components/ActivityFeed';
import { ApprovalsPanel, ChangesPanel, TimelinePanel } from '../components/MissionPanels';
import { SplitLayout } from '../components/Splitter';
import { ContractTab } from '../components/ContractTab';
import { MemoryTab } from '../components/MemoryTab';
import { ProofPackPanel } from '../components/ProofPackPanel';
import { ReviewBar } from '../components/ReviewBar';
import { useReport, VerificationPanel } from '../components/VerificationPanel';
import { Callout, EmptyState, Kbd, StateBadge, useNow } from '../components/ui';

const EMPTY: ActivityEvent[] = [];
const PAGE = 500;

async function loadHistory(missionId: string, ingest: (id: string, e: ActivityEvent[]) => void) {
  let after: number | undefined;
  for (let guard = 0; guard < 200; guard++) {
    const batch = await api.missionEvents(missionId, after, PAGE);
    if (batch.length === 0) break;
    ingest(missionId, batch);
    after = batch[batch.length - 1].id;
    if (batch.length < PAGE) break;
  }
}

export function MissionWorkspace({ id }: { id: string }) {
  const mission = useStore((s) => s.missions[id]);
  const events = useStore((s) => s.activity[id]) ?? EMPTY;
  const streams = useStore((s) => s.streams[id]);
  const allApprovals = useStore((s) => s.approvals);
  const allSessions = useStore((s) => s.sessions);
  const project = useStore((s) => (mission ? s.projects.find((p) => p.id === mission.projectId) : undefined));
  const ingestDetail = useStore((s) => s.ingestDetail);
  const ingestEvents = useStore((s) => s.ingestEvents);
  const navigate = useStore((s) => s.navigate);
  const notify = useStore((s) => s.notify);

  const [detail, setDetail] = useState<MissionDetail | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [tab, setTab] = useState('changes');

  const approvals = useMemo(
    () => Object.values(allApprovals).filter((a) => a.missionId === id).sort((a, b) => a.createdAt.localeCompare(b.createdAt)),
    [allApprovals, id],
  );
  const sessions = useMemo(
    () => Object.values(allSessions).filter((s) => s.missionId === id).sort((a, b) => a.startedAt.localeCompare(b.startedAt)),
    [allSessions, id],
  );
  const pendingCount = approvals.filter((a) => a.state === 'pending').length;
  const sessionSig = sessions.map((s) => `${s.id}:${s.state}`).join('|');
  const fileEvents = useMemo(() => events.filter((e) => e.kind === 'file.changed').length, [events]);
  const { report, error: reportError, loading: reportLoading, reload: reloadReport } = useReport(id, mission?.updatedAt, mission?.state, fileEvents);

  // Initial load: detail + paged history. Live events merge into the same list, deduped by id.
  useEffect(() => {
    let cancelled = false;
    setDetail(null);
    setLoadError(null);
    (async () => {
      try {
        const d = await api.missionGet(id);
        if (cancelled) return;
        ingestDetail(d);
        setDetail(d);
        if (d.approvals.some((a) => a.state === 'pending')) setTab('approvals');
        await loadHistory(id, ingestEvents);
      } catch (e) {
        if (!cancelled) setLoadError(errMsg(e));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [id, ingestDetail, ingestEvents]);

  // Keep detail (running flag, transitions) fresh when the mission or its sessions change.
  const firstRun = useRef(true);
  useEffect(() => {
    if (firstRun.current) {
      firstRun.current = false;
      return;
    }
    let cancelled = false;
    const t = window.setTimeout(() => {
      api.missionGet(id).then(
        (d) => {
          if (!cancelled) {
            ingestDetail(d);
            setDetail(d);
          }
        },
        (e) => !cancelled && notify('error', `Could not refresh mission: ${errMsg(e)}`),
      );
    }, 250);
    return () => {
      cancelled = true;
      window.clearTimeout(t);
    };
  }, [id, mission?.updatedAt, mission?.state, sessionSig, ingestDetail, notify]);

  // A new pending approval pulls the Approvals tab forward.
  const prevPending = useRef(0);
  useEffect(() => {
    if (pendingCount > prevPending.current) setTab('approvals');
    prevPending.current = pendingCount;
  }, [pendingCount]);

  if (!mission) {
    return (
      <div className="p-6">
        <button className="btn btn-sm" onClick={() => navigate({ name: 'missions' })}>
          <ArrowLeft className="size-3.5" aria-hidden /> Missions
        </button>
        {loadError ? (
          <div className="mt-4 max-w-xl">
            <Callout tone="bad" title="Could not load mission">
              <pre className="whitespace-pre-wrap break-words text-[12px]">{loadError}</pre>
            </Callout>
          </div>
        ) : (
          <EmptyState title="Loading mission…" />
        )}
      </div>
    );
  }

  const running = detail?.running ?? false;
  const detached = !!detail && isActiveState(mission.state) && !running;
  const canFollowUp = ['awaiting_review', 'failed', 'interrupted'].includes(mission.state);
  const canResume = mission.state === 'failed' || mission.state === 'interrupted';
  const canDiscard = !running && !['merged', 'rejected', 'cancelled'].includes(mission.state);

  return (
    <div className="flex h-full min-h-0 flex-col">
      <Header
        mission={mission}
        projectName={project?.name ?? detail?.project.name ?? ''}
        running={running}
        detached={detached}
        canResume={canResume}
        canDiscard={canDiscard}
        sessions={sessions}
      />
      <ReviewBar mission={mission} report={report} onReload={() => void reloadReport()} />
      <div className="min-h-0 flex-1">
        <SplitLayout storageKey="mission.right" fixed="end" defaultSize={500} min={340} max={1100}>
          <div className="flex min-h-0 flex-1 flex-col">
            {detached && (
              <div className="p-2 pb-0">
                <Callout tone="warn" title="No agent process is attached">
                  This mission is recorded as “{mission.state.replace('_', ' ')}” but the core reports no running agent for it. Nothing is working on it right now.
                </Callout>
              </div>
            )}
            {mission.state === 'interrupted' && (
              <div className="p-2 pb-0">
                <Callout tone="warn" title="Interrupted">
                  The agent process is gone (FORGE or the agent exited unexpectedly). You can resume the mission or discard it.
                </Callout>
              </div>
            )}
            {mission.lastError && (
              <div className="p-2 pb-0">
                <Callout tone="bad" title="Last error">
                  <pre className="max-h-48 overflow-auto whitespace-pre-wrap break-words text-[12px]">{mission.lastError}</pre>
                </Callout>
              </div>
            )}
            <ActivityFeed events={events} approvals={allApprovals} streams={streams} onFocusApprovals={() => setTab('approvals')} />
            {canFollowUp && <FollowUp missionId={id} />}
          </div>
          <Tabs.Root value={tab} onValueChange={setTab} className="flex min-h-0 flex-1 flex-col bg-panel">
            <Tabs.List className="flex shrink-0 overflow-x-auto border-b border-line [scrollbar-width:thin]" aria-label="Mission panels" onWheel={(e) => { if (e.deltaY !== 0) e.currentTarget.scrollLeft += e.deltaY; }}>
              {[
                { v: 'approvals', label: 'Approvals', count: pendingCount },
                { v: 'changes', label: 'Changes', count: 0 },
                { v: 'verification', label: 'Verification', count: report ? report.unresolved.length : 0 },
                { v: 'contract', label: 'Contract', count: 0 },
                { v: 'memory', label: 'Memory', count: 0 },
                { v: 'proof', label: 'Proof Pack', count: 0 },
                { v: 'timeline', label: 'Timeline', count: 0 },
              ].map((t) => (
                <Tabs.Trigger
                  key={t.v}
                  value={t.v}
                  className="relative flex h-9 shrink-0 items-center gap-1 px-2 text-[12px] text-muted transition-colors hover:text-fg data-[state=active]:text-fg data-[state=active]:after:absolute data-[state=active]:after:inset-x-2 data-[state=active]:after:bottom-0 data-[state=active]:after:h-0.5 data-[state=active]:after:bg-accent"
                >
                  {t.label}
                  {t.count > 0 && <span className="rounded-full bg-warn px-1.5 text-[11px] font-semibold text-black">{t.count}</span>}
                </Tabs.Trigger>
              ))}
            </Tabs.List>
            <Tabs.Content value="approvals" className="flex min-h-0 flex-1 flex-col data-[state=inactive]:hidden">
              <ApprovalsPanel approvals={approvals} />
            </Tabs.Content>
            <Tabs.Content value="changes" className="flex min-h-0 flex-1 flex-col data-[state=inactive]:hidden">
              <ChangesPanel mission={mission} fileEventCount={fileEvents} />
            </Tabs.Content>
            <Tabs.Content value="verification" className="flex min-h-0 flex-1 flex-col data-[state=inactive]:hidden">
              <VerificationPanel mission={mission} report={report} error={reportError} loading={reportLoading} reload={() => void reloadReport()} />
            </Tabs.Content>
            <Tabs.Content value="contract" className="flex min-h-0 flex-1 flex-col data-[state=inactive]:hidden">
              <ContractTab mission={mission} />
            </Tabs.Content>
            <Tabs.Content value="memory" className="flex min-h-0 flex-1 flex-col data-[state=inactive]:hidden">
              <MemoryTab mission={mission} />
            </Tabs.Content>
            <Tabs.Content value="proof" className="flex min-h-0 flex-1 flex-col data-[state=inactive]:hidden">
              <ProofPackPanel mission={mission} />
            </Tabs.Content>
            <Tabs.Content value="timeline" className="flex min-h-0 flex-1 flex-col data-[state=inactive]:hidden">
              <TimelinePanel transitions={detail?.transitions ?? []} sessions={sessions} />
            </Tabs.Content>
          </Tabs.Root>
        </SplitLayout>
      </div>
    </div>
  );
}

function Header({
  mission, projectName, running, detached, canResume, canDiscard, sessions,
}: {
  mission: Mission;
  projectName: string;
  running: boolean;
  detached: boolean;
  canResume: boolean;
  canDiscard: boolean;
  sessions: { startedAt: string; endedAt: string | null }[];
}) {
  const navigate = useStore((s) => s.navigate);
  const now = useNow(running ? 1000 : 60000);
  const [busy, setBusy] = useState(false);

  const elapsed = useMemo(() => {
    if (sessions.length === 0) return null;
    const start = Math.min(...sessions.map((s) => Date.parse(s.startedAt)));
    const open = sessions.some((s) => !s.endedAt);
    const end = running || open ? now : Math.max(...sessions.map((s) => (s.endedAt ? Date.parse(s.endedAt) : 0)));
    return formatDuration(end - start);
  }, [sessions, running, now]);

  const act = async (fn: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await fn();
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="shrink-0 border-b border-line bg-panel px-4 py-2.5">
      <div className="flex items-center gap-2">
        <button className="btn btn-ghost btn-sm !px-1" onClick={() => navigate({ name: 'missions' })} aria-label="Back to missions">
          <ArrowLeft className="size-4" aria-hidden />
        </button>
        <h1 className="m-0 min-w-0 flex-1 truncate text-[15px] font-semibold" title={mission.objective}>{mission.title || mission.id}</h1>
        <StateBadge state={mission.state} detached={detached} />
      </div>
      <div className="mt-1 flex flex-wrap items-center gap-x-4 gap-y-0.5 pl-8 text-[12px] text-muted">
        <span>{projectName || 'unknown project'}</span>
        <span>branch <span className="mono text-fg">{mission.branch ?? '—'}</span></span>
        <span title={mission.baseCommit ?? undefined}>base <span className="mono text-fg">{shortSha(mission.baseCommit)}</span>{mission.baseRef ? ` (${mission.baseRef})` : ''}</span>
        <span>{providerLabel(mission.provider)}{mission.model ? <> · <span className="mono text-fg">{mission.model}</span></> : ' · default model'}</span>
        <span>{mission.permissionProfile === 'accept_edits' ? 'auto-approve edits' : 'ask before edits'}</span>
        <span>elapsed {elapsed ?? '—'}{running ? ' (live)' : ''}</span>
      </div>
      <div className="mt-2 flex flex-wrap items-center gap-2 pl-8">
        {mission.state === 'ready' && (
          <button className="btn btn-primary" disabled={busy} onClick={() => void act(() => startMission(mission.id))}>
            <Play className="size-3.5" aria-hidden /> Start
          </button>
        )}
        {canResume && (
          <button className="btn btn-primary" disabled={busy || running} onClick={() => void act(() => startMission(mission.id))}>
            <RotateCcw className="size-3.5" aria-hidden /> Resume
          </button>
        )}
        <button className="btn" disabled={!running || busy} title={running ? undefined : 'No agent process is attached'} onClick={() => void act(() => stopMission(mission.id))}>
          <Square className="size-3.5" aria-hidden /> Stop
        </button>
        <button className="btn" disabled={!mission.worktreePath} title={mission.worktreePath ?? 'No worktree yet'} onClick={() => void openInEditor(mission.id)}>
          <ExternalLink className="size-3.5" aria-hidden /> Open worktree in editor
        </button>
        <div className="flex-1" />
        <button className="btn btn-danger" disabled={!canDiscard} title={canDiscard ? undefined : running ? 'Stop the agent before discarding' : 'Already closed'} onClick={() => beginDiscard(mission.id)}>
          <Trash2 className="size-3.5" aria-hidden /> Discard
        </button>
      </div>
    </div>
  );
}

function FollowUp({ missionId }: { missionId: string }) {
  const [text, setText] = useState('');
  const [busy, setBusy] = useState(false);
  const send = async () => {
    if (!text.trim() || busy) return;
    setBusy(true);
    const m = await followupMission(missionId, text.trim());
    setBusy(false);
    if (m) setText('');
  };
  return (
    <div className="shrink-0 border-t border-line bg-panel p-3">
      <label className="label" htmlFor="followup">Follow-up instruction (starts a new turn in the same worktree)</label>
      <div className="flex items-end gap-2">
        <textarea
          id="followup"
          className="input min-h-[56px] resize-y"
          value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') {
              e.preventDefault();
              void send();
            }
          }}
          placeholder="What should the agent do next?"
        />
        <button className="btn btn-primary" disabled={!text.trim() || busy} onClick={() => void send()}>
          <Send className="size-3.5" aria-hidden /> Send <Kbd>Ctrl+Enter</Kbd>
        </button>
      </div>
    </div>
  );
}
