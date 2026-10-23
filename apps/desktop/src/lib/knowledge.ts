// Pure helpers for the Knowledge (memory) UI. Trust is derived only from the core's `effectiveState`.
import type { Tone } from './format';
import type { ContextItem, KnowledgePage, KnowledgeSource, PageStatus } from './types';

export interface StateChip {
  /** The literal state word (verified / proposed / disputed / superseded / archived). */
  label: PageStatus;
  tone: Tone;
  /** Verified earlier, edited since: unreviewed again. */
  editedSinceVerified: boolean;
}

/** Green only when the page is verified RIGHT NOW. An edited page is proposed, never green. */
export function stateChip(p: Pick<KnowledgePage, 'effectiveState' | 'modifiedSinceVerified'>): StateChip {
  const tone: Tone =
    p.effectiveState === 'verified' ? 'ok'
    : p.effectiveState === 'proposed' ? 'warn'
    : p.effectiveState === 'disputed' ? 'bad'
    : 'idle';
  return { label: p.effectiveState, tone, editedSinceVerified: p.modifiedSinceVerified && p.effectiveState !== 'verified' };
}

export const EDITED_NOTE = 'edited since verified — unreviewed again';

export function contextTrustChip(item: Pick<ContextItem, 'trust'>): { label: string; tone: Tone } {
  return item.trust === 'verified' ? { label: 'verified', tone: 'ok' } : { label: 'pinned, NOT verified', tone: 'warn' };
}

export function sourceStateTone(s: KnowledgeSource['state'] | string): Tone {
  return s === 'current' ? 'ok' : s === 'changed' || s === 'missing' ? 'warn' : 'idle';
}

export function sourceNeedsAttention(s: Pick<KnowledgeSource, 'state'>): boolean {
  return s.state === 'changed' || s.state === 'missing';
}

/** The verify call is rejected when the file changed after the reviewer opened it. */
export function isChangedSinceOpened(message: string): boolean {
  return /changed since you opened it/i.test(message);
}

export type VerifyOutcome = { kind: 'ok' } | { kind: 'reload'; message: string } | { kind: 'error'; message: string };

export function classifyVerifyError(message: string): VerifyOutcome {
  return isChangedSinceOpened(message)
    ? { kind: 'reload', message: 'This page changed since you opened it. It has been reloaded: review the new text before verifying.' }
    : { kind: 'error', message };
}

export interface DiffOp { type: 'ctx' | 'add' | 'del'; text: string }

/** Simple LCS line diff of current -> proposed. */
export function lineDiff(oldText: string, newText: string): DiffOp[] {
  const a = oldText.split(/\r?\n/);
  const b = newText.split(/\r?\n/);
  if (a.length * b.length > 4_000_000) {
    return [...a.map((text) => ({ type: 'del' as const, text })), ...b.map((text) => ({ type: 'add' as const, text }))];
  }
  const n = a.length;
  const m = b.length;
  const dp: number[][] = Array.from({ length: n + 1 }, () => new Array<number>(m + 1).fill(0));
  for (let i = n - 1; i >= 0; i--) for (let j = m - 1; j >= 0; j--) dp[i][j] = a[i] === b[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
  const out: DiffOp[] = [];
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (a[i] === b[j]) { out.push({ type: 'ctx', text: a[i] }); i++; j++; }
    else if (dp[i + 1][j] >= dp[i][j + 1]) out.push({ type: 'del', text: a[i++] });
    else out.push({ type: 'add', text: b[j++] });
  }
  while (i < n) out.push({ type: 'del', text: a[i++] });
  while (j < m) out.push({ type: 'add', text: b[j++] });
  return out;
}

export interface PageFilters {
  states: ReadonlySet<string>;
  kinds: ReadonlySet<string>;
  authors: ReadonlySet<string>;
  roots: ReadonlySet<string>;
}
export const NO_FILTERS: PageFilters = { states: new Set(), kinds: new Set(), authors: new Set(), roots: new Set() };

/** An empty set for a dimension means "no restriction". */
export function filterPages(pages: KnowledgePage[], f: PageFilters): KnowledgePage[] {
  return pages.filter(
    (p) =>
      (f.states.size === 0 || f.states.has(p.effectiveState)) &&
      (f.kinds.size === 0 || f.kinds.has(p.kind)) &&
      (f.authors.size === 0 || f.authors.has(p.author)) &&
      (f.roots.size === 0 || f.roots.has(p.rootKind)),
  );
}

export function toggleIn(set: ReadonlySet<string>, v: string): Set<string> {
  const n = new Set(set);
  if (n.has(v)) n.delete(v);
  else n.add(v);
  return n;
}

/** Pages an agent or the system proposed that a person has not reviewed yet. Imported notes are not "proposals". */
export function proposalsToReview(pages: KnowledgePage[]): KnowledgePage[] {
  return pages.filter((p) => p.effectiveState === 'proposed' && p.origin !== 'imported' && (p.author === 'agent' || p.author === 'system'));
}

export function firstLine(text: string, max = 80): string {
  const l = text.split(/\r?\n/).map((x) => x.replace(/^#+\s*/, '').trim()).find(Boolean) ?? '';
  return l.length > max ? l.slice(0, max - 1) + '…' : l;
}

export function shortHash(h: string | null | undefined): string {
  return h ? h.slice(0, 10) : '—';
}

/** "2 verified notes would be attached" / "No verified notes match yet" */
export function previewSummary(items: Pick<ContextItem, 'trust'>[]): string {
  const n = items.length;
  if (n === 0) return 'No verified notes match yet';
  const pinned = items.filter((i) => i.trust !== 'verified').length;
  const verified = n - pinned;
  const parts: string[] = [];
  if (verified) parts.push(`${verified} verified note${verified === 1 ? '' : 's'}`);
  if (pinned) parts.push(`${pinned} pinned unverified note${pinned === 1 ? '' : 's'}`);
  return `${parts.join(' and ')} would be attached`;
}

export function linesOf(text: string): string[] {
  return text.split(/\r?\n/).map((l) => l.trim()).filter(Boolean);
}
