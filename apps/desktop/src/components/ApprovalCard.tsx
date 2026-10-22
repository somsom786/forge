import { useState } from 'react';
import { Check, ShieldAlert, ShieldX, X } from 'lucide-react';
import { decideApproval } from '../lib/actions';
import { RISK_TONE, relTime } from '../lib/format';
import type { Approval } from '../lib/types';
import { Badge, Kbd } from './ui';

export function ApprovalInput({ a }: { a: Approval }) {
  let text: string;
  try {
    text = JSON.stringify(a.detail?.input ?? {}, null, 2);
  } catch {
    text = String(a.detail?.input);
  }
  return (
    <details className="group rounded-md border border-line bg-bg">
      <summary className="cursor-pointer select-none px-2 py-1 text-[12px] text-muted hover:text-fg">Raw tool input</summary>
      <pre className="max-h-64 overflow-auto whitespace-pre-wrap break-all border-t border-line p-2 text-[12px] text-fg">{text}</pre>
    </details>
  );
}

export function ApprovalCard({ approval: a, compact }: { approval: Approval; compact?: boolean }) {
  const [note, setNote] = useState('');
  const [showNote, setShowNote] = useState(false);
  const [busy, setBusy] = useState(false);
  const pending = a.state === 'pending';
  const blocked = a.state === 'auto_denied';

  const decide = async (approved: boolean) => {
    if (busy || !pending) return;
    setBusy(true);
    await decideApproval(a.id, approved, note);
    setBusy(false);
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (!pending || e.ctrlKey || e.metaKey || e.altKey) return;
    const t = e.target as HTMLElement;
    if (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.tagName === 'SUMMARY') return;
    const k = e.key.toLowerCase();
    if (k === 'a') {
      e.preventDefault();
      void decide(true);
    } else if (k === 'd') {
      e.preventDefault();
      void decide(false);
    }
  };

  const border = pending
    ? a.risk === 'high'
      ? 'border-bad/60'
      : a.risk === 'medium'
        ? 'border-warn/60'
        : 'border-line-strong'
    : blocked
      ? 'border-bad/30 bg-bad-dim/50'
      : 'border-line';

  return (
    <div
      tabIndex={pending ? 0 : -1}
      onKeyDown={onKeyDown}
      aria-label={`Approval request: ${a.tool}. ${pending ? 'Press A to approve or D to deny.' : ''}`}
      className={`rounded-md border bg-raised p-3 outline-offset-2 ${border} ${pending ? '' : 'opacity-90'}`}
    >
      <div className="flex flex-wrap items-center gap-2">
        {blocked ? <ShieldX className="size-4 text-bad" aria-hidden /> : <ShieldAlert className={`size-4 ${pending ? 'text-warn' : 'text-muted'}`} aria-hidden />}
        <span className="mono font-semibold text-fg">{a.tool}</span>
        {pending && <Badge tone={RISK_TONE[a.risk]}>{a.risk} risk</Badge>}
        {blocked && <Badge tone="bad">Blocked by policy</Badge>}
        {!pending && !blocked && <Badge tone={a.state === 'approved' ? 'ok' : 'idle'}>{a.state.replace('_', ' ')}</Badge>}
        {!pending && !blocked && a.risk !== 'low' && <Badge tone={RISK_TONE[a.risk]}>{a.risk} risk</Badge>}
        <span className="ml-auto text-[11px] text-muted">{relTime(a.decidedAt ?? a.createdAt)}</span>
      </div>
      <div className="mono mt-1.5 whitespace-pre-wrap break-words text-[12.5px] text-fg">{a.summary}</div>

      {a.warnings.length > 0 && (
        <ul className="m-0 mt-2 list-none space-y-1 p-0">
          {a.warnings.map((w, i) => (
            <li key={i} className="flex gap-1.5 text-warn">
              <span aria-hidden>!</span>
              <span>{w}</span>
            </li>
          ))}
        </ul>
      )}
      {a.detail?.blockedPath && (
        <div className="mt-1.5 text-[12px] text-muted">
          Path outside worktree: <span className="mono text-fg">{a.detail.blockedPath}</span>
        </div>
      )}
      {a.detail?.providerReason && <div className="mt-1 text-[12px] text-muted">Provider reason: {a.detail.providerReason}</div>}

      {!compact && (
        <div className="mt-2">
          <ApprovalInput a={a} />
        </div>
      )}

      {!pending && a.decisionNote && (
        <div className={`mt-2 whitespace-pre-wrap text-[12px] ${blocked ? 'text-bad' : 'text-muted'}`}>
          {blocked ? 'Reason: ' : 'Note: '}
          {a.decisionNote}
        </div>
      )}
      {!pending && a.decidedBy && !blocked && <div className="mt-1 text-[11px] text-faint">Decided by {a.decidedBy}</div>}

      {pending && (
        <div className="mt-3 space-y-2">
          {showNote && (
            <input
              className="input"
              placeholder="Optional note for the agent / audit log"
              value={note}
              onChange={(e) => setNote(e.target.value)}
              aria-label="Decision note"
              autoFocus
            />
          )}
          <div className="flex items-center gap-2">
            <button className="btn btn-primary" disabled={busy} onClick={() => void decide(true)}>
              <Check className="size-3.5" aria-hidden /> Approve <Kbd>A</Kbd>
            </button>
            <button className="btn btn-danger" disabled={busy} onClick={() => void decide(false)}>
              <X className="size-3.5" aria-hidden /> Deny <Kbd>D</Kbd>
            </button>
            {!showNote && (
              <button className="btn btn-ghost btn-sm ml-auto" onClick={() => setShowNote(true)}>
                Add note
              </button>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
