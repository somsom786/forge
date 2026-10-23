// Real UI, real core, scripted agent: create a mission from the dialog, watch it stream, answer the approval in the
// UI, review the result, set a contract/run a check, approve and merge. Screenshots land in the given directory.
import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { connect } from './cdp.mjs';

const shots = process.argv[2] ?? 'shots';
mkdirSync(shots, { recursive: true });
const ui = await connect();
const shot = async (n) => { await ui.screenshot(join(shots, n)); console.log('  shot', n); };
const step = (s) => console.log(`\n== ${s}`);

const repo = mkdtempSync(join(tmpdir(), 'forge-e2e-flow-'));
const g = (...a) => execFileSync('git', a, { cwd: repo, stdio: 'pipe' }).toString();
g('init', '-b', 'main'); g('config', 'user.email', 'e@e'); g('config', 'user.name', 'e');
writeFileSync(join(repo, 'package.json'), JSON.stringify({ name: 'demo', scripts: { test: 'node -e "process.exit(0)"' } }));
writeFileSync(join(repo, 'README.md'), '# demo\n');
g('add', '-A'); g('commit', '-m', 'init');
await ui.invoke('project_add', { path: repo });
await ui.eval('location.reload()');
await ui.waitFor("document.body.innerText.includes('Command Center') && !document.body.innerText.includes('Loading from the core')");
await shot('10-command-center.png');

step('open New mission dialog');
await ui.clickText('New mission', 'button');
await ui.waitFor("!!document.querySelector('textarea')");
await shot('11-new-mission-dialog.png');
console.log('  dialog text:', (await ui.text()).replace(/\s+/g, ' ').slice(0, 900));

step('fill and submit');
await ui.clickRadio('Scripted test agent');
await ui.type('textarea', 'Add a short note file for the demo project. FORGE E2E run.');
await shot('12-dialog-filled.png');
await ui.clickText('Create & start', 'button');

step('mission workspace: running / awaiting approval');
await ui.waitFor("document.body.innerText.includes('Approve')", { timeout: 30000 });
await shot('13-awaiting-approval.png');
console.log('  workspace text:', (await ui.text()).replace(/\s+/g, ' ').slice(0, 1200));

step('approve in the UI');
await ui.clickText('Approve', 'button');
await ui.waitFor("/awaiting review|Awaiting review/i.test(document.body.innerText)", { timeout: 30000 });
await shot('14-awaiting-review.png');

step('changes tab');
await ui.clickText('Changes', '[role=tab], button');
await ui.waitFor("document.body.innerText.includes('FORGE_TEST.md')", { timeout: 15000 });
await shot('15-changes-diff.png');
ui.close();
