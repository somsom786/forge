import { useEffect, useMemo, useState } from 'react';
import * as Dialog from '@radix-ui/react-dialog';
import { X } from 'lucide-react';
import { api } from '../lib/api';
import { createMission, guarded, refreshProviders, startMission } from '../lib/actions';
import { ContractEditor } from './ContractEditor';
import { contractIsEmpty, normalizeContract } from '../lib/verification';
import type { Contract, ContextPackage } from '../lib/types';
import { previewSummary } from '../lib/knowledge';
import { ContextView } from './ContextView';
import { errMsg, providerLabel } from '../lib/format';
import { useStore } from '../lib/store';
import type { PermissionProfile, ProviderId, ProviderInfo } from '../lib/types';
import { defaultModel, modelRequired, modelSatisfied } from '../lib/provider';
import { Kbd } from './ui';

export function providerAvailability(p: ProviderInfo): { ok: boolean; reason: string | null } {
  if (p.id === 'acp' && p.models.length === 0) return { ok: false, reason: 'No ACP agent is registered. Register one on the Agents screen first.' };
  if (!p.adapterAvailable) return { ok: false, reason: `${p.name} has no adapter in this build yet, so missions cannot use it.` };
  if (!p.installed) return { ok: false, reason: `${p.name} is not available: ${p.notes.length ? p.notes.join(' ') : 'not found on this machine.'}` };
  if (p.signedIn === false) return { ok: false, reason: `${p.name} is installed but not signed in. Sign in with its CLI first.` };
  return { ok: true, reason: null };
}

export function billingNote(p: ProviderInfo): string {
  if (p.id === 'claude-local') return 'Runs on this machine: no provider billing.';
  if (p.id === 'acp') return 'Each ACP agent signs in on its own. FORGE removes API keys from its environment but cannot tell how it bills.';
  const m = (p.authMethod ?? '').toLowerCase();
  if (!p.authMethod) return 'Auth method not reported. FORGE cannot confirm how usage is billed.';
  if (m.includes('claude.ai') || m.includes('subscription') || m.includes('chatgpt') || m.includes('oauth')) {
    return `Signed in via ${p.authMethod}: billed to your subscription. Usage figures in FORGE are provider estimates.`;
  }
  return `Auth method "${p.authMethod}": billing depends on that account. FORGE cannot confirm cost.`;
}

const PROFILES: { id: PermissionProfile; label: string; help: string }[] = [
  { id: 'default', label: 'Ask before edits and commands', help: 'Every file edit and shell command waits for your approval.' },
  { id: 'accept_edits', label: 'Auto-approve edits inside the worktree; commands still ask', help: 'File edits within the mission worktree proceed; commands still wait for you.' },
];

const EMPTY_CONTRACT: Contract = { version: 1, inScope: [], outOfScope: [], criteria: [], checks: [], securityConstraints: [], reviewExpectations: null };

export function NewMissionDialog() {
  const { open, projectId: presetProject } = useStore((s) => s.newMission);
  const close = useStore((s) => s.closeNewMission);
  const projects = useStore((s) => s.projects);
  const currentProjectId = useStore((s) => s.currentProjectId);
  const providers = useStore((s) => s.providers);
  const navigate = useStore((s) => s.navigate);

  const [projectId, setProjectId] = useState('');
  const [objective, setObjective] = useState('');
  const [title, setTitle] = useState('');
  const [provider, setProvider] = useState<ProviderId | ''>('');
  const [profile, setProfile] = useState<PermissionProfile>('default');
  const [model, setModel] = useState('');
  const [busy, setBusy] = useState(false);
  const [contract, setContract] = useState<Contract>(EMPTY_CONTRACT);
  const [showContract, setShowContract] = useState(false);
  const [useMemory, setUseMemory] = useState(true);
  const [pkg, setPkg] = useState<ContextPackage | null>(null);
  const [pkgError, setPkgError] = useState<string | null>(null);
  const [showPkg, setShowPkg] = useState(false);

  // Reset whenever the dialog opens.
  useEffect(() => {
    if (!open) return;
    setProjectId(presetProject ?? currentProjectId ?? projects[0]?.id ?? '');
    setObjective('');
    setTitle('');
    setProfile('default');
    setBusy(false);
    setContract(EMPTY_CONTRACT);
    setShowContract(false);
    setUseMemory(true);
    setPkg(null);
    setShowPkg(false);
    void refreshProviders();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const firstAvailable = useMemo(() => providers?.find((p) => providerAvailability(p).ok)?.id ?? '', [providers]);
  useEffect(() => {
    if (!open) return;
    const cur = providers?.find((p) => p.id === provider);
    if (!cur || !providerAvailability(cur).ok) setProvider(firstAvailable);
  }, [open, providers, provider, firstAvailable]);

  const selected = providers?.find((p) => p.id === provider);
  // Debounced preview of the verified knowledge that would be attached.
  useEffect(() => {
    if (!open || !useMemory || !projectId || !objective.trim()) { setPkg(null); setPkgError(null); return; }
    let off = false;
    const t = window.setTimeout(() => {
      api.contextPreview(projectId, objective.trim(), [], []).then(
        (p) => { if (!off) { setPkg(p); setPkgError(null); } },
        (e) => { if (!off) setPkgError(errMsg(e)); },
      );
    }, 600);
    return () => { off = true; window.clearTimeout(t); };
  }, [open, useMemory, projectId, objective]);

  const hasList = (selected?.models.length ?? 0) > 0;
  // Default to the first listed model whenever the provider changes.
  useEffect(() => {
    setModel(defaultModel(selected));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [provider]);
  const canSubmit = !!projectId && objective.trim().length > 0 && !!selected && providerAvailability(selected).ok && modelSatisfied(selected, model) && !busy;

  const submit = async (start: boolean) => {
    if (!canSubmit || !provider) return;
    setBusy(true);
    const wantsContract = !contractIsEmpty(contract);
    let m = await createMission(
      {
        projectId,
        objective: objective.trim(),
        title: title.trim() || undefined,
        provider,
        model: model.trim() || undefined,
        permissionProfile: profile,
      },
      false,
    );
    let ready = !!m;
    if (m && wantsContract) {
      // The contract is saved on the new mission before anything runs.
      const base = await guarded(api.contractGet(m.id), 'Mission created but its contract could not be read');
      const saved = base && (await guarded(api.contractSet(m.id, { ...base, ...normalizeContract(contract), version: base.version }), 'Mission created but the contract was not saved'));
      if (!saved) ready = false;
    }
    if (m && !useMemory) {
      // Only call when the user changed the default.
      const r = await guarded(api.missionMemorySet(m.id, false, []), 'Mission created but memory could not be turned off');
      if (!r) ready = false;
      else m = r;
    }
    if (m && start && ready) m = (await startMission(m.id)) ?? m;
    setBusy(false);
    if (m) {
      close();
      navigate({ name: 'mission', id: m.id });
    }
  };

  return (
    <Dialog.Root open={open} onOpenChange={(o) => !o && close()}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-50 bg-black/60" />
        <Dialog.Content
          className="fixed left-1/2 top-1/2 z-50 flex max-h-[90vh] w-[640px] max-w-[94vw] -translate-x-1/2 -translate-y-1/2 flex-col rounded-lg border border-line-strong bg-panel shadow-2xl shadow-black/60"
          onKeyDown={(e) => {
            if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') {
              e.preventDefault();
              void submit(true);
            }
          }}
          aria-describedby={undefined}
        >
          <div className="flex items-center justify-between border-b border-line px-4 py-3">
            <Dialog.Title className="m-0 text-[14px] font-semibold">New mission</Dialog.Title>
            <Dialog.Close className="btn btn-ghost btn-sm !px-1" aria-label="Close">
              <X className="size-4" />
            </Dialog.Close>
          </div>
          <div className="min-h-0 flex-1 space-y-4 overflow-y-auto p-4">
            <div>
              <label className="label" htmlFor="nm-project">Project</label>
              <select id="nm-project" className="input" value={projectId} onChange={(e) => setProjectId(e.target.value)}>
                {projects.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.name} ({p.defaultBranch})
                  </option>
                ))}
              </select>
            </div>
            <div>
              <label className="label" htmlFor="nm-objective">Objective (required)</label>
              <textarea
                id="nm-objective"
                className="input min-h-[110px] resize-y"
                placeholder="Describe what the agent should accomplish. Be specific about files, behavior and how to verify."
                value={objective}
                onChange={(e) => setObjective(e.target.value)}
                autoFocus
              />
            </div>
            <div>
              <label className="label" htmlFor="nm-title">Title (optional; derived from the objective if empty)</label>
              <input id="nm-title" className="input" value={title} onChange={(e) => setTitle(e.target.value)} />
            </div>

            <fieldset className="m-0 border-0 p-0">
              <legend className="label">Provider</legend>
              {providers === null && <div className="text-muted">Detecting providers…</div>}
              <div className="space-y-1.5">
                {providers?.map((p) => {
                  const av = providerAvailability(p);
                  const checked = provider === p.id;
                  return (
                    <label
                      key={p.id}
                      className={`flex items-start gap-3 rounded-md border px-3 py-2 ${
                        av.ok ? 'cursor-pointer hover:bg-hover' : 'cursor-not-allowed opacity-60'
                      } ${checked ? 'border-accent bg-accent-dim' : 'border-line-strong'}`}
                    >
                      <input type="radio" name="provider" className="mt-1" checked={checked} disabled={!av.ok} onChange={() => setProvider(p.id)} />
                      <div className="min-w-0 flex-1">
                        <div className="flex flex-wrap items-center gap-x-2">
                          <span className="font-medium">{providerLabel(p.id)}</span>
                          <span className="mono text-[11px] text-muted">{p.version ?? 'version unknown'}</span>
                          <span className="text-[11px] text-muted">
                            {p.installed ? 'installed' : 'not installed'}
                            {p.signedIn === true ? ' · signed in' : p.signedIn === false ? ' · not signed in' : ''}
                            {p.authMethod ? ` · ${p.authMethod}` : ''}
                            {p.plan ? ` · plan ${p.plan}` : ''}
                          </span>
                        </div>
                        {av.reason ? <div className="text-warn">{av.reason}</div> : <div className="text-muted">{billingNote(p)}</div>}
                        {p.id === 'claude-local' && p.notes.length > 0 && av.ok && (
                          <ul className="m-0 mt-1 list-disc pl-4 text-muted">{p.notes.map((n, i) => <li key={i}>{n}</li>)}</ul>
                        )}
                      </div>
                    </label>
                  );
                })}
              </div>
            </fieldset>

            <fieldset className="m-0 border-0 p-0">
              <legend className="label">Permission profile</legend>
              <div className="space-y-1.5">
                {PROFILES.map((pr) => (
                  <label key={pr.id} className={`flex cursor-pointer items-start gap-3 rounded-md border px-3 py-2 hover:bg-hover ${profile === pr.id ? 'border-accent bg-accent-dim' : 'border-line-strong'}`}>
                    <input type="radio" name="profile" className="mt-1" checked={profile === pr.id} onChange={() => setProfile(pr.id)} />
                    <div>
                      <div className="font-medium">{pr.label}</div>
                      <div className="text-muted">{pr.help}</div>
                    </div>
                  </label>
                ))}
              </div>
            </fieldset>

            <div>
              {hasList ? (
                <>
                  <label className="label" htmlFor="nm-model">{selected?.id === 'acp' ? 'ACP agent' : 'Model'}{modelRequired(selected) ? ' (required)' : ''}</label>
                  <select id="nm-model" className="input mono" value={model} onChange={(e) => setModel(e.target.value)}>
                    {!modelRequired(selected) && <option value="">Provider default</option>}
                    {selected!.models.map((m) => (
                      <option key={m} value={m}>{m}</option>
                    ))}
                  </select>
                </>
              ) : (
                <>
                  <label className="label" htmlFor="nm-model">Model (optional; provider default if empty)</label>
                  <input id="nm-model" className="input mono" value={model} onChange={(e) => setModel(e.target.value)} placeholder="e.g. a model alias accepted by the CLI" />
                </>
              )}
            </div>
            <div className="rounded-md border border-line-strong p-3" aria-label="Project memory">
              <label className="flex items-center gap-2">
                <input type="checkbox" checked={useMemory} onChange={(e) => setUseMemory(e.target.checked)} />
                <span className="font-medium">Use verified project knowledge</span>
              </label>
              <div className="mt-1 text-[12px] text-muted">
                {!useMemory ? 'Memory will be off for this mission.' : !objective.trim() ? 'Write an objective to see which notes match.' : pkgError ? `Preview unavailable: ${pkgError}` : pkg ? `${previewSummary(pkg.items)}${pkg.code?.items.length ? `; plus ${pkg.code.items.length} relevant-code pointer${pkg.code.items.length === 1 ? '' : 's'}` : ''}` : 'Checking…'}
              </div>
              {pkg && useMemory && (
                <>
                  <button type="button" className="btn btn-sm btn-ghost mt-1" aria-expanded={showPkg} onClick={() => setShowPkg(!showPkg)}>See what would be sent</button>
                  {showPkg && <div className="mt-2"><ContextView ctx={{ items: pkg.items, omitted: pkg.omitted, charBudget: pkg.charBudget, text: pkg.text, sha256: pkg.sha256, code: pkg.code }} /></div>}
                </>
              )}
            </div>
            <div className="rounded-md border border-line-strong">
              <button type="button" className="flex w-full items-center justify-between px-3 py-2 text-left hover:bg-hover" aria-expanded={showContract} onClick={() => setShowContract(!showContract)}>
                <span className="font-medium">Success criteria &amp; checks <span className="font-normal text-muted">(optional)</span></span>
                <span className="text-muted">{showContract ? 'Hide' : 'Show'}</span>
              </button>
              {showContract && (
                <div className="border-t border-line p-3">
                  <ContractEditor value={contract} onChange={setContract} projectId={projectId} />
                </div>
              )}
            </div>
          </div>
          <div className="flex items-center justify-between gap-2 border-t border-line px-4 py-3">
            <span className="text-muted">Worktree and branch are created when the mission is created.</span>
            <div className="flex gap-2">
              <button className="btn" disabled={!canSubmit} onClick={() => void submit(false)}>
                Create
              </button>
              <button className="btn btn-primary" disabled={!canSubmit} onClick={() => void submit(true)}>
                Create &amp; start <Kbd>Ctrl+Enter</Kbd>
              </button>
            </div>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
