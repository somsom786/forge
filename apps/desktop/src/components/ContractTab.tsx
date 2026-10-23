import { useEffect, useState } from 'react';
import { api } from '../lib/api';
import { errMsg } from '../lib/format';
import { useStore } from '../lib/store';
import type { Contract, Mission } from '../lib/types';
import { normalizeContract } from '../lib/verification';
import { ContractEditor } from './ContractEditor';
import { Callout } from './ui';

export function ContractTab({ mission }: { mission: Mission }) {
  const notify = useStore((s) => s.notify);
  const upsert = useStore((s) => s.upsertMission);
  const [contract, setContract] = useState<Contract | null>(null);
  const [saved, setSaved] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const finished = ['merged', 'rejected', 'cancelled'].includes(mission.state);

  useEffect(() => {
    let cancelled = false;
    api.contractGet(mission.id).then(
      (c) => {
        if (!cancelled) {
          setContract(c);
          setSaved(JSON.stringify(c));
        }
      },
      (e) => !cancelled && setError(errMsg(e)),
    );
    return () => {
      cancelled = true;
    };
  }, [mission.id]);

  const dirty = contract !== null && JSON.stringify(contract) !== saved;

  const save = async () => {
    if (!contract) return;
    setBusy(true);
    try {
      const n = normalizeContract(contract);
      const m = await api.contractSet(mission.id, n);
      upsert(m);
      setContract(n);
      setSaved(JSON.stringify(n));
      notify('info', 'Contract saved. Evidence is re-evaluated against it.');
    } catch (e) {
      notify('error', `Could not save contract: ${errMsg(e)}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="min-h-0 flex-1 space-y-3 overflow-y-auto p-3">
      {finished && <Callout tone="info">This mission is finished, so its contract can no longer be edited.</Callout>}
      {error && <Callout tone="bad" title="Could not load the contract"><pre className="whitespace-pre-wrap break-words text-[12px]">{error}</pre></Callout>}
      {contract && (
        <>
          <ContractEditor value={contract} onChange={setContract} disabled={finished} projectId={mission.projectId} />
          {!finished && (
            <div className="sticky bottom-0 flex items-center gap-2 border-t border-line bg-panel py-2">
              <button className="btn btn-primary" disabled={!dirty || busy} onClick={() => void save()}>Save contract</button>
              {dirty && <button className="btn" onClick={() => setContract(JSON.parse(saved))}>Discard edits</button>}
              {dirty && <span className="text-[12px] text-warn">Unsaved changes</span>}
            </div>
          )}
        </>
      )}
    </div>
  );
}
