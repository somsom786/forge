import { readFileSync, realpathSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, isAbsolute, join } from 'node:path';
import { pathToFileURL } from 'node:url';

export const CLAUDE_SDK_VERSION = '0.3.309';
const require = createRequire(import.meta.url);

/** Load a separately installed dependency. No SDK code is redistributed in FORGE's engine bundle. */
export function resolveClaudeSdk(env: NodeJS.ProcessEnv = process.env, resolveDefault = () => require.resolve('@anthropic-ai/claude-agent-sdk')): string {
  let root: string;
  try {
    const override = env.FORGE_CLAUDE_SDK_PATH;
    if (override !== undefined) {
      if (!isAbsolute(override)) throw new Error('FORGE_CLAUDE_SDK_PATH must be an absolute package directory');
      root = realpathSync(override);
    } else {
      root = dirname(resolveDefault());
    }
    const pkg = JSON.parse(readFileSync(join(root, 'package.json'), 'utf8')) as { name?: string; version?: string };
    if (pkg.name !== '@anthropic-ai/claude-agent-sdk' || pkg.version !== CLAUDE_SDK_VERSION) {
      throw new Error(`expected @anthropic-ai/claude-agent-sdk ${CLAUDE_SDK_VERSION}`);
    }
    return realpathSync(join(root, 'sdk.mjs'));
  } catch (e) {
    throw new Error(`Claude's separately installed Agent SDK is unavailable or incompatible. Install @anthropic-ai/claude-agent-sdk@${CLAUDE_SDK_VERSION} in your provider dependencies and set FORGE_CLAUDE_SDK_PATH to its package directory. ${e instanceof Error ? e.message : String(e)}`);
  }
}

export async function loadClaudeSdk(env: NodeJS.ProcessEnv = process.env): Promise<typeof import('@anthropic-ai/claude-agent-sdk')> {
  const sdk = await import(pathToFileURL(resolveClaudeSdk(env)).href) as typeof import('@anthropic-ai/claude-agent-sdk');
  if (typeof sdk.query !== 'function') throw new Error('the installed Claude Agent SDK does not export query');
  return sdk;
}
