// Bundle FORGE's engine. The proprietary Claude Agent SDK is a separately installed runtime dependency.
import { build } from 'esbuild';
import { mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const out = join(root, 'bundle');
rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });
const result = await build({
  absWorkingDir: root,
  entryPoints: [join(root, 'src/main.ts')],
  outfile: join(out, 'main.js'),
  bundle: true,
  platform: 'node',
  format: 'esm',
  target: 'node20',
  sourcemap: false,
  logLevel: 'info',
  metafile: true,
  external: ['@anthropic-ai/claude-agent-sdk', '@anthropic-ai/claude-agent-sdk/*'],
  // Some bundled CommonJS dependencies call require(); provide it in ESM output.
  banner: { js: "import { createRequire as __cr } from 'node:module'; const require = __cr(import.meta.url);" },
});
if (Object.keys(result.metafile.inputs).some((p) => p.replaceAll('\\', '/').includes('node_modules/@anthropic-ai/'))) throw new Error('refusing to bundle Anthropic SDK code');
writeFileSync(join(out, 'inputs.json'), JSON.stringify(Object.keys(result.metafile.inputs).sort(), null, 2));
console.log('bundled ->', join(out, 'main.js'));
