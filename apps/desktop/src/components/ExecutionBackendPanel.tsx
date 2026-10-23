import { useEffect, useState } from 'react';
import { ShieldAlert, ShieldCheck } from 'lucide-react';
import { api } from '../lib/api';
import { levelTone, PROPERTY_ROWS } from '../lib/execution';
import { errMsg } from '../lib/format';
import { useStore } from '../lib/store';
import type { ExecutionBackendInfo, Project } from '../lib/types';
import { Modal } from './Modal';
import { Badge, Callout } from './ui';

/** Where a project's checks and experiments run, with what each choice actually enforces. */
export function ExecutionBackendPanel({ project }: { project: Project }) {
  const notify = useStore((s) => s.notify);
  const [list, setList] = useState<ExecutionBackendInfo[] | null>(null);
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  useEffect(() => { void api.executionBackends().then(setList).catch((e) => notify('error', errMsg(e))); }, [notify]);
  const current = list?.find((b) => b.id === project.executionBackend);
  const isolated = project.executionBackend !== 'trusted_local';

  const choose = async (b: ExecutionBackendInfo) => {
    setBusy(true);
    try {
      await api.projectSetExecutionBackend(project.id, b.id);
      useStore.getState().setProjects(await api.projectList());
      notify('info', `Checks and experiments in ${project.name} now run on: ${b.label}`);
      setOpen(false);
    } catch (e) {
      notify('error', errMsg(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <button className={`btn btn-sm ${isolated ? '' : 'border-warn/40 text-warn'}`} onClick={() => setOpen(true)} title="Where this project's checks and experiments run">
        {isolated ? <ShieldCheck className="size-3.5" aria-hidden /> : <ShieldAlert className="size-3.5" aria-hidden />}
        Checks run: {current?.label ?? project.executionBackend}
      </button>
      <Modal open={open} onClose={() => setOpen(false)} wide title={`Where ${project.name} runs checks and experiments`}>
        <p className="m-0 text-muted">Checks and experiments execute code from the repository, including code an agent wrote. This choice decides what that code can reach. It does not change how the agents themselves run.</p>
        {list === null && <div className="text-muted">Checking what this machine supports…</div>}
        {list?.map((b) => (
          <div key={b.id} className={`space-y-2 rounded-md border p-3 ${b.id === project.executionBackend ? 'border-accent' : 'border-line-strong'} ${b.available ? '' : 'opacity-70'}`}>
            <div className="flex items-center gap-2">
              <span className="font-medium">{b.label}</span>
              {b.id === project.executionBackend && <Badge tone="run">current</Badge>}
              {!b.implemented && <Badge tone="idle">not implemented</Badge>}
              {b.implemented && !b.available && <Badge tone="warn">unavailable here</Badge>}
              <span className="flex-1" />
              <button className="btn btn-sm btn-primary" disabled={!b.available || busy || b.id === project.executionBackend} onClick={() => void choose(b)}>Use this</button>
            </div>
            {b.unavailableReason && <div className="text-[12px] text-muted">{b.unavailableReason}</div>}
            <table className="w-full border-collapse text-left text-[12px]">
              <tbody>
                {PROPERTY_ROWS.map((r) => (
                  <tr key={r.key} className="align-top">
                    <td className="w-[90px] py-0.5 pr-2 text-muted">{r.label}</td>
                    <td className="w-[80px] py-0.5 pr-2"><Badge tone={levelTone(b.capabilities[r.key])}>{b.capabilities[r.key].level}</Badge></td>
                    <td className="py-0.5">{b.capabilities[r.key].detail}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ))}
        <Callout tone="info">If the selected backend stops being available (for example bubblewrap is uninstalled), checks fail with the reason. FORGE never quietly runs them somewhere less isolated.</Callout>
      </Modal>
    </>
  );
}
