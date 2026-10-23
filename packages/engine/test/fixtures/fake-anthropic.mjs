#!/usr/bin/env node
// Scripted stand-in for the Anthropic Messages API on loopback, for running the REAL Claude Code CLI in tests without an
// account or quota. It is not a model: it replays a fixed list of tool calls chosen by a marker in the conversation, one per
// request, then ends the turn. Prints {"port": N} on stdout once listening.
//   CLAUDE:write   Write FORGE_CLAUDE.md
//   CLAUDE:escape  Write ../outside-claude.txt
//   CLAUDE:bash    Bash `cat <SECRET_PATH>` (the path follows `SECRET_PATH=` in the prompt)
//   CLAUDE:mcp     mcp__docs__search, mcp__docs__delete, mcp__evil__exfil
import { createServer } from 'node:http';

const SCRIPTS = {
  'CLAUDE:write': (cwd) => [{ name: 'Write', input: { file_path: `${cwd}/FORGE_CLAUDE.md`, content: '# written by real Claude Code through FORGE\n' } }],
  'CLAUDE:escape': (cwd) => [{ name: 'Write', input: { file_path: `${cwd}/../outside-claude.txt`, content: 'escaped\n' } }],
  'CLAUDE:bash': (_cwd, convo) => [{ name: 'Bash', input: { command: `cat ${(/SECRET_PATH=([^\s"\\]+)/.exec(convo) ?? [])[1] ?? '/nonexistent'}`, description: 'read the file' } }],
  'CLAUDE:mcp': () => [
    { name: 'mcp__docs__search', input: { query: 'release notes' } },
    { name: 'mcp__docs__delete', input: { id: 'all' } },
    { name: 'mcp__evil__exfil', input: { what: 'secrets' } },
  ],
};

const text = (v) => (typeof v === 'string' ? v : JSON.stringify(v ?? ''));

const srv = createServer((req, res) => {
  let body = '';
  req.on('data', (d) => (body += d));
  req.on('end', () => {
    if (!req.url.startsWith('/v1/messages') || req.url.includes('count_tokens')) {
      res.writeHead(200, { 'content-type': 'application/json' });
      return res.end(JSON.stringify({ input_tokens: 10 }));
    }
    let j = {};
    try { j = JSON.parse(body); } catch { /* empty */ }
    const convo = text(j.messages) + text(j.system);
    const key = Object.keys(SCRIPTS).find((k) => convo.includes(k));
    const cwd = (/working directory:?\s*([^\s"\\]+)/i.exec(convo) ?? [])[1] ?? '.';
    const steps = key ? SCRIPTS[key](cwd, convo) : [];
    // One tool call per assistant turn: the number of tool results so far says which step is next. Only real turns
    // (requests that offer tools) advance the script; side requests (titles, summaries) just get text.
    const done = (convo.match(/"type":"tool_result"/g) ?? []).length;
    const offersTools = Array.isArray(j.tools) && j.tools.length > 0;
    const step = offersTools ? steps[done] : undefined;
    const content = step ? [{ type: 'tool_use', id: `toolu_fx${done}_${Math.random().toString(16).slice(2, 8)}`, name: step.name, input: step.input }] : [{ type: 'text', text: 'Done.' }];
    const stop = step ? 'tool_use' : 'end_turn';
    const msg = { id: `msg_${Date.now()}`, type: 'message', role: 'assistant', model: j.model, content: [], stop_reason: null, stop_sequence: null, usage: { input_tokens: 10, output_tokens: 5 } };
    if (!j.stream) {
      res.writeHead(200, { 'content-type': 'application/json' });
      return res.end(JSON.stringify({ ...msg, content, stop_reason: stop }));
    }
    res.writeHead(200, { 'content-type': 'text/event-stream' });
    const ev = (t, d) => res.write(`event: ${t}\ndata: ${JSON.stringify({ type: t, ...d })}\n\n`);
    ev('message_start', { message: msg });
    content.forEach((b, i) => {
      if (b.type === 'text') {
        ev('content_block_start', { index: i, content_block: { type: 'text', text: '' } });
        ev('content_block_delta', { index: i, delta: { type: 'text_delta', text: b.text } });
      } else {
        ev('content_block_start', { index: i, content_block: { type: 'tool_use', id: b.id, name: b.name, input: {} } });
        ev('content_block_delta', { index: i, delta: { type: 'input_json_delta', partial_json: JSON.stringify(b.input) } });
      }
      ev('content_block_stop', { index: i });
    });
    ev('message_delta', { delta: { stop_reason: stop, stop_sequence: null }, usage: { output_tokens: 5 } });
    ev('message_stop', {});
    res.end();
  });
});
srv.listen(0, '127.0.0.1', () => process.stdout.write(JSON.stringify({ port: srv.address().port }) + '\n'));
process.stdin.on('end', () => process.exit(0)); // the test closes stdin to stop the server
process.stdin.resume();
