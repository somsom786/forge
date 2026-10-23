import { useCallback, useEffect, useMemo, useState } from 'react';
import { Plus, RefreshCw } from 'lucide-react';
import { api } from '../lib/api';
import { errMsg } from '../lib/format';
import { groupInventory } from '../lib/harness';
import { shortHash } from '../lib/knowledge';
import { useStore } from '../lib/store';
import type { DoctorFinding, HarnessInventory, HarnessItem, McpAuthorizationStatus } from '../lib/types';
import { FileEditor } from '../components/harness/FileEditor';
import { HistoryPanel } from '../components/harness/HistoryPanel';
import { HooksTab } from '../components/harness/HooksTab';
import { McpServerCard } from '../components/harness/McpServerCard';
import { NewFileDialog } from '../components/harness/NewFileDialog';
import { SplitLayout } from '../components/Splitter';
import { Badge, Callout, EmptyState, PageHeader } from '../components/ui';

type DetailTab = 'file' | 'hooks' | 'history';
const SEV_TONE = { problem: 'bad', warning: 'warn', info: 'idle' } as const;

export function HarnessStudio() {
  const projectId = useStore((s) => s.currentProjectId);
  const project = useStore((s) => s.projects.find((p) => p.id === s.currentProjectId));
  const tick = useStore((s) => (s.currentProjectId ? s.harnessTick[s.currentProjectId] ?? 0 : 0));
  const notify = useStore((s) => s.notify);

  const [inv, setInv] = useState<HarnessInventory | null>(null);
  const [findings, setFindings] = useState<DoctorFinding[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [tab, setTab] = useState<DetailTab>('file');
  const [draft, setDraft] = useState<{ rel: string; starter: string } | null>(null);
  const [newOpen, setNewOpen] = useState(false);
  const [showHistory, setShowHistory] = useState(false);
  const [loading, setLoading] = useState(false);

  const load = useCallback(async () => {
    if (!projectId) return;
    setLoading(true);
    try {
      const [i, f] = await Promise.all([api.harnessInventory(projectId), api.harnessDoctor(projectId)]);
      setInv(i);
      setFindings(f);
      setError(null);
    } catch (e) {
      setError(errMsg(e));
    } finally {
      setLoading(false);
    }
  }, [projectId]);

  useEffect(() => { setInv(null); setSelected(null); setDraft(null); }, [projectId]);
  useEffect(() => { void load(); }, [load, tick]);

  const groups = useMemo(() => groupInventory(inv?.items ?? []), [inv]);
  const item = inv?.items.find((i) => i.id === selected) ?? null;

  const resolve = (subject: string | null): HarnessItem | undefined =>
    subject ? inv?.items.find((i) => i.id === subject || i.rel === subject || i.path === subject) : undefined;

  const select = (id: string) => { setSelected(id); setDraft(null); setTab('file'); setShowHistory(false); };

  if (!projectId || !project) {
    return <div className="flex h-full flex-col"><PageHeader title="Harness Studio" /><EmptyState title="Choose a project">Harness Studio shows the agent configuration of a project. Add or select one first.</EmptyState></div>;
  }

  const markReviewed = async (f: DoctorFinding) => {
    const rel = resolve(f.subject)?.rel ?? f.subject;
    if (!rel) return;
    try { await api.harnessMarkReviewed(projectId, rel); notify('info', `Marked ${rel} as reviewed`); await load(); } catch (e) { notify('error', errMsg(e)); }
  };

  const empty = inv && inv.items.length === 0;

  return (
    <div className="flex h-full min-h-0 flex-col">
      <PageHeader
        title="Harness Studio"
        sub={`${project.name}: the agent configuration (instructions, settings, rules, skills, subagents, commands, MCP).`}
        actions={
          <>
            <button className="btn" onClick={() => void load()} disabled={loading}><RefreshCw className={`size-3.5 ${loading ? 'animate-spin' : ''}`} aria-hidden /> Refresh</button>
            <button className="btn" onClick={() => { setShowHistory(true); setSelected(null); setDraft(null); }}>Project history</button>
            <button className="btn btn-primary" onClick={() => setNewOpen(true)}><Plus className="size-3.5" aria-hidden /> New file</button>
          </>
        }
      />
      {error && <div className="p-3"><Callout tone="bad" title="Could not read the harness"><pre className="whitespace-pre-wrap break-words text-[12px]">{error}</pre></Callout></div>}

      {inv && inv.notes.length > 0 && (
        <div className="border-b border-line bg-panel px-5 py-2" aria-label="Notes">
          <ul className="m-0 list-disc pl-4 text-[12px] text-muted">{inv.notes.map((n, i) => <li key={i} className="break-words">{n}</li>)}</ul>
          {inv.truncated && <div className="mt-1 text-[12px] text-warn">The inventory was truncated; some files are not listed.</div>}
        </div>
      )}

      {findings.length > 0 && (
        <details className="border-b border-line bg-panel px-5 py-2" open={findings.some((f) => f.severity !== 'info')}>
          <summary className="cursor-pointer text-[12px] font-medium">Doctor: {findings.filter((f) => f.severity === 'problem').length} problem(s), {findings.filter((f) => f.severity === 'warning').length} warning(s), {findings.filter((f) => f.severity === 'info').length} note(s)</summary>
          <ul className="m-0 mt-1 max-h-44 list-none space-y-1 overflow-y-auto p-0">
            {findings.map((f, i) => {
              const target = resolve(f.subject);
              return (
                <li key={i} className="flex flex-wrap items-start gap-2 text-[12px]">
                  <Badge tone={SEV_TONE[f.severity]}>{f.severity}</Badge>
                  <div className="min-w-0 flex-1">
                    <div className="break-words">{f.message}</div>
                    {f.suggestion && <div className="break-words text-muted">{f.suggestion}</div>}
                  </div>
                  {target && <button className="btn btn-sm" onClick={() => select(target.id)}>Show {target.name}</button>}
                  {f.kind === 'changed_since_review' && <button className="btn btn-sm" onClick={() => void markReviewed(f)}>Mark reviewed</button>}
                </li>
              );
            })}
          </ul>
        </details>
      )}

      <div className="min-h-0 flex-1">
        <SplitLayout storageKey="harness.list" fixed="start" defaultSize={340} min={240} max={620}>
          <nav aria-label="Inventory" className="min-h-0 flex-1 overflow-y-auto bg-panel/40">
            {inv === null && !error && <div className="p-4 text-muted">Loading…</div>}
            {empty && (
              <EmptyState title="No configuration files" actions={<button className="btn btn-primary" onClick={() => setDraft({ rel: 'CLAUDE.md', starter: '# Project instructions\n\nDescribe how to build, test and work in this repository.\n' })}>Create CLAUDE.md</button>}>
                This project has no agent instructions or settings yet.
              </EmptyState>
            )}
            {groups.map((g) => (
              <section key={g.kind} aria-label={g.label}>
                <h3 className="sticky top-0 z-[1] m-0 border-b border-line bg-panel px-3 py-1 text-[11px] font-medium uppercase tracking-wide text-muted">{g.label} ({g.items.length})</h3>
                {g.items.map((it) => (
                  <button key={it.id} onClick={() => select(it.id)} aria-current={selected === it.id} className={`block w-full border-b border-line px-3 py-1.5 text-left ${selected === it.id ? 'bg-accent-dim' : 'hover:bg-hover'}`}>
                    <div className="flex items-center gap-1.5">
                      <span className="min-w-0 flex-1 truncate font-medium" title={it.name}>{it.name}</span>
                      <Badge tone={it.scope === 'project' ? 'run' : 'idle'}>{it.scope}</Badge>
                    </div>
                    <div className="mono truncate text-[11px] text-faint" title={it.path}>{it.rel ?? it.path}</div>
                    <div className="flex flex-wrap gap-x-3 text-[11px]">
                      <span className={it.loadedInMissions ? 'text-ok' : 'text-muted'}>read by FORGE missions: {it.loadedInMissions ? 'yes' : 'no'}</span>
                      {it.warnings.length > 0 && <span className="text-warn">{it.warnings.length} warning{it.warnings.length === 1 ? '' : 's'}</span>}
                    </div>
                  </button>
                ))}
              </section>
            ))}
          </nav>
          <div className="flex min-h-0 flex-1 flex-col overflow-y-auto p-4">
            {showHistory && (
              <div className="space-y-2">
                <h2 className="m-0 text-[15px] font-semibold">Project history</h2>
                <HistoryPanel projectId={projectId} onChanged={() => void load()} />
              </div>
            )}
            {!showHistory && draft && (
              <div className="space-y-2">
                <h2 className="m-0 break-all text-[15px] font-semibold">New file: {draft.rel}</h2>
                <FileEditor projectId={projectId} rel={draft.rel} editable starter={draft.starter} onSaved={() => { void load(); }} />
              </div>
            )}
            {!showHistory && !draft && item && <Detail projectId={projectId} item={item} inv={inv!} tab={tab} setTab={setTab} reload={() => void load()} />}
            {!showHistory && !draft && !item && !empty && inv && <EmptyState title="Select an item">Choose a file on the left to read or edit it.</EmptyState>}
          </div>
        </SplitLayout>
      </div>
      {newOpen && <NewFileDialog onClose={() => setNewOpen(false)} onChoose={(rel, starter) => { setNewOpen(false); setShowHistory(false); setSelected(null); setDraft({ rel, starter }); }} />}
    </div>
  );
}

function Detail({ projectId, item, inv, tab, setTab, reload }: { projectId: string; item: HarnessItem; inv: HarnessInventory; tab: DetailTab; setTab: (t: DetailTab) => void; reload: () => void }) {
  const tabs: { v: DetailTab; label: string }[] = [{ v: 'file', label: 'File' }, ...(item.kind === 'settings' ? [{ v: 'hooks' as const, label: 'Hooks' }] : []), { v: 'history', label: 'History' }];
  const servers = inv.mcpServers.filter((s) => s.sourceItem === item.id);
  const hasProjectServers = servers.some((s) => s.scope === 'project');
  const [auths, setAuths] = useState<McpAuthorizationStatus[]>([]);
  useEffect(() => {
    if (!hasProjectServers) return;
    let live = true;
    api.mcpAuthorizations(projectId).then((a) => { if (live) setAuths(a); }).catch(() => { if (live) setAuths([]); });
    return () => { live = false; };
  }, [projectId, hasProjectServers, inv]);
  const editable = item.editable && item.rel !== null;
  const active = tabs.some((t) => t.v === tab) ? tab : 'file';
  return (
    <div className="space-y-3">
      <header>
        <h2 className="m-0 break-words text-[15px] font-semibold">{item.name}</h2>
        <div className="mt-1 flex flex-wrap items-center gap-1.5">
          <Badge>{item.kind}</Badge>
          <Badge tone={item.scope === 'project' ? 'run' : 'idle'}>{item.scope}</Badge>
          <Badge tone={item.loadedInMissions ? 'ok' : 'idle'}>read by FORGE missions: {item.loadedInMissions ? 'yes' : 'no'}</Badge>
        </div>
        <div className="mono mt-1 break-all text-[11.5px] text-muted">{item.path} · {item.bytes} bytes · sha256 {shortHash(item.sha256)}</div>
        {item.description && <p className="m-0 mt-1 break-words text-muted">{item.description}</p>}
      </header>
      {item.warnings.length > 0 && <Callout tone="warn" title="Warnings"><ul className="m-0 list-disc pl-4">{item.warnings.map((w, i) => <li key={i} className="break-words">{w}</li>)}</ul></Callout>}
      {!editable && <Callout tone="info">{item.scope === 'user' ? 'FORGE never edits user-level files. Edit this file yourself outside FORGE.' : 'FORGE cannot edit this file (it is outside the editable allowlist).'}</Callout>}
      {servers.length > 0 && (
        <section aria-label="MCP servers">
          <h3 className="m-0 mb-1 text-[11px] font-medium uppercase tracking-wide text-muted">MCP servers</h3>
          <ul className="m-0 list-none space-y-1 p-0">{servers.map((s) => (
            <McpServerCard key={s.name} projectId={projectId} server={s} status={s.scope === 'project' ? auths.find((a) => a.authorization.name === s.name) : undefined} onChanged={reload} />
          ))}</ul>
        </section>
      )}
      <div role="tablist" aria-label="Item views" className="flex gap-1 border-b border-line">
        {tabs.map((t) => (
          <button key={t.v} role="tab" aria-selected={active === t.v} onClick={() => setTab(t.v)} className={`h-8 px-3 ${active === t.v ? 'border-b-2 border-accent text-fg' : 'text-muted hover:text-fg'}`}>{t.label}</button>
        ))}
      </div>
      <div role="tabpanel">
        {active === 'file' && (item.rel !== null
          ? <FileEditor key={item.id + item.sha256} projectId={projectId} rel={item.rel} editable={editable} onSaved={reload} />
          : <div className="text-muted">User-level file: its content is not shown here.</div>)}
        {active === 'hooks' && <HooksTab projectId={projectId} itemId={item.id} hooks={inv.hooks} onChanged={reload} />}
        {active === 'history' && (item.rel !== null ? <HistoryPanel projectId={projectId} rel={item.rel} onChanged={reload} /> : <div className="text-muted">FORGE keeps no history for user-level files.</div>)}
      </div>
    </div>
  );
}
