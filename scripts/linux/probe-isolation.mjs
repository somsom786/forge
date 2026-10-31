// Harmless component probe for bubblewrap on Linux/WSL. It reads/writes only temporary fixture files, starts no provider,
// and installs nothing. This exercises OS effects, not the Rust supervisor or a FORGE Windows execution backend.
import assert from 'node:assert/strict';
import { execFile, spawn } from 'node:child_process';
import { promisify } from 'node:util';
import { createServer } from 'node:http';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, unlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

if (process.platform !== 'linux') throw new Error('Linux only; this does not implement Windows isolation');
const run = promisify(execFile);
const bwrap = process.argv[2] ?? 'bwrap';
const root = mkdtempSync(join(tmpdir(), 'forge-isolation-probe-'));
const workspace = join(root, 'workspace');
const outside = join(root, 'outside');
mkdirSync(workspace); mkdirSync(outside);
const secret = join(outside, 'fixture.txt');
const marker = join(outside, 'written.txt');
writeFileSync(secret, 'harmless-fixture');
const env = { ...process.env, FORGE_FIXTURE_SECRET: 'fixture-environment' };
const runtime = process.execPath;
const server = createServer((_req, res) => res.end('fixture-server'));
await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
const endpoint = `http://127.0.0.1:${server.address().port}`;

const program = `const fs=require('node:fs'); (async()=>{
  let read=false,write=false,network=false;
  try{read=fs.readFileSync(${JSON.stringify(secret)},'utf8')==='harmless-fixture'}catch{}
  try{fs.writeFileSync(${JSON.stringify(marker)},'fixture');write=true}catch{}
  fs.writeFileSync(${JSON.stringify(join(workspace, 'inside.txt'))},'workspace-fixture');
  try{network=(await (await fetch(${JSON.stringify(endpoint)},{signal:AbortSignal.timeout(1000)})).text())==='fixture-server'}catch{}
  console.log(JSON.stringify({read,write,network,environment:process.env.FORGE_FIXTURE_SECRET!==undefined}));
})().catch(e=>{console.error(e);process.exit(1)});`;

function args(agent, code, engine) {
  const a = ['--unshare-all'];
  if (agent) a.push('--share-net');
  a.push('--die-with-parent', '--new-session', '--cap-drop', 'ALL');
  if (!agent) a.push('--clearenv');
  for (const dir of ['/usr', '/bin', '/sbin', '/lib', '/lib64']) if (existsSync(dir)) a.push('--ro-bind', dir, dir);
  for (const file of ['/etc/passwd', '/etc/group', '/etc/hosts', '/etc/nsswitch.conf', '/etc/resolv.conf', '/etc/ld.so.cache']) if (existsSync(file)) a.push('--ro-bind', file, file);
  a.push('--proc', '/proc', '--dev', '/dev', '--tmpfs', '/tmp', '--tmpfs', '/home', '--tmpfs', '/root', '--tmpfs', '/run', '--dir', '/home/forge');
  a.push('--bind', workspace, workspace, '--chdir', workspace, '--setenv', 'HOME', '/home/forge', '--setenv', 'PATH', '/usr/bin:/bin');
  if (engine) a.push('--ro-bind', engine, '/home/forge/engine.mjs', '--setenv', 'FORGE_ALLOW_FAKE_PROVIDER', '1', '--setenv', 'FORGE_SANDBOX', 'linux_namespaces', '--', runtime, '/home/forge/engine.mjs');
  else a.push('--', runtime, '-e', code);
  return a;
}

function runEngine(engine, prompt, cancel = false) {
  return new Promise((resolve, reject) => {
    const child = spawn(bwrap, args(true, null, engine), { env: { PATH: '/usr/bin:/bin' }, stdio: ['pipe', 'pipe', 'pipe'] });
    let output = '', diagnostics = '';
    const deadline = setTimeout(() => { child.kill('SIGKILL'); reject(new Error('contained engine timed out')); }, 8000);
    const cancellation = cancel ? setTimeout(() => child.stdin.write(JSON.stringify({ t: 'cancel' }) + '\n'), 300) : null;
    child.stdout.on('data', (chunk) => { output += chunk; if (output.length > 1_000_000) child.kill('SIGKILL'); });
    child.stderr.on('data', (chunk) => { diagnostics = (diagnostics + chunk).slice(-8000); });
    child.on('error', reject);
    child.on('close', (code) => {
      clearTimeout(deadline); if (cancellation) clearTimeout(cancellation);
      if (code !== 0) reject(new Error(`contained engine exit ${code}: ${diagnostics}`));
      else { try { resolve(output.trim().split('\n').filter(Boolean).map((line) => JSON.parse(line))); } catch (e) { reject(e); } }
    });
    child.stdin.on('error', () => {});
    child.stdin.write(JSON.stringify({ t: 'start', sessionId: 'fixture', provider: 'fake', cwd: workspace, prompt, permissionMode: 'default' }) + '\n');
  });
}

try {
  const version = await run(bwrap, ['--version'], { timeout: 5000 });
  const host = JSON.parse((await run(runtime, ['-e', program], { env, timeout: 5000 })).stdout);
  assert.deepEqual(host, { read: true, write: true, network: true, environment: true });
  unlinkSync(marker);
  const check = JSON.parse((await run(bwrap, args(false, program), { env, timeout: 8000 })).stdout);
  assert.deepEqual(check, { read: false, write: false, network: false, environment: false });
  assert(!existsSync(marker));
  const agentEnv = { PATH: '/usr/bin:/bin', HOME: process.env.HOME ?? '/home/forge' };
  const agent = JSON.parse((await run(bwrap, args(true, program), { env: agentEnv, timeout: 8000 })).stdout);
  assert.deepEqual(agent, { read: false, write: false, network: true, environment: false });
  assert.equal(readFileSync(join(workspace, 'inside.txt'), 'utf8'), 'workspace-fixture');
  let engine = null;
  if (process.argv[3]) {
    const events = await runEngine(process.argv[3], `FAKE:snoop SNOOP=${secret}`);
    const snoop = events.find((e) => e.payload?.message === 'snoop')?.payload;
    assert(snoop?.read.startsWith('ERROR') && snoop.wrote === false && snoop.sandbox === 'linux_namespaces');
    const cancellation = await runEngine(process.argv[3], 'FAKE:hang', true);
    assert(cancellation.some((e) => e.t === 'turn_complete' && e.status === 'interrupted'));
    engine = { bundledEngineOutsideAccessDenied: true, cancellationCompleted: true, provider: 'scripted fixture' };
  }
  console.log(JSON.stringify({ tool: version.stdout.trim(), hostControl: host, isolatedCheck: check, containedAgent: agent, engine, note: 'Linux component effects only; no FORGE Windows backend or Rust supervisor was tested.' }, null, 2));
} finally {
  await new Promise((resolve) => server.close(resolve));
  rmSync(root, { recursive: true, force: true });
}
