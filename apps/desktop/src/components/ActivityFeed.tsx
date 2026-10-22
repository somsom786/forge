import { Fragment, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { AlertTriangle, ArrowDown, Brain, Check, FilePen, Gauge, RefreshCw, ShieldAlert, Wrench, X } from 'lucide-react';
import { buildFeed, filterFeed, type FeedItem, type FilterGroup } from '../lib/feed';
import { clockTime, fmtCost, fmtNum, fmtUtilization, formatDuration, localTime } from '../lib/format';
import type { ActivityEvent, Approval, UsageReport } from '../lib/types';
import { Badge } from './ui';

interface Props {
  events: ActivityEvent[];
  approvals: Record<string, Approval>;
  /** messageId -> accumulated streamed text (transient). */
  streams: Record<string, string> | undefined;
  onFocusApprovals: () => void;
}

const FILTERS: { key: FilterGroup; label: string }[] = [
  { key: 'thinking', label: 'Thinking' },
  { key: 'tools', label: 'Tool calls' },
  { key: 'files', label: 'File changes' },
  { key: 'status', label: 'Status & session' },
];

/** Plain text with ``` fenced blocks rendered as <pre>. Everything is a React text node: no HTML injection. */
function RichText({ text }: { text: string }) {
  const parts = text.split(/```/);
  return (
    <>
      {parts.map((part, i) => {
        if (i % 2 === 0) return part ? <div key={i} className="whitespace-pre-wrap break-words">{part.replace(/^\n+|\n+$/g, '')}</div> : <Fragment key={i} />;
        const nl = part.indexOf('\n');
        const lang = nl >= 0 ? part.slice(0, nl).trim() : '';
        const code = nl >= 0 ? part.slice(nl + 1) : part;
        return (
          <div key={i} className="my-1.5 overflow-hidden rounded-md border border-line bg-bg">
            {lang && <div className="border-b border-line px-2 py-0.5 text-[11px] text-faint">{lang}</div>}
            <pre className="overflow-x-auto p-2 text-[12px] leading-[1.5]">{code.replace(/\n$/, '')}</pre>
          </div>
        );
      })}
    </>
  );
}

function stringify(v: unknown): string {
  if (typeof v === 'string') return v;
  try {
    return JSON.stringify(v, null, 2) ?? '';
  } catch {
    return String(v);
  }
}

function Time({ at }: { at: string }) {
  return <span className="mono w-[58px] shrink-0 pt-0.5 text-[11px] text-faint">{clockTime(at)}</span>;
}

function Row({ at, children, icon }: { at: string; children: React.ReactNode; icon?: React.ReactNode }) {
  return (
    <div className="flex gap-2 px-4 py-1">
      <Time at={at} />
      {icon && <span className="mt-0.5 shrink-0">{icon}</span>}
      <div className="min-w-0 flex-1">{children}</div>
    </div>
  );
}

function ToolRow({ item }: { item: Extract<FeedItem, { type: 'tool' }> }) {
  const { call, result } = item;
  const p = call?.payload ?? {};
  const r = result?.payload ?? {};
  const name = (p.name ?? r.name ?? 'tool') as string;
  const ok = result ? r.ok !== false : null;
  const at = (call ?? result)!.at;
  return (
    <Row at={at} icon={<Wrench className="size-3.5 text-muted" aria-hidden />}>
      <details className="group rounded-md border border-line bg-panel/60">
        <summary className="flex cursor-pointer items-center gap-2 px-2 py-1 hover:bg-hover">
          <span className="mono text-[12px] font-semibold text-fg">{name}</span>
          <span className="mono min-w-0 flex-1 truncate text-[12px] text-muted">{(p.summary as string) ?? ''}</span>
          {ok === true && <Check className="size-3.5 shrink-0 text-ok" aria-label="succeeded" />}
          {ok === false && <X className="size-3.5 shrink-0 text-bad" aria-label="failed" />}
          {ok === null && <span className="shrink-0 text-[11px] text-faint">no result yet</span>}
        </summary>
        <div className="space-y-2 border-t border-line p-2">
          {call && (
            <div>
              <div className="mb-0.5 text-[11px] uppercase tracking-wide text-faint">Input</div>
              <pre className="max-h-56 overflow-auto whitespace-pre-wrap break-all rounded border border-line bg-bg p-2 text-[12px]">{stringify(p.input)}</pre>
            </div>
          )}
          {result && (
            <div>
              <div className={`mb-0.5 text-[11px] uppercase tracking-wide ${ok ? 'text-faint' : 'text-bad'}`}>{ok ? 'Output' : 'Error output'}</div>
              <pre className="max-h-72 overflow-auto whitespace-pre-wrap break-all rounded border border-line bg-bg p-2 text-[12px]">{stringify(r.output)}</pre>
              {r.truncated && <div className="mt-1 text-[11px] text-warn">Output was truncated by the engine.</div>}
            </div>
          )}
        </div>
      </details>
    </Row>
  );
}

function UsageLine({ u }: { u: UsageReport }) {
  return (
    <span className="mono text-[11.5px] text-muted">
      est. {fmtNum(u.inputTokens)} in / {fmtNum(u.outputTokens)} out
      {u.cacheReadTokens ? ` / ${fmtNum(u.cacheReadTokens)} cache-read` : ''}
      {u.costUsdEstimate !== undefined ? ` · ${fmtCost(u.costUsdEstimate)} est.` : ''}
      {u.durationMs !== undefined ? ` · ${formatDuration(u.durationMs)}` : ''}
      {u.numTurns !== undefined ? ` · ${u.numTurns} turn${u.numTurns === 1 ? '' : 's'}` : ''}
    </span>
  );
}

function EventRow({ ev, approvals, onFocusApprovals }: { ev: ActivityEvent; approvals: Record<string, Approval>; onFocusApprovals: () => void }) {
  const p = ev.payload ?? {};
  switch (ev.kind) {
    case 'agent.text':
      return (
        <Row at={ev.at}>
          <div className="rounded-md border border-line bg-raised px-3 py-2 text-[13px] leading-[1.55]">
            <RichText text={String(p.text ?? '')} />
          </div>
        </Row>
      );
    case 'agent.thinking':
      return (
        <Row at={ev.at} icon={<Brain className="size-3.5 text-faint" aria-hidden />}>
          <details>
            <summary className="cursor-pointer text-[12px] text-faint hover:text-muted">Thinking ({String(p.text ?? '').length.toLocaleString()} chars)</summary>
            <pre className="mt-1 max-h-64 overflow-auto whitespace-pre-wrap break-words rounded border border-line bg-bg p-2 text-[12px] italic text-muted">{String(p.text ?? '')}</pre>
          </details>
        </Row>
      );
    case 'file.changed':
      return (
        <Row at={ev.at} icon={<FilePen className="size-3.5 text-accent" aria-hidden />}>
          <span className="text-muted">Changed </span>
          <span className="mono text-[12px] text-fg">{String(p.path ?? '')}</span>
          {p.tool && <span className="text-[11px] text-faint"> via {String(p.tool)}</span>}
        </Row>
      );
    case 'approval.requested': {
      const a = approvals[String(p.approvalId)];
      const state = a?.state ?? 'pending';
      return (
        <Row at={ev.at} icon={<ShieldAlert className="size-3.5 text-warn" aria-hidden />}>
          <div className="flex flex-wrap items-center gap-2 rounded-md border border-warn/30 bg-warn-dim/60 px-2 py-1">
            <span className="text-muted">Approval requested:</span>
            <span className="mono text-[12px] font-semibold">{String(p.tool ?? a?.tool ?? '')}</span>
            <span className="mono min-w-0 flex-1 truncate text-[12px] text-muted">{String(p.summary ?? a?.summary ?? '')}</span>
            {p.risk && <Badge tone={p.risk === 'high' ? 'bad' : p.risk === 'medium' ? 'warn' : 'ok'}>{String(p.risk)} risk</Badge>}
            {state === 'pending' ? (
              <button className="btn btn-sm" onClick={onFocusApprovals}>Review</button>
            ) : (
              <Badge tone={state === 'approved' ? 'ok' : state === 'auto_denied' ? 'bad' : 'idle'}>{state === 'auto_denied' ? 'blocked by policy' : state.replace('_', ' ')}</Badge>
            )}
          </div>
        </Row>
      );
    }
    case 'approval.decided': {
      const d = String(p.decision ?? '');
      const blocked = d === 'auto_denied';
      return (
        <Row at={ev.at} icon={<ShieldAlert className={`size-3.5 ${blocked ? 'text-bad' : 'text-muted'}`} aria-hidden />}>
          <span className={blocked ? 'text-bad' : 'text-muted'}>
            {blocked ? 'Blocked by policy' : d === 'approved' ? 'Approved' : d === 'denied' ? 'Denied' : d.replace('_', ' ')}
          </span>{' '}
          <span className="mono text-[12px]">{String(p.tool ?? '')}</span>
          {p.note && <span className="text-muted"> — {String(p.note)}</span>}
        </Row>
      );
    }
    case 'turn.completed': {
      const status = String(p.status ?? '');
      const tone = status === 'success' ? 'ok' : status === 'cancelled' ? 'idle' : 'bad';
      const denials = Array.isArray(p.permissionDenials) ? p.permissionDenials.length : 0;
      return (
        <Row at={ev.at}>
          <div className="rounded-md border border-line-strong bg-panel px-3 py-1.5">
            <div className="flex flex-wrap items-center gap-2">
              <span className="text-muted">Turn finished</span>
              <Badge tone={tone}>{status || 'unknown'}</Badge>
              {p.usage && <UsageLine u={p.usage as UsageReport} />}
              {denials > 0 && <span className="text-[12px] text-warn">{denials} denied tool call{denials === 1 ? '' : 's'}</span>}
            </div>
            {p.message && <div className="mt-1 whitespace-pre-wrap break-words text-[12px] text-muted">{String(p.message)}</div>}
          </div>
        </Row>
      );
    }
    case 'error':
      return (
        <Row at={ev.at} icon={<AlertTriangle className="size-3.5 text-bad" aria-hidden />}>
          <pre className="whitespace-pre-wrap break-words rounded-md border border-bad/40 bg-bad-dim p-2 text-[12px]">{String(p.message ?? stringify(p))}</pre>
        </Row>
      );
    case 'status':
      return (
        <Row at={ev.at}>
          <span className="text-muted">{String(p.message ?? stringify(p))}</span>
        </Row>
      );
    case 'provider.retry':
      return (
        <Row at={ev.at} icon={<RefreshCw className="size-3.5 text-warn" aria-hidden />}>
          <span className="text-warn">
            Provider API retry {p.attempt !== undefined ? `${p.attempt}${p.maxRetries !== undefined ? ` of ${p.maxRetries}` : ''}` : ''}
            {p.delayMs !== undefined ? ` in ${Math.round(Number(p.delayMs) / 100) / 10}s` : ''}
            {p.status !== undefined ? ` (HTTP ${p.status})` : ''}
          </span>
        </Row>
      );
    case 'provider.rate_limit': {
      const hasUtil = typeof p.utilization === 'number';
      return (
        <Row at={ev.at} icon={<Gauge className="size-3.5 text-muted" aria-hidden />}>
          <span className="text-muted">
            Rate limit{p.rateLimitType ? ` (${String(p.rateLimitType)})` : ''}: {String(p.status ?? 'reported')}
            {hasUtil ? ` · ${fmtUtilization(p.utilization)} used (provider-reported)` : ''}
            {p.resetsAt ? ` · resets ${localTime(p.resetsAt)}` : ''}
            {p.isUsingOverage ? ' · using overage' : ''}
          </span>
        </Row>
      );
    }
    case 'session.started': {
      const src = p.credentialSource as string | undefined;
      const subscription = src === 'none';
      return (
        <Row at={ev.at}>
          <div className="rounded-md border border-line bg-panel px-3 py-1.5 text-[12px]">
            <div className="flex flex-wrap items-center gap-x-3 gap-y-0.5">
              <span className="font-medium text-fg">Session started</span>
              {p.model && <span className="mono">{String(p.model)}</span>}
              {p.claudeCodeVersion && <span className="mono text-muted">Claude Code {String(p.claudeCodeVersion)}</span>}
              {subscription && <Badge tone="ok">subscription login</Badge>}
              {!src && <span className="text-muted">credential source not reported</span>}
            </div>
            {src && !subscription && (
              <div className="mt-1.5 flex gap-1.5 rounded border border-warn/50 bg-warn-dim px-2 py-1.5 text-warn" role="alert">
                <AlertTriangle className="mt-0.5 size-3.5 shrink-0" aria-hidden />
                <span>
                  Credential source is <span className="mono">{src}</span>, not a subscription login. This session may be billed per token to an API key.
                </span>
              </div>
            )}
          </div>
        </Row>
      );
    }
    default:
      return (
        <Row at={ev.at}>
          <span className="mono text-[11.5px] text-faint">{ev.kind} {stringify(p).slice(0, 300)}</span>
        </Row>
      );
  }
}

export function ActivityFeed({ events, approvals, streams, onFocusApprovals }: Props) {
  const [hidden, setHidden] = useState<Set<FilterGroup>>(() => new Set<FilterGroup>(['thinking']));
  const items = useMemo(() => filterFeed(buildFeed(events), hidden), [events, hidden]);
  const scroller = useRef<HTMLDivElement>(null);
  const [atBottom, setAtBottom] = useState(true);
  const atBottomRef = useRef(true);
  const streamText = streams ? Object.entries(streams) : [];
  const streamSig = streamText.reduce((n, [, t]) => n + t.length, 0);

  const onScroll = () => {
    const el = scroller.current;
    if (!el) return;
    const near = el.scrollHeight - el.scrollTop - el.clientHeight < 48;
    atBottomRef.current = near;
    setAtBottom(near);
  };

  useLayoutEffect(() => {
    const el = scroller.current;
    if (el && atBottomRef.current) el.scrollTop = el.scrollHeight;
  }, [items.length, streamSig]);

  // Initial history load should land at the bottom.
  useEffect(() => {
    const el = scroller.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, []);

  const toggle = (g: FilterGroup) =>
    setHidden((h) => {
      const n = new Set(h);
      if (n.has(g)) n.delete(g);
      else n.add(g);
      return n;
    });

  return (
    <div className="relative flex min-h-0 flex-1 flex-col">
      <div className="flex flex-wrap items-center gap-x-4 gap-y-1 border-b border-line px-4 py-1.5 text-[12px]" role="group" aria-label="Activity filters">
        <span className="text-muted">Show:</span>
        {FILTERS.map((f) => (
          <label key={f.key} className="flex cursor-pointer items-center gap-1.5 text-muted hover:text-fg">
            <input type="checkbox" checked={!hidden.has(f.key)} onChange={() => toggle(f.key)} />
            {f.label}
          </label>
        ))}
        <span className="ml-auto text-faint">{events.length} events</span>
      </div>
      <div ref={scroller} onScroll={onScroll} className="min-h-0 flex-1 overflow-y-auto py-2" role="log" aria-label="Mission activity">
        {items.length === 0 && streamText.length === 0 && <div className="px-4 py-10 text-center text-muted">No activity recorded yet.</div>}
        {items.map((it) =>
          it.type === 'tool' ? <ToolRow key={it.key} item={it} /> : <EventRow key={it.key} ev={it.ev} approvals={approvals} onFocusApprovals={onFocusApprovals} />,
        )}
        {streamText.map(([id, text]) => (
          <div key={`stream-${id}`} className="flex gap-2 px-4 py-1">
            <span className="w-[58px] shrink-0" />
            <div className="min-w-0 flex-1 rounded-md border border-dashed border-run/40 bg-raised px-3 py-2 text-[13px] leading-[1.55]">
              <RichText text={text} />
              <div className="mt-1 text-[11px] text-run">streaming…</div>
            </div>
          </div>
        ))}
      </div>
      {!atBottom && (
        <button
          className="btn btn-primary absolute bottom-3 left-1/2 -translate-x-1/2 shadow-lg shadow-black/50"
          onClick={() => {
            const el = scroller.current;
            if (el) el.scrollTo({ top: el.scrollHeight, behavior: 'smooth' });
          }}
        >
          <ArrowDown className="size-3.5" aria-hidden /> Jump to latest
        </button>
      )}
    </div>
  );
}

