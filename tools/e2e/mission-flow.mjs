// Real UI, real core, scripted agent: create a mission from the dialog, approve in the UI, define a contract, run a real
// check from the UI, approve, merge, read the Proof Pack. Screenshots land in the given directory.
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { connect } from './cdp.mjs';

const shots = process.argv[2] ?? 'shots';
mkdirSync(shots, { recursive: true });
const ui = await connect();
const shot = async (n) => { await ui.screenshot(join(shots, n)); console.log('  shot', n); };
const step = (s) => console.log(`\n== ${s}`);
const bodyHas = (re) => ui.waitFor(`/${re}/i.test(document.body.innerText)`, { timeout: 30000 });

const repo = mkdtempSync(join(tmpdir(), 'forge-e2e-flow-'));
const g = (...a) => execFileSync('git', a, { cwd: repo, stdio: 'pipe' }).toString();
g('init', '-b', 'main'); g('config', 'user.email', 'e@e'); g('config', 'user.name', 'e');
writeFileSync(join(repo, 'package.json'), JSON.stringify({ name: 'demo', scripts: { test: 'node -e "process.exit(0)"' } }));
writeFileSync(join(repo, 'README.md'), '# demo\n');
g('add', '-A'); g('commit', '-m', 'init');
const project = await ui.invoke('project_add', { path: repo });
await ui.eval('location.reload()');
await ui.waitFor("document.body.innerText.includes('Command Center') && !document.body.innerText.includes('Loading from the core')");

step('New mission dialog -> scripted agent -> Create & start');
await ui.clickButton('New mission');
await ui.waitFor("!!document.querySelector('textarea')");
await ui.selectOption('[role=dialog]', project.id);
await ui.clickRadio('Scripted test agent');
await ui.type('textarea', 'Add a short note file for the demo project. FORGE E2E run.');
await ui.clickButton('Create & start');
await bodyHas('Needs approval');
await shot('20-awaiting-approval.png');

step('Approve in the UI');
await ui.clickButton('Approve');
await bodyHas('Awaiting review');
const mission = (await ui.invoke('mission_list', {})).find((m) => m.projectId === project.id);
console.log('  mission state via core:', mission.state);

step('Changes tab shows the real diff');
await ui.clickTab('Changes');
await ui.waitFor("document.body.innerText.includes('FORGE_TEST.md')");
await shot('21-changes.png');

step('Contract: add a criterion + required check (through the real command), then run it from the UI');
const check = { id: 'exists', name: 'Note exists', command: `node -e "process.exit(require('fs').existsSync('FORGE_TEST.md')?0:1)"`, kind: 'test', timeoutSecs: 60, required: true };
await ui.invoke('contract_set', { missionId: mission.id, contract: { version: 1, inScope: [], outOfScope: [], securityConstraints: [], reviewExpectations: null,
  checks: [check], criteria: [{ id: 'note', text: 'The note file exists', verifiedBy: ['exists'], manual: false, scopeRule: false }, { id: 'scope', text: 'No unrelated modifications', verifiedBy: [], manual: false, scopeRule: true }] } });
await ui.clickTab('Verification');
await bodyHas('Not established');
await shot('22-verification-before-check.png');
console.log('  before:', (await ui.text()).replace(/\s+/g, ' ').match(/Not established.{0,300}/)?.[0]);
await ui.clickButton('Run');
await ui.waitFor("!!document.querySelector('[role=dialog]')");
await shot('22b-run-check-dialog.png');
console.log('  confirm dialog text:', (await ui.eval("document.querySelector('[role=dialog]').innerText")).replace(/\s+/g, ' ').slice(0, 300));
await ui.clickButton('Run check', '[role=dialog]');
await ui.waitFor("document.body.innerText.includes('Everything in the contract is established')", { timeout: 60000 });
await shot('23-verification-after-check.png');
console.log('  native dialogs seen:', JSON.stringify(ui.dialogs));

step('Approve (nothing unresolved now)');
await ui.clickButton('Approve');
await ui.waitFor("!!document.querySelector('[role=dialog]')");
await shot('24-approve-dialog.png');
await ui.clickButton('Approve', '[role=dialog]');
await ui.waitFor(`window.__TAURI_INTERNALS__.invoke('mission_get', { missionId: ${JSON.stringify(mission.id)} }).then(d => d.mission.state === 'approved')`);

step('Merge');
await ui.clickButton('Merge');
await ui.waitFor(`window.__TAURI_INTERNALS__.invoke('mission_get', { missionId: ${JSON.stringify(mission.id)} }).then(d => d.mission.state === 'merged')`, { timeout: 60000 });
await shot('25-merged.png');
console.log('  file in the user checkout after merge:', existsSync(join(repo, 'FORGE_TEST.md')));

step('Proof Pack tab');
await ui.clickTab('Proof Pack');
// The pack sealed by the merge must appear without any manual refresh (the tab label itself says "Proof Pack").
await ui.waitFor("/sha256 [0-9a-f]{10}/.test(document.body.innerText) && !document.body.innerText.includes('No Proof Pack has been generated')", { timeout: 15000 });
await shot('26-proof-pack.png');
console.log('  console errors:', ui.consoleLog.filter((l) => /^(error|EXCEPTION)/.test(l)));
ui.close();
