import { useState } from 'react';
import { api } from '../../lib/api';
import { errMsg } from '../../lib/format';
import { canSave, classifyDiff, invisibleSegments, isChangedOnDisk } from '../../lib/harness';
import { useStore } from '../../lib/store';
import type { HarnessPreview } from '../../lib/types';
import { Callout } from '../ui';
import { DryRunDialog } from './DryRunDialog';

/** Plain text where invisible / direction-changing characters are shown as <U+XXXX> in a distinct style. */
export function VisibleText({ text }: { text: string }) {
  return (
    <>
      {invisibleSegments(text).map((s, i) =>
        s.invisible ? <span key={i} className="rounded-sm bg-warn/30 px-0.5 font-semibold text-warn" title="Invisible or direction-changing character">{s.text}</span> : <span key={i}>{s.text}</span>,
      )}
    </>
  );
}

export function DiffText({ diff }: { diff: string }) {
  const lines = classifyDiff(diff);
  if (lines.length === 0) return <div className="px-3 py-2 text-muted">No changes.</div>;
  return (
    <div className="max-h-80 overflow-auto rounded border border-line bg-bg" aria-label="Diff">
      <div className="w-max min-w-full">
        {lines.map((l, i) => (
          <pre
            key={i}
            className={`px-2 text-[12px] leading-[1.5] whitespace-pre ${
              l.kind === 'add' ? 'bg-[#13271a] text-ok' : l.kind === 'del' ? 'bg-[#2d1618] text-bad' : l.kind === 'hunk' ? 'bg-accent-dim/60 text-accent' : l.kind === 'meta' ? 'text-faint' : ''
            }`}
          >
            {l.text ? <VisibleText text={l.text} /> : ' '}
          </pre>
        ))}
      </div>
    </div>
  );
}

interface Props {
  projectId: string;
  preview: HarnessPreview;
  /** Hash to send as expectedSha256 (the file hash the edit/preview was based on). */
  expectedSha256: string | null;
  onApplied: () => void;
  onCancel: () => void;
  onReload: () => void;
}

/** Shared "review then save" panel for file edits and hook add/remove. Always saves preview.text. */
export function PreviewPanel({ projectId, preview, expectedSha256, onApplied, onCancel, onReload }: Props) {
  const notify = useStore((s) => s.notify);
  const [ack, setAck] = useState(false);
  const [note, setNote] = useState('');
  const [busy, setBusy] = useState(false);
  const [diskChanged, setDiskChanged] = useState<string | null>(null);
  const [dry, setDry] = useState<{ event: string; command: string } | null>(null);

  const save = async () => {
    setBusy(true);
    setDiskChanged(null);
    try {
      await api.harnessApply({
        projectId, rel: preview.rel, text: preview.text, expectedSha256, expectedAfterSha256: preview.afterSha256,
        acknowledgeRisks: ack, note: note.trim() || undefined,
      });
      notify('info', `Saved ${preview.rel}`);
      onApplied();
    } catch (e) {
      const m = errMsg(e);
      if (isChangedOnDisk(m)) setDiskChanged(m);
      else notify('error', m);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="space-y-3 rounded-md border border-line-strong bg-panel p-3" aria-label="Review changes">
      <div className="flex flex-wrap items-center gap-2">
        <h3 className="m-0 text-[12px] font-medium uppercase tracking-wide text-muted">Review changes</h3>
        <span className="mono break-all text-[12px]">{preview.rel}</span>
        <span className="text-[12px]"><span className="text-ok">+{preview.added}</span> <span className="text-bad">−{preview.removed}</span></span>
        {!preview.exists && <span className="text-[12px] text-accent">new file</span>}
      </div>
      {preview.unchanged && <Callout tone="info">This would not change the file.</Callout>}
      {preview.problems.length > 0 && (
        <Callout tone="bad" title="Cannot be saved"><ul className="m-0 list-disc pl-4">{preview.problems.map((p, i) => <li key={i} className="break-words">{p}</li>)}</ul></Callout>
      )}
      {preview.warnings.length > 0 && (
        <Callout tone="warn" title="Warnings"><ul className="m-0 list-disc pl-4">{preview.warnings.map((p, i) => <li key={i} className="break-words">{p}</li>)}</ul></Callout>
      )}
      {preview.risks.length > 0 && (
        <Callout tone="warn" title="Changes that run commands or change where traffic goes">
          <ul className="m-0 list-none space-y-2 p-0">
            {preview.risks.map((r, i) => (
              <li key={i}>
                <div className="text-[12px]"><span className="mono font-medium">{r.event}</span></div>
                <pre className="whitespace-pre-wrap break-all rounded border border-line bg-bg p-1.5 text-[12px]"><VisibleText text={r.display} /></pre>
                <ul className="m-0 list-disc pl-4 text-[12px]">{r.risks.map((x, k) => <li key={k}>{x}</li>)}</ul>
                {!r.event.startsWith('settings.') && <button className="btn btn-sm mt-1" onClick={() => setDry({ event: r.event, command: r.command })}>Dry run…</button>}
              </li>
            ))}
          </ul>
          <label className="mt-2 flex items-start gap-2">
            <input type="checkbox" className="mt-1" checked={ack} onChange={(e) => setAck(e.target.checked)} />
            <span>I understand these hooks run on my machine</span>
          </label>
        </Callout>
      )}
      <DiffText diff={preview.diff} />
      {diskChanged && (
        <Callout tone="bad" title="The file changed on disk">
          <p className="m-0 break-words">{diskChanged}</p>
          <p className="m-0 mt-1">Nothing was overwritten. Reloading discards this edit; FORGE will not decide that for you.</p>
          <button className="btn btn-sm mt-2" onClick={onReload}>Reload from disk</button>
        </Callout>
      )}
      <div>
        <label className="label" htmlFor="hp-note">Note (optional, recorded in history)</label>
        <input id="hp-note" className="input" value={note} onChange={(e) => setNote(e.target.value)} />
      </div>
      <div className="flex gap-2">
        <button className="btn btn-primary" disabled={busy || !canSave(preview, ack)} onClick={() => void save()}>Save</button>
        <button className="btn" onClick={onCancel} disabled={busy}>Back</button>
      </div>
      {dry && <DryRunDialog projectId={projectId} event={dry.event} command={dry.command} onClose={() => setDry(null)} />}
    </div>
  );
}
