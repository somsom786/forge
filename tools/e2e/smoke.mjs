// First look at the real app: initial screen, provider detection, project registration through the real IPC.
import { execFileSync } from 'node:child_process';
import { mkdtempSync, writeFileSync, mkdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { connect } from './cdp.mjs';

const shots = process.argv[2] ?? 'shots';
mkdirSync(shots, { recursive: true });
const ui = await connect();
console.log('page title:', await ui.eval('document.title'), '| tauri runtime:', await ui.eval('typeof window.__TAURI_INTERNALS__'));
await ui.waitFor('document.body.innerText.length > 20');
await ui.screenshot(join(shots, '01-initial.png'));
console.log('--- initial text ---\n' + (await ui.text()).slice(0, 600));

const info = await ui.invoke('app_info');
console.log('appInfo:', JSON.stringify(info));
const providers = await ui.invoke('providers_detect');
console.log('providers:', providers.map((p) => `${p.id}(installed=${p.installed}, adapter=${p.adapterAvailable}, models=${p.models.length})`).join(', '));

// A throwaway git repo registered through the same command the UI's folder picker uses.
const repo = mkdtempSync(join(tmpdir(), 'forge-e2e-repo-'));
const g = (...a) => execFileSync('git', a, { cwd: repo, stdio: 'pipe' });
g('init', '-b', 'main'); g('config', 'user.email', 'e@e'); g('config', 'user.name', 'e');
mkdirSync(join(repo, 'src'));
writeFileSync(join(repo, 'README.md'), '# E2E demo\n');
writeFileSync(join(repo, 'src', 'app.txt'), 'hello\n');
g('add', '-A'); g('commit', '-m', 'init');
const project = await ui.invoke('project_add', { path: repo });
console.log('project:', project.name, project.rootPath);
await ui.eval('location.reload()');
await ui.waitFor(`document.body.innerText.includes(${JSON.stringify(project.name)})`);
await ui.screenshot(join(shots, '02-with-project.png'));
console.log('console log:', ui.consoleLog.slice(-5));
console.log(JSON.stringify({ repo, projectId: project.id }));
ui.close();
