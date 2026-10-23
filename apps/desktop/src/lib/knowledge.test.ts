import { describe, expect, it } from 'vitest';
import {
  classifyVerifyError, contextTrustChip, filterPages, firstLine, lineDiff, NO_FILTERS, previewSummary, proposalsToReview, sourceNeedsAttention,
  stateChip, toggleIn,
} from './knowledge';
import type { KnowledgePage } from './types';

const pg = (o: Partial<KnowledgePage>): KnowledgePage => ({
  id: 'p', projectId: 'x', rootKind: 'managed', relPath: 'a.md', slug: 'a', title: 'A', kind: 'note', scope: 'project', author: 'user', origin: 'forge',
  status: 'proposed', effectiveState: 'proposed', modifiedSinceVerified: false, verifiedSha256: null, verifiedAt: null, verifiedNote: null,
  currentSha256: 'h', topic: null, supersedesId: null, supersededById: null, files: [], tags: [], warnings: [], missing: false,
  createdAt: '', updatedAt: '', indexedAt: '', ...o,
});

describe('stateChip', () => {
  it('is green only for effectiveState verified', () => {
    for (const s of ['proposed', 'disputed', 'superseded', 'archived'] as const) expect(stateChip(pg({ effectiveState: s })).tone).not.toBe('ok');
    expect(stateChip(pg({ status: 'verified', effectiveState: 'verified' }))).toMatchObject({ label: 'verified', tone: 'ok', editedSinceVerified: false });
  });
  it('shows an edited-after-verified page as proposed, never green', () => {
    const c = stateChip(pg({ status: 'verified', effectiveState: 'proposed', modifiedSinceVerified: true }));
    expect(c).toMatchObject({ label: 'proposed', tone: 'warn', editedSinceVerified: true });
  });
});

describe('context trust chip', () => {
  it('maps trust levels', () => {
    expect(contextTrustChip({ trust: 'verified' })).toEqual({ label: 'verified', tone: 'ok' });
    expect(contextTrustChip({ trust: 'pinned_unverified' })).toEqual({ label: 'pinned, NOT verified', tone: 'warn' });
  });
});

describe('verify conflict handling', () => {
  it('asks for a reload when the page changed since it was opened', () => {
    expect(classifyVerifyError('page changed since you opened it; review again').kind).toBe('reload');
  });
  it('passes other errors through verbatim', () => {
    expect(classifyVerifyError('disk full')).toEqual({ kind: 'error', message: 'disk full' });
  });
});

describe('lineDiff', () => {
  it('marks added, removed and unchanged lines', () => {
    const d = lineDiff('a\nb\nc', 'a\nB\nc\nd');
    expect(d.map((x) => `${x.type}:${x.text}`)).toEqual(['ctx:a', 'del:b', 'add:B', 'ctx:c', 'add:d']);
  });
  it('is all context for identical text', () => expect(lineDiff('x\ny', 'x\ny').every((o) => o.type === 'ctx')).toBe(true));
  it('handles empty old text', () => expect(lineDiff('', 'a').filter((o) => o.type === 'add')).toHaveLength(1));
});

describe('filters', () => {
  const pages = [
    pg({ id: '1', effectiveState: 'verified', kind: 'decision', author: 'user' }),
    pg({ id: '2', effectiveState: 'proposed', kind: 'note', author: 'agent' }),
    pg({ id: '3', effectiveState: 'proposed', kind: 'note', author: 'user', rootKind: 'vault', origin: 'imported' }),
  ];
  it('returns everything with no filters', () => expect(filterPages(pages, NO_FILTERS)).toHaveLength(3));
  it('combines dimensions with AND and values within a dimension with OR', () => {
    expect(filterPages(pages, { ...NO_FILTERS, states: new Set(['proposed']) }).map((p) => p.id)).toEqual(['2', '3']);
    expect(filterPages(pages, { ...NO_FILTERS, states: new Set(['proposed']), authors: new Set(['agent']) }).map((p) => p.id)).toEqual(['2']);
    expect(filterPages(pages, { ...NO_FILTERS, kinds: new Set(['decision', 'note']), roots: new Set(['vault']) }).map((p) => p.id)).toEqual(['3']);
  });
  it('toggles set members immutably', () => {
    const s = new Set(['a']);
    expect([...toggleIn(s, 'b')]).toEqual(['a', 'b']);
    expect([...toggleIn(s, 'a')]).toEqual([]);
    expect(s.size).toBe(1);
  });
  it('counts only non-imported agent/system proposals for review', () => {
    expect(proposalsToReview(pages).map((p) => p.id)).toEqual(['2']);
  });
});

describe('misc', () => {
  it('summarizes previews', () => {
    expect(previewSummary([])).toBe('No verified notes match yet');
    expect(previewSummary([{ trust: 'verified' }, { trust: 'verified' }])).toBe('2 verified notes would be attached');
    expect(previewSummary([{ trust: 'pinned_unverified' }])).toContain('pinned unverified');
  });
  it('derives a title from the first line', () => expect(firstLine('\n## Heading here\nbody')).toBe('Heading here'));
  it('flags changed or missing sources', () => {
    expect(sourceNeedsAttention({ state: 'changed' })).toBe(true);
    expect(sourceNeedsAttention({ state: 'current' })).toBe(false);
  });
});
