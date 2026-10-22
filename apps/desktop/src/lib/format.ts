import type { Mission, MissionState, Risk } from './types';

export function errMsg(e: unknown): string {
  if (typeof e === 'string') return e;
  if (e instanceof Error) return e.message;
  try {
    return JSON.stringify(e);
  } catch {
    return String(e);
  }
}

export function relTime(iso: string | null | undefined, now: number = Date.now()): string {
  if (!iso) return '—';
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return '—';
  const s = Math.round((now - t) / 1000);
  if (s < 5) return 'just now';
  if (s < 60) return `${s}s ago`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h ago`;
  const d = Math.floor(h / 24);
  if (d < 14) return `${d}d ago`;
  return new Date(t).toLocaleDateString();
}

/** Accepts an ISO string, epoch milliseconds, or epoch seconds. */
export function localTime(v: string | number | null | undefined): string {
  if (v === null || v === undefined || v === '') return '—';
  const d = typeof v === 'number' ? new Date(v < 1e12 ? v * 1000 : v) : new Date(v);
  if (Number.isNaN(d.getTime())) return String(v);
  return d.toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'medium' });
}

export function clockTime(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return '';
  return d.toLocaleTimeString(undefined, { hour12: false });
}

export function formatDuration(ms: number): string {
  if (!Number.isFinite(ms) || ms < 0) return '—';
  const s = Math.floor(ms / 1000);
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ${String(s % 60).padStart(2, '0')}s`;
  const h = Math.floor(m / 60);
  return `${h}h ${String(m % 60).padStart(2, '0')}m`;
}

export function shortSha(s: string | null | undefined): string {
  return s ? s.slice(0, 8) : '—';
}

export function fmtNum(n: number | undefined | null): string {
  if (n === undefined || n === null) return '—';
  return n.toLocaleString();
}

export type Tone = 'ok' | 'warn' | 'bad' | 'run' | 'idle';

export const STATE_META: Record<MissionState, { label: string; tone: Tone }> = {
  draft: { label: 'Draft', tone: 'idle' },
  ready: { label: 'Ready', tone: 'idle' },
  preparing: { label: 'Preparing', tone: 'run' },
  running: { label: 'Running', tone: 'run' },
  awaiting_approval: { label: 'Needs approval', tone: 'warn' },
  awaiting_review: { label: 'Awaiting review', tone: 'warn' },
  approved: { label: 'Approved', tone: 'ok' },
  merged: { label: 'Merged', tone: 'ok' },
  rejected: { label: 'Rejected', tone: 'idle' },
  failed: { label: 'Failed', tone: 'bad' },
  cancelled: { label: 'Cancelled', tone: 'idle' },
  interrupted: { label: 'Interrupted', tone: 'warn' },
};

export const TONE_CLASS: Record<Tone, string> = {
  ok: 'bg-ok-dim text-ok border-ok/30',
  warn: 'bg-warn-dim text-warn border-warn/30',
  bad: 'bg-bad-dim text-bad border-bad/30',
  run: 'bg-run-dim text-run border-run/30',
  idle: 'bg-raised text-muted border-line-strong',
};

export const RISK_TONE: Record<Risk, Tone> = { low: 'ok', medium: 'warn', high: 'bad' };

export function isActiveState(s: MissionState): boolean {
  return s === 'preparing' || s === 'running' || s === 'awaiting_approval';
}

export function missionLabel(m: Pick<Mission, 'title' | 'id'>): string {
  return m.title || m.id.slice(0, 8);
}

export function providerLabel(id: string): string {
  return id === 'claude' ? 'Claude Code' : id === 'codex' ? 'Codex' : id === 'fake' ? 'Scripted test agent' : id;
}

/** Utilization may arrive as a 0..1 fraction or already as a percentage. */
export function fmtUtilization(u: number): string {
  const pct = u <= 1 ? u * 100 : u;
  return `${Math.round(pct)}%`;
}

export function fmtCost(usd: number | undefined | null): string {
  if (usd === undefined || usd === null) return '—';
  return `$${usd.toFixed(usd < 1 ? 3 : 2)}`;
}
