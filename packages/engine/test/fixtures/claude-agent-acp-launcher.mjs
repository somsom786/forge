#!/usr/bin/env node
// Test-only launcher for the real third-party ACP agent `@agentclientprotocol/claude-agent-acp` (installed outside FORGE's
// dependencies, in ACP_INTEROP_AGENT_DIR). It points the agent's Claude SDK at the scripted local model and then runs the
// agent unchanged. FORGE itself strips such endpoint redirects from agent environments; this wrapper is test scaffolding.
import { pathToFileURL } from 'node:url';
import { join } from 'node:path';

// FORGE strips FORGE_* variables from agent environments, so the test passes the directory under a neutral name.
const dir = process.env.ACP_INTEROP_AGENT_DIR;
Object.assign(process.env, {
  ANTHROPIC_BASE_URL: `http://127.0.0.1:${process.env.FAKE_MODEL_PORT}`,
  ANTHROPIC_API_KEY: 'scripted-model-no-key',
  ANTHROPIC_MODEL: 'fake-model',
  CLAUDE_CONFIG_DIR: process.env.FAKE_CLAUDE_CONFIG_DIR,
  NO_PROXY: '127.0.0.1,localhost',
  no_proxy: '127.0.0.1,localhost',
  CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC: '1',
});
await import(pathToFileURL(join(dir, 'node_modules/@agentclientprotocol/claude-agent-acp/dist/index.js')).href);
