import { RefreshCw } from 'lucide-react';
import { refreshAppInfo, refreshProviders } from '../lib/actions';
import { providerLabel } from '../lib/format';
import { useStore } from '../lib/store';
import type { CapabilityMatrix } from '../lib/types';
import { Badge, PageHeader } from '../components/ui';

const CAPS: { key: keyof CapabilityMatrix; label: string }[] = [
  { key: 'sessionStart', label: 'Start session' },
  { key: 'resume', label: 'Resume' },
  { key: 'streaming', label: 'Streaming' },
  { key: 'interrupt', label: 'Interrupt' },
  { key: 'toolEvents', label: 'Tool events' },
  { key: 'fileChanges', label: 'File changes' },
  { key: 'approvals', label: 'Approvals' },
  { key: 'usageReporting', label: 'Usage reporting' },
  { key: 'modelSelection', label: 'Model selection' },
  { key: 'worktreeIntegration', label: 'Worktree integration' },
];

function Row({ k, children }: { k: string; children: React.ReactNode }) {
  return (
    <div className="grid grid-cols-[200px_1fr] gap-3 border-b border-line py-1.5">
      <dt className="text-muted">{k}</dt>
      <dd className="m-0 min-w-0 break-words">{children}</dd>
    </div>
  );
}

export function SettingsScreen() {
  const info = useStore((s) => s.appInfo);
  const providers = useStore((s) => s.providers);
  const yn = (v: boolean) => <Badge tone={v ? 'ok' : 'bad'}>{v ? 'found' : 'missing'}</Badge>;

  return (
    <div className="flex h-full min-h-0 flex-col">
      <PageHeader
        title="Settings"
        sub="Read-only environment information reported by the FORGE core."
        actions={
          <button className="btn" onClick={() => { void refreshAppInfo(); void refreshProviders(); }}>
            <RefreshCw className="size-3.5" aria-hidden /> Re-detect
          </button>
        }
      />
      <div className="min-h-0 flex-1 space-y-8 overflow-auto p-5">
        <section aria-label="Application">
          <h2 className="m-0 mb-2 text-[12px] font-medium uppercase tracking-wide text-muted">Application</h2>
          {!info ? (
            <div className="text-muted">Waiting for the core…</div>
          ) : (
            <dl className="m-0 max-w-3xl">
              <Row k="FORGE version"><span className="mono">{info.version}</span></Row>
              <Row k="Home directory"><span className="mono text-[12px]">{info.home}</span></Row>
              <Row k="Git">{info.gitVersion ? <span className="mono">{info.gitVersion}</span> : <Badge tone="bad">not found</Badge>}</Row>
              <Row k="Node runtime (engine)">{yn(info.nodeFound)}</Row>
              <Row k="Agent engine bundle">{yn(info.engineFound)}</Row>
              <Row k="Claude Code CLI">{yn(info.claudeFound)}</Row>
              <Row k="Concurrency limit"><span className="mono">{info.maxConcurrent}</span> agent sessions at once</Row>
              <Row k="Live sessions now"><span className="mono">{info.liveSessions}</span></Row>
              <Row k="Scripted test agent">
                {info.scriptedAgentEnabled ? <Badge tone="warn">enabled (development only)</Badge> : <Badge tone="idle">disabled</Badge>}
              </Row>
            </dl>
          )}
        </section>

        <section aria-label="Providers">
          <h2 className="m-0 mb-2 text-[12px] font-medium uppercase tracking-wide text-muted">Providers</h2>
          {providers === null && <div className="text-muted">Detecting…</div>}
          <div className="space-y-5">
            {providers?.map((p) => (
              <div key={p.id} className="rounded-md border border-line bg-panel">
                <div className="flex flex-wrap items-center gap-2 border-b border-line px-3 py-2">
                  <span className="text-[14px] font-semibold">{providerLabel(p.id)}</span>
                  <Badge tone={p.installed ? 'ok' : 'bad'}>{p.installed ? 'installed' : 'not installed'}</Badge>
                  <Badge tone={p.adapterAvailable ? 'ok' : 'warn'}>{p.adapterAvailable ? 'adapter available' : 'no adapter yet'}</Badge>
                  {p.signedIn !== null && <Badge tone={p.signedIn ? 'ok' : 'warn'}>{p.signedIn ? 'signed in' : 'not signed in'}</Badge>}
                  {p.version && <span className="mono text-[12px] text-muted">{p.version}</span>}
                </div>
                <div className="grid grid-cols-[120px_1fr] gap-x-3 gap-y-1 px-3 py-2 text-[12px]">
                  <span className="text-muted">Path</span>
                  <span className="mono break-all">{p.path ?? '—'}</span>
                  <span className="text-muted">Auth method</span>
                  <span>{p.authMethod ?? 'not reported'}</span>
                  <span className="text-muted">Plan</span>
                  <span>{p.plan ?? 'not reported'}</span>
                  {p.notes.length > 0 && (
                    <>
                      <span className="text-muted">Notes</span>
                      <ul className="m-0 list-disc pl-4">{p.notes.map((n, i) => <li key={i}>{n}</li>)}</ul>
                    </>
                  )}
                </div>
                <table className="tbl" aria-label={`${p.name} capabilities`}>
                  <thead>
                    <tr><th className="w-[220px]">Capability</th><th className="w-[110px]">Status</th><th>Note</th></tr>
                  </thead>
                  <tbody>
                    {CAPS.map((c) => {
                      const cap = p.capabilities[c.key];
                      return (
                        <tr key={c.key}>
                          <td>{c.label}</td>
                          <td><Badge tone={cap.supported ? 'ok' : 'idle'}>{cap.supported ? 'supported' : 'unavailable'}</Badge></td>
                          <td className="text-muted">{cap.note ?? ''}</td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              </div>
            ))}
          </div>
        </section>
      </div>
    </div>
  );
}
