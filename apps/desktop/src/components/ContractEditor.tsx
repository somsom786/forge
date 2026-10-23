import { useState } from 'react';
import { Lightbulb, Plus, Trash2 } from 'lucide-react';
import { api } from '../lib/api';
import { errMsg } from '../lib/format';
import { useStore } from '../lib/store';
import type { CheckSpec, Contract } from '../lib/types';
import { CHECK_KINDS, linesToList, listToLines, slugId } from '../lib/verification';

interface Props {
  value: Contract;
  onChange: (c: Contract) => void;
  disabled?: boolean;
  projectId: string;
}

/** Editable contract: scope globs, acceptance criteria and checks. Nothing is added or run automatically. */
export function ContractEditor({ value, onChange, disabled, projectId }: Props) {
  const notify = useStore((s) => s.notify);
  const [suggestions, setSuggestions] = useState<CheckSpec[] | null>(null);
  const [picked, setPicked] = useState<Set<number>>(new Set());
  const [loading, setLoading] = useState(false);
  const set = (patch: Partial<Contract>) => onChange({ ...value, ...patch });

  const taken = () => new Set(value.checks.map((c) => c.id).filter(Boolean));

  const suggest = async () => {
    setLoading(true);
    try {
      setSuggestions(await api.checksSuggest(projectId));
      setPicked(new Set());
    } catch (e) {
      notify('error', `Could not suggest checks: ${errMsg(e)}`);
    } finally {
      setLoading(false);
    }
  };

  const addPicked = () => {
    const t = taken();
    const added: CheckSpec[] = [];
    suggestions?.forEach((s, i) => {
      if (!picked.has(i)) return;
      const id = slugId(s.name, t);
      t.add(id);
      added.push({ ...s, id });
    });
    set({ checks: [...value.checks, ...added] });
    setSuggestions(null);
  };

  const updateCheck = (i: number, patch: Partial<CheckSpec>) => set({ checks: value.checks.map((c, j) => (j === i ? { ...c, ...patch } : c)) });
  const removeCheck = (i: number) => {
    const gone = value.checks[i];
    set({
      checks: value.checks.filter((_, j) => j !== i),
      criteria: value.criteria.map((k) => ({ ...k, verifiedBy: k.verifiedBy.filter((v) => v !== gone.id) })),
    });
  };
  const addCheck = () =>
    set({ checks: [...value.checks, { id: slugId('new-check', taken()), name: '', command: '', kind: 'test', timeoutSecs: 300, required: true }] });

  return (
    <fieldset disabled={disabled} className="m-0 min-w-0 space-y-4 border-0 p-0">
      <div className="grid grid-cols-2 gap-3">
        <div>
          <label className="label" htmlFor="ce-in">In-scope globs (one per line; empty = unrestricted)</label>
          <textarea id="ce-in" className="input mono min-h-[64px] text-[12px]" value={listToLines(value.inScope)} onChange={(e) => set({ inScope: linesToList(e.target.value) })} placeholder={'src/\ntests/'} />
        </div>
        <div>
          <label className="label" htmlFor="ce-out">Out-of-scope globs (one per line)</label>
          <textarea id="ce-out" className="input mono min-h-[64px] text-[12px]" value={listToLines(value.outOfScope)} onChange={(e) => set({ outOfScope: linesToList(e.target.value) })} placeholder={'package-lock.json\n.github/'} />
        </div>
      </div>

      <section aria-label="Checks">
        <div className="mb-1.5 flex items-center gap-2">
          <h4 className="m-0 text-[12px] font-medium uppercase tracking-wide text-muted">Checks</h4>
          <span className="text-[11px] text-faint">Commands run in the mission worktree on your machine, only when you press Run.</span>
          <div className="flex-1" />
          <button type="button" className="btn btn-sm" onClick={() => void suggest()} disabled={loading || !projectId}>
            <Lightbulb className="size-3.5" aria-hidden /> {loading ? 'Looking…' : 'Suggest checks from this project'}
          </button>
          <button type="button" className="btn btn-sm" onClick={addCheck}>
            <Plus className="size-3.5" aria-hidden /> Add check
          </button>
        </div>
        {suggestions && (
          <div className="mb-2 rounded-md border border-accent/40 bg-accent-dim/40 p-2">
            {suggestions.length === 0 ? (
              <div className="text-muted">No checks could be suggested for this project.</div>
            ) : (
              <>
                <div className="mb-1 text-muted">Tick the ones to add. Nothing is added until you confirm.</div>
                {suggestions.map((s, i) => (
                  <label key={i} className="flex items-start gap-2 py-0.5">
                    <input
                      type="checkbox"
                      className="mt-1"
                      checked={picked.has(i)}
                      onChange={() => setPicked((p) => { const n = new Set(p); if (n.has(i)) n.delete(i); else n.add(i); return n; })}
                    />
                    <span>
                      <span className="font-medium">{s.name}</span> <span className="text-[11px] text-faint">{s.kind}</span>
                      <span className="mono block text-[12px] text-muted">{s.command}</span>
                    </span>
                  </label>
                ))}
              </>
            )}
            <div className="mt-1.5 flex gap-2">
              <button type="button" className="btn btn-sm btn-primary" disabled={picked.size === 0} onClick={addPicked}>Add {picked.size} selected</button>
              <button type="button" className="btn btn-sm" onClick={() => setSuggestions(null)}>Dismiss</button>
            </div>
          </div>
        )}
        {value.checks.length === 0 && <div className="text-muted">No checks defined.</div>}
        <div className="space-y-2">
          {value.checks.map((c, i) => (
            <div key={i} className="rounded-md border border-line-strong p-2">
              <div className="grid grid-cols-[1fr_120px_90px] gap-2">
                <input className="input" aria-label="Check name" placeholder="Name" value={c.name} onChange={(e) => updateCheck(i, { name: e.target.value })} />
                <select className="input" aria-label="Check kind" value={c.kind} onChange={(e) => updateCheck(i, { kind: e.target.value as CheckSpec['kind'] })}>
                  {CHECK_KINDS.map((k) => <option key={k}>{k}</option>)}
                </select>
                <input className="input" type="number" min={1} aria-label="Timeout in seconds" title="Timeout (seconds)" value={c.timeoutSecs} onChange={(e) => updateCheck(i, { timeoutSecs: Math.max(1, Number(e.target.value) || 1) })} />
              </div>
              <input className="input mono mt-2 text-[12px]" aria-label="Command" placeholder="Command, e.g. npm test" value={c.command} onChange={(e) => updateCheck(i, { command: e.target.value })} />
              <div className="mt-1.5 flex items-center gap-3">
                <label className="flex items-center gap-1.5 text-muted"><input type="checkbox" checked={c.required} onChange={(e) => updateCheck(i, { required: e.target.checked })} /> Required</label>
                <button type="button" className="btn btn-sm btn-ghost btn-danger ml-auto" onClick={() => removeCheck(i)} aria-label={`Remove check ${c.name || i + 1}`}>
                  <Trash2 className="size-3.5" aria-hidden /> Remove
                </button>
              </div>
            </div>
          ))}
        </div>
      </section>

      <section aria-label="Acceptance criteria">
        <div className="mb-1.5 flex items-center">
          <h4 className="m-0 text-[12px] font-medium uppercase tracking-wide text-muted">Acceptance criteria</h4>
          <div className="flex-1" />
          <button type="button" className="btn btn-sm" onClick={() => set({ criteria: [...value.criteria, { id: '', text: '', verifiedBy: [], manual: false, scopeRule: false }] })}>
            <Plus className="size-3.5" aria-hidden /> Add criterion
          </button>
        </div>
        {value.criteria.length === 0 && <div className="text-muted">No criteria defined.</div>}
        <div className="space-y-2">
          {value.criteria.map((k, i) => {
            const upd = (patch: Partial<typeof k>) => set({ criteria: value.criteria.map((x, j) => (j === i ? { ...x, ...patch } : x)) });
            return (
              <div key={i} className="rounded-md border border-line-strong p-2">
                <textarea className="input min-h-[44px]" aria-label="Criterion text" placeholder="What must be true when this is done?" value={k.text} onChange={(e) => upd({ text: e.target.value })} />
                <div className="mt-1.5 flex flex-wrap items-start gap-x-4 gap-y-1">
                  <label className="flex items-center gap-1.5 text-muted"><input type="checkbox" checked={k.manual} onChange={(e) => upd({ manual: e.target.checked })} /> Needs human verdict</label>
                  <label className="flex items-center gap-1.5 text-muted"><input type="checkbox" checked={k.scopeRule} onChange={(e) => upd({ scopeRule: e.target.checked })} /> No unrelated changes (scope rule)</label>
                  <button type="button" className="btn btn-sm btn-ghost btn-danger ml-auto" onClick={() => set({ criteria: value.criteria.filter((_, j) => j !== i) })} aria-label={`Remove criterion ${i + 1}`}>
                    <Trash2 className="size-3.5" aria-hidden /> Remove
                  </button>
                </div>
                {value.checks.length > 0 && (
                  <div className="mt-1.5">
                    <label className="label" htmlFor={`ce-vb-${i}`}>Verified by checks (Ctrl/Shift-click for several)</label>
                    <select
                      id={`ce-vb-${i}`}
                      multiple
                      className="input min-h-[56px]"
                      value={k.verifiedBy}
                      onChange={(e) => upd({ verifiedBy: Array.from(e.target.selectedOptions, (o) => o.value) })}
                    >
                      {value.checks.map((c) => (
                        <option key={c.id} value={c.id}>{c.name || c.id}</option>
                      ))}
                    </select>
                  </div>
                )}
              </div>
            );
          })}
        </div>
      </section>
    </fieldset>
  );
}
