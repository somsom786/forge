import { useEffect, useMemo, useState } from 'react';
import { api } from '../../lib/api';
import { errMsg } from '../../lib/format';
import { customHookSpec, HOOK_EVENTS, oddChars, revealOdd, validateHookSpec } from '../../lib/harness';
import { useStore } from '../../lib/store';
import type { HarnessPreview, HookDef, HookTemplate } from '../../lib/types';
import { Modal } from '../Modal';
import { Badge, Callout } from '../ui';
import { DryRunDialog } from './DryRunDialog';
import { PreviewPanel } from './PreviewPanel';

const NO_DRY_RUN = 'This hook uses a form FORGE cannot reproduce faithfully (a non-command type, or extra fields like args/shell), so it cannot be tested here.';

export function HooksTab({ projectId, itemId, hooks, onChanged }: { projectId: string; itemId: string; hooks: HookDef[]; onChanged: () => void }) {
  const notify = useStore((s) => s.notify);
  const mine = hooks.filter((h) => h.sourceItem === itemId);
  const [dry, setDry] = useState<HookDef | null>(null);
  const [pending, setPending] = useState<HarnessPreview | null>(null);
  const [adding, setAdding] = useState(false);

  const remove = async (h: HookDef) => {
    try { setPending(await api.harnessHookPreviewRemove(projectId, h.id)); } catch (e) { notify('error', errMsg(e)); }
  };

  return (
    <div className="space-y-3">
      <div className="flex items-center gap-2">
        <span className="text-[12px] text-muted">{mine.length} hook{mine.length === 1 ? '' : 's'} in this file</span>
        <button className="btn btn-sm btn-primary ml-auto" onClick={() => setAdding(true)}>Add hook…</button>
      </div>
      {mine.length === 0 && <div className="text-muted">No hooks are configured in this file.</div>}
      <ul className="m-0 list-none space-y-2 p-0">
        {mine.map((h) => (
          <li key={h.id} className="rounded-md border border-line p-2.5">
            <div className="flex flex-wrap items-center gap-2">
              <span className="font-medium">{h.event}</span>
              {h.matcher && <Badge>matcher: {h.matcher}</Badge>}
              {!h.knownEvent && <Badge tone="warn">unknown event</Badge>}
              {h.kind !== 'command' && <Badge tone="warn">type: {h.kind}</Badge>}
              <span className="text-[12px] text-muted">{h.timeoutSecs ? `timeout ${h.timeoutSecs}s` : 'default timeout'}</span>
              <span className="ml-auto flex gap-1.5">
                <button className="btn btn-sm" disabled={!h.dryRunSupported} title={h.dryRunSupported ? undefined : NO_DRY_RUN} onClick={() => setDry(h)}>Dry run</button>
                <button className="btn btn-sm btn-danger" onClick={() => void remove(h)}>Remove</button>
              </span>
            </div>
            <pre className="mt-1 whitespace-pre-wrap break-all rounded border border-line bg-bg p-1.5 text-[12px]">{revealOdd(h.command)}</pre>
            {!h.dryRunSupported && <div className="mt-0.5 text-[11.5px] text-warn">{NO_DRY_RUN}</div>}
            <div className="mt-0.5 text-[11.5px] text-faint">runs in FORGE missions: {h.runsInForgeMissions ? 'yes' : 'no'}. It runs in Claude Code sessions you start yourself.</div>
          </li>
        ))}
      </ul>
      {pending && (
        <PreviewPanel projectId={projectId} preview={pending} expectedSha256={pending.beforeSha256} onApplied={() => { setPending(null); onChanged(); }} onCancel={() => setPending(null)} onReload={() => setPending(null)} />
      )}
      {dry && <DryRunDialog projectId={projectId} event={dry.event} command={dry.command} onClose={() => setDry(null)} />}
      {adding && <AddHookDialog projectId={projectId} existingEvents={Array.from(new Set(hooks.map((h) => h.event)))} onClose={() => setAdding(false)} onPreview={(p) => { setAdding(false); setPending(p); }} />}
    </div>
  );
}

function AddHookDialog({ projectId, existingEvents, onClose, onPreview }: { projectId: string; existingEvents: string[]; onClose: () => void; onPreview: (p: HarnessPreview) => void }) {
  const notify = useStore((s) => s.notify);
  const [templates, setTemplates] = useState<HookTemplate[] | null>(null);
  const [tpl, setTpl] = useState('custom');
  const [params, setParams] = useState<Record<string, string>>({});
  const [event, setEvent] = useState('PreToolUse');
  const [matcher, setMatcher] = useState('');
  const [command, setCommand] = useState('');
  const [timeout, setTimeoutSecs] = useState(30);
  const [busy, setBusy] = useState(false);
  const [dry, setDry] = useState(false);
  const [previewError, setPreviewError] = useState<string | null>(null);

  useEffect(() => {
    api.harnessHookTemplates().then(setTemplates, (e) => { notify('error', errMsg(e)); setTemplates([]); });
  }, [notify]);

  const events = useMemo(() => Array.from(new Set([...HOOK_EVENTS, ...existingEvents])), [existingEvents]);
  const t = templates?.find((x) => x.id === tpl) ?? null;
  const chooseTemplate = (id: string) => {
    setTpl(id);
    const x = templates?.find((y) => y.id === id);
    setParams(x ? Object.fromEntries(x.params.map((p) => [p.key, p.default])) : {});
  };

  const custom = { event, matcher, command, timeoutSecs: timeout };
  const problems = tpl === 'custom' ? validateHookSpec(custom) : [];
  const odd = tpl === 'custom' ? oddChars(command) : [];
  const paramLines = t ? t.params.some((p) => /[\r\n]/.test(params[p.key] ?? '')) : false;

  const preview = async () => {
    setBusy(true);
    setPreviewError(null);
    try {
      const spec = tpl === 'custom' ? customHookSpec(custom) : { templateId: tpl, params };
      onPreview(await api.harnessHookPreviewAdd(projectId, spec));
    } catch (e) { setPreviewError(errMsg(e)); } finally { setBusy(false); }
  };

  return (
    <Modal open onClose={onClose} wide title="Add hook"
      footer={<><button className="btn" onClick={onClose} disabled={busy}>Cancel</button><button className="btn btn-primary" disabled={busy || problems.length > 0 || paramLines} onClick={() => void preview()}>Preview</button></>}>
      <div>
        <label className="label" htmlFor="ah-tpl">Template</label>
        <select id="ah-tpl" className="input" value={tpl} onChange={(e) => chooseTemplate(e.target.value)} disabled={!templates}>
          <option value="custom">Custom</option>
          {templates?.map((x) => <option key={x.id} value={x.id}>{x.name}</option>)}
        </select>
      </div>
      {t && (
        <div className="space-y-2">
          <p className="m-0">{t.description}</p>
          <div className="text-[12px] text-muted">Event {t.event}{t.matcher ? ` · matcher ${t.matcher}` : ''} · timeout {t.timeoutSecs}s · requires: {t.requires}</div>
          {t.params.map((p) => (
            <div key={p.key}>
              <label className="label" htmlFor={`ah-${p.key}`}>{p.label}</label>
              <input id={`ah-${p.key}`} className="input mono" value={params[p.key] ?? ''} onChange={(e) => setParams({ ...params, [p.key]: e.target.value })} />
              {oddChars(params[p.key] ?? '').length > 0 && <div className="text-[12px] text-warn">Contains unusual characters: {revealOdd(params[p.key] ?? '')}</div>}
            </div>
          ))}
          {paramLines && <div className="text-[12px] text-bad">Values must be single-line.</div>}
        </div>
      )}
      {tpl === 'custom' && (
        <div className="space-y-2">
          <div className="grid grid-cols-[1fr_1fr_100px] gap-2">
            <div><label className="label" htmlFor="ah-ev">Event</label><select id="ah-ev" className="input" value={event} onChange={(e) => setEvent(e.target.value)}>{events.map((x) => <option key={x}>{x}</option>)}</select></div>
            <div><label className="label" htmlFor="ah-m">Matcher (optional)</label><input id="ah-m" className="input mono" value={matcher} onChange={(e) => setMatcher(e.target.value)} placeholder="e.g. Bash" /></div>
            <div><label className="label" htmlFor="ah-to">Timeout (s)</label><input id="ah-to" className="input" type="number" min={1} max={600} value={timeout} onChange={(e) => setTimeoutSecs(Number(e.target.value))} /></div>
          </div>
          <div>
            <label className="label" htmlFor="ah-cmd">Command (single line)</label>
            <input id="ah-cmd" className="input mono" value={command} onChange={(e) => setCommand(e.target.value)} />
            {odd.length > 0 && <div className="mt-0.5 text-[12px] text-warn">Unusual characters: <span className="mono break-all">{revealOdd(command)}</span></div>}
          </div>
          {problems.length > 0 && command !== '' && <ul className="m-0 list-disc pl-4 text-[12px] text-bad">{problems.map((p) => <li key={p}>{p}</li>)}</ul>}
          <button className="btn btn-sm" disabled={!command.trim() || /[\r\n]/.test(command)} onClick={() => setDry(true)}>Dry run this command…</button>
        </div>
      )}
      {previewError && <Callout tone="bad" title="This hook cannot be previewed"><pre className="whitespace-pre-wrap break-words text-[12px]">{previewError}</pre></Callout>}
      <p className="m-0 text-[12px] text-muted">Preview shows the exact change to .claude/settings.json. Nothing is written until you save.</p>
      {dry && <DryRunDialog projectId={projectId} event={event} command={command} onClose={() => setDry(false)} />}
    </Modal>
  );
}
