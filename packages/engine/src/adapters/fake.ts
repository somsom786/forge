// A scripted TEST agent. It is not a provider integration and must never be presented
// as one: it exists so FORGE's own machinery (worktrees, approvals, persistence, crash
// recovery, cancellation) can be exercised deterministically without spending provider
// quota. It performs real filesystem writes inside the mission cwd only after FORGE
// grants permission, so approval enforcement is genuinely end-to-end.
//
// Directives in the prompt select behaviour:
//   FAKE:crash   exit abruptly mid-run (simulates an agent/process crash)
//   FAKE:hang    never finish until cancelled
//   FAKE:deny    attempt a write outside the worktree (policy must deny it)
//   FAKE:env     attempt to write a protected file (.env)
//   FAKE:replay  ask twice with the SAME toolUseId but different input (a re-ask must not inherit approval)
//   FAKE:orphan  leave a descendant process holding the stdio pipes after finishing

import { spawn } from 'node:child_process';
import { mkdir, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import type { CapabilityMatrix } from '../protocol.js';
import type { ProviderAdapter, RunContext } from './types.js';

const zeroUsage = { inputTokens: 0, outputTokens: 0, cacheReadTokens: 0, cacheCreationTokens: 0 };

export const FAKE_CAPABILITIES: CapabilityMatrix = {
  sessionStart: { supported: true },
  resume: { supported: false },
  streaming: { supported: true },
  interrupt: { supported: true },
  toolEvents: { supported: true },
  fileChanges: { supported: true },
  approvals: { supported: true },
  usageReporting: { supported: false, note: 'Test agent reports no usage.' },
  modelSelection: { supported: false },
  worktreeIntegration: { supported: true },
};

const sleep = (ms: number, signal: AbortSignal) =>
  new Promise<void>((res) => {
    const t = setTimeout(res, ms);
    signal.addEventListener('abort', () => { clearTimeout(t); res(); }, { once: true });
  });

export const fakeAdapter: ProviderAdapter = {
  id: 'fake',
  capabilities: () => FAKE_CAPABILITIES,

  async run(ctx: RunContext) {
    const { prompt, cwd } = ctx.start;
    const sid = 'fake-' + ctx.start.sessionId.slice(0, 8);
    ctx.providerSession(sid);
    ctx.emit('session.started', { providerSessionId: sid, model: 'scripted-test-agent', credentialSource: 'scripted', cwd });
    // Test visibility: what this agent was actually handed (the task text, and whether memory leaked into the system prompt).
    ctx.emit('status', { message: 'prompt received', promptPreview: prompt.slice(0, 8000), systemPromptHasKnowledge: (ctx.start.appendSystemPrompt ?? '').includes('FORGE_PROJECT_KNOWLEDGE') });
    ctx.delta('m1', 'Planning the change… ');
    await sleep(50, ctx.signal);
    ctx.emit('agent.text', { messageId: 'm1', text: `Plan: carry out "${prompt.slice(0, 80)}" using the scripted test agent.` });

    if (prompt.includes('FAKE:crash')) process.exit(7);
    if (prompt.includes('FAKE:hang')) {
      await sleep(10 * 60_000, ctx.signal);
      ctx.turnComplete({ status: 'interrupted', message: 'Cancelled', usage: zeroUsage, permissionDenials: 0 });
      return;
    }

    if (prompt.includes('FAKE:orphan')) {
      // A grandchild inheriting stdout/stderr: the pipes stay open after the engine itself exits.
      const child = spawn(process.execPath, ['-e', 'setInterval(()=>{},1000)'], { stdio: ['ignore', 'inherit', 'inherit'] });
      ctx.emit('status', { message: 'spawned a lingering descendant', childPid: child.pid });
    }

    const targets: string[] = [];
    if (prompt.includes('FAKE:deny')) targets.push(resolve(cwd, '..', 'outside-worktree.txt'));
    else if (prompt.includes('FAKE:env')) targets.push(join(cwd, '.env'));
    else if (prompt.includes('FAKE:replay')) targets.push(join(cwd, 'FORGE_TEST.md'), join(cwd, 'FORGE_TEST_2.md'));
    else targets.push(join(cwd, 'FORGE_TEST.md'));

    let denials = 0;
    for (const [i, file] of targets.entries()) {
      const toolUseId = prompt.includes('FAKE:replay') ? 'tu_dup' : `tu_write_${i}`;
      const input = { file_path: file, content: `# scripted change\n\nprompt: ${prompt.slice(0, 200)}\n` };
      ctx.emit('tool.call', { toolUseId, name: 'Write', summary: file, input });
      const d = await ctx.requestPermission({ tool: 'Write', input, toolUseId, source: 'hook' });
      if (ctx.signal.aborted) break;
      if (d.behavior === 'allow') {
        await mkdir(dirname(file), { recursive: true });
        await writeFile(file, input.content, 'utf8');
        ctx.emit('tool.result', { toolUseId, name: 'Write', ok: true, output: `wrote ${file}` });
        ctx.emit('file.changed', { path: file, tool: 'Write', toolUseId });
      } else {
        denials++;
        ctx.emit('tool.result', { toolUseId, name: 'Write', ok: false, output: d.message ?? 'denied' });
      }
    }

    if (ctx.signal.aborted) {
      ctx.turnComplete({ status: 'interrupted', message: 'Cancelled', usage: zeroUsage, permissionDenials: denials });
      return;
    }
    ctx.emit('agent.text', { messageId: 'm2', text: denials ? 'Some actions were denied.' : 'Done.' });
    ctx.turnComplete({ status: 'success', usage: zeroUsage, permissionDenials: denials, providerSessionId: sid });
  },
};
