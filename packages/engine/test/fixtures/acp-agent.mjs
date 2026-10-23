#!/usr/bin/env node
// Scripted ACP agent for tests (agent side of the official SDK). Behaviour is chosen by a marker in the prompt; the last
// message chunk is a `RESULT {...}` line describing what happened, for assertions.
//   ACP:edit    ask to edit FORGE_ACP.md (allow_once/allow_always/reject_once), then write it through fs/write_text_file
//   ACP:fswrite write FORGE_FS.md through fs/write_text_file without asking first
//   ACP:escape  ask to edit ../outside-acp.txt
//   ACP:always  ask with only allow_always / reject_always options
//   ACP:link    ask to edit FORGE_LINK.md, then replace it with a link to a file outside before writing
//   ACP:exec    ask to execute `git push origin main`
//   ACP:read    read /etc/hostname-like file outside the worktree through fs/read_text_file
//   ACP:hang    wait until cancelled
//   ACP:report  report what the client offered (capabilities, mcp servers)
// argv: --proto=N  answer initialize with protocol version N;  --auth  newSession fails with auth_required
import * as acp from '@agentclientprotocol/sdk';
import { Readable, Writable } from 'node:stream';
import { symlinkSync, writeFileSync, mkdtempSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { tmpdir } from 'node:os';

const proto = Number((process.argv.find((a) => a.startsWith('--proto=')) ?? '').split('=')[1] || acp.PROTOCOL_VERSION);
const needAuth = process.argv.includes('--auth');

class Scripted {
  constructor(conn) { this.conn = conn; this.sessions = new Map(); }
  async initialize(p) {
    this.clientCaps = p.clientCapabilities;
    return { protocolVersion: proto, agentCapabilities: { loadSession: false, mcpCapabilities: { http: false } }, agentInfo: { name: 'scripted-acp', version: '0.0.1' }, authMethods: needAuth ? [{ id: 'login', name: 'Log in' }] : [] };
  }
  async authenticate() { return {}; }
  async newSession(p) {
    if (needAuth) throw acp.RequestError.authRequired();
    const id = `acp-${Math.random().toString(16).slice(2, 10)}`;
    this.sessions.set(id, { cwd: p.cwd, mcp: p.mcpServers.map((m) => m.name), abort: null });
    return { sessionId: id };
  }
  async cancel(p) { this.sessions.get(p.sessionId)?.abort?.abort(); }
  async setSessionMode() { return {}; }
  async prompt(p) {
    const s = this.sessions.get(p.sessionId);
    const text = p.prompt.map((b) => (b.type === 'text' ? b.text : '')).join('');
    const say = (t) => this.conn.sessionUpdate({ sessionId: p.sessionId, update: { sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: t } } });
    const result = {};
    const opts = (always) => always
      ? [{ optionId: 'aa', name: 'Always allow', kind: 'allow_always' }, { optionId: 'ra', name: 'Always reject', kind: 'reject_always' }]
      : [{ optionId: 'ao', name: 'Allow', kind: 'allow_once' }, { optionId: 'aa', name: 'Always allow', kind: 'allow_always' }, { optionId: 'ro', name: 'Reject', kind: 'reject_once' }];
    const askEdit = async (id, path, always = false, kind = 'edit', rawInput = {}) => {
      await this.conn.sessionUpdate({ sessionId: p.sessionId, update: { sessionUpdate: 'tool_call', toolCallId: id, title: `${kind} ${path ?? ''}`, kind, status: 'pending', locations: path ? [{ path }] : [], rawInput } });
      const r = await this.conn.requestPermission({ sessionId: p.sessionId, toolCall: { toolCallId: id, title: `${kind} ${path ?? ''}`, kind, locations: path ? [{ path }] : [], rawInput }, options: opts(always) });
      return r.outcome.outcome === 'selected' ? r.outcome.optionId : 'cancelled';
    };
    const write = async (path, content) => {
      try { await this.conn.writeTextFile({ sessionId: p.sessionId, path, content }); return 'ok'; } catch (e) { return `error: ${e.message ?? e}`; }
    };
    await say('Working on it. ');
    if (text.includes('ACP:edit')) {
      const f = join(s.cwd, 'FORGE_ACP.md');
      result.edit = await askEdit('t1', f);
      if (result.edit === 'ao') result.write = await write(f, '# written through FORGE\n');
      await this.conn.sessionUpdate({ sessionId: p.sessionId, update: { sessionUpdate: 'tool_call_update', toolCallId: 't1', status: result.write === 'ok' ? 'completed' : 'failed' } });
    }
    if (text.includes('ACP:fswrite')) result.write = await write(join(s.cwd, 'FORGE_FS.md'), 'direct\n');
    if (text.includes('ACP:escape')) result.edit = await askEdit('t2', resolve(s.cwd, '..', 'outside-acp.txt'));
    if (text.includes('ACP:always')) result.edit = await askEdit('t3', join(s.cwd, 'FORGE_ALWAYS.md'), true);
    if (text.includes('ACP:link')) {
      const f = join(s.cwd, 'FORGE_LINK.md');
      result.edit = await askEdit('t4', f);
      const outside = join(mkdtempSync(join(tmpdir(), 'acp-victim-')), 'victim.txt');
      writeFileSync(outside, 'original\n');
      result.victim = outside;
      try { symlinkSync(outside, f); result.write = await write(f, 'pwned\n'); } catch (e) { result.symlinkError = String(e.code ?? e); }
    }
    if (text.includes('ACP:exec')) result.exec = await askEdit('t5', null, false, 'execute', { command: 'git push origin main' });
    if (text.includes('ACP:read')) {
      try { await this.conn.readTextFile({ sessionId: p.sessionId, path: resolve(s.cwd, '..', 'secret-acp.txt') }); result.read = 'ok'; } catch (e) { result.read = `error: ${e.message ?? e}`; }
    }
    if (text.includes('ACP:report')) Object.assign(result, { clientCaps: this.clientCaps, mcp: s.mcp, promptHasGuidance: text.includes('FORGE mission') });
    if (text.includes('ACP:hang')) {
      s.abort = new AbortController();
      await new Promise((res) => s.abort.signal.addEventListener('abort', res, { once: true }));
      return { stopReason: 'cancelled' };
    }
    await say(`RESULT ${JSON.stringify(result)}`);
    return { stopReason: 'end_turn' };
  }
}

const stream = acp.ndJsonStream(Writable.toWeb(process.stdout), Readable.toWeb(process.stdin));
new acp.AgentSideConnection((conn) => new Scripted(conn), stream);
