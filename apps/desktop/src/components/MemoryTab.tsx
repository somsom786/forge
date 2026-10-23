import { useEffect, useState } from 'react';
import { api } from '../lib/api';
import { errMsg } from '../lib/format';
import { useStore } from '../lib/store';
import type { ContextPackage, ContextPackageRow, KnowledgePage, Mission } from '../lib/types';
import { ContextView, type ContextShape } from './ContextView';
import { Badge, Callout } from './ui';

export function MemoryTab({ mission }: { mission: Mission }) {
  const navigate = useStore((s) => s.navigate);
  const started = !['draft', 'ready'].includes(mission.state);
  const [rows, setRows] = useState<ContextPackageRow[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let off = false;
    api.contextForMission(mission.id).then((r) => !off && setRows(r), (e) => !off && setError(errMsg(e)));
    return () => { off = true; };
  }, [mission.id, mission.state]);

  const open = (id: string) => navigate({ name: 'knowledge', pageId: id });

  return (
    <div className="min-h-0 flex-1 space-y-4 overflow-y-auto p-3">
      {error && <Callout tone="bad" title="Could not load context"><pre className="whitespace-pre-wrap text-[12px]">{error}</pre></Callout>}
      <Settings mission={mission} readOnly={started} />
      {started ? (
        rows === null ? <div className="text-muted">Loading…</div> : rows.length === 0 ? (
          <div className="rounded-md border border-dashed border-line-strong px-3 py-4 text-muted">
            {mission.useMemory ? 'No project knowledge qualified.' : 'Memory was off for this mission.'}
          </div>
        ) : (
          <ContextView heading="What the agent was told" ctx={newest(rows)} onOpenPage={open} />
        )
      ) : (
        <Preview mission={mission} />
      )}
    </div>
  );
}

function newest(rows: ContextPackageRow[]): ContextShape {
  const r = [...rows].sort((a, b) => b.createdAt.localeCompare(a.createdAt))[0];
  return { items: r.items, omitted: r.omitted, charBudget: r.charBudget, text: r.injectedText, sha256: r.injectedSha256, code: r.params.relevantCode };
}

function Settings({ mission, readOnly }: { mission: Mission; readOnly: boolean }) {
  const notify = useStore((s) => s.notify);
  const upsert = useStore((s) => s.upsertMission);
  const [use, setUse] = useState(mission.useMemory);
  const [pins, setPins] = useState<string[]>(mission.memoryPins);
  const [pages, setPages] = useState<KnowledgePage[]>([]);
  const [busy, setBusy] = useState(false);

  useEffect(() => { setUse(mission.useMemory); setPins(mission.memoryPins); }, [mission.useMemory, mission.memoryPins]);
  useEffect(() => {
    let off = false;
    api.knowledgeList(mission.projectId).then((p) => !off && setPages(p), (e) => notify('error', errMsg(e)));
    return () => { off = true; };
  }, [mission.projectId, notify]);

  const dirty = use !== mission.useMemory || pins.join() !== mission.memoryPins.join();
  const save = async () => {
    setBusy(true);
    try { upsert(await api.missionMemorySet(mission.id, use, pins)); notify('info', 'Memory settings saved'); } catch (e) { notify('error', errMsg(e)); } finally { setBusy(false); }
  };
  const candidates = pages.filter((p) => p.effectiveState !== 'archived' && p.effectiveState !== 'superseded');

  return (
    <section aria-label="Memory settings" className="rounded-md border border-line bg-panel p-3">
      <label className="flex items-center gap-2">
        <input type="checkbox" checked={use} disabled={readOnly || busy} onChange={(e) => setUse(e.target.checked)} />
        <span className="font-medium">Use verified project knowledge</span>
      </label>
      {readOnly && <div className="mt-1 text-[12px] text-muted">Fixed at the first turn.</div>}
      <div className="mt-2">
        <div className="label">Pinned pages {readOnly ? '' : '(included even if not verified)'}</div>
        {candidates.length === 0 ? <div className="text-muted">No knowledge pages in this project.</div> : (
          <ul className="m-0 max-h-44 list-none overflow-y-auto p-0">
            {candidates.map((p) => (
              <li key={p.id}>
                <label className="flex items-start gap-2 py-0.5">
                  <input type="checkbox" className="mt-1" checked={pins.includes(p.id)} disabled={readOnly || busy} onChange={(e) => setPins(e.target.checked ? [...pins, p.id] : pins.filter((x) => x !== p.id))} />
                  <span className="min-w-0">
                    <span className="break-words">{p.title}</span>{' '}
                    <Badge tone={p.effectiveState === 'verified' ? 'ok' : 'warn'}>{p.effectiveState}</Badge>
                    {pins.includes(p.id) && p.effectiveState !== 'verified' && <span className="block text-[11.5px] text-warn">Not verified: the agent will be told this as pinned, unverified context.</span>}
                  </span>
                </label>
              </li>
            ))}
          </ul>
        )}
      </div>
      {!readOnly && <button className="btn btn-primary mt-2" disabled={!dirty || busy} onClick={() => void save()}>Save memory settings</button>}
    </section>
  );
}

function Preview({ mission }: { mission: Mission }) {
  const notify = useStore((s) => s.notify);
  const [pkg, setPkg] = useState<ContextPackage | null>(null);
  useEffect(() => {
    if (!mission.useMemory && mission.memoryPins.length === 0) { setPkg(null); return; }
    let off = false;
    (async () => {
      try {
        const c = await api.contractGet(mission.id).catch(() => null);
        const p = await api.contextPreview(mission.projectId, mission.objective, c?.inScope ?? [], mission.memoryPins);
        if (!off) setPkg(p);
      } catch (e) { if (!off) notify('error', errMsg(e)); }
    })();
    return () => { off = true; };
  }, [mission.id, mission.projectId, mission.objective, mission.useMemory, mission.memoryPins, notify]);

  if (!mission.useMemory && mission.memoryPins.length === 0) return <div className="text-muted">Memory is off for this mission.</div>;
  if (!pkg) return <div className="text-muted">Building preview…</div>;
  return <ContextView heading="Preview: what the agent would be told if started now" ctx={{ items: pkg.items, omitted: pkg.omitted, charBudget: pkg.charBudget, text: pkg.text, sha256: pkg.sha256, code: pkg.code }} />;
}
