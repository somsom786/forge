import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { StoragePanel } from './StoragePanel';

const mocks = vi.hoisted(() => ({
  usage: vi.fn(), preview: vi.fn(), apply: vi.fn(), notify: vi.fn(),
}));
vi.mock('../lib/api', () => ({ api: { storageUsage: mocks.usage, storageCleanupPreview: mocks.preview, storageCleanupApply: mocks.apply } }));
vi.mock('../lib/store', () => ({ useStore: (select: (state: { notify: typeof mocks.notify }) => unknown) => select({ notify: mocks.notify }) }));
vi.mock('./Modal', () => ({ Modal: ({ open, children, footer }: { open: boolean; children: React.ReactNode; footer: React.ReactNode }) => open ? <div role="dialog">{children}{footer}</div> : null }));

afterEach(() => { cleanup(); vi.resetAllMocks(); });

it('shows cleanup failures and retains them until a successful retry', async () => {
  mocks.usage.mockResolvedValue({ databaseBytes: 0, events: 0, runBytes: 0, runFiles: 0, proofBytes: 0, proofFiles: 0, experimentBytes: 0, worktrees: 0, untrackedWorktrees: [] });
  mocks.preview.mockResolvedValue({ olderThanDays: 30, missions: [{ id: 'm', title: 'Mission', state: 'cancelled', events: 1, files: ['run.log'], bytes: 5 }], orphanFiles: [], experiments: [], skippedExperiments: [], events: 1, files: 1, bytes: 5, token: 'fixture' });
  mocks.apply.mockResolvedValueOnce({ eventsDeleted: 0, filesDeleted: 0, bytesFreed: 0, failures: ['run.log: file is locked', 'mission m: transcript retained'] })
    .mockResolvedValueOnce({ eventsDeleted: 1, filesDeleted: 1, bytesFreed: 5, failures: [] });
  render(<StoragePanel />);
  fireEvent.click(screen.getByRole('button', { name: /preview clean-up/i }));
  fireEvent.click(await screen.findByRole('button', { name: /remove what is listed/i }));
  expect(await screen.findByText('run.log: file is locked')).toBeTruthy();
  expect(screen.getByText('mission m: transcript retained')).toBeTruthy();
  expect(screen.getByText(/preview clean-up again before retrying/i)).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: /preview clean-up/i }));
  fireEvent.click(await screen.findByRole('button', { name: /remove what is listed/i }));
  await waitFor(() => expect(screen.queryByText('run.log: file is locked')).toBeNull());
  expect(mocks.apply).toHaveBeenCalledTimes(2);
});
