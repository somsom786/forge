// Stage what the installer ships next to the app: the single-file engine bundle and a Node runtime to run it with.
// Run automatically by `npm run package` (see apps/desktop/src-tauri/tauri.bundle.conf.json).
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { copyFileSync, mkdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');
const staged = join(repo, 'apps', 'desktop', 'src-tauri', 'resources-staged');
rmSync(staged, { recursive: true, force: true });
mkdirSync(join(staged, 'engine'), { recursive: true });
mkdirSync(join(staged, 'node'), { recursive: true });

execFileSync(process.execPath, [join(repo, 'packages', 'engine', 'scripts', 'bundle.mjs')], { stdio: 'inherit' });
copyFileSync(join(repo, 'packages', 'engine', 'bundle', 'main.js'), join(staged, 'engine', 'main.js'));
copyFileSync(join(repo, 'packages', 'engine', 'bundle', 'inputs.json'), join(staged, 'engine', 'inputs.json'));
writeFileSync(join(staged, 'CLAUDE_SDK_SETUP.txt'), `Claude Code integration requires a separately installed @anthropic-ai/claude-agent-sdk@0.3.309.\nFORGE does not redistribute this SDK. The Providers screen reports the exact installation command for your platform.\nAlternatively set FORGE_CLAUDE_SDK_PATH to the absolute installed package directory and restart FORGE.\nThe SDK is loaded from there; no provider credentials are copied or installed by FORGE.\nCodex and ACP do not require this dependency. Vendor terms still apply to the separately installed software and service.\n`);

// The Node that is running this script is the one we ship (the engine bundle targets Node >= 20).
const exe = process.platform === 'win32' ? 'node.exe' : 'node';
copyFileSync(process.execPath, join(staged, 'node', exe));
const sha256 = (path) => createHash('sha256').update(readFileSync(path)).digest('hex');
const manifest = {
  version: 1,
  revision: execFileSync('git', ['-C', repo, 'rev-parse', 'HEAD']).toString().trim(),
  trackedChanges: execFileSync('git', ['-C', repo, 'status', '--porcelain', '--untracked-files=no']).length > 0,
  node: { version: process.version, platform: process.platform, arch: process.arch, sha256: sha256(join(staged, 'node', exe)) },
  engineSha256: sha256(join(staged, 'engine', 'main.js')),
  engineInputsSha256: sha256(join(staged, 'engine', 'inputs.json')),
  npmLockSha256: sha256(join(repo, 'package-lock.json')),
  cargoLockSha256: sha256(join(repo, 'apps', 'desktop', 'src-tauri', 'Cargo.lock')),
  externalClaudeSdk: '0.3.309',
};
writeFileSync(join(staged, 'BUILD_MANIFEST.json'), JSON.stringify(manifest, null, 2) + '\n');
// Licence notices for everything above and the app itself, shipped with the installer.
execFileSync(process.execPath, [join(repo, 'tools', 'package', 'notices.mjs'), '--out', join(staged, 'THIRD_PARTY_NOTICES.txt'), '--summary', join(staged, 'notices-summary.json')], { stdio: 'inherit' });
console.log(`staged engine bundle and ${process.version} (${(statSync(join(staged, 'node', exe)).size / 1048576).toFixed(0)} MiB) in ${staged}`);
