// Exploration only: speak JSON-RPC to `codex app-server` over stdio and log every message. Not part of the product.
//   node research/codex-probe.mjs <cwd> "<prompt>" [model] [modelProvider]
import { spawn } from 'node:child_process';
import { writeFileSync, appendFileSync } from 'node:fs';

const [cwd, prompt, model, modelProvider] = process.argv.slice(2);
const log = process.env.PROBE_LOG ?? 'research/codex-probe.log';
writeFileSync(log, '');
const extra = process.env.PROBE_ARGS ? JSON.parse(process.env.PROBE_ARGS) : [];
const p = spawn(process.env.CODEX_EXE ?? 'codex', ['app-server', ...extra], { cwd, stdio: ['pipe', 'pipe', 'pipe'], shell: !process.env.CODEX_EXE && process.platform === 'win32' });
let id = 0;
const pending = new Map();
const send = (method, params) => new Promise((res, rej) => {
  const i = ++id;
  pending.set(i, { res, rej, method });
  p.stdin.write(JSON.stringify({ jsonrpc: '2.0', id: i, method, params }) + '\n');
});
const notify = (method, params) => p.stdin.write(JSON.stringify({ jsonrpc: '2.0', method, params }) + '\n');
let buf = '';
let threadId = null;
let done;
const finished = new Promise((r) => { done = r; });
p.stdout.on('data', (d) => {
  buf += d;
  let n;
  while ((n = buf.indexOf('\n')) >= 0) {
    const line = buf.slice(0, n).trim();
    buf = buf.slice(n + 1);
    if (!line) continue;
    appendFileSync(log, line + '\n');
    let m;
    try { m = JSON.parse(line); } catch { continue; }
    if (m.id !== undefined && pending.has(m.id) && (m.result !== undefined || m.error)) {
      const { res, rej } = pending.get(m.id);
      pending.delete(m.id);
      m.error ? rej(new Error(JSON.stringify(m.error))) : res(m.result);
    } else if (m.id !== undefined && m.method) {
      // server -> client request (approvals): answer "decline" so nothing runs unattended in the probe
      console.log('SERVER REQUEST', m.method, JSON.stringify(m.params).slice(0, 300));
      const decision = m.method.includes('requestApproval') ? { decision: 'decline' } : {};
      p.stdin.write(JSON.stringify({ jsonrpc: '2.0', id: m.id, result: decision }) + '\n');
    } else if (m.method) {
      if (!/delta|outputDelta|tokenUsage|rateLimits/i.test(m.method)) console.log('notify', m.method, JSON.stringify(m.params).slice(0, 220));
      if (m.method === 'turn/completed' || m.method === 'error') done();
    }
  }
});
p.stderr.on('data', (d) => appendFileSync(log, '[stderr] ' + d));
p.on('exit', (c) => { console.log('codex exited', c); done(); });

const init = await send('initialize', { clientInfo: { name: 'forge-probe', title: 'FORGE probe', version: '0.0.1' }, capabilities: null });
console.log('initialize ->', JSON.stringify(init).slice(0, 300));
notify('initialized', {});
if (process.env.PROBE_CONFIGREAD) {
  const cr = await send('config/read', { cwd, includeLayers: false });
  console.log('CONFIG KEYS', Object.keys(cr.config).join(','));
  console.log('MCP', JSON.stringify(cr.config.mcp_servers ?? cr.config.mcpServers ?? null).slice(0, 800));
  console.log('ORIGINS', Object.keys(cr.origins).filter((k) => /mcp/i.test(k)).join(','));
  p.stdin.end(); setTimeout(() => process.exit(0), 300); await new Promise(() => {});
}
if (process.env.PROBE_LIST) {
  const ml = await send('model/list', {});
  console.log(JSON.stringify((ml.data ?? ml.models ?? ml).map?.((m) => ({ id: m.id ?? m.model, name: m.displayName, efforts: (m.supportedReasoningEfforts ?? []).map((e) => e.reasoningEffort ?? e), def: m.defaultReasoningEffort, hidden: m.hidden })), null, 0));
  p.stdin.end(); setTimeout(() => process.exit(0), 300); await new Promise(() => {});
}
const started = await send('thread/start', { cwd, approvalPolicy: process.env.PROBE_APPROVAL ? JSON.parse(process.env.PROBE_APPROVAL) : 'untrusted', sandbox: 'workspace-write', ephemeral: true, ...(model ? { model } : {}), ...(modelProvider ? { modelProvider } : {}), ...(process.env.PROBE_THREAD_CONFIG ? { config: JSON.parse(process.env.PROBE_THREAD_CONFIG) } : {}) });
console.log('thread/start ->', JSON.stringify(started).slice(0, 600));
threadId = started.thread?.id ?? started.threadId;
const turn = await send('turn/start', { threadId, input: [{ type: 'text', text: prompt, text_elements: [] }], ...(process.env.PROBE_EFFORT ? { effort: process.env.PROBE_EFFORT } : {}) });
console.log('turn/start ->', JSON.stringify(turn).slice(0, 300));
await Promise.race([finished, new Promise((r) => setTimeout(r, 120000))]);
p.stdin.end();
setTimeout(() => process.exit(0), 500);
