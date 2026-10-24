import { useEffect, useState } from 'react';
import { Plus, Trash2 } from 'lucide-react';
import { api } from '../lib/api';
import { errMsg } from '../lib/format';
import { refreshProviders } from '../lib/actions';
import { useStore } from '../lib/store';
import type { AcpAgent } from '../lib/types';
import { Modal } from './Modal';
import { Callout, Mono } from './ui';

/** Agents spoken to over the Agent Client Protocol. Registering one lets FORGE start that program for missions. */
export function AcpAgentsPanel() {
  const notify = useStore((s) => s.notify);
  const [agents, setAgents] = useState<AcpAgent[] | null>(null);
  const [open, setOpen] = useState(false);
  const [name, setName] = useState('');
  const [command, setCommand] = useState('');
  const [args, setArgs] = useState('');
  const [busy, setBusy] = useState(false);

  const load = () => api.acpAgents().then(setAgents).catch((e) => notify('error', errMsg(e)));
  useEffect(() => { void load(); }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const register = async () => {
    setBusy(true);
    try {
      // One argument per line: no shell quoting rules to get wrong.
      const a = await api.acpAgentRegister(name.trim(), command.trim(), args.split('\n').map((x) => x.trim()).filter(Boolean));
      notify('info', `Registered ${a.name} (${a.command})`);
      setOpen(false);
      setName(''); setCommand(''); setArgs('');
      await load();
      void refreshProviders();
    } catch (e) {
      notify('error', errMsg(e));
    } finally {
      setBusy(false);
    }
  };
  const remove = async (a: AcpAgent) => {
    try {
      await api.acpAgentRemove(a.id);
      await load();
      void refreshProviders();
    } catch (e) {
      notify('error', errMsg(e));
    }
  };

  return (
    <section aria-label="ACP agents" className="space-y-2">
      <div className="flex items-center gap-3">
        <h2 className="m-0 text-[11px] font-medium uppercase tracking-wide text-faint">ACP agents</h2>
        <span className="flex-1" />
        <button className="btn btn-sm" onClick={() => setOpen(true)}><Plus className="size-3.5" aria-hidden /> Register agent</button>
      </div>
      {agents === null ? <div className="text-muted">Loading…</div> : agents.length === 0 ? (
        <div className="text-muted">None registered. Any program that speaks the Agent Client Protocol over stdio can run FORGE missions.</div>
      ) : (
        <table className="w-full border-collapse text-left">
          <tbody>
            {agents.map((a) => (
              <tr key={a.id} className="border-b border-line">
                <td className="py-1.5 pr-3 font-medium">{a.name}</td>
                <td className="py-1.5 pr-3"><Mono className="break-all">{[a.command, ...a.args].join(' ')}</Mono></td>
                <td className="py-1.5 text-right"><button className="btn btn-sm" onClick={() => void remove(a)} aria-label={`Remove ${a.name}`}><Trash2 className="size-3" aria-hidden /> Remove</button></td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      <Modal open={open} onClose={() => setOpen(false)} title="Register an ACP agent">
        <Callout tone="warn">FORGE will start this program, with your privileges, for every mission that uses it. Register only programs you trust, as you would install a CLI.</Callout>
        <label className="block space-y-1"><span className="label">Name</span><input className="input" value={name} onChange={(e) => setName(e.target.value)} placeholder="e.g. gemini" /></label>
        <label className="block space-y-1"><span className="label">Program (full path, or a name on PATH)</span><input className="input mono" value={command} onChange={(e) => setCommand(e.target.value)} placeholder="e.g. C:\Program Files\nodejs\node.exe" /></label>
        <label className="block space-y-1"><span className="label">Arguments (one per line)</span><textarea className="input mono h-20" value={args} onChange={(e) => setArgs(e.target.value)} placeholder={'e.g.\n--experimental-acp'} /></label>
        <ul className="m-0 list-disc space-y-0.5 pl-4 text-[12px] text-muted">
          <li>FORGE answers every permission request the agent sends and performs the file reads and writes it asks for, through FORGE policy and your approvals.</li>
          <li>A tool the agent runs without asking cannot be seen or stopped by FORGE; review the mission's diff before merging, as always.</li>
          <li>The agent signs in on its own. FORGE never signs in for it and removes API keys from its environment.</li>
        </ul>
        <div className="flex justify-end gap-2">
          <button className="btn" onClick={() => setOpen(false)}>Cancel</button>
          <button className="btn btn-primary" disabled={busy || !name.trim() || !command.trim()} onClick={() => void register()}>Register</button>
        </div>
      </Modal>
    </section>
  );
}
