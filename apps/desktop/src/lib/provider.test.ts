import { describe, expect, it } from 'vitest';
import { costText, credentialChip, defaultModel, modelRequired, modelSatisfied } from './provider';
import type { ProviderInfo } from './types';

const p = (id: ProviderInfo['id'], models: string[]) => ({ id, models }) as ProviderInfo;

describe('credentialChip', () => {
  it('treats none as subscription login', () => expect(credentialChip('none')).toEqual({ kind: 'subscription' }));
  it('shows local-endpoint as a neutral local chip, not a warning', () => {
    expect(credentialChip('local-endpoint', 'forge-qwen')).toEqual({ kind: 'local', model: 'forge-qwen' });
    expect(credentialChip('local-endpoint')).toEqual({ kind: 'local', model: null });
  });
  it('shows scripted as a neutral chip, not a billing warning', () => expect(credentialChip('scripted')).toEqual({ kind: 'scripted' }));
  it('warns for any other source', () => expect(credentialChip('ANTHROPIC_API_KEY')).toEqual({ kind: 'api-warning', source: 'ANTHROPIC_API_KEY' }));
  it('reports missing source as unreported', () => expect(credentialChip(undefined)).toEqual({ kind: 'unreported' }));
});

describe('model selection', () => {
  it('defaults to the first model', () => {
    expect(defaultModel(p('claude-local', ['forge-a', 'b']))).toBe('forge-a');
    expect(defaultModel(p('claude', []))).toBe('');
    expect(defaultModel(undefined)).toBe('');
  });
  it('requires a listed model only for claude-local', () => {
    const local = p('claude-local', ['forge-a']);
    expect(modelRequired(local)).toBe(true);
    expect(modelSatisfied(local, '')).toBe(false);
    expect(modelSatisfied(local, 'nope')).toBe(false);
    expect(modelSatisfied(local, 'forge-a')).toBe(true);
    expect(modelSatisfied(p('claude', []), '')).toBe(true);
  });
});

describe('costText', () => {
  it('omits absent costs entirely', () => {
    expect(costText(undefined)).toBeNull();
    expect(costText(null)).toBeNull();
  });
  it('formats reported costs as estimates', () => {
    expect(costText(0.412)).toBe('$0.412 est.');
    expect(costText(2.5)).toBe('$2.50 est.');
  });
});
