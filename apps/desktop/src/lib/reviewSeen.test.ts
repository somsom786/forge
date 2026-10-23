import { describe, expect, it } from 'vitest';
import { classifyApproveError, isPersistentBlock, isStaleReviewRefusal, seenFrom, touchedInputsChip } from './verification';

describe('seenFrom', () => {
  it('takes fingerprint and contract hash from the displayed report', () => {
    expect(seenFrom({ fingerprint: 'fp1', contractSha256: 'cs1' })).toEqual({ fingerprint: 'fp1', contractSha256: 'cs1' });
  });
  it('is null without a displayed report, so approve cannot be attempted', () => expect(seenFrom(null)).toBeNull());
});

describe('stale review refusal', () => {
  const msg = 'the changes (or the success criteria) are not the ones you reviewed';
  it('is recognised for approvals and verdicts', () => {
    expect(isStaleReviewRefusal(msg)).toBe(true);
    expect(isStaleReviewRefusal('these are not the ones you were looking at')).toBe(true);
    expect(isStaleReviewRefusal('disk full')).toBe(false);
  });
  it('keeps the dialog open, reloads the report, and shows the core message verbatim', () => {
    expect(classifyApproveError(msg)).toEqual({ keepDialog: true, reload: true, message: msg });
  });
  it('does not reload for unrelated errors but still keeps the dialog and text', () => {
    expect(classifyApproveError('boom')).toEqual({ keepDialog: true, reload: false, message: 'boom' });
  });
});

describe('touchedInputsChip', () => {
  it('is absent when nothing changed', () => expect(touchedInputsChip([])).toBeNull());
  it('lists the touched files with the required tooltip', () => {
    const c = touchedInputsChip(['package.json', 'jest.config.js'])!;
    expect(c.text).toBe('inputs changed: package.json, jest.config.js');
    expect(c.tooltip).toBe('A passing result is weaker evidence: the agent edited files that define or configure this check.');
  });
});

describe('isPersistentBlock', () => {
  it('flags git-filter and merge refusals', () => {
    expect(isPersistentBlock('this repository configures external git programs (filter.x.clean = y). FORGE will not run them')).toBe(true);
    expect(isPersistentBlock('merging would overwrite 2 file(s) in your checkout that are not tracked by git: a, b')).toBe(true);
    expect(isPersistentBlock('the main checkout moved since the merge was prepared')).toBe(true);
    expect(isPersistentBlock('the merge produced a different tree than the one predicted')).toBe(true);
    expect(isPersistentBlock('some other error')).toBe(false);
  });
});
