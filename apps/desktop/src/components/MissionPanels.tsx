import { useCallback, useEffect, useRef, useState } from 'react';
import { RefreshCw } from 'lucide-react';
import { api } from '../lib/api';
import { errMsg, fmtCost, fmtNum, formatDuration, localTime, STATE_META } from '../lib/format';
import type { AgentSession, Approval, ChangedFile, Mission, MissionChanges, MissionDiff, Transition } from '../lib/types';
import { ApprovalCard } from './ApprovalCard';
import { DiffViewer } from './DiffViewer';
import { Badge, Callout, EmptyState } from './ui';

export function ApprovalsPanel({ approvals }: { approvals: Approval[] }) {
  const pending = approvals.filter((a) => a.state === 'pending');
  const history = approvals.filter((a) => a.state !== 'pending').sort((a, b) => (b.decidedAt ?? b.createdAt).localeCompare(a.decidedAt ?? a.createdAt));
  return (
    <div className="min-h-0 flex-1 space-y-4 overflow-y-auto p-3">
      <section aria-label="Pending approvals">
        <h3 className="m-0 mb-2 text-[11px] font-medium uppercase tracking-wide text-muted">Pending ({pending.length})</h3>
        {pending.length === 0 ? (
          <div className="rounded-md border border-dashed border-line-strong px-3 py-4 text-center text-muted">Nothing is waiting for your decision.</div>
        ) : (
          <div className="space-y-2.5">
            {pending.map((a) => (
              <ApprovalCard key={a.id} approval={a} />
            ))}
          </div>
        )}
      </section>
      <section aria-label="Approval history">
        <h3 className="m-0 mb-2 text-[11px] font-medium uppercase tracking-wide text-muted">History ({history.length})</h3>
        {history.length === 0 ? (
          <div className="text-muted">No decisions yet.</div>
        ) : (
          <div className="space-y-2">
            {history.map((a) => (
              <ApprovalCard key={a.id} approval={a} compact />
            ))}
          </div>
        )}
      </section>
    </div>
  );
}

interface ChangesProps {
  mission: Mission;
  /** Bumps whenever a file.changed event arrives. */
  fileEventCount: number;
}

export function ChangesPanel({ mission, fileEventCount }: ChangesProps) {
  const [changes, setChanges] = useState<MissionChanges | null>(null);
  const [diff, setDiff] = useState<MissionDiff | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [focus, setFocus] = useState<{ path: string; nonce: number } | null>(null);
  const seq = useRef(0);
  const hasTree = !!mission.worktreePath || !!mission.branch;

  const load = useCallback(async () => {
    if (!hasTree) return;
    const mine = ++seq.current;
    setLoading(true);
    try {
      const [c, d] = await Promise.all([api.missionChanges(mission.id), api.missionDiff(mission.id)]);
      if (mine !== seq.current) return;
      setChanges(c);
      setDiff(d);
      setError(null);
    } catch (e) {
      if (mine === seq.current) setError(errMsg(e));
    } finally {
      if (mine === seq.current) setLoading(false);
    }
  }, [mission.id, hasTree]);

  // Debounced refresh on file events, and when the mission changes state (e.g. reaches review).
  useEffect(() => {
    const t = window.setTimeout(() => void load(), changes ? 700 : 0);
    return () => window.clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [load, fileEventCount, mission.state]);

  if (!hasTree) {
    return <EmptyState title="No worktree yet">Changes appear once the mission has been started and a worktree exists.</EmptyState>;
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex items-center gap-2 border-b border-line px-3 py-1.5">
        {changes && (
          <div className="flex flex-wrap items-center gap-x-3 text-[12px] text-muted">
            <span>{changes.files.length} file{changes.files.length === 1 ? '' : 's'}</span>
            <span title="Files with uncommitted changes in the worktree">{changes.safety.dirtyFiles} dirty</span>
            <span title="Commits on the mission branch not yet merged">{changes.safety.unmergedCommits} unmerged commit{changes.safety.unmergedCommits === 1 ? '' : 's'}</span>
            {!changes.safety.exists && <Badge tone="warn">worktree missing</Badge>}
          </div>
        )}
        <button className="btn btn-sm btn-ghost ml-auto" onClick={() => void load()} disabled={loading} aria-label="Refresh changes">
          <RefreshCw className={`size-3.5 ${loading ? 'animate-spin' : ''}`} aria-hidden /> Refresh
        </button>
      </div>
      {error && (
        <div className="p-2">
          <Callout tone="bad" title="Could not load changes">
            <pre className="whitespace-pre-wrap break-words text-[12px]">{error}</pre>
          </Callout>
        </div>
      )}
      <div className="min-h-0 flex-1 overflow-auto">
        {changes && changes.files.length > 0 && <FileList files={changes.files} onPick={(path) => setFocus({ path, nonce: Date.now() })} />}
        {changes && changes.files.length === 0 && !error && <div className="px-3 py-6 text-center text-muted">No changed files.</div>}
        {diff && <DiffViewer diff={diff.diff} truncated={diff.truncated} focus={focus} />}
      </div>
    </div>
  );
}

function FileList({ files, onPick }: { files: ChangedFile[]; onPick: (path: string) => void }) {
  return (
    <ul className="m-0 max-h-[34%] list-none overflow-y-auto border-b border-line p-0" aria-label="Changed files">
      {files.map((f) => (
        <li key={f.path}>
          <button className="flex w-full items-center gap-2 px-3 py-1 text-left hover:bg-hover" onClick={() => onPick(f.path)}>
            <span className="mono min-w-0 flex-1 truncate text-[12px]" title={f.path}>{f.path}</span>
            {f.binary ? (
              <span className="text-[11px] text-warn">binary</span>
            ) : (
              <span className="mono shrink-0 text-[11.5px]">
                <span className="text-ok">+{f.added ?? '?'}</span> <span className="text-bad">−{f.removed ?? '?'}</span>
              </span>
            )}
          </button>
        </li>
      ))}
    </ul>
  );
}

export function TimelinePanel({ transitions, sessions }: { transitions: Transition[]; sessions: AgentSession[] }) {
  return (
    <div className="min-h-0 flex-1 space-y-5 overflow-y-auto p-3">
      <section>
        <h3 className="m-0 mb-2 text-[11px] font-medium uppercase tracking-wide text-muted">State history</h3>
        {transitions.length === 0 && <div className="text-muted">No transitions recorded.</div>}
        <ol className="m-0 list-none space-y-0 p-0">
          {transitions.map((t, i) => (
            <li key={i} className="relative border-l border-line-strong pb-3 pl-4">
              <span className="absolute -left-[4px] top-1.5 size-[7px] rounded-full bg-accent" aria-hidden />
              <div className="flex flex-wrap items-center gap-1.5">
                <Badge tone={STATE_META[t.fromState]?.tone ?? 'idle'}>{STATE_META[t.fromState]?.label ?? t.fromState}</Badge>
                <span className="text-faint" aria-label="to">→</span>
                <Badge tone={STATE_META[t.toState]?.tone ?? 'idle'}>{STATE_META[t.toState]?.label ?? t.toState}</Badge>
                <span className="text-[11px] text-muted">{localTime(t.at)}</span>
              </div>
              {t.reason && <div className="mt-0.5 whitespace-pre-wrap break-words text-[12px] text-muted">{t.reason}</div>}
            </li>
          ))}
        </ol>
      </section>
      <section>
        <h3 className="m-0 mb-2 text-[11px] font-medium uppercase tracking-wide text-muted">Agent sessions</h3>
        {sessions.length === 0 && <div className="text-muted">No sessions yet.</div>}
        <div className="space-y-2">
          {sessions.map((s) => (
            <div key={s.id} className="rounded-md border border-line bg-raised p-2.5 text-[12px]">
              <div className="flex flex-wrap items-center gap-2">
                <Badge tone={s.state === 'live' || s.state === 'starting' ? 'run' : s.state === 'lost' ? 'warn' : 'idle'}>{s.state}</Badge>
                <span className="mono text-muted">{s.id.slice(0, 8)}</span>
                <span className="text-muted">{s.provider}</span>
                {s.pid !== null && <span className="mono text-muted">pid {s.pid}</span>}
              </div>
              <dl className="m-0 mt-1.5 grid grid-cols-[84px_1fr] gap-x-2 gap-y-0.5">
                <dt className="text-faint">Started</dt>
                <dd className="m-0">{localTime(s.startedAt)}</dd>
                <dt className="text-faint">Ended</dt>
                <dd className="m-0">{s.endedAt ? localTime(s.endedAt) : '—'}</dd>
                {s.startedAt && (
                  <>
                    <dt className="text-faint">Duration</dt>
                    <dd className="m-0">{formatDuration((s.endedAt ? Date.parse(s.endedAt) : Date.now()) - Date.parse(s.startedAt))}{!s.endedAt && s.state === 'live' ? ' (so far)' : ''}</dd>
                  </>
                )}
                {s.endReason && (
                  <>
                    <dt className="text-faint">End reason</dt>
                    <dd className="m-0 break-words">{s.endReason}</dd>
                  </>
                )}
                {s.usage && (
                  <>
                    <dt className="text-faint">Usage (est.)</dt>
                    <dd className="mono m-0">
                      {fmtNum(s.usage.inputTokens)} in / {fmtNum(s.usage.outputTokens)} out
                      {s.usage.cacheReadTokens ? ` / ${fmtNum(s.usage.cacheReadTokens)} cache-read` : ''}
                      {s.usage.costUsdEstimate !== undefined ? ` · ${fmtCost(s.usage.costUsdEstimate)} est.` : ''}
                    </dd>
                  </>
                )}
              </dl>
            </div>
          ))}
        </div>
      </section>
    </div>
  );
}

