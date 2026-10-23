import { Fragment, useCallback, useEffect, useRef, useState } from 'react';
import { AlertTriangle, Play, RefreshCw, Square } from 'lucide-react';
import { api } from '../lib/api';
import { cancelCheck, runCheck, setVerdict } from '../lib/actions';
import { errMsg, formatDuration } from '../lib/format';
import { useStore } from '../lib/store';
import type { CheckReport, CriterionReport, Mission, VerificationReport } from '../lib/types';
import { statusDetail, statusLabel, statusTone } from '../lib/verification';
import { EvidenceGraph } from './EvidenceGraph';
import { Badge, Callout } from './ui';

/** Loads the derived verification report and refreshes it on pushes, state changes and file changes. */
export function useReport(id: string, updatedAt: string | undefined, state: string | undefined, fileEventCount: number) {
  const tick = useStore((s) => s.verifyTick[id] ?? 0);
  const [report, setReport] = useState<VerificationReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const seq = useRef(0);

  const reload = useCallback(async () => {
    const mine = ++seq.current;
    setLoading(true);
    try {
      const r = await api.verificationReport(id);
      if (mine === seq.current) {
        setReport(r);
        setError(null);
      }
    } catch (e) {
      if (mine === seq.current) setError(errMsg(e));
    } finally {
      if (mine === seq.current) setLoading(false);
    }
  }, [id]);

  useEffect(() => {
    const t = window.setTimeout(() => void reload(), 300);
    return () => window.clearTimeout(t);
  }, [reload, tick, updatedAt, state, fileEventCount]);

  return { report, error, loading, reload };
}

const RUN_TONE = { passed: 'ok', running: 'run', failed: 'bad', error: 'bad', timeout: 'bad', cancelled: 'idle' } as const;

function NotEstablished({ items, blockers }: { items: string[]; blockers: string[] }) {
  if (items.length === 0 && blockers.length === 0) {
    return <div className="rounded-md border border-ok/30 bg-ok-dim/50 px-3 py-2 text-ok">Everything in the contract is established for the current changes.</div>;
  }
  return (
    <div className="space-y-2">
      {blockers.length > 0 && (
        <Callout tone="bad" title="Hard blockers (cannot be overridden)">
          <ul className="m-0 list-disc pl-4">{blockers.map((b, i) => <li key={i}>{b}</li>)}</ul>
        </Callout>
      )}
      {items.length > 0 && (
        <Callout tone="warn" title="Not established">
          <ul className="m-0 list-disc pl-4">{items.map((b, i) => <li key={i}>{b}</li>)}</ul>
        </Callout>
      )}
    </div>
  );
}

function CriterionRow({ k, report, missionId, canJudge }: { k: CriterionReport; report: VerificationReport; missionId: string; canJudge: boolean }) {
  const [note, setNote] = useState('');
  const [busy, setBusy] = useState(false);
  const names = (id: string) => report.checks.find((c) => c.spec.id === id)?.spec.name ?? id;
  const detail = statusDetail(k.status);
  const judge = async (pass: boolean) => {
    setBusy(true);
    await setVerdict(missionId, k.id, pass, note);
    setBusy(false);
    setNote('');
  };
  return (
    <li className="border-b border-line px-3 py-2">
      <div className="flex items-start gap-2">
        <div className="min-w-0 flex-1">
          <div className="whitespace-pre-wrap break-words">{k.text}</div>
          {detail && <div className="mt-0.5 whitespace-pre-wrap break-words text-[12px] text-muted">{detail}</div>}
          {k.manualVerdict && (
            <div className="mt-0.5 text-[11.5px] text-faint">
              Verdict by {k.manualVerdict.decidedBy}: {k.manualVerdict.pass ? 'pass' : 'fail'}
              {k.manualVerdict.note ? ` — ${k.manualVerdict.note}` : ''}
            </div>
          )}
        </div>
        <Badge tone={statusTone(k.status)}>{statusLabel(k.status, names)}</Badge>
      </div>
      {k.manual && (
        <div className="mt-2 flex items-center gap-2">
          <input className="input !h-7" placeholder="Optional note" aria-label="Verdict note" value={note} onChange={(e) => setNote(e.target.value)} />
          <button className="btn btn-sm" disabled={busy || !canJudge} onClick={() => void judge(true)}>Pass</button>
          <button className="btn btn-sm btn-danger" disabled={busy || !canJudge} onClick={() => void judge(false)}>Fail</button>
        </div>
      )}
    </li>
  );
}

function CheckRow({ c, missionId, anyRunning, canRun }: { c: CheckReport; missionId: string; anyRunning: boolean; canRun: boolean }) {
  const [open, setOpen] = useState(false);
  const r = c.latest;
  const stale = !!r && !c.fresh && r.state !== 'running';
  const running = r?.state === 'running';
  return (
    <Fragment>
      <tr className={stale ? 'opacity-60' : ''}>
        <td>
          <div className="font-medium">{c.spec.name}{c.spec.required && <span className="ml-1.5 text-[10.5px] text-faint">required</span>}</div>
          <div className="mono max-w-[260px] truncate text-[11.5px] text-muted" title={c.spec.command}>{c.spec.command}</div>
        </td>
        <td>
          {r ? (
            <div className="space-y-0.5">
              <div className="flex items-center gap-1.5">
                <Badge tone={RUN_TONE[r.state]}>{r.state}</Badge>
                {r.exitCode !== null && <span className="mono text-[11.5px] text-muted">exit {r.exitCode}</span>}
                {r.durationMs !== null && <span className="text-[11.5px] text-muted">{formatDuration(r.durationMs)}</span>}
              </div>
              {stale && <div className="text-[11.5px] text-warn">ran against earlier changes — re-run</div>}
              {r.mutated && (
                <div className="flex items-center gap-1 text-[11.5px] text-warn">
                  <AlertTriangle className="size-3" aria-hidden /> this check modified files in the worktree
                </div>
              )}
            </div>
          ) : (
            <span className="text-muted">never run</span>
          )}
        </td>
        <td>
          <div className="flex justify-end gap-1.5">
            {r && <button className="btn btn-sm btn-ghost" onClick={() => setOpen(!open)} aria-expanded={open}>{open ? 'Hide output' : 'Output'}</button>}
            {running ? (
              <button className="btn btn-sm" onClick={() => void cancelCheck(missionId)}><Square className="size-3" aria-hidden /> Cancel</button>
            ) : (
              <button
                className="btn btn-sm btn-primary"
                disabled={anyRunning || !canRun}
                title="Runs this command in the mission worktree on your machine"
                onClick={() => {
                  if (window.confirm(`Run "${c.spec.command}"?\n\nRuns this command in the mission worktree on your machine, with your user privileges.`)) void runCheck(missionId, c.spec.id);
                }}
              >
                <Play className="size-3" aria-hidden /> {r ? 'Re-run' : 'Run'}
              </button>
            )}
          </div>
        </td>
      </tr>
      {open && r && (
        <tr>
          <td colSpan={3} className="!p-0">
            <pre className="max-h-64 overflow-auto whitespace-pre-wrap break-all border-b border-line bg-bg p-2 text-[12px]">{r.outputTail || '(no output)'}</pre>
            <div className="bg-bg px-2 pb-1 text-[11px] text-faint">
              {r.outputTruncated ? 'Output truncated: showing the tail only.' : 'Complete output.'}
              {r.logPath && <> Full log: <span className="mono">{r.logPath}</span></>}
            </div>
          </td>
        </tr>
      )}
    </Fragment>
  );
}

interface Props {
  mission: Mission;
  report: VerificationReport | null;
  error: string | null;
  loading: boolean;
  reload: () => void;
}

export function VerificationPanel({ mission, report, error, loading, reload }: Props) {
  const finished = ['merged', 'rejected', 'cancelled'].includes(mission.state);
  const hasWorktree = !!mission.worktreePath;
  return (
    <div className="min-h-0 flex-1 space-y-4 overflow-y-auto p-3">
      <div className="flex items-center gap-2">
        <span className="text-[12px] text-muted">
          {report ? <>Evidence is tied to changes <span className="mono">{report.fingerprint.slice(0, 10)}</span></> : 'Loading verification report…'}
        </span>
        <button className="btn btn-sm btn-ghost ml-auto" onClick={reload} disabled={loading}>
          <RefreshCw className={`size-3.5 ${loading ? 'animate-spin' : ''}`} aria-hidden /> Refresh
        </button>
      </div>
      {error && (
        <Callout tone="bad" title="Could not load the verification report">
          <pre className="whitespace-pre-wrap break-words text-[12px]">{error}</pre>
        </Callout>
      )}
      {report && (
        <>
          <NotEstablished items={report.unresolved} blockers={report.hardBlockers} />

          <section aria-label="Acceptance criteria">
            <h3 className="m-0 mb-1 text-[11px] font-medium uppercase tracking-wide text-muted">Acceptance criteria ({report.criteria.length})</h3>
            {report.criteria.length === 0 ? (
              <div className="text-muted">No acceptance criteria in the contract. Add some in the Contract tab.</div>
            ) : (
              <ul className="m-0 list-none rounded-md border border-line p-0">
                {report.criteria.map((k) => <CriterionRow key={k.id} k={k} report={report} missionId={mission.id} canJudge={!finished} />)}
              </ul>
            )}
          </section>

          <section aria-label="Checks">
            <h3 className="m-0 mb-1 text-[11px] font-medium uppercase tracking-wide text-muted">Checks ({report.checks.length})</h3>
            {report.checks.length === 0 ? (
              <div className="text-muted">No checks in the contract. Checks only run when you press Run.</div>
            ) : (
              <div className="rounded-md border border-line">
                <table className="tbl">
                  <thead><tr><th>Check</th><th>Latest result</th><th /></tr></thead>
                  <tbody>
                    {report.checks.map((c) => (
                      <CheckRow key={c.spec.id} c={c} missionId={mission.id} anyRunning={report.checks.some((x) => x.latest?.state === 'running')} canRun={hasWorktree && !finished} />
                    ))}
                  </tbody>
                </table>
              </div>
            )}
            {!hasWorktree && <div className="mt-1 text-muted">Checks need the mission worktree, which does not exist yet.</div>}
          </section>

          <section aria-label="Evidence graph">
            <h3 className="m-0 mb-1 text-[11px] font-medium uppercase tracking-wide text-muted">Evidence graph</h3>
            {report.criteria.length === 0 && report.checks.length === 0 ? (
              <div className="text-muted">Nothing to graph: the contract has no criteria or checks.</div>
            ) : (
              <EvidenceGraph report={report} />
            )}
          </section>

          {report.scopeConfigured && (
            <section aria-label="Scope">
              <h3 className="m-0 mb-1 text-[11px] font-medium uppercase tracking-wide text-muted">Scope</h3>
              {report.scope.outsideInScope.length === 0 && report.scope.inOutOfScope.length === 0 ? (
                <div className="text-ok">All {report.files.length} changed file{report.files.length === 1 ? '' : 's'} are within scope.</div>
              ) : (
                <div className="space-y-2">
                  {report.scope.inOutOfScope.length > 0 && (
                    <FileGroup title="Changed files in out-of-scope paths" tone="bad" files={report.scope.inOutOfScope} />
                  )}
                  {report.scope.outsideInScope.length > 0 && (
                    <FileGroup title="Changed files outside the in-scope paths" tone="warn" files={report.scope.outsideInScope} />
                  )}
                </div>
              )}
            </section>
          )}
        </>
      )}
    </div>
  );
}

function FileGroup({ title, tone, files }: { title: string; tone: 'bad' | 'warn'; files: string[] }) {
  return (
    <div className={`rounded-md border px-3 py-2 ${tone === 'bad' ? 'border-bad/40 bg-bad-dim/50' : 'border-warn/40 bg-warn-dim/50'}`}>
      <div className="mb-1 font-medium">{title} ({files.length})</div>
      <ul className="m-0 list-none p-0">{files.map((f) => <li key={f} className="mono text-[12px]">{f}</li>)}</ul>
    </div>
  );
}
