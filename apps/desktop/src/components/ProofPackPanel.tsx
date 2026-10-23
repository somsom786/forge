import { Fragment, useCallback, useEffect, useState, type ReactNode } from 'react';
import { Copy, FileText } from 'lucide-react';
import { api } from '../lib/api';
import { errMsg, localTime } from '../lib/format';
import { useStore } from '../lib/store';
import type { Mission, ProofPack } from '../lib/types';
import { Callout } from './ui';

/** Tiny safe markdown view: headings, lists, tables and fenced code as React nodes. No HTML is ever injected. */
export function SafeMarkdown({ text }: { text: string }) {
  const lines = text.split(/\r?\n/);
  const out: ReactNode[] = [];
  let i = 0;
  while (i < lines.length) {
    const l = lines[i];
    if (l.startsWith('```')) {
      const buf: string[] = [];
      i++;
      while (i < lines.length && !lines[i].startsWith('```')) buf.push(lines[i++]);
      i++;
      out.push(<pre key={out.length} className="overflow-x-auto rounded border border-line bg-bg p-2 text-[12px]">{buf.join('\n')}</pre>);
    } else if (/^#{1,4}\s/.test(l)) {
      const lvl = l.match(/^#+/)![0].length;
      out.push(<div key={out.length} className={`mt-3 font-semibold ${lvl <= 1 ? 'text-[15px]' : lvl === 2 ? 'text-[13.5px] border-b border-line pb-0.5' : 'text-[13px] text-muted'}`}>{l.replace(/^#+\s*/, '')}</div>);
      i++;
    } else if (l.trim().startsWith('|')) {
      const rows: string[][] = [];
      while (i < lines.length && lines[i].trim().startsWith('|')) {
        const cells = lines[i].trim().replace(/^\||\|$/g, '').split('|').map((c) => c.trim());
        if (!cells.every((c) => /^:?-{2,}:?$/.test(c))) rows.push(cells);
        i++;
      }
      out.push(
        <div key={out.length} className="overflow-x-auto">
          <table className="tbl text-[12px]">
            <tbody>
              {rows.map((r, ri) => (
                <tr key={ri} className={ri === 0 ? 'font-medium text-muted' : ''}>{r.map((c, ci) => <td key={ci}>{c}</td>)}</tr>
              ))}
            </tbody>
          </table>
        </div>,
      );
    } else if (/^\s*([-*]|\d+\.)\s/.test(l)) {
      const items: string[] = [];
      while (i < lines.length && /^\s*([-*]|\d+\.)\s/.test(lines[i])) items.push(lines[i++].replace(/^\s*([-*]|\d+\.)\s/, ''));
      out.push(<ul key={out.length} className="m-0 list-disc pl-5">{items.map((t, k) => <li key={k} className="break-words">{t}</li>)}</ul>);
    } else if (l.trim() === '') {
      i++;
    } else {
      const buf: string[] = [];
      while (i < lines.length && lines[i].trim() !== '' && !/^(#{1,4}\s|```|\s*([-*]|\d+\.)\s|\s*\|)/.test(lines[i])) buf.push(lines[i++]);
      if (buf.length === 0) { buf.push(lines[i++]); }
      out.push(<p key={out.length} className="m-0 whitespace-pre-wrap break-words">{buf.join('\n')}</p>);
    }
  }
  return <div className="space-y-2 text-[13px]">{out.map((n, k) => <Fragment key={k}>{n}</Fragment>)}</div>;
}

export function ProofPackPanel({ mission }: { mission: Mission }) {
  const notify = useStore((s) => s.notify);
  const [packs, setPacks] = useState<ProofPack[]>([]);
  const [sel, setSel] = useState<string | null>(null);
  const [raw, setRaw] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const list = await api.proofpackList(mission.id);
      const sorted = [...list].sort((a, b) => b.createdAt.localeCompare(a.createdAt));
      setPacks(sorted);
      setSel((cur) => cur ?? sorted[0]?.id ?? null);
      setError(null);
    } catch (e) {
      setError(errMsg(e));
    }
  }, [mission.id]);
  useEffect(() => { void load(); }, [load]);

  const generate = async () => {
    setBusy(true);
    try {
      const p = await api.proofpackGenerate(mission.id);
      await load();
      setSel(p.id);
      notify('info', 'Proof Pack generated');
    } catch (e) {
      notify('error', `Could not generate Proof Pack: ${errMsg(e)}`);
    } finally {
      setBusy(false);
    }
  };

  const pack = packs.find((p) => p.id === sel);
  const copy = async () => {
    if (!pack) return;
    try {
      await navigator.clipboard.writeText(pack.markdown);
      notify('info', 'Markdown copied');
    } catch (e) {
      notify('error', `Could not copy: ${errMsg(e)}`);
    }
  };

  return (
    <div className="min-h-0 flex-1 space-y-3 overflow-y-auto p-3">
      <div className="text-[12px] text-muted">A Proof Pack records evidence; it is not a guarantee of correctness.</div>
      <div className="flex flex-wrap items-center gap-2">
        <button className="btn btn-primary" disabled={busy} onClick={() => void generate()}><FileText className="size-3.5" aria-hidden /> {busy ? 'Generating…' : 'Generate Proof Pack'}</button>
        {packs.length > 0 && (
          <select className="input !w-auto" aria-label="Previous Proof Packs" value={sel ?? ''} onChange={(e) => setSel(e.target.value)}>
            {packs.map((p) => <option key={p.id} value={p.id}>{localTime(p.createdAt)} · sha256 {p.contentSha256.slice(0, 10)}</option>)}
          </select>
        )}
      </div>
      {error && <Callout tone="bad" title="Could not list Proof Packs"><pre className="whitespace-pre-wrap text-[12px]">{error}</pre></Callout>}
      {packs.length === 0 && !error && <div className="text-muted">No Proof Pack has been generated for this mission.</div>}
      {pack && (
        <div className="space-y-2">
          <div className="flex flex-wrap items-center gap-2 text-[12px] text-muted">
            <span>Generated {localTime(pack.createdAt)}</span>
            <span className="mono">sha256 {pack.contentSha256.slice(0, 16)}</span>
            <button className="btn btn-sm ml-auto" onClick={() => void copy()}><Copy className="size-3.5" aria-hidden /> Copy markdown</button>
            <button className="btn btn-sm" aria-pressed={raw} onClick={() => setRaw(!raw)}>{raw ? 'Formatted view' : 'Raw text'}</button>
          </div>
          {pack.diffPath && <div className="text-[12px] text-muted">Exact diff: <span className="mono break-all text-fg">{pack.diffPath}</span></div>}
          <div className="rounded-md border border-line bg-panel p-3">
            {raw ? <pre className="overflow-x-auto whitespace-pre-wrap break-words text-[12px]">{pack.markdown}</pre> : <SafeMarkdown text={pack.markdown} />}
          </div>
        </div>
      )}
    </div>
  );
}
