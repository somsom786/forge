import { describe, expect, it } from 'vitest';
import { backendShort } from './execution';

describe('execution backend labels', () => {
  it('never presents trusted local or an unknown value as isolated', () => {
    expect(backendShort('trusted_local')).toMatchObject({ isolated: false, tone: 'warn' });
    expect(backendShort('linux_namespaces').isolated).toBe(true);
    expect(backendShort('something_new')).toMatchObject({ isolated: false, tone: 'warn' });
    expect(backendShort(undefined).isolated).toBe(false);
  });
});
