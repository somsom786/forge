import { useCallback, useEffect, useState } from 'react';
import { api } from '../../lib/api';
import { errMsg, localTime } from '../../lib/format';
import { shortHash } from '../../lib/knowledge';
import { useStore } from '../../lib/store';
import type { HarnessChange, RollbackPreview } from '../../lib/types';
import { Modal } from '../Modal';
import { Badge, Callout } from '../ui';
import { DiffText } from './PreviewPanel';

export function HistoryPanel({ projectId, rel, onChanged }: { projectId: string; rel?: string; onChanged: () => void }) {
  const tick = useStore((s) => s.harnessTick[projectId] ?? 0);
  const notify = useStore((s) => s.notify);
  const [list, setList] = useState<HarnessChange[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [rb, setRb] = useState<{ change: HarnessChange; preview: RollbackPreview | null } | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    try { setList(await api.harnessHistory(projectId, rel)); setError(null); } catch (e) { setError(errMsg(e)); }
  }, [projectId, rel]);
  useEffect(() => { void load(); }, [load, tick]);

  const openRollback = async (change: HarnessChange) => {
    setRb({ change, preview: null });
    try { setRb({ change, preview: await api.harnessRollbackPreview(change.id) }); } catch (e) { notify('error', errMsg(e)); setRb(null); }
  };
  const doRollback = async () => {
    if (!rb) return;
    setBusy(true);
    try {
      await api.harnessRollback(rb.change.id);
      notify('info', `Rolled back ${rb.change.rel}`);
      setRb(null);
      await load();
      onChanged();
    } catch (e) { notify('error', errMsg(e)); } finally { setBusy(false); }
  };

  if (error) return <Callout tone="bad" title="Could not load history"><pre className="whitespace-pre-wrap text-[12px]">{error}</pre></Callout>;
  if (!list) return <div className="text-muted">Loading…</div>;
  if (list.length === 0) return <div className="text-muted">No FORGE-made changes {rel ? 'to this file' : 'in this project'} yet.</div>;

  return (
    <>
      <ul className="m-0 list-none divide-y divide-line rounded-md border border-line p-0" aria-label="Change history">
        {list.map((c) => {
          const canRollback = c.state === 'applied' && c.kind !== 'rollback' && !c.rolledBackBy;
          return (
            <li key={c.id} className="space-y-0.5 px-3 py-2">
              <div className="flex flex-wrap items-center gap-2">
                <Badge tone={c.kind === 'rollback' ? 'run' : c.kind === 'create' ? 'ok' : 'idle'}>{c.kind}</Badge>
                <Badge tone={c.state === 'applied' ? 'ok' : c.state === 'failed' ? 'bad' : 'warn'}>{c.state}</Badge>
                {c.rolledBackBy && <Badge tone="warn">rolled back</Badge>}
                <span className="text-[12px] text-muted">{localTime(c.createdAt)}</span>
                {canRollback && <button className="btn btn-sm ml-auto" onClick={() => void openRollback(c)}>Roll back…</button>}
              </div>
              {!rel && <div className="mono break-all text-[12px]">{c.rel}</div>}
              {c.note && <div className="break-words text-[12px] text-muted">{c.note}</div>}
              <div className="mono text-[11px] text-faint">{shortHash(c.beforeSha256)} → {shortHash(c.afterSha256)}{c.rolledBackBy ? ` · rolled back by ${c.rolledBackBy.slice(0, 8)}` : ''}</div>
            </li>
          );
        })}
      </ul>
      <Modal open={!!rb} onClose={() => !busy && setRb(null)} wide title={`Roll back ${rb?.change.rel ?? ''}`}
        footer={<><button className="btn" onClick={() => setRb(null)} disabled={busy}>Cancel</button><button className="btn btn-danger-solid" disabled={busy || !rb?.preview || rb.preview.conflict !== null} onClick={() => void doRollback()}>Roll back</button></>}>
        {!rb?.preview && <div className="text-muted">Checking…</div>}
        {rb?.preview?.conflict && (
          <Callout tone="bad" title="Cannot roll back">
            <p className="m-0 break-words">{rb.preview.conflict}</p>
            <p className="m-0 mt-1">The file changed after this edit, so rolling back would overwrite newer work. Edit the file by hand instead.</p>
          </Callout>
        )}
        {rb?.preview && !rb.preview.conflict && (
          <>
            {rb.preview.removesFile && <Callout tone="warn" title="This will remove the file">The file did not exist before this change, so rolling back deletes it.</Callout>}
            <DiffText diff={rb.preview.diff} />
          </>
        )}
      </Modal>
    </>
  );
}
