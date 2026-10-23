// Agents screen helpers: token formatting, honest cost wording, capability lists.
import type { CapabilityMatrix, ProviderUsage } from './types';

export function formatTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(n >= 10_000_000 ? 0 : 1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(n >= 10_000 ? 0 : 1)}k`;
  return String(n);
}

/** What FORGE can truthfully say about cost. A subscription login is not billed per token; a local model costs nothing. */
export function costText(u: ProviderUsage, authMethod: string | null | undefined): string {
  if (u.provider === 'claude-local') return 'local model: no provider cost';
  const sub = /claude\.ai|subscription|chatgpt|oauth/i.test(authMethod ?? '');
  if (u.costUsdEstimate === null) return sub ? 'subscription: no per-token bill reported' : 'no cost reported';
  const est = `$${u.costUsdEstimate.toFixed(2)} provider estimate`;
  return sub ? `${est} (subscription: not what you are billed)` : est;
}

export const CAPABILITY_LABEL: Record<keyof CapabilityMatrix, string> = {
  sessionStart: 'Start', resume: 'Resume', streaming: 'Streaming', interrupt: 'Interrupt', toolEvents: 'Tool events',
  fileChanges: 'File changes', approvals: 'Approvals', usageReporting: 'Usage', modelSelection: 'Model choice', worktreeIntegration: 'Worktrees',
};

export function capabilityList(m: CapabilityMatrix): { key: keyof CapabilityMatrix; label: string; supported: boolean; note?: string }[] {
  return (Object.keys(CAPABILITY_LABEL) as (keyof CapabilityMatrix)[]).map((k) => ({ key: k, label: CAPABILITY_LABEL[k], supported: !!m[k]?.supported, note: m[k]?.note }));
}
