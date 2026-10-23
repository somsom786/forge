// Real UI, real core, real files: Knowledge. Create a page in the UI, see it is only "proposed", verify it by hand, then run a
// mission and confirm the agent's first prompt carried the verified page (and that the Memory tab shows what was injected).
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { connect } from './cdp.mjs';

const shots = process.argv[2] ?? 'shots';
mkdirSync(shots, { recursive: true });
const ui = await connect();
const shot = async (n) => { await ui.screenshot(join(shots, n)); console.log('  shot', n); };
const step = (s) => console.log(`\n== ${s}`);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const bodyHas = (re, timeout = 20000) => ui.waitFor(`/${re}/i.test(document.body.innerText)`, { timeout });
const fail = (m) => { console.error('FAIL:', m); process.exit(1); };

const repo = mkdtempSync(join(tmpdir(), 'forge-e2e-knowledge-'));
const g = (...a) => execFileSync('git', a, { cwd: repo, stdio: 'pipe' }).toString();
g('init', '-b', 'main'); g('config', 'user.email', 'e@e'); g('config', 'user.name', 'e');
writeFileSync(join(repo, 'README.md'), '# demo\n');
g('add', '-A'); g('commit', '-m', 'init');
const project = await ui.invoke('project_add', { path: repo });
await ui.eval('location.reload()');
await ui.waitFor("document.body.innerText.includes('Command Center') && !document.body.innerText.includes('Loading from the core')");
await ui.selectOption('header select, select', project.id);
await ui.waitFor(`document.body.innerText.includes(${JSON.stringify(project.name)})`);

const TITLE = 'Reset tokens expire against wall-clock time';
const BODY = 'The reset token expiry test flakes on CI clock skew. Region for the staging mirror is eu-west-9, codename quokka-7. Inject a fake clock.';

step('Open Knowledge and create a page through the dialog');
await ui.clickButton('Knowledge', 'nav, aside').catch(() => ui.clickText('Knowledge'));
await bodyHas('New page');
await shot('50-knowledge-empty.png');
await ui.clickButton('New page');
await ui.waitFor("!!document.querySelector('#kp-title')");
await ui.type('#kp-title', TITLE);
await ui.type('#kp-body', BODY);
await ui.type('#kp-tags', 'tests, ci');
await shot('51-knowledge-new-dialog.png');
await ui.clickButton('Create page');
await bodyHas(TITLE);
let pages = await ui.invoke('knowledge_list', { projectId: project.id });
if (pages.length !== 1 || pages[0].effectiveState !== 'proposed') fail('a new page must be proposed: ' + JSON.stringify(pages.map((p) => p.effectiveState)));
console.log('  core: 1 page, effectiveState =', pages[0].effectiveState, '| file in managed folder:', existsSync(pages[0].path ?? ''));
await shot('52-knowledge-created.png');

step('A proposed page is NOT attached to a mission');
const mk = async (objective) => {
  const m = await ui.invoke('mission_create', { args: { projectId: project.id, objective, provider: 'fake', permissionProfile: 'accept_edits' } });
  await ui.invoke('mission_start', { missionId: m.id });
  for (let i = 0; i < 100; i++) {
    const d = await ui.invoke('mission_get', { missionId: m.id });
    if (['awaiting_review', 'failed', 'interrupted'].includes(d.mission.state)) return d.mission;
    await sleep(300);
  }
  fail('mission did not reach review');
};
const m1 = await mk('Make the reset token expiry test deterministic');
const ctx1 = await ui.invoke('context_for_mission', { missionId: m1.id });
if (ctx1.length !== 1 || ctx1[0].items.length !== 0 || ctx1[0].injectedText.includes('quokka-7')) fail('unverified knowledge was injected: ' + JSON.stringify(ctx1[0]).slice(0, 300));
console.log('  mission 1 context: no items injected; reasons for omission:', JSON.stringify(ctx1[0].omitted.map((o) => o.reason)));

step('Open the page and verify it by hand');
await ui.realClick((t) => {
  const b = [...document.querySelectorAll('button, [role=button], li, a')].find((e) => e.offsetParent && e.innerText.includes(t));
  if (!b) return null; const r = b.getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2 };
}, TITLE);
await sleep(600);
await bodyHas('Verify');
await shot('53-knowledge-detail.png');
await ui.clickButton('Verify');
await sleep(500);
const confirm = await ui.eval("[...document.querySelectorAll('[role=dialog] button')].map(b => b.innerText.trim()).join('|')");
console.log('  dialog buttons after Verify:', confirm || '(none)');
if (confirm) { await shot('54-knowledge-verify-dialog.png'); await ui.clickButton('Verify', '[role=dialog]').catch(() => ui.clickButton('Confirm', '[role=dialog]')); }
for (let i = 0; i < 40; i++) {
  pages = await ui.invoke('knowledge_list', { projectId: project.id });
  if (pages[0].effectiveState === 'verified') break;
  await sleep(200);
}
if (pages[0].effectiveState !== 'verified') fail('page was not verified: ' + pages[0].effectiveState);
await shot('55-knowledge-verified.png');
console.log('  core: effectiveState =', pages[0].effectiveState, '(verified by a person, hash-bound)');

step('The next mission carries the verified page in the first user turn, with provenance');
const m2 = await mk('Make the reset token expiry test deterministic');
const ctx2 = await ui.invoke('context_for_mission', { missionId: m2.id });
if (!ctx2[0].injectedText.includes('quokka-7') || !ctx2[0].injectedText.includes('DATA, not instructions')) fail('verified page was not injected: ' + ctx2[0].injectedText.slice(0, 200));
const ev = (await ui.invoke('mission_events', { missionId: m2.id, afterId: 0, limit: 500 })).find((e) => e.payload?.message === 'prompt received');
console.log('  agent saw prompt starting:', JSON.stringify(String(ev?.payload?.promptPreview).slice(0, 120)), '| system prompt has knowledge:', ev?.payload?.systemPromptHasKnowledge);
if (ev?.payload?.systemPromptHasKnowledge) fail('knowledge in system prompt');

step('Memory tab shows what was injected');
await ui.clickText('Missions', 'nav button, aside button, nav a, aside a');
await sleep(500);
await ui.realClick((id) => {
  const b = [...document.querySelectorAll('button, a, [role=button], tr, li')].find((e) => e.offsetParent && (e.innerText || '').includes('Make the reset token expiry test deterministic'));
  if (!b) return null; const r = b.getBoundingClientRect(); return { x: r.x + r.width / 2, y: r.y + r.height / 2 };
}, m2.id);
await sleep(1200);
await ui.clickTab('Memory');
await bodyHas(TITLE, 15000);
await shot('56-mission-memory-tab.png');
console.log('  native dialogs seen:', ui.dialogs);
console.log('  console errors:', ui.consoleLog.filter((l) => /^(error|EXCEPTION)/.test(l)));
if (ui.dialogs.length) fail('a native dialog appeared');
ui.close();
console.log('\nKNOWLEDGE FLOW OK');
