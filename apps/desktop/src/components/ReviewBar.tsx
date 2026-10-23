import { useState } from 'react';
import { Check, GitMerge, MessageSquareWarning, Trash2, X } from 'lucide-react';
import { api } from '../lib/api';
import { mergeMission, submitReview } from '../lib/actions';
import { errMsg, shortSha } from '../lib/format';
import { isLossWarning } from '../lib/actions';
import { useStore } from '../lib/store';
import type { MergeOutcome, Mission, VerificationReport } from '../lib/types';
import { approvalGate, OVERRIDE_MIN } from '../lib/verification';
import { Callout } from './ui';
import { Modal } from './Modal';

type Which = null | 'approve' | 'changes' | 'reject' | 'cleanup';

export function ReviewBar({ mission, report }: { mission: Mission; report: VerificationReport | null }) {
  const [which, setWhich] = useState<Which>(null);
  const [text, setText] = useState('');
  const [busy, setBusy] = useState(false);
  const [mergeError, setMergeError] = useState<string | null>(null);
  const [outcome, setOutcome] = useState<MergeOutcome | null>(null);
  const [lossMessage, setLossMessage] = useState<string | null>(null);
  const notify = useStore((s) => s.notify);

  const state = mission.state;
  const finished = ['merged', 'rejected', 'cancelled'].includes(state);
  const canCleanup = finished && !!mission.worktreePath;
  const gate = approvalGate(report, text);

  if (state !== 'awaiting_review' && state !== 'approved' && !canCleanup && !outcome) return null;

  const close = () => { setWhich(null); setText(''); setLossMessage(null); };

  const doReview = async (decision: 'approve' | 'reject' | 'request_changes') => {
    setBusy(true);
    const m = await submitReview(mission.id, decision, decision === 'approve' ? undefined : text, decision === 'approve' && gate.needsOverride ? text : undefined);
    setBusy(false);
    if (m) close();
  };

  const doMerge = async (squash: boolean) => {
    setBusy(true);
    setMergeError(null);
    const r = await mergeMission(mission.id, squash);
    setBusy(false);
    if (r.outcome) setOutcome(r.outcome);
    else setMergeError(r.error);
  };

  const doCleanup = async (confirm: boolean) => {
    setBusy(true);
    try {
      await api.missionCleanup(mission.id, confirm);
      const d = await api.missionGet(mission.id);
      useStore.getState().ingestDetail(d);
      notify('info', 'Worktree cleaned up');
      close();
    } catch (e) {
      const msg = errMsg(e);
      if (!confirm && isLossWarning(msg)) setLossMessage(msg);
      else { notify('error', `Cleanup failed: ${msg}`); close(); }
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="shrink-0 space-y-2 border-b border-line bg-raised px-4 py-2" aria-label="Review">
      <div className="flex flex-wrap items-center gap-2">
        {state === 'awaiting_review' && (
          <>
            <span className="mr-1 text-muted">Review:</span>
            <button className="btn btn-primary" onClick={() => setWhich('approve')}><Check className="size-3.5" aria-hidden /> Approve…</button>
            <button className="btn" onClick={() => setWhich('changes')}><MessageSquareWarning className="size-3.5" aria-hidden /> Request changes…</button>
            <button className="btn btn-danger" onClick={() => setWhich('reject')}><X className="size-3.5" aria-hidden /> Reject…</button>
            {report && report.unresolved.length > 0 && <span className="text-[12px] text-warn">{report.unresolved.length} thing{report.unresolved.length === 1 ? '' : 's'} not established</span>}
            {report && report.hardBlockers.length > 0 && <span className="text-[12px] text-bad">Approval blocked: {report.hardBlockers[0]}</span>}
          </>
        )}
        {state === 'approved' && !outcome && (
          <>
            <span className="mr-1 text-ok">Approved.</span>
            <button className="btn btn-primary" disabled={busy} onClick={() => void doMerge(false)} title="Merge the mission branch with a merge commit"><GitMerge className="size-3.5" aria-hidden /> Merge</button>
            <button className="btn" disabled={busy} onClick={() => void doMerge(true)} title="Combine all mission commits into one commit on the target branch"><GitMerge className="size-3.5" aria-hidden /> Squash merge</button>
          </>
        )}
        {canCleanup && (
          <button className="btn" onClick={() => setWhich('cleanup')}><Trash2 className="size-3.5" aria-hidden /> Clean up worktree…</button>
        )}
      </div>

      {mergeError && (
        <Callout tone="bad" title="Merge did not complete">
          <pre className="max-h-48 overflow-auto whitespace-pre-wrap break-words text-[12px]">{mergeError}</pre>
        </Callout>
      )}
      {outcome && (
        <Callout tone="info" title={outcome.squashed ? 'Squash merged' : 'Merged'}>
          <div>Commit <span className="mono">{shortSha(outcome.commit)}</span> on <span className="mono">{outcome.target}</span> (from <span className="mono">{outcome.branch}</span>){outcome.cleanedUp ? '; worktree cleaned up' : ''}.</div>
          {outcome.notes.length > 0 && <ul className="m-0 mt-1 list-disc pl-4 text-muted">{outcome.notes.map((n, i) => <li key={i}>{n}</li>)}</ul>}
        </Callout>
      )}

      <Modal
        open={which === 'approve'}
        onClose={close}
        title={gate.needsOverride ? 'Approve anyway — recorded in the Proof Pack' : 'Approve changes'}
        footer={
          <>
            <button className="btn" onClick={close} disabled={busy}>Cancel</button>
            <button className="btn btn-primary" disabled={!gate.canApprove || busy} onClick={() => void doReview('approve')}>{gate.needsOverride ? 'Approve anyway' : 'Approve'}</button>
          </>
        }
      >
        {gate.blockers.length > 0 && (
          <Callout tone="bad" title="Cannot approve yet (not overridable)"><ul className="m-0 list-disc pl-4">{gate.blockers.map((b, i) => <li key={i}>{b}</li>)}</ul></Callout>
        )}
        {report && report.unresolved.length > 0 && (
          <Callout tone="warn" title="Not established"><ul className="m-0 list-disc pl-4">{report.unresolved.map((b, i) => <li key={i}>{b}</li>)}</ul></Callout>
        )}
        {gate.needsOverride && gate.blockers.length === 0 && (
          <div>
            <label className="label" htmlFor="override">Override reason (at least {OVERRIDE_MIN} characters). Approve anyway — recorded in the Proof Pack.</label>
            <textarea id="override" className="input min-h-[70px]" value={text} onChange={(e) => setText(e.target.value)} autoFocus />
            <div className="mt-0.5 text-[11px] text-faint">{text.trim().length}/{OVERRIDE_MIN} minimum</div>
          </div>
        )}
        {!gate.needsOverride && gate.blockers.length === 0 && <div className="text-muted">Everything in the contract is established for the current changes. Approval makes the mission mergeable; it does not merge it.</div>}
      </Modal>

      <Modal
        open={which === 'changes'}
        onClose={close}
        title="Request changes"
        footer={
          <>
            <button className="btn" onClick={close} disabled={busy}>Cancel</button>
            <button className="btn btn-primary" disabled={!text.trim() || busy} onClick={() => void doReview('request_changes')}>Send to agent</button>
          </>
        }
      >
        <label className="label" htmlFor="rc">What should change? This is sent to the agent as a follow-up and starts a new turn.</label>
        <textarea id="rc" className="input min-h-[100px]" value={text} onChange={(e) => setText(e.target.value)} autoFocus />
      </Modal>

      <Modal
        open={which === 'reject'}
        onClose={close}
        tone="bad"
        title="Reject mission?"
        footer={
          <>
            <button className="btn" onClick={close} disabled={busy}>Keep reviewing</button>
            <button className="btn btn-danger-solid" disabled={busy} onClick={() => void doReview('reject')}>Reject</button>
          </>
        }
      >
        <p className="m-0">Rejecting is final: the mission cannot be approved or merged afterwards. Its worktree is not deleted by this action.</p>
        <div>
          <label className="label" htmlFor="rj">Reason (optional)</label>
          <textarea id="rj" className="input min-h-[60px]" value={text} onChange={(e) => setText(e.target.value)} />
        </div>
      </Modal>

      <Modal
        open={which === 'cleanup'}
        onClose={close}
        tone="bad"
        title={lossMessage ? 'Permanent data loss' : 'Clean up worktree?'}
        footer={
          <>
            <button className="btn" onClick={close} disabled={busy}>Keep worktree</button>
            <button className="btn btn-danger-solid" disabled={busy} onClick={() => void doCleanup(!!lossMessage)}>{lossMessage ? 'Permanently delete' : 'Check and clean up'}</button>
          </>
        }
      >
        {lossMessage ? (
          <pre className="whitespace-pre-wrap break-words rounded-md border border-bad/40 bg-bad-dim p-3 text-[12px]">{lossMessage}</pre>
        ) : (
          <p className="m-0">This removes the mission's leftover worktree. FORGE first checks for uncommitted changes and unmerged commits and will show exactly what would be lost before deleting anything.</p>
        )}
      </Modal>
    </div>
  );
}
