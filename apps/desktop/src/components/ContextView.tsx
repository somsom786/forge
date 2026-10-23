import { useState } from 'react';
import { localTime } from '../lib/format';
import { kindShort, relevantCodeSummary } from '../lib/codemap';
import { contextTrustChip, shortHash, sourceNeedsAttention, sourceStateTone } from '../lib/knowledge';
import type { ContextItem, ContextOmitted, RelevantCode } from '../lib/types';
import { Badge } from './ui';

export interface ContextShape {
  items: ContextItem[];
  omitted: ContextOmitted[];
  charBudget: number;
  text: string;
  sha256: string;
  /** The "Relevant code" section; undefined for packages recorded before it existed. */
  code?: RelevantCode | null;
}

/** Shows exactly what an agent was (or would be) told: included notes with their trust, omissions with reasons, the verbatim text. */
export function ContextView({ ctx, heading, onOpenPage }: { ctx: ContextShape; heading?: string; onOpenPage?: (id: string) => void }) {
  const [showText, setShowText] = useState(false);
  return (
    <div className="space-y-3">
      {heading && <h3 className="m-0 text-[11px] font-medium uppercase tracking-wide text-muted">{heading}</h3>}
      <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-[12px] text-muted">
        <span>{ctx.items.length} note{ctx.items.length === 1 ? '' : 's'} included</span>
        <span>{ctx.text.length.toLocaleString()} characters sent (knowledge budget {ctx.charBudget.toLocaleString()}{ctx.code ? `, code budget ${ctx.code.budgetBytes.toLocaleString()} bytes` : ''})</span>
        <span className="mono">sha256 {shortHash(ctx.sha256)}</span>
      </div>

      {ctx.items.length === 0 && <div className="text-muted">No project knowledge qualified.</div>}
      <ul className="m-0 list-none space-y-2 p-0">
        {ctx.items.map((it) => {
          const chip = contextTrustChip(it);
          return (
            <li key={it.pageId} className="rounded-md border border-line bg-raised p-2.5">
              <div className="flex flex-wrap items-center gap-2">
                {onOpenPage ? <button className="font-medium text-accent hover:underline" onClick={() => onOpenPage(it.pageId)}>{it.title}</button> : <span className="font-medium">{it.title}</span>}
                <Badge>{it.kind}</Badge>
                <Badge tone={chip.tone}>{chip.label}</Badge>
                <span className="text-[11.5px] text-muted">author: {it.author} · {it.origin}</span>
              </div>
              {it.trust === 'verified' && it.verifiedAt && <div className="text-[11.5px] text-muted">Verified {localTime(it.verifiedAt)}{it.verifiedNote ? ` — ${it.verifiedNote}` : ''}</div>}
              {it.trust === 'pinned_unverified' && <div className="text-[11.5px] text-warn">You pinned this note; no person has verified this text.</div>}
              {it.reasons.length > 0 && <div className="text-[11.5px] text-muted">Included because: {it.reasons.join('; ')}</div>}
              {it.stale && <div className="text-[11.5px] text-warn">A source this note was written from has changed or disappeared since.</div>}
              {it.sources.length > 0 && (
                <ul className="m-0 mt-1 list-none p-0">
                  {it.sources.map((s, i) => (
                    <li key={i} className="flex flex-wrap items-center gap-1.5 text-[11.5px]">
                      <span className="text-faint">{s.kind}</span><span className="mono break-all">{s.locator}</span>
                      <Badge tone={sourceStateTone(s.state)}>{s.state}</Badge>
                      {sourceNeedsAttention({ state: s.state as never }) && <span className="text-warn">source changed since this was written</span>}
                    </li>
                  ))}
                </ul>
              )}
              <details className="mt-1">
                <summary className="cursor-pointer text-[12px] text-muted hover:text-fg">Excerpt{it.truncated ? ' (truncated to fit the budget)' : ''}</summary>
                <pre className="mt-1 max-h-48 overflow-auto whitespace-pre-wrap break-words rounded border border-line bg-bg p-2 text-[12px]">{it.excerpt}</pre>
              </details>
            </li>
          );
        })}
      </ul>

      {ctx.omitted.length > 0 && (
        <div>
          <h4 className="m-0 mb-1 text-[11px] font-medium uppercase tracking-wide text-muted">Left out ({ctx.omitted.length})</h4>
          <ul className="m-0 list-none space-y-0.5 p-0">
            {ctx.omitted.map((o) => (
              <li key={o.pageId} className="text-[12px]">
                {onOpenPage ? <button className="text-accent hover:underline" onClick={() => onOpenPage(o.pageId)}>{o.title}</button> : <span>{o.title}</span>}
                <span className="text-muted"> — {o.reason}</span>
              </li>
            ))}
          </ul>
        </div>
      )}

      <CodeSection code={ctx.code} />

      <div>
        <button className="btn btn-sm" aria-pressed={showText} onClick={() => setShowText(!showText)}>{showText ? 'Hide exact text' : 'View exact text sent'}</button>
        {showText && (
          <pre className="mt-2 max-h-80 overflow-auto whitespace-pre-wrap break-words rounded border border-line bg-bg p-2 text-[12px]">{ctx.text || '(nothing was appended to the prompt)'}</pre>
        )}
      </div>
    </div>
  );
}

/** The labelled "Relevant code" pointers: which symbols, where, and why each was chosen. */
function CodeSection({ code }: { code: RelevantCode | null | undefined }) {
  return (
    <div>
      <h4 className="m-0 mb-1 text-[11px] font-medium uppercase tracking-wide text-muted">Relevant code</h4>
      <div className="text-[12px] text-muted">{relevantCodeSummary(code)}</div>
      {code && code.items.length > 0 && (
        <>
          <ul className="m-0 mt-1 list-none space-y-1 p-0">
            {code.items.map((c) => (
              <li key={`${c.file}:${c.startLine}:${c.name}`} className="text-[12px]">
                <div className="flex flex-wrap items-baseline gap-1.5">
                  <Badge>{kindShort(c.kind)}</Badge>
                  <span className="mono font-medium">{c.name}</span>
                  <span className="mono text-muted">{c.file}:{c.startLine}</span>
                </div>
                <div className="text-[11.5px] text-muted">Included because: {c.reasons.join('; ')}</div>
              </li>
            ))}
          </ul>
          <div className="mt-1 text-[11.5px] text-faint">
            From FORGE's symbol index of {code.indexHead ? `commit ${code.indexHead.slice(0, 10)}${code.indexDirty ? ' plus uncommitted changes' : ''}` : 'the working tree'}; sent as reference data after the knowledge notes, marked as not instructions.
            {code.indexTruncated ? ` The index was partial: ${code.indexTruncated}` : ''}
          </div>
        </>
      )}
    </div>
  );
}
