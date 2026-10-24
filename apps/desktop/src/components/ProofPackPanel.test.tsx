import { cleanup, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { ProofSummary } from './ProofPackPanel';

// Content shaped exactly as proof.rs records it (field names checked against build_pack).
const content = {
  unresolved: ['Required check "unit" has not passed on the current changes'],
  criteria: [
    { text: 'adds a CLI flag', status: { status: 'verified_by_checks', evidence: ['unit'] } },
    { text: 'docs updated', status: { status: 'not_checked' } },
  ],
  checks: [
    { name: 'unit', fresh: true, latest: { state: 'passed' } },
    { name: 'lint', fresh: false, latest: { state: 'passed' } },
    { name: 'e2e', fresh: true, latest: { state: 'failed' } },
    { name: 'secrets', fresh: false, latest: null },
  ],
  changes: { files: [{ path: 'a.rs' }, { path: 'b.rs' }] },
  sessions: [{ ranOn: 'agent_host' }],
};

afterEach(cleanup);

describe('ProofSummary', () => {
  it('states what is open, from the recorded content', () => {
    render(<ProofSummary content={content} />);
    expect(screen.getByText('1 not established')).toBeTruthy();
    expect(screen.getByText('criteria 1/2 met')).toBeTruthy();
    expect(screen.getByText('checks 1/4 passed, 1 not passing, 1 passed on older changes, 1 never run')).toBeTruthy();
    expect(screen.getByText('2 files changed')).toBeTruthy();
    expect(screen.getByText('agent ran on the host')).toBeTruthy();
  });

  it('only says "isolated" when every session was', () => {
    const { rerender } = render(<ProofSummary content={{ ...content, unresolved: [], sessions: [{ ranOn: 'linux_namespaces' }] }} />);
    expect(screen.getByText('nothing outstanding')).toBeTruthy();
    expect(screen.getByText('agent isolated')).toBeTruthy();
    rerender(<ProofSummary content={{ ...content, sessions: [{ ranOn: 'linux_namespaces' }, { ranOn: 'agent_host' }] }} />);
    expect(screen.getByText('agent ran on the host')).toBeTruthy();
    // A session refused at launch (ranOn null) ran nowhere and does not count.
    rerender(<ProofSummary content={{ ...content, sessions: [{ ranOn: 'linux_namespaces' }, { ranOn: null }] }} />);
    expect(screen.getByText('agent isolated')).toBeTruthy();
    // Packs written before sessions recorded this have no ranOn: they ran on the host.
    rerender(<ProofSummary content={{ ...content, sessions: [{}] }} />);
    expect(screen.getByText('agent ran on the host')).toBeTruthy();
  });
});
