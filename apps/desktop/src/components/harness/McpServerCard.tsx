import { useState } from 'react';
import { api } from '../../lib/api';
import { errMsg } from '../../lib/format';
import { shortHash } from '../../lib/knowledge';
import { parseAllowlist } from '../../lib/mcp';
import { useStore } from '../../lib/store';
import type { McpAuthorizationStatus, McpPreview, McpServerDef } from '../../lib/types';
import { Modal } from '../Modal';
import { Badge, Callout } from '../ui';

/**
 * One MCP server from the inventory. Project servers can be authorized: the user reviews the exact definition FORGE read
 * (and what authorizing means), FORGE pins it by hash, and missions get that stored definition, never the live file.
 */
export function McpServerCard({ projectId, server, status, onChanged }: { projectId: string; server: McpServerDef; status: McpAuthorizationStatus | undefined; onChanged: () => void }) {
  const notify = useStore((s) => s.notify);
  const [preview, setPreview] = useState<McpPreview | null>(null);
  const [allow, setAllow] = useState('');
  const [busy, setBusy] = useState(false);

  const review = async () => {
    try {
      const p = await api.mcpPreview(projectId, server.name);
      setAllow((status?.authorization.toolAllowlist ?? []).join(', '));
      setPreview(p);
    } catch (e) {
      notify('error', errMsg(e));
    }
  };
  const act = async (f: () => Promise<unknown>, msg: string) => {
    setBusy(true);
    try {
      await f();
      notify('info', msg);
      setPreview(null);
      onChanged();
    } catch (e) {
      notify('error', errMsg(e));
    } finally {
      setBusy(false);
    }
  };
  const tools = parseAllowlist(allow);

  return (
    <li className="rounded border border-line p-2 text-[12px]">
      <div className="flex flex-wrap items-center gap-1.5">
        <span className="font-medium">{server.name}</span>
        <Badge>{server.transport}</Badge>
        <Badge tone={server.runsInForgeMissions ? 'warn' : 'idle'}>runs in FORGE missions: {server.runsInForgeMissions ? 'yes (authorized)' : 'no'}</Badge>
        {status?.changedSinceAuthorization && <Badge tone="warn" title="Missions still receive the definition you authorized">changed since authorized</Badge>}
        <span className="flex-1" />
        {server.scope === 'project' && <button className="btn btn-sm" onClick={() => void review()}>{status ? 'Review' : 'Review & authorize…'}</button>}
        {status && <button className="btn btn-sm" disabled={busy} onClick={() => void act(() => api.mcpRevoke(status.authorization.id), `${server.name} will not run in new missions`)}>Revoke</button>}
      </div>
      <div className="mono break-all text-muted">{server.command ?? server.url ?? ''}</div>
      {status && (
        <div className="text-faint">
          authorized {status.authorization.authorizedAt.slice(0, 16).replace('T', ' ')} · sha256 {shortHash(status.authorization.definitionSha256)} · tools: {status.authorization.toolAllowlist.length ? status.authorization.toolAllowlist.join(', ') : 'any (each call still asks you)'}
        </div>
      )}
      {preview && (
        <Modal open onClose={() => setPreview(null)} wide title={`Authorize MCP server "${server.name}" for this project?`}>
          <div className="mono space-y-0.5 break-all rounded border border-line p-2 text-[12px]">
            <div>transport: {preview.definition.transport}</div>
            {preview.definition.command && <div>command: {preview.definition.command} {preview.definition.args.join(' ')}</div>}
            {preview.definition.url && <div>url: {preview.definition.url}</div>}
            <div>environment variables passed: {preview.definition.envNames.length ? preview.definition.envNames.join(', ') : 'none'}</div>
            <div className="text-muted">sha256 {preview.definitionSha256}</div>
          </div>
          <ul className="m-0 list-disc space-y-0.5 pl-4">{preview.summary.map((s, i) => <li key={i} className="break-words">{s}</li>)}</ul>
          {status?.changedSinceAuthorization && <Callout tone="warn">.mcp.json no longer matches what you authorized. Missions keep the authorized definition until you authorize this one or revoke.</Callout>}
          <label className="mt-2 block space-y-1">
            <span className="text-muted">Allowed tools (optional; comma separated, e.g. <span className="mono">search, read</span>). Empty: any tool may be asked for.</span>
            <input className="input w-full" value={allow} onChange={(e) => setAllow(e.target.value)} placeholder="any tool" />
          </label>
          <div className="flex justify-end gap-2">
            <button className="btn" onClick={() => setPreview(null)}>Cancel</button>
            <button className="btn btn-primary" disabled={busy} onClick={() => void act(() => api.mcpAuthorize(projectId, server.name, preview.definitionSha256, tools), `${server.name} authorized for new missions`)}>
              Authorize{tools.length ? ` (${tools.length} tool${tools.length > 1 ? 's' : ''})` : ''}
            </button>
          </div>
        </Modal>
      )}
    </li>
  );
}
