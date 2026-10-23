import { describe, expect, it } from 'vitest';
import { capabilityList, costText, formatTokens } from './agents';
import type { CapabilityMatrix, ProviderUsage } from './types';

const u = (o: Partial<ProviderUsage>): ProviderUsage => ({ provider: 'claude', sessions: 1, live: 0, inputTokens: 0, outputTokens: 0, cacheReadTokens: 0, cacheCreationTokens: 0, turns: 0, costUsdEstimate: null, sessionsWithUsage: 0, ...o });

describe('agents helpers', () => {
  it('formats token counts compactly', () => {
    expect([formatTokens(950), formatTokens(1234), formatTokens(48_210), formatTokens(2_500_000)]).toEqual(['950', '1.2k', '48k', '2.5M']);
  });
  it('never presents an estimate as a bill', () => {
    expect(costText(u({ costUsdEstimate: 0.412 }), 'claude.ai')).toBe('$0.41 provider estimate (subscription: not what you are billed)');
    expect(costText(u({}), 'chatgpt')).toBe('subscription: no per-token bill reported');
    expect(costText(u({ provider: 'claude-local', costUsdEstimate: 1 }), null)).toBe('local model: no provider cost');
    expect(costText(u({ costUsdEstimate: 2 }), 'api key')).toBe('$2.00 provider estimate');
  });
  it('lists every capability with its note', () => {
    const m = { resume: { supported: false, note: 'n' } } as unknown as CapabilityMatrix;
    const l = capabilityList(m);
    expect(l).toHaveLength(10);
    expect(l.find((c) => c.key === 'resume')).toMatchObject({ supported: false, note: 'n' });
  });
});
