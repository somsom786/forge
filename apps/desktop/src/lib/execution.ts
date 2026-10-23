// Execution backends: how the UI describes where a command ran. Trusted local is never presented as isolated.
import type { Tone } from './format';
import type { BackendProperty, ExecutionBackendId, ExecutionBackendInfo } from './types';

export const BACKEND_SHORT: Record<ExecutionBackendId, { label: string; tone: Tone; isolated: boolean }> = {
  trusted_local: { label: 'Not isolated', tone: 'warn', isolated: false },
  linux_namespaces: { label: 'Isolated (Linux namespaces)', tone: 'ok', isolated: true },
  windows_sandbox: { label: 'Windows Sandbox', tone: 'idle', isolated: true },
  isolated_wsl: { label: 'Isolated WSL', tone: 'idle', isolated: true },
};

export function backendShort(id: string | null | undefined): { label: string; tone: Tone; isolated: boolean } {
  return BACKEND_SHORT[id as ExecutionBackendId] ?? { label: id ? `Unknown backend "${id}"` : 'Unknown backend', tone: 'warn', isolated: false };
}

export const PROPERTY_ROWS: { key: keyof Omit<ExecutionBackendInfo['capabilities'], 'operatingSystems' | 'interactiveTerminal' | 'concurrentInstances'>; label: string }[] = [
  { key: 'filesystemIsolation', label: 'Files' },
  { key: 'credentialExposure', label: 'Credentials' },
  { key: 'networkControl', label: 'Network' },
  { key: 'processIsolation', label: 'Processes' },
  { key: 'resourceLimits', label: 'Resources' },
  { key: 'workspaceMapping', label: 'Workspace' },
  { key: 'persistentState', label: 'Leftovers' },
];

export function levelTone(p: BackendProperty): Tone {
  return p.level === 'enforced' ? 'ok' : p.level === 'partial' ? 'warn' : 'bad';
}
