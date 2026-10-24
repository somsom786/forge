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
  const [busy, setBusy] = useState(false);
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
      notify(r.failures.length ? 'warn' : 'info', `Removed ${r.eventsDeleted} transcript events and ${r.filesDeleted} files (${bytes(r.bytesFreed)})${r.failures.length ? `; ${r.failures.length} could not be removed` : ''}.`);
      setPlan(null);
      load();
    } catch (e) {
      notify('error', errMsg(e));
    } finally {
      setBusy(false);
    }
  };
  const nothing = plan && plan.missions.length === 0 && plan.orphanFiles.length === 0;

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
      <div className="mt-3 flex flex-wrap items-center gap-2">
        <label className="flex items-center gap-2 text-[13px]">
          Clean up finished missions older than
          <input className="input !w-20" type="number" min={0} max={3650} value={days} onChange={(e) => setDays(Math.max(0, Math.min(3650, Number(e.target.value) || 0)))} aria-label="Age in days" />
          days
        </label>
        <button className="btn" disabled={busy} onClick={() => void preview()}><HardDrive className="size-3.5" aria-hidden /> Preview clean-up</button>
      </div>
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
        {plan && (nothing ? <div className="text-muted">Nothing to remove: no finished mission older than {plan.olderThanDays} days has logs or a transcript left.</div> : (
          <>
            <p className="m-0">
              Removes the check logs and agent transcripts of {plan.missions.length} finished mission(s) last changed more than {plan.olderThanDays} days ago
              {plan.orphanFiles.length ? `, and ${plan.orphanFiles.length} log file(s) no check refers to` : ''}: {plan.events.toLocaleString()} events, {plan.files} files, {bytes(plan.bytes)}.
            </p>
            <Callout tone="info">Kept: each mission, its state history, approvals, review decisions, check results with their output tail, Proof Packs, memory and the audit log. If anything changes before you confirm, nothing is removed and you are asked to preview again.</Callout>
            <ul className="m-0 list-disc pl-5 text-[12.5px]">
              {plan.missions.slice(0, 50).map((m) => <li key={m.id}>{m.title} <span className="text-muted">({m.state}, {m.events} events, {m.files.length} files, {bytes(m.bytes)})</span></li>)}
            </ul>
          </>
        ))}
      </Modal>
    </section>
  );
}
