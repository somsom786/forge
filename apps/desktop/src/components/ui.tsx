import { useEffect, useState, type ReactNode } from 'react';
import { AlertTriangle, CircleCheck, Info, X } from 'lucide-react';
import { STATE_META, TONE_CLASS, type Tone } from '../lib/format';
import { useStore } from '../lib/store';
import type { MissionState } from '../lib/types';

export function Badge({ tone = 'idle', children, title }: { tone?: Tone; children: ReactNode; title?: string }) {
  return (
    <span className={`badge ${TONE_CLASS[tone]}`} title={title}>
      {children}
    </span>
  );
}

export function StateBadge({ state, detached }: { state: MissionState; detached?: boolean }) {
  const m = STATE_META[state];
  const live = (state === 'running' || state === 'preparing') && !detached;
  return (
    <Badge tone={detached ? 'warn' : m.tone}>
      {live && <span className="live-dot inline-block size-1.5 rounded-full bg-current" aria-hidden />}
      {m.label}
    </Badge>
  );
}

/** Re-renders on an interval so relative times stay current. */
export function useNow(intervalMs = 30000): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const t = window.setInterval(() => setNow(Date.now()), intervalMs);
    return () => window.clearInterval(t);
  }, [intervalMs]);
  return now;
}

export function EmptyState({ title, children, actions }: { title: string; children?: ReactNode; actions?: ReactNode }) {
  return (
    <div className="mx-auto flex max-w-md flex-col items-center gap-2 px-6 py-16 text-center">
      <div className="text-[15px] font-medium text-fg">{title}</div>
      {children && <div className="text-muted">{children}</div>}
      {actions && <div className="mt-2 flex gap-2">{actions}</div>}
    </div>
  );
}

export function PageHeader({ title, sub, actions }: { title: string; sub?: ReactNode; actions?: ReactNode }) {
  return (
    <div className="flex items-center justify-between gap-4 border-b border-line px-5 py-3">
      <div className="min-w-0">
        <h1 className="m-0 text-[15px] font-semibold text-fg">{title}</h1>
        {sub && <div className="truncate text-muted">{sub}</div>}
      </div>
      <div className="flex shrink-0 items-center gap-2">{actions}</div>
    </div>
  );
}

export function Toasts() {
  const notices = useStore((s) => s.notices);
  const dismiss = useStore((s) => s.dismiss);
  return (
    <div className="pointer-events-none fixed bottom-4 right-4 z-[100] flex w-[380px] max-w-[calc(100vw-2rem)] flex-col gap-2" aria-live="polite">
      {notices.map((n) => (
        <Toast key={n.id} id={n.id} level={n.level} message={n.message} link={n.link} onClose={() => dismiss(n.id)} />
      ))}
    </div>
  );
}

function Toast({ level, message, link, onClose }: { id: number; level: 'info' | 'warn' | 'error'; message: string; link?: { label: string; pageId: string }; onClose: () => void }) {
  const navigate = useStore((s) => s.navigate);
  useEffect(() => {
    if (level === 'error') return; // errors stay until dismissed
    const t = window.setTimeout(onClose, level === 'warn' ? 12000 : 5000);
    return () => window.clearTimeout(t);
  }, [level, onClose]);
  const tone = level === 'error' ? 'border-bad/50 bg-bad-dim' : level === 'warn' ? 'border-warn/50 bg-warn-dim' : 'border-line-strong bg-raised';
  const Icon = level === 'info' ? CircleCheck : level === 'warn' ? AlertTriangle : Info;
  const color = level === 'error' ? 'text-bad' : level === 'warn' ? 'text-warn' : 'text-accent';
  return (
    <div role={level === 'error' ? 'alert' : 'status'} className={`pointer-events-auto flex items-start gap-2 rounded-md border px-3 py-2 shadow-lg shadow-black/40 ${tone}`}>
      <Icon className={`mt-0.5 size-4 shrink-0 ${color}`} aria-hidden />
      <div className="min-w-0 flex-1 whitespace-pre-wrap break-words text-fg">{message}{link && <> <button className="text-accent underline" onClick={() => { navigate({ name: 'knowledge', pageId: link.pageId }); onClose(); }}>{link.label}</button></>}</div>
      <button className="btn btn-ghost btn-sm !px-1" onClick={onClose} aria-label="Dismiss notification">
        <X className="size-3.5" />
      </button>
    </div>
  );
}

export function Callout({ tone, title, children }: { tone: 'warn' | 'bad' | 'info'; title?: string; children: ReactNode }) {
  const cls = tone === 'bad' ? 'border-bad/40 bg-bad-dim' : tone === 'warn' ? 'border-warn/40 bg-warn-dim' : 'border-line-strong bg-raised';
  const color = tone === 'bad' ? 'text-bad' : tone === 'warn' ? 'text-warn' : 'text-accent';
  return (
    <div className={`rounded-md border px-3 py-2 ${cls}`} role={tone === 'info' ? undefined : 'alert'}>
      {title && (
        <div className={`mb-1 flex items-center gap-1.5 font-medium ${color}`}>
          <AlertTriangle className="size-3.5" aria-hidden /> {title}
        </div>
      )}
      <div className="text-fg">{children}</div>
    </div>
  );
}

export function Kbd({ children }: { children: ReactNode }) {
  return <span className="kbd">{children}</span>;
}

export function Mono({ children, className = '' }: { children: ReactNode; className?: string }) {
  return <span className={`mono text-[12px] ${className}`}>{children}</span>;
}
