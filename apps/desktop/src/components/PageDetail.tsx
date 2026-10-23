import { useCallback, useEffect, useMemo, useState } from 'react';
import { ExternalLink, Pencil } from 'lucide-react';
import { api } from '../lib/api';
import { errMsg, localTime, relTime } from '../lib/format';
import {
  classifyVerifyError, EDITED_NOTE, lineDiff, shortHash, sourceNeedsAttention, sourceStateTone, stateChip,
} from '../lib/knowledge';
import { useStore } from '../lib/store';
import type { KnowledgePage, KnowledgePageDetail, KnowledgeRevision } from '../lib/types';
import { Modal } from './Modal';
import { SafeMarkdown } from './ProofPackPanel';
import { Badge, Callout } from './ui';

type Dlg = null | 'verify' | 'dispute' | 'archive' | 'restore' | 'supersede' | 'forget' | 'edit';

export function StateChips({ page }: { page: KnowledgePage }) {
  const c = stateChip(page);
  return (
    <>
      <Badge tone={c.tone}>{c.label}</Badge>
      {c.editedSinceVerified && <Badge tone="warn">{EDITED_NOTE}</Badge>}
    </>
  );
}

function stripFrontmatter(t: string): string {
  return t.replace(/^---\r?\n[\s\S]*?\r?\n---\r?\n?/, '');
}

interface Props {
  pageId: string;
  /** Other pages in the project, for the supersede picker and link titles. */
  pages: KnowledgePage[];
  obsidianInstalled: boolean;
  onOpenPage: (id: string) => void;
  onChanged: () => void;
  onForgotten: () => void;
}

export function PageDetail({ pageId, pages, obsidianInstalled, onOpenPage, onChanged, onForgotten }: Props) {
  const notify = useStore((s) => s.notify);
  const [d, setD] = useState<KnowledgePageDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [raw, setRaw] = useState(false);
  const [dlg, setDlg] = useState<Dlg>(null);
  const [note, setNote] = useState('');
  const [other, setOther] = useState('');
  const [edit, setEdit] = useState('');
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    try {
      const r = await api.knowledgeGet(pageId);
      setD(r);
      setError(null);
      return r;
    } catch (e) {
      setError(errMsg(e));
      return null;
    }
  }, [pageId]);
  useEffect(() => {
    setD(null);
    void load();
  }, [load]);

  const title = (id: string | null) => (id ? pages.find((p) => p.id === id)?.title ?? id.slice(0, 8) : '');
  const close = () => { setDlg(null); setNote(''); setOther(''); };
  const run = async (fn: () => Promise<unknown>, ok: string) => {
    setBusy(true);
    try {
      await fn();
      notify('info', ok);
      close();
      await load();
      onChanged();
    } catch (e) {
      notify('error', errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  if (error) return <div className="p-4"><Callout tone="bad" title="Could not load the page"><pre className="whitespace-pre-wrap text-[12px]">{error}</pre></Callout></div>;
  if (!d) return <div className="p-4 text-muted">Loading…</div>;
  const p = d.page;
  const chip = stateChip(p);
  const candidates = pages.filter((x) => x.id !== p.id && (x.effectiveState === 'verified' || x.effectiveState === 'proposed'));

  const verify = async () => {
    setBusy(true);
    try {
      await api.knowledgeVerify(p.id, p.currentSha256, note.trim() || undefined);
      notify('info', 'Page verified');
      close();
      await load();
      onChanged();
    } catch (e) {
      const o = classifyVerifyError(errMsg(e));
      if (o.kind === 'reload') {
        close();
        await load();
        onChanged();
        notify('warn', o.message);
      } else notify('error', o.kind === 'error' ? o.message : errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  const btn = (label: string, which: Dlg, disabled?: boolean, danger?: boolean) => (
    <button className={`btn btn-sm ${danger ? 'btn-danger' : ''}`} disabled={disabled} onClick={() => { setDlg(which); if (which === 'edit') setEdit(d.text); }}>{label}</button>
  );

  return (
    <div className="min-h-0 flex-1 space-y-4 overflow-y-auto p-4" aria-label="Page detail">
      <header>
        <h2 className="m-0 break-words text-[16px] font-semibold">{p.title}</h2>
        <div className="mt-1.5 flex flex-wrap items-center gap-1.5">
          <StateChips page={p} />
          <Badge>author: {p.author}</Badge>
          <Badge>{p.origin === 'imported' ? 'imported' : 'created in FORGE'}</Badge>
          <Badge>{p.kind}</Badge>
          {p.rootKind === 'vault' && <Badge tone="run">in vault</Badge>}
          {p.missing && <Badge tone="bad">file missing</Badge>}
        </div>
        <div className="mono mt-1.5 break-all text-[11.5px] text-muted">{p.rootKind}/{p.relPath} · sha256 {shortHash(p.currentSha256)}</div>
        {(p.topic || p.files.length > 0 || p.tags.length > 0) && (
          <dl className="m-0 mt-1.5 grid grid-cols-[60px_1fr] gap-x-2 gap-y-0.5 text-[12px]">
            {p.topic && <><dt className="text-faint">Topic</dt><dd className="m-0">{p.topic}</dd></>}
            {p.files.length > 0 && <><dt className="text-faint">Files</dt><dd className="mono m-0 break-all">{p.files.join(', ')}</dd></>}
            {p.tags.length > 0 && <><dt className="text-faint">Tags</dt><dd className="m-0">{p.tags.join(', ')}</dd></>}
          </dl>
        )}
        {p.verifiedAt && chip.label === 'verified' && (
          <div className="mt-1.5 text-[12px] text-muted">Verified {localTime(p.verifiedAt)}{p.verifiedNote ? ` — ${p.verifiedNote}` : ''}</div>
        )}
        {chip.editedSinceVerified && <div className="mt-1.5 text-[12px] text-warn">This page was verified earlier but its text has changed since. It is treated as proposed until a person verifies it again.</div>}
      </header>

      {p.warnings.length > 0 && (
        <Callout tone="warn" title="Parse warnings"><ul className="m-0 list-disc pl-4">{p.warnings.map((w, i) => <li key={i}>{w}</li>)}</ul></Callout>
      )}

      <div className="flex flex-wrap gap-1.5" role="group" aria-label="Trust actions">
        {btn('Verify…', 'verify', chip.label === 'verified' || p.missing)}
        {btn('Dispute…', 'dispute', chip.label === 'disputed')}
        {btn('Archive…', 'archive', chip.label === 'archived')}
        {btn('Restore…', 'restore', chip.label === 'proposed' || chip.label === 'verified')}
        {btn('Supersede…', 'supersede', candidates.length === 0)}
        {btn('Forget…', 'forget', false, true)}
        <button className="btn btn-sm" disabled={!obsidianInstalled} title={obsidianInstalled ? undefined : 'Obsidian is not installed on this machine'} onClick={() => void run(() => api.knowledgeOpenInObsidian(p.id), 'Opened in Obsidian')}>
          <ExternalLink className="size-3 shrink-0" aria-hidden /> Open in Obsidian
        </button>
        <button className="btn btn-sm" onClick={() => { setDlg('edit'); setEdit(d.text); }}><Pencil className="size-3 shrink-0" aria-hidden /> Edit…</button>
      </div>
      {!d.writable && <div className="text-[12px] text-warn">This page is in a read-only vault: you can propose revisions but they cannot be applied until you grant write access.</div>}

      <section aria-label="Body">
        <div className="mb-1 flex items-center gap-2">
          <h3 className="m-0 text-[11px] font-medium uppercase tracking-wide text-muted">Body</h3>
          <button className="btn btn-sm ml-auto" aria-pressed={raw} onClick={() => setRaw(!raw)}>{raw ? 'Rendered' : 'Raw'}</button>
        </div>
        <div className="rounded-md border border-line bg-panel p-3">
          {raw ? <pre className="overflow-x-auto whitespace-pre-wrap break-words text-[12px]">{d.text}</pre> : <SafeMarkdown text={stripFrontmatter(d.text)} />}
        </div>
      </section>

      <section aria-label="Provenance">
        <h3 className="m-0 mb-1 text-[11px] font-medium uppercase tracking-wide text-muted">Provenance ({d.sources.length})</h3>
        {d.sources.length === 0 && <div className="text-muted">No recorded sources.</div>}
        <ul className="m-0 list-none space-y-1.5 p-0">
          {d.sources.map((s) => (
            <li key={s.id} className="rounded-md border border-line p-2">
              <div className="flex flex-wrap items-center gap-2">
                <Badge>{s.kind}</Badge>
                <span className="mono min-w-0 break-all text-[12px]">{s.locator}</span>
                <Badge tone={sourceStateTone(s.state)}>{s.state}</Badge>
              </div>
              {sourceNeedsAttention(s) && <div className="mt-1 text-[12px] text-warn">source changed since this was written{s.state === 'missing' ? ' (it no longer exists)' : ''}</div>}
              <div className="mt-0.5 text-[11px] text-faint">captured {localTime(s.capturedAt)} by {s.extractor}</div>
              {s.snapshot && (
                <details className="mt-1">
                  <summary className="cursor-pointer text-[12px] text-muted hover:text-fg">Snapshot (redacted)</summary>
                  <pre className="mt-1 max-h-48 overflow-auto whitespace-pre-wrap break-all rounded border border-line bg-bg p-2 text-[12px]">{s.snapshot}</pre>
                </details>
              )}
            </li>
          ))}
        </ul>
      </section>

      <section aria-label="Review history">
        <h3 className="m-0 mb-1 text-[11px] font-medium uppercase tracking-wide text-muted">Review history ({d.reviews.length})</h3>
        {d.reviews.length === 0 && <div className="text-muted">Never reviewed.</div>}
        <ul className="m-0 list-none space-y-1 p-0">
          {d.reviews.map((r, i) => (
            <li key={i} className="text-[12px]">
              <span className="font-medium">{r.action}</span> by {r.reviewer} · {localTime(r.at)}
              {r.revisionSha256 && <span className="mono text-faint"> · {shortHash(r.revisionSha256)}</span>}
              {r.note && <div className="whitespace-pre-wrap break-words text-muted">{r.note}</div>}
            </li>
          ))}
        </ul>
      </section>

      <section aria-label="Links">
        <h3 className="m-0 mb-1 text-[11px] font-medium uppercase tracking-wide text-muted">Links ({d.links.length}) and backlinks ({d.backlinks.length})</h3>
        <ul className="m-0 list-none space-y-0.5 p-0">
          {d.links.map((l, i) => (
            <li key={i} className="text-[12px]">
              {l.kind === 'file' ? (
                <><span className="text-faint">file </span><span className="mono break-all">{l.target}</span></>
              ) : l.toPage ? (
                <><span className="text-faint">link </span><button className="text-accent hover:underline" onClick={() => onOpenPage(l.toPage!)}>{title(l.toPage) || l.target}</button></>
              ) : (
                <><span className="text-faint">link </span><span>{l.target}</span> <span className="text-faint">(unresolved)</span></>
              )}
              {l.ambiguous && <span className="ml-1.5 text-warn">matches more than one page</span>}
            </li>
          ))}
        </ul>
        {d.backlinks.length > 0 && (
          <div className="mt-1.5 text-[12px]"><span className="text-faint">Linked from: </span>
            {d.backlinks.map((b) => <button key={b.id} className="mr-2 text-accent hover:underline" onClick={() => onOpenPage(b.id)}>{b.title}</button>)}
          </div>
        )}
      </section>

      <section aria-label="Revisions">
        <h3 className="m-0 mb-1 text-[11px] font-medium uppercase tracking-wide text-muted">Revisions ({d.revisions.length})</h3>
        {d.revisions.length === 0 && <div className="text-muted">No proposed revisions.</div>}
        <div className="space-y-2">
          {d.revisions.map((r) => <RevisionCard key={r.id} r={r} current={d.text} writable={d.writable} busy={busy} onApply={() => void run(() => api.knowledgeApplyRevision(r.id), 'Revision applied')} onReject={() => void run(() => api.knowledgeRejectRevision(r.id), 'Revision rejected')} />)}
        </div>
      </section>

      <Modal open={dlg === 'verify'} onClose={close} title="Verify this exact text" wide
        footer={<><button className="btn" onClick={close} disabled={busy}>Cancel</button><button className="btn btn-primary" disabled={busy} onClick={() => void verify()}>Verify</button></>}>
        <p className="m-0 text-muted">Verification is bound to this exact content (sha256 <span className="mono text-fg">{shortHash(p.currentSha256)}</span>). Only verified pages are attached to missions automatically. If the page changes before you confirm, verification is refused.</p>
        <pre className="max-h-72 overflow-auto whitespace-pre-wrap break-words rounded border border-line bg-bg p-2 text-[12px]">{d.text}</pre>
        <label className="label" htmlFor="vnote">Note (optional)</label>
        <input id="vnote" className="input" value={note} onChange={(e) => setNote(e.target.value)} />
      </Modal>

      <Modal open={dlg === 'dispute' || dlg === 'archive' || dlg === 'restore'} onClose={close}
        title={dlg === 'dispute' ? 'Dispute this page?' : dlg === 'archive' ? 'Archive this page?' : 'Restore this page?'}
        footer={<><button className="btn" onClick={close} disabled={busy}>Cancel</button><button className="btn btn-primary" disabled={busy} onClick={() => dlg && dlg !== 'verify' && void run(() => api.knowledgeSetStatus(p.id, dlg as 'dispute' | 'archive' | 'restore', note.trim() || undefined), `Page ${dlg === 'dispute' ? 'disputed' : dlg === 'archive' ? 'archived' : 'restored'}`)}>{dlg === 'dispute' ? 'Dispute' : dlg === 'archive' ? 'Archive' : 'Restore'}</button></>}>
        <p className="m-0">
          {dlg === 'dispute' && 'A disputed page is no longer attached to missions. It stays in the index so the disagreement is visible.'}
          {dlg === 'archive' && 'An archived page is no longer attached to missions and is hidden from default views. Nothing is deleted.'}
          {dlg === 'restore' && 'Restoring returns the page to Proposed. It must be verified again before it is attached to missions.'}
        </p>
        <label className="label" htmlFor="snote">Note (optional)</label>
        <input id="snote" className="input" value={note} onChange={(e) => setNote(e.target.value)} />
      </Modal>

      <Modal open={dlg === 'supersede'} onClose={close} title="Supersede with another page"
        footer={<><button className="btn" onClick={close} disabled={busy}>Cancel</button><button className="btn btn-primary" disabled={busy || !other} onClick={() => void run(() => api.knowledgeSupersede(p.id, other, note.trim() || undefined), 'Page superseded')}>Supersede</button></>}>
        <p className="m-0">This page will be marked superseded and stop being attached to missions. The replacement is NOT automatically verified: it keeps its own state.</p>
        <label className="label" htmlFor="sup">Replacement page</label>
        <select id="sup" className="input" value={other} onChange={(e) => setOther(e.target.value)}>
          <option value="">Choose a page…</option>
          {candidates.map((c) => <option key={c.id} value={c.id}>{c.title} ({c.effectiveState})</option>)}
        </select>
        <label className="label" htmlFor="supn">Note (optional)</label>
        <input id="supn" className="input" value={note} onChange={(e) => setNote(e.target.value)} />
      </Modal>

      <Modal open={dlg === 'forget'} onClose={close} tone="bad" title="Forget this page permanently?"
        footer={<><button className="btn" onClick={close} disabled={busy}>Keep page</button><button className="btn btn-danger-solid" disabled={busy} onClick={() => void (async () => { setBusy(true); try { await api.knowledgeForget(p.id); notify('info', 'Page forgotten'); close(); onChanged(); onForgotten(); } catch (e) { notify('error', errMsg(e)); } finally { setBusy(false); } })()}>Forget permanently</button></>}>
        <p className="m-0">This is irreversible. It will delete:</p>
        <ul className="m-0 list-disc pl-5">
          <li>the page file{d.writable ? '' : ' (not in a read-only vault: the file stays on disk, only FORGE\'s records are removed)'},</li>
          <li>its index rows, links and revisions,</li>
          <li>the text it left in past context snapshots of missions.</li>
        </ul>
      </Modal>

      <Modal open={dlg === 'edit'} onClose={close} wide title="Edit page text"
        footer={<><button className="btn" onClick={close} disabled={busy}>Cancel</button><button className="btn btn-primary" disabled={busy || edit === d.text} onClick={() => void run(() => api.knowledgeProposeRevision(p.id, edit, note.trim() || undefined), 'Revision proposed: review it below, then apply')}>Propose revision</button></>}>
        <p className="m-0 text-muted">Your edit becomes a revision you can review and apply. FORGE never overwrites the page silently{d.writable ? '' : ', and this vault is read-only so the revision can only be proposed'}.</p>
        <textarea className="input mono min-h-[280px] text-[12px]" aria-label="Page text" value={edit} onChange={(e) => setEdit(e.target.value)} />
        <label className="label" htmlFor="enote">Note (optional)</label>
        <input id="enote" className="input" value={note} onChange={(e) => setNote(e.target.value)} />
      </Modal>
    </div>
  );
}

function RevisionCard({ r, current, writable, busy, onApply, onReject }: { r: KnowledgeRevision; current: string; writable: boolean; busy: boolean; onApply: () => void; onReject: () => void }) {
  const ops = useMemo(() => lineDiff(current, r.text), [current, r.text]);
  const changed = ops.filter((o) => o.type !== 'ctx').length;
  return (
    <div className="rounded-md border border-line p-2">
      <div className="flex flex-wrap items-center gap-2">
        <Badge tone={r.state === 'proposed' ? 'warn' : r.state === 'applied' ? 'ok' : r.state === 'conflict' ? 'bad' : 'idle'}>{r.state}</Badge>
        <span className="text-[12px] text-muted">by {r.author} · {relTime(r.createdAt)}</span>
        <span className="text-[12px] text-faint">{changed} changed line{changed === 1 ? '' : 's'}</span>
        {r.state === 'proposed' && (
          <span className="ml-auto flex gap-1.5">
            <button className="btn btn-sm btn-primary" disabled={!writable || busy} title={writable ? undefined : 'This vault is read-only. Grant write access to apply revisions.'} onClick={onApply}>Apply</button>
            <button className="btn btn-sm" disabled={busy} onClick={onReject}>Reject</button>
          </span>
        )}
      </div>
      {r.note && <div className="mt-0.5 text-[12px] text-muted">{r.note}</div>}
      {r.state === 'conflict' && <div className="mt-1 text-[12px] text-warn">The page was edited after this was proposed; nothing was overwritten.</div>}
      {r.state === 'proposed' && !writable && <div className="mt-1 text-[12px] text-warn">Cannot apply: the vault is read-only.</div>}
      <details className="mt-1" open={r.state === 'proposed'}>
        <summary className="cursor-pointer text-[12px] text-muted hover:text-fg">Changes against the current text</summary>
        <div className="mt-1 max-h-72 overflow-auto rounded border border-line bg-bg">
          {ops.map((o, i) => (
            <div key={i} className={`flex ${o.type === 'add' ? 'bg-[#13271a]' : o.type === 'del' ? 'bg-[#2d1618]' : ''}`}>
              <span className={`w-5 shrink-0 select-none text-center ${o.type === 'add' ? 'text-ok' : o.type === 'del' ? 'text-bad' : 'text-faint'}`}>{o.type === 'add' ? '+' : o.type === 'del' ? '−' : ' '}</span>
              <pre className="whitespace-pre-wrap break-words pr-2 text-[12px]">{o.text || ' '}</pre>
            </div>
          ))}
        </div>
      </details>
    </div>
  );
}
