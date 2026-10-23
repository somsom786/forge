// Stage what the installer ships next to the app: the single-file engine bundle and a Node runtime to run it with.
// Run automatically by `npm run package` (see apps/desktop/src-tauri/tauri.bundle.conf.json).
import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync, rmSync, statSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');
const staged = join(repo, 'apps', 'desktop', 'src-tauri', 'resources-staged');
rmSync(staged, { recursive: true, force: true });
mkdirSync(join(staged, 'engine'), { recursive: true });
mkdirSync(join(staged, 'node'), { recursive: true });

execFileSync(process.execPath, [join(repo, 'packages', 'engine', 'scripts', 'bundle.mjs')], { stdio: 'inherit' });
copyFileSync(join(repo, 'packages', 'engine', 'bundle', 'main.js'), join(staged, 'engine', 'main.js'));

// The Node that is running this script is the one we ship (the engine bundle targets Node >= 20).
const exe = process.platform === 'win32' ? 'node.exe' : 'node';
copyFileSync(process.execPath, join(staged, 'node', exe));
console.log(`staged engine bundle and ${process.version} (${(statSync(join(staged, 'node', exe)).size / 1048576).toFixed(0)} MiB) in ${staged}`);
