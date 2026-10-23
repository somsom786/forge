import { memo, useEffect, useMemo, useState } from 'react';
import { ChevronDown, ChevronRight } from 'lucide-react';
import { parseDiff, toSideBySide, type DiffFile, type DiffHunk, type DiffLine } from '../lib/diff';
import { readStore, writeStore } from '../lib/storage';
import { Callout } from './ui';

export type DiffMode = 'unified' | 'split';
const MAX_LINES_PER_FILE = 1500;

const LINE_BG: Record<string, string> = {
  add: 'bg-[#13271a]',
  del: 'bg-[#2d1618]',
  ctx: '',
};
const NO_BG: Record<string, string> = { add: 'bg-[#183221] text-ok/80', del: 'bg-[#3a1c1f] text-bad/80', ctx: 'text-faint' };

function Marker({ type }: { type: string }) {
  return <span className={`w-4 shrink-0 select-none text-center ${type === 'add' ? 'text-ok' : type === 'del' ? 'text-bad' : 'text-faint'}`}>{type === 'add' ? '+' : type === 'del' ? '−' : ' '}</span>;
}

function UnifiedHunk({ h }: { h: DiffHunk }) {
  return (
    <div className="w-max min-w-full">
      <div className="mono bg-accent-dim/60 px-3 py-0.5 text-[11.5px] text-accent">
        {h.header}
      </div>
      {h.lines.map((l, i) => (
        <div key={i} className={`flex ${LINE_BG[l.type]}`}>
          <span className={`mono w-11 shrink-0 select-none px-1 text-right text-[11.5px] ${NO_BG[l.type]}`}>{l.oldNo ?? ''}</span>
          <span className={`mono w-11 shrink-0 select-none px-1 text-right text-[11.5px] ${NO_BG[l.type]}`}>{l.newNo ?? ''}</span>
          <Marker type={l.type} />
          <pre className="pr-4 text-[12.5px] leading-[1.5] whitespace-pre">{l.text || ' '}{l.noNewline && <span className="ml-2 select-none text-[11px] italic text-faint">no newline at end of file</span>}</pre>
        </div>
      ))}
    </div>
  );
}

function Half({ l, side }: { l: DiffLine | null; side: 'left' | 'right' }) {
  if (!l) return <div className="flex min-w-[360px] flex-1 bg-raised/40"><span className="w-11 shrink-0" /></div>;
  return (
    <div className={`flex min-w-[360px] flex-1 ${LINE_BG[l.type]}`}>
      <span className={`mono w-11 shrink-0 select-none px-1 text-right text-[11.5px] ${NO_BG[l.type]}`}>{(side === 'left' ? l.oldNo : l.newNo) ?? ''}</span>
      <Marker type={l.type} />
      <pre className="pr-3 text-[12.5px] leading-[1.5] whitespace-pre">{l.text || ' '}</pre>
    </div>
  );
}

function SplitHunk({ h }: { h: DiffHunk }) {
  const rows = useMemo(() => toSideBySide(h), [h]);
  return (
    <div className="w-max min-w-full">
      <div className="mono bg-accent-dim/60 px-3 py-0.5 text-[11.5px] text-accent">{h.header}</div>
      {rows.map((r, i) => (
        <div key={i} className="flex">
          <Half l={r.left} side="left" />
          <div className="w-px shrink-0 bg-line" />
          <Half l={r.right} side="right" />
        </div>
      ))}
    </div>
  );
}

const STATUS_LABEL: Record<string, string> = { added: 'added', deleted: 'deleted', renamed: 'renamed', copied: 'copied', modified: 'modified' };
const STATUS_COLOR: Record<string, string> = { added: 'text-ok', deleted: 'text-bad', renamed: 'text-accent', copied: 'text-accent', modified: 'text-muted' };

const FileSection = memo(function FileSection({ f, mode, collapsed, onToggle }: { f: DiffFile; mode: DiffMode; collapsed: boolean; onToggle: () => void }) {
  const [showAll, setShowAll] = useState(false);
  const total = f.hunks.reduce((n, h) => n + h.lines.length, 0);
  const limited = !showAll && total > MAX_LINES_PER_FILE;
  let budget = limited ? MAX_LINES_PER_FILE : Infinity;
  const hunks: DiffHunk[] = [];
  for (const h of f.hunks) {
    if (budget <= 0) break;
    if (h.lines.length > budget) {
      hunks.push({ ...h, lines: h.lines.slice(0, budget) });
      budget = 0;
    } else {
      hunks.push(h);
      budget -= h.lines.length;
    }
  }
  return (
    <section data-diff-file={f.path} className="border-b border-line">
      <button
        className="sticky top-0 z-[2] flex w-full items-center gap-2 border-b border-line bg-raised px-3 py-1.5 text-left hover:bg-hover"
        onClick={onToggle}
        aria-expanded={!collapsed}
      >
        {collapsed ? <ChevronRight className="size-3.5 shrink-0" aria-hidden /> : <ChevronDown className="size-3.5 shrink-0" aria-hidden />}
        <span className="mono truncate text-[12.5px] text-fg">
          {f.status === 'renamed' || f.status === 'copied' ? `${f.oldPath} → ${f.newPath}` : f.path}
        </span>
        <span className={`text-[11px] ${STATUS_COLOR[f.status]}`}>{STATUS_LABEL[f.status]}</span>
        {f.binary && <span className="text-[11px] text-warn">binary</span>}
        <span className="mono ml-auto shrink-0 text-[11.5px]">
          <span className="text-ok">+{f.added}</span> <span className="text-bad">−{f.removed}</span>
        </span>
      </button>
      {!collapsed && (
        <div className="overflow-x-auto bg-bg" tabIndex={0} role="region" aria-label={`Diff of ${f.path}`}>
          {f.binary && <div className="px-3 py-2 text-muted">Binary file: no textual diff available.</div>}
          {f.status === 'renamed' && (
            <div className="px-3 py-1.5 text-muted">
              Renamed from <span className="mono text-fg">{f.oldPath}</span>
              {f.similarity !== null && ` (${f.similarity}% similar)`}
              {f.hunks.length === 0 && !f.binary && ' with no content changes'}
            </div>
          )}
          {f.modeChange && <div className="px-3 py-1.5 text-muted">File mode changed: <span className="mono text-fg">{f.modeChange}</span></div>}
          {f.hunks.length === 0 && !f.binary && f.status !== 'renamed' && !f.modeChange && (
            <div className="px-3 py-2 text-muted">No textual changes in this diff section.</div>
          )}
          {hunks.map((h, i) => (mode === 'unified' ? <UnifiedHunk key={i} h={h} /> : <SplitHunk key={i} h={h} />))}
          {limited && (
            <div className="px-3 py-2">
              <button className="btn btn-sm" onClick={() => setShowAll(true)}>
                Show all {total.toLocaleString()} lines
              </button>
            </div>
          )}
        </div>
      )}
    </section>
  );
});

interface Props {
  diff: string;
  truncated: boolean;
  /** Request to scroll to (and expand) a file. The nonce lets the same file be requested twice. */
  focus?: { path: string; nonce: number } | null;
}

export function DiffViewer({ diff, truncated, focus }: Props) {
  const files = useMemo(() => parseDiff(diff), [diff]);
  const [mode, setMode] = useState<DiffMode>(() => (readStore('forge.diffMode') === 'split' ? 'split' : 'unified'));
  const [collapsed, setCollapsed] = useState<Record<string, boolean>>({});

  const setModePersist = (m: DiffMode) => {
    setMode(m);
    writeStore('forge.diffMode', m);
  };

  useEffect(() => {
    if (!focus) return;
    setCollapsed((c) => (c[focus.path] ? { ...c, [focus.path]: false } : c));
    const t = window.setTimeout(() => {
      const nodes = document.querySelectorAll<HTMLElement>('[data-diff-file]');
      for (const n of nodes) {
        if (n.dataset.diffFile === focus.path) {
          n.scrollIntoView({ block: 'start', behavior: 'smooth' });
          break;
        }
      }
    }, 0);
    return () => window.clearTimeout(t);
  }, [focus]);

  const allCollapsed = files.length > 0 && files.every((f) => collapsed[f.path]);

  return (
    <div className="flex min-w-0 flex-col">
      <div className="flex items-center gap-2 border-b border-line px-3 py-1.5">
        <div className="inline-flex overflow-hidden rounded-[5px] border border-line-strong" role="group" aria-label="Diff layout">
          {(['unified', 'split'] as const).map((m) => (
            <button
              key={m}
              className={`h-6 px-2.5 text-[12px] ${mode === m ? 'bg-accent-dim text-fg' : 'bg-raised text-muted hover:text-fg'}`}
              aria-pressed={mode === m}
              onClick={() => setModePersist(m)}
            >
              {m === 'unified' ? 'Unified' : 'Side by side'}
            </button>
          ))}
        </div>
        <button
          className="btn btn-sm btn-ghost"
          disabled={files.length === 0}
          onClick={() => setCollapsed(allCollapsed ? {} : Object.fromEntries(files.map((f) => [f.path, true])))}
        >
          {allCollapsed ? 'Expand all' : 'Collapse all'}
        </button>
        <span className="ml-auto text-[12px] text-muted">{files.length} file{files.length === 1 ? '' : 's'} in diff</span>
      </div>
      {truncated && (
        <div className="p-2">
          <Callout tone="warn" title="Diff truncated">
            The core cut this diff off at its size limit. Open the worktree in your editor to see everything.
          </Callout>
        </div>
      )}
      {files.length === 0 && <div className="px-3 py-6 text-center text-muted">{diff.trim() ? 'The diff could not be parsed into files.' : 'No diff output.'}</div>}
      {files.map((f) => (
        <FileSection
          key={f.path + (f.oldPath ?? '')}
          f={f}
          mode={mode}
          collapsed={!!collapsed[f.path]}
          onToggle={() => setCollapsed((c) => ({ ...c, [f.path]: !c[f.path] }))}
        />
      ))}
    </div>
  );
}
