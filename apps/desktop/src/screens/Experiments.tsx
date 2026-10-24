import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { FlaskConical, Play, Plus, RefreshCw, Square, Trash2, Trophy } from 'lucide-react';
import { api } from '../lib/api';
import {
  blocking, chartDomain, draftProblems, draftToSpec, EXPERIMENT_STATE_TONE, formatChange, formatValue, metricLabel, progress, toPercent,
  VERDICT_META, type ExperimentDraft, type VariantDraft,
} from '../lib/experiments';
import { errMsg, relTime } from '../lib/format';
import { useStore } from '../lib/store';
import type { Experiment, ExperimentDetail, ExperimentReport, Mission } from '../lib/types';
import { Modal } from '../components/Modal';
import { SplitLayout } from '../components/Splitter';
import { Badge, Callout, EmptyState, Mono, PageHeader, useNow } from '../components/ui';

const DONE: Mission['state'][] = ['merged', 'rejected', 'cancelled'];

export function ExperimentsScreen() {
  const projectId = useStore((s) => s.currentProjectId);
  const project = useStore((s) => s.projects.find((p) => p.id === s.currentProjectId));
  const notify = useStore((s) => s.notify);
  const [list, setList] = useState<Experiment[] | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!projectId) return;
    try {
      const l = await api.experimentList(projectId);
      setList(l);
      setError(null);
      setSelected((cur) => (cur && l.some((e) => e.id === cur) ? cur : l[0]?.id ?? null));
    } catch (e) {
      setError(errMsg(e));
    }
  }, [projectId]);

  useEffect(() => { setList(null); setSelected(null); void load(); }, [load]);

  if (!projectId || !project) {
    return <div className="flex h-full flex-col"><PageHeader title="Experiments" /><EmptyState title="Choose a project">Experiments measure revisions of a project. Add or select one first.</EmptyState></div>;
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <PageHeader
        title="Experiments"
        sub={`${project.name}: measure a baseline against branches, commits or missions with your own benchmark, in isolated worktrees.`}
        actions={
          <>
            <button className="btn" onClick={() => void load()}><RefreshCw className="size-3.5" aria-hidden /> Refresh</button>
            <button className="btn btn-primary" onClick={() => setCreating(true)}><Plus className="size-3.5" aria-hidden /> New experiment</button>
          </>
        }
      />
      {error && <div className="p-4"><Callout tone="bad" title="Could not load experiments">{error}</Callout></div>}
      {list && list.length === 0 && (
        <EmptyState title="No experiments yet" actions={<button className="btn btn-primary" onClick={() => setCreating(true)}><FlaskConical className="size-3.5" aria-hidden /> New experiment</button>}>
          Compare the code on your default branch with one or more branches, commits or missions: FORGE runs your benchmark the same number of times on each, in separate worktrees, and only names a winner when every run agrees.
        </EmptyState>
      )}
      {list && list.length > 0 && (
        <div className="min-h-0 flex-1">
          <SplitLayout storageKey="experiments.list" fixed="start" defaultSize={340} min={260} max={560}>
              <ul className="m-0 h-full list-none overflow-y-auto p-0" aria-label="Experiments">
                {list.map((e) => (
                  <li key={e.id}>
                    <button
                      className={`flex w-full flex-col gap-1 border-b border-line px-4 py-2.5 text-left ${selected === e.id ? 'bg-accent-dim' : 'hover:bg-hover'}`}
                      aria-current={selected === e.id ? 'true' : undefined}
                      onClick={() => setSelected(e.id)}
                    >
                      <span className="flex items-center gap-2">
                        <span className="min-w-0 flex-1 truncate font-medium text-fg">{e.name}</span>
                        <Badge tone={EXPERIMENT_STATE_TONE[e.state]}>{e.state}</Badge>
                      </span>
                      <span className="flex items-center gap-2 text-[12px] text-muted">
                        <Mono className="truncate">{e.command}</Mono>
                      </span>
                      {e.report && <span className="text-[12px] text-muted">{e.report.conclusion === 'winner' ? 'Winner found' : e.report.conclusion === 'no_winner' ? 'No winner' : 'Inconclusive'} · attempt {e.attempt}</span>}
                    </button>
                  </li>
                ))}
              </ul>
            <div className="min-h-0 min-w-0 flex-1">{selected ? <ExperimentView key={selected} id={selected} onChanged={() => void load()} /> : <EmptyState title="Select an experiment" />}</div>
          </SplitLayout>
        </div>
      )}
      <NewExperimentDialog
        open={creating}
        projectId={projectId}
        defaultBranch={project.defaultBranch}
        onClose={() => setCreating(false)}
        onCreated={(e) => { setCreating(false); notify('info', `Created "${e.name}". Nothing has run yet.`); void load().then(() => setSelected(e.id)); }}
      />
    </div>
  );
}

// ---- detail ------------------------------------------------------------------------------------------------------------

function ExperimentView({ id, onChanged }: { id: string; onChanged: () => void }) {
  const notify = useStore((s) => s.notify);
  const now = useNow(5000);
  const [d, setD] = useState<ExperimentDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [confirm, setConfirm] = useState(false);
  const [busy, setBusy] = useState(false);
  const poll = useRef<number | null>(null);

  const fetchDetail = useCallback(async () => {
    try {
      setD(await api.experimentGet(id));
      setError(null);
    } catch (e) {
      setError(errMsg(e));
    }
  }, [id]);

  const stopPolling = () => { if (poll.current !== null) { window.clearInterval(poll.current); poll.current = null; } };
  const startPolling = () => { stopPolling(); poll.current = window.setInterval(() => void fetchDetail(), 1000); };

  useEffect(() => { void fetchDetail(); return stopPolling; }, [fetchDetail]);
  // An attempt started elsewhere (or before this view opened) is followed live too.
  useEffect(() => {
    if (d?.experiment.state === 'running' && !busy && poll.current === null) startPolling();
    if (d && d.experiment.state !== 'running' && !busy) stopPolling();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [d?.experiment.state, busy]);

  const run = async () => {
    setConfirm(false);
    setBusy(true);
    startPolling();
    try {
      const done = await api.experimentRun(id);
      setD(done);
      const r = done.experiment.report;
      notify(done.experiment.state === 'completed' ? 'info' : 'warn', `"${done.experiment.name}" ${done.experiment.state}${r ? `: ${r.explanation}` : done.experiment.lastError ? `: ${done.experiment.lastError}` : ''}`);
    } catch (e) {
      notify('error', errMsg(e));
      await fetchDetail();
    } finally {
      stopPolling();
      setBusy(false);
      onChanged();
    }
  };

  const cancel = async () => {
    try { await api.experimentCancel(id); notify('info', 'Cancelling after the current run…'); } catch (e) { notify('error', errMsg(e)); }
  };

  if (error) return <div className="p-4"><Callout tone="bad" title="Could not load the experiment">{error}</Callout></div>;
  if (!d) return <div className="p-6 text-muted">Loading…</div>;
  const e = d.experiment;
  const running = e.state === 'running' || busy;
  const p = progress(d);
  const label = (vid: string) => d.variants.find((v) => v.id === vid)?.label ?? vid;

  return (
    <div className="flex h-full min-h-0 flex-col overflow-y-auto">
      <div className="flex items-start justify-between gap-4 border-b border-line px-5 py-3">
        <div className="min-w-0 space-y-1">
          <div className="flex items-center gap-2">
            <h2 className="m-0 truncate text-[15px] font-semibold">{e.name}</h2>
            <Badge tone={EXPERIMENT_STATE_TONE[e.state]}>{e.state}</Badge>
          </div>
          {e.objective && <div className="text-muted">{e.objective}</div>}
          <div className="flex flex-wrap gap-x-4 gap-y-1 text-[12px] text-muted">
            <span>Metric: {metricLabel(e.metric)}</span>
            <span>{e.repetitions} measured run{e.repetitions === 1 ? '' : 's'} + {e.warmup} warm-up per revision</span>
            <span>Timeout {e.timeoutSecs} s</span>
            {e.attempt > 0 && <span>Attempt {e.attempt}{e.finishedAt ? `, finished ${relTime(e.finishedAt, now)}` : ''}</span>}
          </div>
        </div>
        <div className="flex shrink-0 gap-2">
          {running ? (
            <button className="btn btn-danger" onClick={() => void cancel()}><Square className="size-3.5" aria-hidden /> Cancel</button>
          ) : (
            <button className="btn btn-primary" onClick={() => setConfirm(true)}><Play className="size-3.5" aria-hidden /> {e.attempt > 0 ? 'Run again' : 'Run'}</button>
          )}
        </div>
      </div>

      <div className="space-y-4 p-5">
        <div className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-[12px]">
          <span className="text-muted">Benchmark</span><Mono>{e.command}</Mono>
          <span className="text-muted">Correctness</span>{e.correctnessCommand ? <Mono>{e.correctnessCommand}</Mono> : <span className="text-warn">none: a faster result is not checked for correctness</span>}
        </div>

        {running && (
          <div aria-live="polite" className="space-y-1">
            <div className="flex justify-between text-[12px] text-muted"><span>Running… {p.done} of {p.total} runs</span><span>Runs are interleaved across revisions</span></div>
            <div className="h-1.5 overflow-hidden rounded-full bg-raised"><div className="h-full bg-run transition-all" style={{ width: `${p.total ? (p.done / p.total) * 100 : 0}%` }} /></div>
          </div>
        )}

        {e.lastError && !running && <Callout tone={e.state === 'failed' ? 'bad' : 'warn'} title={e.state === 'failed' ? 'The attempt failed' : 'Note'}>{e.lastError}</Callout>}

        {e.report && !running && <Conclusion report={e.report} label={label} />}

        <RevisionsTable d={d} />

        {e.report && <RangeChart report={e.report} />}

        <RunsTable d={d} label={label} />

        {e.env && (
          <details className="text-[12px] text-muted">
            <summary className="cursor-pointer">Environment of the latest attempt</summary>
            <pre className="mono mt-1 whitespace-pre-wrap">{JSON.stringify(e.env, null, 2)}</pre>
          </details>
        )}
      </div>

      <Modal
        open={confirm}
        onClose={() => setConfirm(false)}
        title="Run this experiment?"
        footer={<><button className="btn" onClick={() => setConfirm(false)}>Cancel</button><button className="btn btn-primary" onClick={() => void run()}><Play className="size-3.5" aria-hidden /> Run {p.total} commands</button></>}
      >
        <p className="m-0">FORGE will check out {d.variants.length} revisions into separate worktrees and run, with your privileges:</p>
        <ul className="m-0 space-y-1 pl-5">
          {e.correctnessCommand && <li><Mono>{e.correctnessCommand}</Mono> once per revision</li>}
          <li><Mono>{e.command}</Mono> {e.warmup + e.repetitions} times per revision</li>
        </ul>
        <Callout tone="warn">These commands execute the code in each revision (including agent-written code) on this machine. They are not sandboxed. Your own checkout is not touched.</Callout>
      </Modal>
    </div>
  );
}

function Conclusion({ report, label }: { report: ExperimentReport; label: (id: string) => string }) {
  if (report.conclusion === 'winner' && report.winner) {
    return (
      <div className="flex items-start gap-3 rounded-md border border-ok/40 bg-ok-dim px-3 py-2" role="status">
        <Trophy className="mt-0.5 size-4 shrink-0 text-ok" aria-hidden />
        <div><div className="font-medium text-ok">Winner: {label(report.winner)}</div><div>{report.explanation}</div></div>
      </div>
    );
  }
  return <Callout tone={report.conclusion === 'no_winner' ? 'info' : 'warn'} title={report.conclusion === 'no_winner' ? 'No winner' : 'Inconclusive'}>{report.explanation}</Callout>;
}

function RevisionsTable({ d }: { d: ExperimentDetail }) {
  const r = d.experiment.report;
  const m = d.experiment.metric;
  return (
    <table className="w-full border-collapse text-left text-[13px]">
      <thead className="text-[11px] uppercase tracking-wide text-faint">
        <tr className="border-b border-line">
          <th className="py-1.5 pr-3 font-medium">Revision</th>
          <th className="py-1.5 pr-3 font-medium">Commit</th>
          <th className="py-1.5 pr-3 text-right font-medium">Median</th>
          <th className="py-1.5 pr-3 text-right font-medium">Range</th>
          <th className="py-1.5 pr-3 text-right font-medium">vs baseline</th>
          <th className="py-1.5 font-medium">Verdict</th>
        </tr>
      </thead>
      <tbody>
        {d.variants.map((v) => {
          const vr = r?.variants.find((x) => x.variantId === v.id);
          const meta = vr ? VERDICT_META[vr.verdict] : null;
          return (
            <tr key={v.id} className="border-b border-line align-top">
              <td className="py-1.5 pr-3">
                <div className="font-medium">{v.label}</div>
                <div className="text-[12px] text-muted">{v.gitRef.startsWith('mission:') ? 'mission snapshot' : v.gitRef}</div>
              </td>
              <td className="py-1.5 pr-3"><Mono className="text-muted">{v.commitSha.slice(0, 10)}</Mono></td>
              <td className="py-1.5 pr-3 text-right tabular-nums">{formatValue(m, vr?.stats?.median)}</td>
              <td className="py-1.5 pr-3 text-right tabular-nums text-muted">{vr?.stats ? `${formatValue(m, vr.stats.min)} – ${formatValue(m, vr.stats.max)} (n=${vr.stats.n})` : '—'}</td>
              <td className={`py-1.5 pr-3 text-right tabular-nums ${vr?.verdict === 'better' ? 'text-ok' : vr?.verdict === 'worse' ? 'text-bad' : ''}`}>{v.isBaseline ? '' : formatChange(vr?.changeVsBaseline)}</td>
              <td className="py-1.5">
                {meta ? <Badge tone={meta.tone}>{meta.label}</Badge> : <span className="text-faint">not run</span>}
                {vr && vr.reasons.length > 0 && <div className="mt-0.5 text-[12px] text-muted">{vr.reasons.join('; ')}</div>}
              </td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}

/** Every measured value of every revision on one axis: the bar spans min..max, the tick marks the median. Non-overlapping bars
 *  are exactly what "better" means here. */
function RangeChart({ report }: { report: ExperimentReport }) {
  const dom = chartDomain(report);
  if (!dom) return null;
  const m = report.metric;
  return (
    <figure className="m-0 space-y-1.5" aria-label="Range of measured values per revision">
      <figcaption className="text-[11px] uppercase tracking-wide text-faint">Measured range · {report.direction} is better</figcaption>
      {report.variants.map((v) => {
        const s = v.stats;
        const tone = v.verdict === 'better' ? 'bg-ok' : v.verdict === 'worse' ? 'bg-bad' : v.isBaseline ? 'bg-accent' : 'bg-muted';
        return (
          <div key={v.variantId} className="flex items-center gap-3">
            <span className="w-[140px] shrink-0 truncate text-[12px]" title={v.label}>{v.label}</span>
            <div className="relative h-4 flex-1 rounded-sm bg-raised">
              {s && (
                <>
                  <div className={`absolute top-1 h-2 rounded-sm opacity-70 ${tone}`} style={{ left: `${toPercent(s.min, dom)}%`, width: `${Math.max(0.8, toPercent(s.max, dom) - toPercent(s.min, dom))}%` }} />
                  <div className="absolute top-0 h-4 w-0.5 bg-fg" style={{ left: `${toPercent(s.median, dom)}%` }} title={`median ${formatValue(m, s.median)}`} />
                </>
              )}
            </div>
            <span className="w-[90px] shrink-0 text-right text-[12px] tabular-nums text-muted">{formatValue(m, s?.median)}</span>
          </div>
        );
      })}
      <div className="flex justify-between pl-[152px] pr-[102px] text-[11px] tabular-nums text-faint"><span>{formatValue(m, Number(dom.lo.toPrecision(3)))}</span><span>{formatValue(m, Number(dom.hi.toPrecision(3)))}</span></div>
    </figure>
  );
}

function RunsTable({ d, label }: { d: ExperimentDetail; label: (id: string) => string }) {
  const [open, setOpen] = useState<string | null>(null);
  if (d.results.length === 0) return null;
  const m = d.experiment.metric;
  return (
    <details open={d.experiment.state === 'running'}>
      <summary className="cursor-pointer text-[11px] uppercase tracking-wide text-faint">Every run of attempt {d.experiment.attempt} ({d.results.length})</summary>
      <table className="mt-1 w-full border-collapse text-left text-[12px]">
        <tbody>
          {d.results.map((r) => (
            <tr key={r.id} className="cursor-pointer border-b border-line hover:bg-hover" onClick={() => setOpen(open === r.id ? null : r.id)}>
              <td className="py-1 pr-2 text-muted">{r.phase}{r.phase === 'correctness' ? '' : ` #${r.iteration + 1}`}</td>
              <td className="py-1 pr-2">{label(r.variantId)}</td>
              <td className="py-1 pr-2"><Badge tone={r.state === 'passed' ? 'ok' : r.state === 'cancelled' ? 'warn' : 'bad'}>{r.state.replace('_', ' ')}</Badge>{r.worktreeChanged && <span className="ml-1 text-warn" title="This run changed files in its worktree">changed files</span>}</td>
              <td className="py-1 pr-2 text-right tabular-nums">{formatValue({ kind: 'duration' }, r.durationMs)}</td>
              <td className="py-1 text-right tabular-nums">{r.phase === 'correctness' ? '' : formatValue(m, r.metricValue)}</td>
            </tr>
          ))}
        </tbody>
      </table>
      {open && <pre className="mono mt-2 max-h-[240px] overflow-auto whitespace-pre-wrap rounded-md border border-line bg-panel p-2 text-[12px]">{d.results.find((r) => r.id === open)?.outputTail || '(no output)'}</pre>}
    </details>
  );
}

// ---- create ------------------------------------------------------------------------------------------------------------

const blankVariant = (): VariantDraft => ({ label: '', source: 'ref', gitRef: '', missionId: '' });

function NewExperimentDialog({ open, projectId, defaultBranch, onClose, onCreated }: { open: boolean; projectId: string; defaultBranch: string; onClose: () => void; onCreated: (e: Experiment) => void }) {
  const missions = useStore((s) => s.missions);
  const candidates = useMemo(
    () => Object.values(missions).filter((m) => m.projectId === projectId && m.worktreePath && !DONE.includes(m.state)).sort((a, b) => b.updatedAt.localeCompare(a.updatedAt)),
    [missions, projectId],
  );
  const initial = (): ExperimentDraft => ({
    name: '', objective: '', command: '', correctnessCommand: '', metricKind: 'duration', metricName: '', direction: 'lower',
    repetitions: 5, warmup: 1, timeoutSecs: 600, baselineRef: defaultBranch, variants: [blankVariant()],
  });
  const [d, setD] = useState<ExperimentDraft>(initial);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => { if (open) { setD(initial()); setError(null); setBusy(false); } }, [open]); // eslint-disable-line react-hooks/exhaustive-deps

  const problems = draftProblems(d);
  const blockers = blocking(problems);
  const set = <K extends keyof ExperimentDraft>(k: K, v: ExperimentDraft[K]) => setD((x) => ({ ...x, [k]: v }));
  const setVariant = (i: number, v: Partial<VariantDraft>) => setD((x) => ({ ...x, variants: x.variants.map((o, j) => (j === i ? { ...o, ...v } : o)) }));

  const save = async () => {
    setBusy(true);
    try {
      onCreated(await api.experimentCreate(projectId, draftToSpec(d)));
    } catch (e) {
      setError(errMsg(e));
      setBusy(false);
    }
  };

  const num = (v: string) => (v.trim() === '' ? NaN : Number(v));
  return (
    <Modal
      open={open}
      onClose={onClose}
      wide
      title="New experiment"
      footer={<><button className="btn" onClick={onClose}>Cancel</button><button className="btn btn-primary" disabled={busy || blockers.length > 0} onClick={() => void save()}>Create (nothing runs yet)</button></>}
    >
      <label className="block space-y-1"><span className="text-muted">Name</span><input className="input" value={d.name} onChange={(e) => set('name', e.target.value)} placeholder="Faster JSON parsing" autoFocus /></label>
      <label className="block space-y-1"><span className="text-muted">Objective (optional)</span><input className="input" value={d.objective} onChange={(e) => set('objective', e.target.value)} placeholder="Reduce p50 request latency without breaking the API" /></label>
      <label className="block space-y-1">
        <span className="text-muted">Benchmark command (runs in each revision's worktree)</span>
        <input className="input mono" value={d.command} onChange={(e) => set('command', e.target.value)} placeholder="npm run bench" />
      </label>
      <label className="block space-y-1">
        <span className="text-muted">Correctness command (must pass before a revision's numbers count; recommended)</span>
        <input className="input mono" value={d.correctnessCommand} onChange={(e) => set('correctnessCommand', e.target.value)} placeholder="npm test" />
      </label>
      <fieldset className="space-y-2 rounded-md border border-line p-3">
        <legend className="px-1 text-muted">Metric</legend>
        <label className="flex items-center gap-2"><input type="radio" checked={d.metricKind === 'duration'} onChange={() => set('metricKind', 'duration')} /> Wall-clock time of the command (lower is better)</label>
        <label className="flex items-center gap-2"><input type="radio" checked={d.metricKind === 'output'} onChange={() => set('metricKind', 'output')} /> A number the command prints</label>
        {d.metricKind === 'output' && (
          <div className="flex items-center gap-2 pl-6">
            <input className="input mono !w-[200px]" value={d.metricName} onChange={(e) => set('metricName', e.target.value)} placeholder="latency_ms" aria-label="Metric name" />
            <select className="input !w-[150px]" value={d.direction} onChange={(e) => set('direction', e.target.value as 'lower' | 'higher')} aria-label="Better direction"><option value="lower">lower is better</option><option value="higher">higher is better</option></select>
            <span className="text-[12px] text-muted">printed as <Mono>FORGE_METRIC {d.metricName || 'name'}=123.4</Mono></span>
          </div>
        )}
      </fieldset>
      <div className="grid grid-cols-3 gap-3">
        <label className="space-y-1"><span className="text-muted">Measured runs</span><input className="input" type="number" min={1} max={100} value={Number.isNaN(d.repetitions) ? '' : d.repetitions} onChange={(e) => set('repetitions', num(e.target.value))} /></label>
        <label className="space-y-1"><span className="text-muted">Warm-up runs</span><input className="input" type="number" min={0} max={10} value={Number.isNaN(d.warmup) ? '' : d.warmup} onChange={(e) => set('warmup', num(e.target.value))} /></label>
        <label className="space-y-1"><span className="text-muted">Timeout per run (s)</span><input className="input" type="number" min={1} value={Number.isNaN(d.timeoutSecs) ? '' : d.timeoutSecs} onChange={(e) => set('timeoutSecs', num(e.target.value))} /></label>
      </div>
      <fieldset className="space-y-2 rounded-md border border-line p-3">
        <legend className="px-1 text-muted">Revisions</legend>
        <label className="flex items-center gap-2"><span className="w-[110px] shrink-0">Baseline</span><input className="input mono" value={d.baselineRef} onChange={(e) => set('baselineRef', e.target.value)} aria-label="Baseline ref" /></label>
        {d.variants.map((v, i) => (
          <div key={i} className="flex items-center gap-2">
            <input className="input !w-[110px] shrink-0" value={v.label} onChange={(e) => setVariant(i, { label: e.target.value })} placeholder={`variant ${i + 1}`} aria-label={`Variant ${i + 1} label`} />
            <select className="input !w-[120px] shrink-0" value={v.source} onChange={(e) => setVariant(i, { source: e.target.value as VariantDraft['source'] })} aria-label={`Variant ${i + 1} source`}>
              <option value="ref">Branch/commit</option>
              <option value="mission" disabled={candidates.length === 0}>Mission</option>
            </select>
            {v.source === 'ref' ? (
              <input className="input mono" value={v.gitRef} onChange={(e) => setVariant(i, { gitRef: e.target.value })} placeholder="feature/faster-parser" aria-label={`Variant ${i + 1} ref`} />
            ) : (
              <select
                className="input"
                value={v.missionId}
                aria-label={`Variant ${i + 1} mission`}
                onChange={(e) => {
                  const m = candidates.find((c) => c.id === e.target.value);
                  setVariant(i, { missionId: e.target.value, label: v.label || (m ? m.title.slice(0, 40) : '') });
                }}
              >
                <option value="">Choose a mission…</option>
                {candidates.map((m) => <option key={m.id} value={m.id}>{m.title}</option>)}
              </select>
            )}
            <button className="btn btn-ghost btn-sm" onClick={() => setD((x) => ({ ...x, variants: x.variants.filter((_, j) => j !== i) }))} aria-label={`Remove variant ${i + 1}`}><Trash2 className="size-3.5" /></button>
          </div>
        ))}
        <button className="btn btn-sm" disabled={d.variants.length >= 8} onClick={() => setD((x) => ({ ...x, variants: [...x.variants, blankVariant()] }))}><Plus className="size-3.5" aria-hidden /> Add variant</button>
        <p className="m-0 text-[12px] text-muted">A mission is measured as its current work, committed or not; FORGE snapshots it when you create the experiment, so the agent can keep working without changing what is measured.</p>
      </fieldset>
      {problems.length > 0 && (
        <ul className="m-0 space-y-0.5 pl-5 text-[12px]">
          {problems.map((p) => <li key={p} className={p.startsWith('Note:') ? 'text-muted' : 'text-warn'}>{p}</li>)}
        </ul>
      )}
      {error && <Callout tone="bad" title="The core refused the experiment">{error}</Callout>}
    </Modal>
  );
}
