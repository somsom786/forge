// Session host. One process = one agent session.
// stdout carries ONLY protocol lines; everything else goes to stderr.

import { createInterface } from 'node:readline';
import { claudeAdapter } from './adapters/claude.js';
import { acpAdapter } from './adapters/acp.js';
import { codexAdapter } from './adapters/codex.js';
import { fakeAdapter } from './adapters/fake.js';
import type { TurnResult } from './adapters/claudeNormalize.js';
import type { PermissionDecision, PermissionRequest, ProviderAdapter, RunContext } from './adapters/types.js';
import type { Command, EngineEvent, StartCommand } from './protocol.js';
import { errMessage } from './util.js';

const ENGINE_VERSION = '0.1.0';

// Keep stdout clean even if a dependency calls console.log.
console.log = (...a: unknown[]) => process.stderr.write(a.map(String).join(' ') + '\n');
console.info = console.log;

function send(ev: EngineEvent): void {
  process.stdout.write(JSON.stringify(ev) + '\n');
}

function adapterFor(start: StartCommand): ProviderAdapter {
  if (start.provider === 'claude' || start.provider === 'claude-local') return claudeAdapter;
  if (start.provider === 'codex') return codexAdapter;
  if (start.provider === 'acp') return acpAdapter;
  if (start.provider === 'fake') {
    if (process.env.FORGE_ALLOW_FAKE_PROVIDER !== '1') throw new Error('the scripted test agent is disabled');
    return fakeAdapter;
  }
  throw new Error(`provider "${start.provider}" has no adapter in this build`);
}

let seq = 0;
const pending = new Map<string, (d: PermissionDecision) => void>();
let permCounter = 0;
const abort = new AbortController();
let started = false;
let finished = false;

function denyAllPending(message: string) {
  for (const [id, resolve] of pending) {
    resolve({ behavior: 'deny', message });
    pending.delete(id);
  }
}

function makeContext(start: StartCommand): RunContext {
  return {
    start,
    signal: abort.signal,
    emit: (kind, payload) => send({ t: 'event', seq: ++seq, kind, payload }),
    delta: (messageId, text) => send({ t: 'delta', messageId, text }),
    providerSession: (id) => send({ t: 'provider_session', providerSessionId: id }),
    requestPermission: ({ signal, ...req }: PermissionRequest) =>
      new Promise<PermissionDecision>((resolve) => {
        if (abort.signal.aborted) return resolve({ behavior: 'deny', message: 'Mission was cancelled' });
        const requestId = `p${++permCounter}`;
        pending.set(requestId, resolve);
        // The provider may withdraw a tool call (interrupt, timeout). Tell the core so a stale prompt does not linger.
        signal?.addEventListener(
          'abort',
          () => {
            if (pending.delete(requestId)) {
              send({ t: 'permission_cancelled', requestId });
              resolve({ behavior: 'deny', message: 'The tool call was withdrawn' });
            }
          },
          { once: true },
        );
        send({ t: 'permission', requestId, ...req });
      }),
    turnComplete: (r: TurnResult) => {
      finished = true;
      send({
        t: 'turn_complete',
        status: r.status,
        message: r.message,
        providerSessionId: r.providerSessionId,
        usage: r.usage,
        permissionDenials: r.permissionDenials,
      });
    },
  };
}

async function start(cmd: StartCommand) {
  if (started) return send({ t: 'fatal', message: 'session already started' });
  started = true;
  let adapter: ProviderAdapter;
  try {
    adapter = adapterFor(cmd);
  } catch (e) {
    send({ t: 'fatal', message: errMessage(e) });
    return exit(1);
  }
  send({ t: 'ready', engine: ENGINE_VERSION, provider: adapter.id, capabilities: adapter.capabilities(), pid: process.pid });
  try {
    await adapter.run(makeContext(cmd));
    if (!finished) {
      send({ t: 'fatal', message: 'adapter returned without reporting a turn result' });
      return exit(1);
    }
    exit(0);
  } catch (e) {
    send({ t: 'fatal', message: errMessage(e) });
    exit(1);
  }
}

function exit(code: number) {
  // Let stdout flush before exiting.
  process.stdout.write('', () => setTimeout(() => process.exit(code), 20));
}

function onCommand(line: string) {
  if (!line.trim()) return;
  let cmd: Command;
  try {
    cmd = JSON.parse(line) as Command;
  } catch {
    process.stderr.write(`[engine] ignoring malformed command: ${line.slice(0, 200)}\n`);
    return;
  }
  switch (cmd.t) {
    case 'start':
      void start(cmd);
      break;
    case 'permission_result': {
      const r = pending.get(cmd.requestId);
      if (r) {
        pending.delete(cmd.requestId);
        r({ behavior: cmd.behavior, message: cmd.message });
      }
      break;
    }
    case 'cancel':
      abort.abort();
      denyAllPending('Mission was cancelled');
      break;
    case 'close':
      break;
    default:
      process.stderr.write(`[engine] unknown command\n`);
  }
}

const rl = createInterface({ input: process.stdin });
rl.on('line', onCommand);
// If the core goes away (crash / force-kill) never leave an agent running unsupervised.
rl.on('close', () => {
  abort.abort();
  denyAllPending('FORGE core disconnected');
  setTimeout(() => process.exit(3), 4000).unref();
});

process.on('uncaughtException', (e) => {
  send({ t: 'fatal', message: `uncaught: ${errMessage(e)}` });
  process.exit(1);
});
process.on('unhandledRejection', (e) => {
  send({ t: 'fatal', message: `unhandled rejection: ${errMessage(e)}` });
  process.exit(1);
});
