import { useCallback, useEffect, useState } from 'react';
import { HardDrive, Trash2 } from 'lucide-react';
import { api } from '../lib/api';
import { errMsg } from '../lib/format';
import { useStore } from '../lib/store';
import type { CleanupPlan, StorageUsage } from '../lib/types';
import { Modal } from './Modal';
import { Callout } from './ui';

export function bytes(n: number): string {
  if (n < 1024) return `${n} B`;
  const u = ['KB', 'MB', 'GB', 'TB'];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < u.length - 1) { v /= 1024; i++; }
  return `${v.toFixed(v < 10 ? 1 : 0)} ${u[i]}`;
}

/** What FORGE keeps on disk, and a previewed, confirmed clean-up of old finished missions' raw logs and transcripts. */
export function StoragePanel() {
  const notify = useStore((s) => s.notify);
  const [usage, setUsage] = useState<StorageUsage | null>(null);
  const [days, setDays] = useState(30);
  const [plan, setPlan] = useState<CleanupPlan | null>(null);
  const [cleanupFailures, setCleanupFailures] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [compactOpen, setCompactOpen] = useState(false);
  const load = useCallback(() => { void api.storageUsage().then(setUsage).catch((e) => notify('error', errMsg(e))); }, [notify]);
  useEffect(load, [load]);

  const preview = async () => {
    setBusy(true);
    try { setPlan(await api.storageCleanupPreview(days)); } catch (e) { notify('error', errMsg(e)); } finally { setBusy(false); }
  };
  const apply = async () => {
    if (!plan) return;
    setBusy(true);
    try {
      const r = await api.storageCleanupApply(plan.olderThanDays, plan.token);
      setCleanupFailures(r.failures);
      notify(r.failures.length ? 'warn' : 'info', `Removed ${r.eventsDeleted} transcript events and ${r.filesDeleted} files (${bytes(r.bytesFreed)})${r.failures.length ? `; ${r.failures.length} issue(s) reported` : ''}.`);
      setPlan(null);
      load();
    } catch (e) {
      notify('error', errMsg(e));
    } finally {
      setBusy(false);
    }
  };
  const nothing = plan && plan.missions.length === 0 && plan.orphanFiles.length === 0 && plan.experiments.length === 0;
  const compact = async () => {
    setBusy(true);
    try { await api.storageCompact(); notify('info', 'Database compacted'); setCompactOpen(false); load(); }
    catch (e) { notify('error', errMsg(e)); }
    finally { setBusy(false); }
  };

  return (
    <section aria-label="Storage">
      <h2 className="m-0 mb-2 text-[12px] font-medium uppercase tracking-wide text-muted">Storage</h2>
      {!usage ? <div className="text-muted">Measuring…</div> : (
        <dl className="m-0 grid max-w-3xl grid-cols-[200px_1fr] gap-x-3 gap-y-1.5">
          <dt className="text-muted">Database</dt><dd className="m-0">{bytes(usage.databaseBytes)} · {usage.events.toLocaleString()} transcript events</dd>
          <dt className="text-muted">Check logs</dt><dd className="m-0">{bytes(usage.runBytes)} in {usage.runFiles} files</dd>
          <dt className="text-muted">Proof Packs</dt><dd className="m-0">{bytes(usage.proofBytes)} in {usage.proofFiles} files (never removed by clean-up)</dd>
          <dt className="text-muted">Experiments</dt><dd className="m-0">{bytes(usage.experimentBytes)}</dd>
          <dt className="text-muted">Worktrees</dt><dd className="m-0">{usage.worktrees}</dd>
        </dl>
      )}
      {usage && usage.untrackedWorktrees.length > 0 && (
        <Callout tone="warn" title={`${usage.untrackedWorktrees.length} worktree folder(s) no mission refers to`}>
          FORGE does not delete these: they may hold work. Inspect them and remove them with git when you are sure.
          <ul className="m-0 mt-1 list-disc pl-5">{usage.untrackedWorktrees.slice(0, 20).map((w) => <li key={w} className="mono break-all text-[12px]">{w}</li>)}</ul>
        </Callout>
      )}
      {cleanupFailures.length > 0 && (
        <Callout tone="warn" title="Clean-up was incomplete">
          Some items may already have been removed. Review the issues below, then preview clean-up again before retrying.
          <ul className="m-0 mt-1 max-h-48 overflow-auto list-disc pl-5 text-[12px]">{cleanupFailures.slice(0, 100).map((failure, index) => <li key={`${index}-${failure}`} className="break-words">{failure}</li>)}</ul>
          {cleanupFailures.length > 100 && <p className="m-0 mt-1">Showing 100 of {cleanupFailures.length} issues. The audit log records the full result.</p>}
        </Callout>
      )}
      <div className="mt-3 flex flex-wrap items-center gap-2">
        <label className="flex items-center gap-2 text-[13px]">
          Clean up finished work older than
          <input className="input !w-20" type="number" min={0} max={3650} value={days} onChange={(e) => setDays(Math.max(0, Math.min(3650, Number(e.target.value) || 0)))} aria-label="Age in days" />
          days
        </label>
        <button className="btn" disabled={busy} onClick={() => void preview()}><HardDrive className="size-3.5" aria-hidden /> Preview clean-up</button>
        <button className="btn" disabled={busy} onClick={() => setCompactOpen(true)}>Compact database</button>
      </div>
      <Modal open={compactOpen} onClose={() => { if (!busy) setCompactOpen(false); }} title="Compact database" footer={<><button className="btn" disabled={busy} onClick={() => setCompactOpen(false)}>Cancel</button><button className="btn btn-primary" disabled={busy} onClick={() => void compact()}>{busy ? 'Compacting...' : 'Compact now'}</button></>}>
        <p>Reclaims unused database pages after cleanup. This can take time and requires all missions, checks and experiments to be idle. It runs only when you choose this action; your stored records are kept.</p>
      </Modal>
      <Modal
        open={plan !== null}
        onClose={() => setPlan(null)}
        wide
        title="Storage clean-up"
        footer={
          <>
            <button className="btn" onClick={() => setPlan(null)}>Cancel</button>
            <button className="btn btn-danger" disabled={busy || !!nothing} onClick={() => void apply()}><Trash2 className="size-3.5" aria-hidden /> Remove what is listed</button>
          </>
        }
      >
        {plan && plan.skippedExperiments.length > 0 && <Callout tone="warn" title="Experiment folders kept for inspection"><ul>{plan.skippedExperiments.map((s) => <li key={s}>{s}</li>)}</ul></Callout>}
        {plan && (nothing ? <div className="text-muted">Nothing to remove: no eligible finished work older than {plan.olderThanDays} days has raw artifacts left.</div> : (
          <>
            <p className="m-0">
              Removes the check logs and agent transcripts of {plan.missions.length} finished mission(s) last changed more than {plan.olderThanDays} days ago
              {plan.orphanFiles.length ? `, and ${plan.orphanFiles.length} log file(s) no check refers to` : ''}, plus raw logs and empty folders for {plan.experiments.length} finished experiment(s): {plan.events.toLocaleString()} events, {plan.files} files, {bytes(plan.bytes)}.
            </p>
            <Callout tone="info">Kept: each mission, its state history, approvals, review decisions, check results with their output tail, Proof Packs, memory and the audit log. If anything changes before you confirm, nothing is removed and you are asked to preview again.</Callout>
            {plan.missions.length > 0 && <details>
              <summary>Inspect all {plan.missions.length} mission(s) and their log files</summary>
              <pre className="max-h-64 overflow-auto whitespace-pre-wrap break-all text-[12px]">{plan.missions.map((m) => `${m.title} (${m.id}; ${m.state}; ${m.events} events; ${bytes(m.bytes)})${m.files.map((f) => `\n  runs/${f}`).join('')}`).join('\n\n')}</pre>
            </details>}
            {plan.orphanFiles.length > 0 && <details>
              <summary>Inspect all {plan.orphanFiles.length} unreferenced log file(s)</summary>
              <pre className="max-h-64 overflow-auto whitespace-pre-wrap break-all text-[12px]">{plan.orphanFiles.map((f) => `runs/${f}`).join('\n')}</pre>
            </details>}
            {plan.experiments.map((e) => <details key={e.id}><summary>{e.name} ({e.state}, {e.files.length} log files, {bytes(e.bytes)})</summary><ul className="list-disc pl-5 text-[12px]">{e.files.map((f) => <li key={f.path} className="mono break-all">experiments/{e.id}/{f.path}</li>)}</ul><div className="text-muted text-[12px]">Only empty folders are removed after these files. Worktrees and unexpected files are kept for inspection.</div></details>)}
          </>
        ))}
      </Modal>
    </section>
  );
}
