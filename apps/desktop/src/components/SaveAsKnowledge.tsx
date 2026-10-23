import { useEffect, useState } from 'react';
import { api } from '../lib/api';
import { errMsg } from '../lib/format';
import { firstLine } from '../lib/knowledge';
import { useStore } from '../lib/store';
import type { ActivityEvent } from '../lib/types';
import { Modal } from './Modal';
import { PAGE_KINDS } from './NewPageDialog';

/** Saves an agent message as a PROPOSED knowledge page (never trusted until a person verifies it). */
export function SaveAsKnowledge({ ev, onClose }: { ev: ActivityEvent | null; onClose: () => void }) {
  const notify = useStore((s) => s.notify);
  const [title, setTitle] = useState('');
  const [kind, setKind] = useState('note');
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (ev) { setTitle(firstLine(String(ev.payload?.text ?? ''))); setKind('note'); }
  }, [ev]);

  const save = async () => {
    if (!ev) return;
    setBusy(true);
    try {
      const p = await api.knowledgeSaveFromEvent(ev.missionId, ev.id, title.trim(), kind);
      notify('info', 'Saved as a proposal; verify it in Knowledge.', { label: 'Open it', pageId: p.id });
      onClose();
    } catch (e) {
      notify('error', errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal open={!!ev} onClose={onClose} title="Save as knowledge"
      footer={<><button className="btn" onClick={onClose} disabled={busy}>Cancel</button><button className="btn btn-primary" disabled={busy || !title.trim()} onClick={() => void save()}>Save as proposal</button></>}>
      <p className="m-0 text-muted">Agent text is untrusted. It is saved as a proposed page and attached to missions only after you verify it.</p>
      <div><label className="label" htmlFor="sk-title">Title</label><input id="sk-title" className="input" value={title} onChange={(e) => setTitle(e.target.value)} autoFocus /></div>
      <div><label className="label" htmlFor="sk-kind">Kind</label>
        <select id="sk-kind" className="input" value={kind} onChange={(e) => setKind(e.target.value)}>{PAGE_KINDS.map((k) => <option key={k}>{k}</option>)}</select></div>
    </Modal>
  );
}
