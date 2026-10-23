// JSON-RPC 2.0 over a child process's stdio (newline-delimited), as spoken by `codex app-server`.
// Transport only: no Codex semantics here, so the adapter can be tested against an in-memory peer.

import { spawn } from 'node:child_process';
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
const STRIPPED_ENV = ['OPENAI_API_KEY', 'CODEX_API_KEY', 'OPENAI_BASE_URL', 'OPENAI_API_BASE', 'OPENAI_ORG_ID', 'OPENAI_ORGANIZATION'];

export function sanitizedCodexEnv(env: NodeJS.ProcessEnv): NodeJS.ProcessEnv {
  const out = { ...env };
  for (const k of STRIPPED_ENV) delete out[k];
  return out;
}

export function spawnCodexServer(exe: string, args: string[], cwd: string, env: NodeJS.ProcessEnv): CodexServer {
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
      if (child.pid && process.platform === 'win32') {
        // Whole tree: a shell command the agent started must not outlive the session.
        spawn('taskkill', ['/PID', String(child.pid), '/T', '/F'], { stdio: 'ignore', windowsHide: true }).on('error', () => {});
      } else {
        child.kill('SIGKILL');
      }
    },
    rpc: {
      request: (method, params) =>
        new Promise((resolve, reject) => {
          const id = ++nextId;
          pending.set(id, { resolve, reject });
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
