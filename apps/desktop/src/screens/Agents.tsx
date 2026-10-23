import { useCallback, useEffect, useState } from 'react';
import { Check, RefreshCw, Square, X } from 'lucide-react';
import { api } from '../lib/api';
import { refreshProviders, stopMission } from '../lib/actions';
import { capabilityList, costText, formatTokens } from '../lib/agents';
import { errMsg, providerLabel, relTime } from '../lib/format';
import { useStore } from '../lib/store';
import type { AgentsOverview, ProviderInfo } from '../lib/types';
import { billingNote, providerAvailability } from '../components/NewMissionDialog';
import { Badge, Callout, EmptyState, Mono, PageHeader, useNow } from '../components/ui';

const WINDOWS = [1, 7, 30] as const;

export function AgentsScreen() {
  const providers = useStore((s) => s.providers);
  const projects = useStore((s) => s.projects);
  const navigate = useStore((s) => s.navigate);
  const liveCount = useStore((s) => s.appInfo?.liveSessions ?? 0);
  const now = useNow(10000);
  const [days, setDays] = useState<(typeof WINDOWS)[number]>(7);
  const [o, setO] = useState<AgentsOverview | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setO(await api.agentsOverview(days));
      setError(null);
    } catch (e) {
      setError(errMsg(e));
    }
  }, [days]);

  // Reload when the window changes and whenever the number of running agents changes.
  useEffect(() => { void load(); }, [load, liveCount]);
  useEffect(() => { void refreshProviders(); }, []);

  const projectName = (id: string) => projects.find((p) => p.id === id)?.name ?? '';
  const live = o?.sessions.filter((r) => r.session.state === 'live' || r.session.state === 'starting') ?? [];
  const recent = o?.sessions.filter((r) => r.session.state !== 'live' && r.session.state !== 'starting') ?? [];
  const auth = (id: string) => providers?.find((p) => p.id === id)?.authMethod;

  return (
    <div className="flex h-full min-h-0 flex-col">
      <PageHeader
        title="Agents"
        sub="The coding agents FORGE can drive, what they are doing now, and what they reported using."
        actions={<button className="btn" onClick={() => { void refreshProviders(); void load(); }}><RefreshCw className="size-3.5" aria-hidden /> Refresh</button>}
      />
      <div className="min-h-0 flex-1 space-y-6 overflow-y-auto p-5">
        {error && <Callout tone="bad" title="Could not load agent sessions">{error}</Callout>}

        <section aria-label="Providers" className="space-y-2">
          <h2 className="m-0 text-[11px] font-medium uppercase tracking-wide text-faint">Providers</h2>
          {providers === null ? <div className="text-muted">Detecting…</div> : (
            <div className="grid grid-cols-[repeat(auto-fill,minmax(340px,1fr))] gap-3">
              {providers.map((p) => <ProviderCard key={p.id} p={p} />)}
            </div>
          )}
        </section>

        <section aria-label="Running now" className="space-y-2">
          <h2 className="m-0 text-[11px] font-medium uppercase tracking-wide text-faint">Running now ({live.length})</h2>
          {live.length === 0 ? <div className="text-muted">No agent is running.</div> : (
            <table className="w-full border-collapse text-left">
              <tbody>
                {live.map((r) => (
                  <tr key={r.session.id} className="border-b border-line">
                    <td className="py-1.5 pr-3"><span className="live-dot mr-2 inline-block size-1.5 rounded-full bg-run" aria-hidden />{r.missionTitle}</td>
                    <td className="py-1.5 pr-3 text-muted">{projectName(r.projectId)}</td>
                    <td className="py-1.5 pr-3">{providerLabel(r.session.provider)}</td>
                    <td className="py-1.5 pr-3 text-muted">started {relTime(r.session.startedAt, now)}</td>
                    <td className="py-1.5 pr-3 text-muted">{r.session.pid ? <Mono>pid {r.session.pid}</Mono> : null}</td>
                    <td className="py-1.5 text-right">
                      <button className="btn btn-sm mr-2" onClick={() => navigate({ name: 'mission', id: r.session.missionId })}>Open mission</button>
                      <button className="btn btn-sm btn-danger" onClick={() => void stopMission(r.session.missionId)}><Square className="size-3" aria-hidden /> Stop</button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </section>

        <section aria-label="Usage" className="space-y-2">
          <div className="flex items-center gap-3">
            <h2 className="m-0 text-[11px] font-medium uppercase tracking-wide text-faint">Reported usage</h2>
            <div className="flex gap-1" role="group" aria-label="Time window">
              {WINDOWS.map((w) => (
                <button key={w} className={`btn btn-sm ${days === w ? 'btn-primary' : ''}`} aria-pressed={days === w} onClick={() => setDays(w)}>{w === 1 ? '24 h' : `${w} days`}</button>
              ))}
            </div>
          </div>
          <p className="m-0 text-[12px] text-muted">Only sessions FORGE started, as their provider reported them. Sessions you ran outside FORGE are not included, and a provider's cost estimate is not your bill.</p>
          {o && o.usage.length === 0 && <EmptyState title="No agent sessions in this window" />}
          {o && o.usage.length > 0 && (
            <table className="w-full border-collapse text-left">
              <thead className="text-[11px] uppercase tracking-wide text-faint">
                <tr className="border-b border-line">
                  <th className="py-1.5 pr-3 font-medium">Provider</th>
                  <th className="py-1.5 pr-3 text-right font-medium">Sessions</th>
                  <th className="py-1.5 pr-3 text-right font-medium">Input</th>
                  <th className="py-1.5 pr-3 text-right font-medium">Output</th>
                  <th className="py-1.5 pr-3 text-right font-medium">Cache read</th>
                  <th className="py-1.5 pr-3 text-right font-medium">Turns</th>
                  <th className="py-1.5 font-medium">Cost</th>
                </tr>
              </thead>
              <tbody>
                {o.usage.map((u) => (
                  <tr key={u.provider} className="border-b border-line">
                    <td className="py-1.5 pr-3">{providerLabel(u.provider)}</td>
                    <td className="py-1.5 pr-3 text-right tabular-nums" title={`${u.sessionsWithUsage} of ${u.sessions} reported usage`}>{u.sessions}{u.sessionsWithUsage < u.sessions && <span className="text-faint"> ({u.sessionsWithUsage} reported)</span>}</td>
                    <td className="py-1.5 pr-3 text-right tabular-nums">{formatTokens(u.inputTokens)}</td>
                    <td className="py-1.5 pr-3 text-right tabular-nums">{formatTokens(u.outputTokens)}</td>
                    <td className="py-1.5 pr-3 text-right tabular-nums">{formatTokens(u.cacheReadTokens)}</td>
                    <td className="py-1.5 pr-3 text-right tabular-nums">{u.turns || '—'}</td>
                    <td className="py-1.5 text-muted">{costText(u, auth(u.provider))}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </section>

        {recent.length > 0 && (
          <section aria-label="Recent sessions" className="space-y-2">
            <h2 className="m-0 text-[11px] font-medium uppercase tracking-wide text-faint">Recent sessions</h2>
            <table className="w-full border-collapse text-left">
              <tbody>
                {recent.slice(0, 50).map((r) => (
                  <tr key={r.session.id} className="cursor-pointer border-b border-line hover:bg-hover" onClick={() => navigate({ name: 'mission', id: r.session.missionId })}>
                    <td className="py-1.5 pr-3">{r.missionTitle}</td>
                    <td className="py-1.5 pr-3 text-muted">{projectName(r.projectId)}</td>
                    <td className="py-1.5 pr-3">{providerLabel(r.session.provider)}</td>
                    <td className="py-1.5 pr-3"><Badge tone={r.session.state === 'lost' ? 'warn' : 'idle'}>{r.session.state}</Badge></td>
                    <td className="py-1.5 pr-3 text-muted">{r.session.endReason ?? ''}</td>
                    <td className="py-1.5 pr-3 text-right tabular-nums text-muted">{r.session.usage ? `${formatTokens(r.session.usage.inputTokens + r.session.usage.outputTokens)} tokens` : ''}</td>
                    <td className="py-1.5 text-right text-muted">{relTime(r.session.startedAt, now)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </section>
        )}
      </div>
    </div>
  );
}

function ProviderCard({ p }: { p: ProviderInfo }) {
  const a = providerAvailability(p);
  return (
    <div className={`space-y-2 rounded-md border p-3 ${a.ok ? 'border-line-strong bg-panel' : 'border-line bg-panel/50'}`}>
      <div className="flex items-center gap-2">
        <span className="font-medium">{p.name}</span>
        {p.version && <Mono className="text-muted">{p.version}</Mono>}
        <span className="flex-1" />
        <Badge tone={a.ok ? 'ok' : p.adapterAvailable ? 'warn' : 'idle'}>{a.ok ? 'Ready' : !p.adapterAvailable ? 'No adapter' : !p.installed ? 'Not installed' : 'Not signed in'}</Badge>
      </div>
      {a.ok ? <div className="text-[12px] text-muted">{billingNote(p)}</div> : <div className="text-[12px] text-muted">{a.reason}</div>}
      {p.notes.length > 0 && a.ok && <div className="text-[12px] text-muted">{p.notes.join(' ')}</div>}
      <ul className="m-0 flex list-none flex-wrap gap-1 p-0" aria-label={`${p.name} capabilities`}>
        {capabilityList(p.capabilities).map((c) => (
          <li key={c.key} title={c.note ?? (c.supported ? 'supported' : 'not supported')} className={`flex items-center gap-1 rounded-sm border px-1.5 text-[11px] ${c.supported ? 'border-line-strong text-fg' : 'border-line text-faint line-through'}`}>
            {c.supported ? <Check className="size-3 text-ok" aria-hidden /> : <X className="size-3" aria-hidden />}{c.label}
          </li>
        ))}
      </ul>
      {p.models.length > 0 && <div className="truncate text-[12px] text-muted" title={p.models.join(', ')}>Models: {p.models.slice(0, 4).join(', ')}{p.models.length > 4 ? ` +${p.models.length - 4}` : ''}</div>}
    </div>
  );
}
