// JSON-RPC 2.0 over a child process's stdio (newline-delimited), as spoken by `codex app-server`.
// Transport only: no Codex semantics here, so the adapter can be tested against an in-memory peer.

import { type ChildProcessWithoutNullStreams, spawn as nodeSpawn } from 'node:child_process';
import { createInterface } from 'node:readline';

export class RpcError extends Error {
  constructor(
    public readonly code: number,
    message: string,
  ) {
    super(message);
  }
}

export interface CodexRpc {
  request<T = any>(method: string, params?: unknown): Promise<T>; // eslint-disable-line @typescript-eslint/no-explicit-any
  notify(method: string, params?: unknown): void;
  /** Server notifications (no id). */
  onNotification(h: (method: string, params: any) => void): void; // eslint-disable-line @typescript-eslint/no-explicit-any
  /** Server → client requests. Resolve with the result, or throw RpcError to answer with an error. */
  onRequest(h: (method: string, params: any) => Promise<unknown>): void; // eslint-disable-line @typescript-eslint/no-explicit-any
}

export interface CodexServer {
  rpc: CodexRpc;
  /** Resolves when the server process is gone. */
  exited: Promise<{ code: number | null; stderrTail: string }>;
  /** Ends the server and its descendants. */
  kill(): void;
}

/** Variables that could make Codex bill an API key or talk to another endpoint instead of the user's own login. */
const REQUEST_TIMEOUT_MS = 90_000;
const STRIPPED_ENV = ['OPENAI_API_KEY', 'CODEX_API_KEY', 'OPENAI_BASE_URL', 'OPENAI_API_BASE', 'OPENAI_ORG_ID', 'OPENAI_ORGANIZATION'];

/** Windows environment names are case-insensitive, so `openai_api_key` must go too. */
export function sanitizedCodexEnv(env: NodeJS.ProcessEnv): NodeJS.ProcessEnv {
  const drop = new Set(STRIPPED_ENV.map((k) => k.toUpperCase()));
  const out: NodeJS.ProcessEnv = {};
  for (const [k, v] of Object.entries(env)) if (!drop.has(k.toUpperCase())) out[k] = v;
  return out;
}

/** Process plumbing, injectable so tests can check what cleanup would run without running it. */
export interface SpawnDeps {
  spawn?: (command: string, args: string[], options: Record<string, unknown>) => ChildProcessWithoutNullStreams;
  platform?: NodeJS.Platform;
  /** Where `%SystemRoot%` is read from for the tree kill (defaults to the engine's own environment, never the worktree). */
  hostEnv?: NodeJS.ProcessEnv;
}

/** The tree-kill command for Windows: an absolute System32 path and a neutral cwd, so nothing in the worktree can stand in for it. */
export function windowsTreeKill(pid: number, hostEnv: NodeJS.ProcessEnv): { command: string; args: string[]; cwd: string } {
  const fromEnv = hostEnv.SystemRoot || hostEnv.windir || '';
  // Only an absolute drive path is acceptable; anything relative would be resolved against a directory we do not control.
  const root = /^[A-Za-z]:\\[^"]*$/.test(fromEnv) && !fromEnv.includes('..') ? fromEnv.replace(/\\+$/, '') : 'C:\\Windows';
  return { command: `${root}\\System32\\taskkill.exe`, args: ['/PID', String(pid), '/T', '/F'], cwd: root };
}

export function spawnCodexServer(exe: string, args: string[], cwd: string, env: NodeJS.ProcessEnv, timeoutMs: number = REQUEST_TIMEOUT_MS, deps: SpawnDeps = {}): CodexServer {
  const spawn = deps.spawn ?? (nodeSpawn as unknown as NonNullable<SpawnDeps['spawn']>);
  const platform = deps.platform ?? process.platform;
  // No shell: the executable is a real binary and nothing here may be re-parsed by cmd.exe.
  const child = spawn(exe, args, { cwd, env: sanitizedCodexEnv(env), stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true });
  let stderrTail = '';
  child.stderr.on('data', (d: Buffer) => {
    stderrTail = (stderrTail + d.toString('utf8')).slice(-4000);
  });

  let nextId = 0;
  const pending = new Map<number, { resolve: (v: any) => void; reject: (e: Error) => void }>(); // eslint-disable-line @typescript-eslint/no-explicit-any
  let notificationHandler: (m: string, p: any) => void = () => {}; // eslint-disable-line @typescript-eslint/no-explicit-any
  let requestHandler: (m: string, p: any) => Promise<unknown> = async () => { // eslint-disable-line @typescript-eslint/no-explicit-any
    throw new RpcError(-32601, 'not supported');
  };

  const write = (obj: unknown) => {
    if (child.stdin.writable) child.stdin.write(JSON.stringify(obj) + '\n');
  };

  const rl = createInterface({ input: child.stdout });
  rl.on('line', (line) => {
    if (!line.trim()) return;
    let m: any; // eslint-disable-line @typescript-eslint/no-explicit-any
    try {
      m = JSON.parse(line);
    } catch {
      return;
    }
    if (m.id !== undefined && m.method) {
      // server → client request
      const id = m.id;
      requestHandler(m.method, m.params ?? {}).then(
        (result) => write({ jsonrpc: '2.0', id, result: result ?? {} }),
        (e) => write({ jsonrpc: '2.0', id, error: { code: e instanceof RpcError ? e.code : -32603, message: e instanceof Error ? e.message : String(e) } }),
      );
    } else if (m.id !== undefined) {
      const p = pending.get(m.id);
      if (!p) return;
      pending.delete(m.id);
      if (m.error) p.reject(new RpcError(m.error.code ?? -32000, m.error.message ?? 'request failed'));
      else p.resolve(m.result);
    } else if (m.method) {
      notificationHandler(m.method, m.params ?? {});
    }
  });

  const exited = new Promise<{ code: number | null; stderrTail: string }>((resolve) => {
    child.on('exit', (code) => {
      for (const [id, p] of pending) {
        pending.delete(id);
        p.reject(new Error(`codex exited (code ${code}) before answering`));
      }
      resolve({ code, stderrTail });
    });
    child.on('error', (e) => {
      stderrTail += `\n${e.message}`;
      for (const [id, p] of pending) {
        pending.delete(id);
        p.reject(new Error(`could not run codex: ${e.message}`));
      }
      resolve({ code: null, stderrTail });
    });
  });

  return {
    exited,
    kill: () => {
      try {
        child.stdin.end();
      } catch {
        /* already closed */
      }
      if (child.pid && platform === 'win32') {
        // Whole tree: a shell command the agent started must not outlive the session.
        // Absolute path and a neutral cwd: the working directory is the (hostile) worktree and Windows looks there first.
        const k = windowsTreeKill(child.pid, deps.hostEnv ?? process.env);
        spawn(k.command, k.args, { stdio: 'ignore', windowsHide: true, cwd: k.cwd }).on('error', () => {});
      } else {
        child.kill('SIGKILL');
      }
    },
    rpc: {
      request: (method, params) =>
        new Promise((resolve, reject) => {
          const id = ++nextId;
          // A server that stops answering must not hang the session forever.
          const timer = setTimeout(() => {
            if (pending.delete(id)) reject(new Error(`codex did not answer ${method} within ${timeoutMs / 1000}s`));
          }, timeoutMs);
          timer.unref();
          pending.set(id, {
            resolve: (v) => { clearTimeout(timer); resolve(v); },
            reject: (e) => { clearTimeout(timer); reject(e); },
          });
          write({ jsonrpc: '2.0', id, method, params: params ?? {} });
        }),
      notify: (method, params) => write({ jsonrpc: '2.0', method, params: params ?? {} }),
      onNotification: (h) => {
        notificationHandler = h;
      },
      onRequest: (h) => {
        requestHandler = h;
      },
    },
  };
}
