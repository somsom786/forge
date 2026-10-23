import { useState } from 'react';
import { NEW_FILE_TYPES, newFilePath, starterText, type NewFileType } from '../../lib/harness';
import { Modal } from '../Modal';

export function NewFileDialog({ onClose, onChoose, initialType }: { onClose: () => void; onChoose: (rel: string, starter: string) => void; initialType?: NewFileType }) {
  const [type, setType] = useState<NewFileType>(initialType ?? 'CLAUDE.md');
  const [name, setName] = useState('');
  const spec = NEW_FILE_TYPES.find((t) => t.type === type)!;
  const res = newFilePath(type, name);
  return (
    <Modal open onClose={onClose} title="New harness file"
      footer={<><button className="btn" onClick={onClose}>Cancel</button><button className="btn btn-primary" disabled={!res.ok} onClick={() => res.ok && onChoose(res.rel, starterText(type, name))}>Continue</button></>}>
      <div>
        <label className="label" htmlFor="nf-type">Type</label>
        <select id="nf-type" className="input" value={type} onChange={(e) => setType(e.target.value as NewFileType)}>
          {NEW_FILE_TYPES.map((t) => <option key={t.type} value={t.type}>{t.label}</option>)}
        </select>
      </div>
      {spec.needsName && (
        <div>
          <label className="label" htmlFor="nf-name">Name</label>
          <input id="nf-name" className="input mono" value={name} onChange={(e) => setName(e.target.value)} autoFocus />
        </div>
      )}
      <div className="text-[12px]">{res.ok ? <>Will create <span className="mono break-all">{res.rel}</span>. You review the content before anything is written.</> : name || !spec.needsName ? <span className="text-bad">{res.error}</span> : null}</div>
    </Modal>
  );
}
