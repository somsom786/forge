import { useState } from 'react';
import { api } from '../../lib/api';
import { errMsg } from '../../lib/format';
import { dryRunView, oddChars, revealOdd } from '../../lib/harness';
import type { HookDryRun } from '../../lib/types';
import { Modal } from '../Modal';
import { Badge, Callout } from '../ui';

/** Confirms the exact command, runs it on this machine with a synthetic event, and shows the outcome. */
export function DryRunDialog({ projectId, event, command, onClose }: { projectId: string; event: string; command: string; onClose: () => void }) {
  const [busy, setBusy] = useState(false);
  const [res, setRes] = useState<HookDryRun | null>(null);
  const [error, setError] = useState<string | null>(null);
  const odd = oddChars(command);

  const run = async () => {
    setBusy(true);
    setError(null);
    try {
      setRes(await api.harnessHookDryRun(projectId, event, command));
    } catch (e) {
      setError(errMsg(e));
    } finally {
      setBusy(false);
    }
  };
  const v = res ? dryRunView(res) : null;

  return (
    <Modal open onClose={onClose} wide title={res ? 'Dry run result' : 'Run this hook command?'}
      footer={res ? <button className="btn" onClick={onClose}>Close</button> : <><button className="btn" onClick={onClose} disabled={busy}>Cancel</button><button className="btn btn-primary" disabled={busy} onClick={() => void run()}>{busy ? 'Running…' : error ? 'Try again' : 'Run on this machine'}</button></>}>
      <div className="text-[12px] text-muted">Event <span className="font-medium text-fg">{event}</span></div>
      <pre className="whitespace-pre-wrap break-all rounded border border-line bg-bg p-2 text-[12px]" aria-label="Full command">{revealOdd(command)}</pre>
      {odd.length > 0 && <div className="text-[12px] text-warn">This command contains {odd.length} unusual character{odd.length === 1 ? '' : 's'}, shown as ⟦U+XXXX⟧: {odd.slice(0, 6).map((o) => o.code).join(', ')}{odd.length > 6 ? '…' : ''}</div>}
      {error && <Callout tone="bad" title="The hook could not be run"><pre className="whitespace-pre-wrap break-words text-[12px]">{error}</pre></Callout>}
      {!res && <p className="m-0">This executes on this machine with your user privileges, in the project folder, with a synthetic event as input. Environment variables whose names look like secrets are withheld.</p>}
      {res && v && (
        <>
          <div className="flex flex-wrap items-center gap-2"><Badge tone={v.tone}>{v.headline}</Badge><span className="text-[12px] text-muted">{res.durationMs} ms</span></div>
          <dl className="m-0 grid grid-cols-[100px_1fr] gap-x-2 gap-y-0.5 text-[12px]">
            {v.rows.map(([k, val]) => <div key={k} className="contents"><dt className="text-faint">{k}</dt><dd className="mono m-0 break-all">{val}</dd></div>)}
          </dl>
          {res.interpretation && <div className="rounded border border-line bg-raised px-2 py-1.5">{res.interpretation}</div>}
          <div><div className="label">stdout</div><pre className="max-h-48 overflow-auto whitespace-pre-wrap break-all rounded border border-line bg-bg p-2 text-[12px]">{res.stdout || '(empty)'}</pre></div>
          <div><div className="label">stderr</div><pre className="max-h-48 overflow-auto whitespace-pre-wrap break-all rounded border border-line bg-bg p-2 text-[12px]">{res.stderr || '(empty)'}</pre></div>
          {v.withheldEnv.length > 0 && <div className="text-[12px] text-muted">Withheld env vars: <span className="mono break-all">{v.withheldEnv.join(', ')}</span></div>}
          <details><summary className="cursor-pointer text-[12px] text-muted hover:text-fg">Synthetic input JSON</summary><pre className="mt-1 max-h-48 overflow-auto whitespace-pre-wrap break-all rounded border border-line bg-bg p-2 text-[12px]">{v.inputJson}</pre></details>
        </>
      )}
    </Modal>
  );
}
