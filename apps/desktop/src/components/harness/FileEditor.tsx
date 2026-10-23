import { useCallback, useEffect, useState } from 'react';
import { api } from '../../lib/api';
import { errMsg } from '../../lib/format';
import { shortHash } from '../../lib/knowledge';
import { useStore } from '../../lib/store';
import type { HarnessFile, HarnessPreview } from '../../lib/types';
import { Callout } from '../ui';
import { PreviewPanel } from './PreviewPanel';

interface Props {
  projectId: string;
  rel: string;
  editable: boolean;
  /** Text to seed a not-yet-existing file with. */
  starter?: string;
  onSaved: () => void;
}

export function FileEditor({ projectId, rel, editable, starter, onSaved }: Props) {
  const notify = useStore((s) => s.notify);
  const [file, setFile] = useState<HarnessFile | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [text, setText] = useState('');
  const [mode, setMode] = useState<'view' | 'edit' | 'review'>('view');
  const [preview, setPreview] = useState<HarnessPreview | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    try {
      const f = await api.harnessRead(projectId, rel);
      setFile(f);
      setError(null);
      setText(!f.exists && starter ? starter : f.text);
      setMode(!f.exists && starter ? 'edit' : 'view');
      setPreview(null);
    } catch (e) {
      setError(errMsg(e));
    }
  }, [projectId, rel, starter]);
  useEffect(() => { setFile(null); void load(); }, [load]);

  if (error) return <Callout tone="bad" title="Could not read the file"><pre className="whitespace-pre-wrap text-[12px]">{error}</pre></Callout>;
  if (!file) return <div className="text-muted">Loading…</div>;

  const review = async () => {
    setBusy(true);
    try { setPreview(await api.harnessPreview(projectId, rel, text)); setMode('review'); } catch (e) { notify('error', errMsg(e)); } finally { setBusy(false); }
  };

  return (
    <div className="space-y-3">
      {!file.exists && <div className="text-[12px] text-accent">This file does not exist yet; saving creates it.</div>}
      {mode === 'view' && (
        <>
          <pre className="max-h-[50vh] overflow-auto whitespace-pre-wrap break-words rounded-md border border-line bg-panel p-3 text-[12px]" aria-label="File content">{file.text || '(empty file)'}</pre>
          <div className="flex items-center gap-2">
            {editable && file.editable ? <button className="btn btn-primary" onClick={() => setMode('edit')}>Edit</button> : <span className="text-[12px] text-muted">This file cannot be edited from FORGE.</span>}
            <span className="mono text-[11px] text-faint">{file.bytes} bytes · sha256 {shortHash(file.sha256)}</span>
          </div>
        </>
      )}
      {mode === 'edit' && (
        <>
          <label className="label" htmlFor="he-text">Editing {rel}</label>
          <textarea id="he-text" aria-label="Edit file content" className="input mono min-h-[300px] text-[12px]" spellCheck={false} value={text} onChange={(e) => setText(e.target.value)} />
          <div className="flex gap-2">
            <button className="btn btn-primary" disabled={busy || (file.exists && text === file.text)} onClick={() => void review()}>Review changes</button>
            <button className="btn" onClick={() => { setText(file.text); setMode('view'); }}>Cancel</button>
          </div>
        </>
      )}
      {mode === 'review' && preview && (
        <PreviewPanel
          projectId={projectId}
          preview={preview}
          expectedSha256={file.sha256}
          onApplied={() => { onSaved(); void load(); }}
          onCancel={() => setMode('edit')}
          onReload={() => void load()}
        />
      )}
    </div>
  );
}
