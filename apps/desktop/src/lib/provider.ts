import type { ProviderInfo } from './types';

export type CredentialChip =
  | { kind: 'subscription' }
  | { kind: 'local'; model: string | null }
  | { kind: 'scripted' }
  | { kind: 'agent' }
  | { kind: 'unreported' }
  | { kind: 'api-warning'; source: string };

/** Decides how a session's credentialSource is presented. Only unknown non-subscription sources warn about API billing. */
export function credentialChip(source: unknown, model?: unknown): CredentialChip {
  if (source === undefined || source === null || source === '') return { kind: 'unreported' };
  if (source === 'none') return { kind: 'subscription' };
  if (source === 'scripted') return { kind: 'scripted' };
  // ACP agents sign in on their own; FORGE strips API keys from their environment but cannot see how they bill.
  if (source === 'agent-managed') return { kind: 'agent' };
  if (source === 'local-endpoint') return { kind: 'local', model: typeof model === 'string' && model ? model : null };
  return { kind: 'api-warning', source: String(source) };
}

/** Default model for a provider that enumerates models: the first one (core lists forge-* tags first). */
export function defaultModel(p: ProviderInfo | undefined): string {
  return p && p.models.length > 0 ? p.models[0] : '';
}

/** A model is mandatory for claude-local, and for ACP (where the "model" names the registered agent); optional elsewhere. */
export function modelRequired(p: ProviderInfo | undefined): boolean {
  return p?.id === 'claude-local' || p?.id === 'acp';
}

export function modelSatisfied(p: ProviderInfo | undefined, chosen: string): boolean {
  return !modelRequired(p) || (chosen.trim() !== '' && (p?.models.includes(chosen) ?? false));
}

/** Cost text, or null when the provider did not report one (never "$0.00" or a placeholder). */
export function costText(usd: number | null | undefined): string | null {
  if (usd === undefined || usd === null || !Number.isFinite(usd)) return null;
  return `$${usd.toFixed(usd < 1 ? 3 : 2)} est.`;
}
