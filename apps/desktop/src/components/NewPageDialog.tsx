import { useEffect, useState } from 'react';
import { api } from '../lib/api';
import { errMsg } from '../lib/format';
import { linesOf } from '../lib/knowledge';
import { useStore } from '../lib/store';
import type { KnowledgePage } from '../lib/types';
import { Modal } from './Modal';

export const PAGE_KINDS = ['architecture', 'component', 'decision', 'convention', 'incident', 'failure_mode', 'solution', 'debt', 'experiment', 'mission_record', 'note'];

export function NewPageDialog({ open, projectId, onClose, onCreated }: { open: boolean; projectId: string; onClose: () => void; onCreated: (p: KnowledgePage) => void }) {
  const notify = useStore((s) => s.notify);
  const [title, setTitle] = useState('');
  const [kind, setKind] = useState('note');
  const [body, setBody] = useState('');
  const [files, setFiles] = useState('');
  const [tags, setTags] = useState('');
  const [topic, setTopic] = useState('');
  const [sources, setSources] = useState('');
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (open) { setTitle(''); setKind('note'); setBody(''); setFiles(''); setTags(''); setTopic(''); setSources(''); setBusy(false); }
  }, [open]);

  const submit = async () => {
    setBusy(true);
    try {
      const p = await api.knowledgeCreate(projectId, {
        title: title.trim(), kind, body,
        files: linesOf(files), tags: tags.split(',').map((t) => t.trim()).filter(Boolean),
        topic: topic.trim() || undefined,
        sources: linesOf(sources).map((locator) => ({ kind: 'repo_file' as const, locator })),
      });
      notify('info', 'Page created as proposed. Verify it to attach it to missions.');
      onCreated(p);
      onClose();
    } catch (e) {
      notify('error', errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Modal open={open} onClose={onClose} title="New knowledge page" wide
      footer={<><button className="btn" onClick={onClose} disabled={busy}>Cancel</button><button className="btn btn-primary" disabled={busy || !title.trim() || !body.trim()} onClick={() => void submit()}>Create page</button></>}>
      <div className="grid grid-cols-[1fr_200px] gap-3">
        <div><label className="label" htmlFor="kp-title">Title</label><input id="kp-title" className="input" value={title} onChange={(e) => setTitle(e.target.value)} autoFocus /></div>
        <div><label className="label" htmlFor="kp-kind">Kind</label>
          <select id="kp-kind" className="input" value={kind} onChange={(e) => setKind(e.target.value)}>{PAGE_KINDS.map((k) => <option key={k}>{k}</option>)}</select></div>
      </div>
      <div><label className="label" htmlFor="kp-body">Body (markdown)</label><textarea id="kp-body" className="input mono min-h-[160px] text-[12px]" value={body} onChange={(e) => setBody(e.target.value)} /></div>
      <div className="grid grid-cols-2 gap-3">
        <div><label className="label" htmlFor="kp-files">Related files (one per line)</label><textarea id="kp-files" className="input mono min-h-[56px] text-[12px]" value={files} onChange={(e) => setFiles(e.target.value)} /></div>
        <div><label className="label" htmlFor="kp-src">Sources: repo files it is based on (one per line)</label><textarea id="kp-src" className="input mono min-h-[56px] text-[12px]" value={sources} onChange={(e) => setSources(e.target.value)} /></div>
        <div><label className="label" htmlFor="kp-tags">Tags (comma separated)</label><input id="kp-tags" className="input" value={tags} onChange={(e) => setTags(e.target.value)} /></div>
        <div><label className="label" htmlFor="kp-topic">Topic (optional)</label><input id="kp-topic" className="input" value={topic} onChange={(e) => setTopic(e.target.value)} /></div>
      </div>
      <p className="m-0 text-muted">Saved as proposed. It is only attached to missions after you verify it.</p>
    </Modal>
  );
}
