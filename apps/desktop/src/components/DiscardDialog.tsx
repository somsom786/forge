import * as Dialog from '@radix-ui/react-dialog';
import { AlertTriangle } from 'lucide-react';
import { attemptDiscard, confirmDiscardLoss } from '../lib/actions';
import { missionLabel } from '../lib/format';
import { useStore } from '../lib/store';

export function DiscardDialog() {
  const d = useStore((s) => s.discard);
  const setDiscard = useStore((s) => s.setDiscard);
  const mission = useStore((s) => (d ? s.missions[d.missionId] : undefined));
  const loss = d?.lossMessage != null;

  return (
    <Dialog.Root open={!!d} onOpenChange={(o) => !o && !d?.busy && setDiscard(null)}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-[60] bg-black/65" />
        <Dialog.Content
          className="fixed left-1/2 top-1/2 z-[60] w-[520px] max-w-[94vw] -translate-x-1/2 -translate-y-1/2 rounded-lg border border-bad/40 bg-panel shadow-2xl shadow-black/60"
          aria-describedby="discard-body"
        >
          <div className="flex items-center gap-2 border-b border-line px-4 py-3">
            <AlertTriangle className="size-4 text-bad" aria-hidden />
            <Dialog.Title className="m-0 text-[14px] font-semibold">{loss ? 'Permanent data loss' : 'Discard mission?'}</Dialog.Title>
          </div>
          <div id="discard-body" className="space-y-3 p-4">
            {mission && <div className="text-muted">Mission: <span className="text-fg">{missionLabel(mission)}</span></div>}
            {loss ? (
              <pre className="whitespace-pre-wrap break-words rounded-md border border-bad/40 bg-bad-dim p-3 text-[12px] text-fg">{d?.lossMessage}</pre>
            ) : (
              <p className="m-0 text-fg">
                Discarding removes this mission's worktree and branch. FORGE first checks for uncommitted changes and unmerged commits; if
                there are any it will tell you exactly what would be lost and ask again before deleting anything.
              </p>
            )}
          </div>
          <div className="flex justify-end gap-2 border-t border-line px-4 py-3">
            <button className="btn" disabled={d?.busy} onClick={() => setDiscard(null)}>
              Keep mission
            </button>
            {loss ? (
              <button className="btn btn-danger-solid" disabled={d?.busy} onClick={() => void confirmDiscardLoss()}>
                {d?.busy ? 'Discarding…' : 'Permanently discard'}
              </button>
            ) : (
              <button className="btn btn-danger-solid" disabled={d?.busy} onClick={() => void attemptDiscard()}>
                {d?.busy ? 'Checking…' : 'Discard…'}
              </button>
            )}
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
