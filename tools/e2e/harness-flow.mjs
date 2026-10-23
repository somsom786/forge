// Real UI, real core, real files: Harness Studio. Edit CLAUDE.md through preview -> save -> history -> rollback, add a hook from
// a template with the risk acknowledgement, dry-run it, and confirm every step against the files on disk.
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { connect } from './cdp.mjs';

const shots = process.argv[2] ?? 'shots';
mkdirSync(shots, { recursive: true });
const ui = await connect();
const shot = async (n) => { await ui.screenshot(join(shots, n)); console.log('  shot', n); };
const step = (s) => console.log(`\n== ${s}`);
const bodyHas = (re, timeout = 20000) => ui.waitFor(`/${re}/i.test(document.body.innerText)`, { timeout });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const selectItem = async (name, scope) => {
  const ok = await ui.realClick((name, scope) => {
    // An inventory row's text starts with its name and then its scope chip, e.g. "CLAUDE.md project ...".
    const b = [...document.querySelectorAll('button')].find((e) => e.offsetParent && e.innerText.replace(/\s+/g, ' ').trim().toLowerCase().startsWith(`${name} ${scope}`.toLowerCase()));
    if (!b) return null;
    b.scrollIntoView({ block: 'center' });
    const r = b.getBoundingClientRect();
    return { x: r.x + r.width / 2, y: r.y + r.height / 2 };
  }, name, scope);
  if (!ok) throw new Error('no inventory item ' + name);
  await sleep(500);
};
const fail = (m) => { console.error('FAIL:', m); process.exit(1); };

const repo = mkdtempSync(join(tmpdir(), 'forge-e2e-harness-'));
const g = (...a) => execFileSync('git', a, { cwd: repo, stdio: 'pipe' }).toString();
g('init', '-b', 'main'); g('config', 'user.email', 'e@e'); g('config', 'user.name', 'e');
const original = '# Demo project\nAlways run the tests.\n';
writeFileSync(join(repo, 'CLAUDE.md'), original);
mkdirSync(join(repo, '.claude'), { recursive: true });
writeFileSync(join(repo, '.claude', 'settings.json'), JSON.stringify({ model: 'sonnet' }, null, 2) + '\n');
g('add', '-A'); g('commit', '-m', 'init');
const project = await ui.invoke('project_add', { path: repo });
await ui.eval('location.reload()');
await ui.waitFor("document.body.innerText.includes('Command Center') && !document.body.innerText.includes('Loading from the core')");
await ui.selectOption('header select, select', project.id);
await ui.waitFor(`document.body.innerText.includes(${JSON.stringify(project.name)})`);

step('Open Harness Studio');
await ui.clickText('Harness Studio', 'nav button, nav a, aside button, aside a, [role=button], button, a');
await bodyHas('Harness Studio');
await bodyHas('CLAUDE.md');
await shot('40-harness-inventory.png');

step('Select CLAUDE.md and edit it');
await selectItem('CLAUDE.md', 'project');
await ui.waitFor("!!document.querySelector('#he-text') || [...document.querySelectorAll('button')].some(b => /^edit$/i.test(b.innerText.trim()))");
if (!(await ui.eval("!!document.querySelector('#he-text')"))) await ui.clickButton('Edit');
await ui.waitFor("!!document.querySelector('#he-text')");
const edited = original + 'Run lint before every commit.\n';
await ui.type('#he-text', edited);
await ui.clickButton('Review changes');
await bodyHas('Run lint before every commit');
await shot('41-harness-preview.png');
if (readFileSync(join(repo, 'CLAUDE.md'), 'utf8') !== original) fail('a preview must not write');

step('Save');
await ui.clickButton('Save');
for (let i = 0; i < 100 && readFileSync(join(repo, 'CLAUDE.md'), 'utf8') !== edited; i++) await sleep(150);
if (readFileSync(join(repo, 'CLAUDE.md'), 'utf8') !== edited) fail('the edit was not written: ' + JSON.stringify(readFileSync(join(repo, 'CLAUDE.md'), 'utf8')) + ' vs ' + JSON.stringify(edited));
console.log('  CLAUDE.md on disk now has the new line');
const hist = await ui.invoke('harness_history', { projectId: project.id, rel: 'CLAUDE.md' });
if (hist.length !== 1 || hist[0].state !== 'applied') fail('history: ' + JSON.stringify(hist));
await shot('42-harness-saved.png');

step('Roll back from the UI');
await ui.clickTab('History');
await bodyHas('edit');
await ui.clickButton('Roll back');
await bodyHas('Run lint before every commit');
await shot('43-harness-rollback-dialog.png');
await ui.clickButton('Roll back', '[role=dialog]');
for (let i = 0; i < 50 && readFileSync(join(repo, 'CLAUDE.md'), 'utf8') !== original; i++) await sleep(150);
if (readFileSync(join(repo, 'CLAUDE.md'), 'utf8') !== original) fail('rollback did not restore the file');
console.log('  rollback restored the original bytes');

step('Add a hook from a template (acknowledging risks) and dry-run it');
await selectItem('.claude/settings.json', 'project');
await sleep(400);
await ui.clickTab('Hooks');
await ui.clickButton('Add hook');
await bodyHas('Template');
await shot('44-harness-add-hook.png');
const tplButtons = await ui.eval("[...document.querySelectorAll('[role=dialog] button, [role=dialog] [role=radio], [role=dialog] label')].map(e => e.innerText.trim()).filter(Boolean)");
console.log('  dialog controls:', JSON.stringify(tplButtons).slice(0, 400));
await ui.selectOption('[role=dialog]', 'Block edits to protected files');
await sleep(300);
await shot('44b-harness-template-chosen.png');
await ui.clickButton('Preview', '[role=dialog]');
await bodyHas('hooks');
await shot('45-harness-hook-preview.png');
const boxes = await ui.eval("[...document.querySelectorAll('input[type=checkbox]')].filter(c => c.offsetParent !== null).length");
if (boxes > 0) { await ui.call(() => { const c = [...document.querySelectorAll('input[type=checkbox]')].filter((x) => x.offsetParent !== null); c[c.length - 1].click(); }); }
await ui.clickButton('Save');
for (let i = 0; i < 50 && !readFileSync(join(repo, '.claude', 'settings.json'), 'utf8').includes('hooks'); i++) await sleep(150);
const settings = JSON.parse(readFileSync(join(repo, '.claude', 'settings.json'), 'utf8'));
if (!settings.hooks || settings.model !== 'sonnet') fail('settings after hook add: ' + JSON.stringify(settings));
console.log('  settings.json now has hooks:', Object.keys(settings.hooks).join(','), '| existing keys kept:', settings.model);
await shot('46-harness-hook-saved.png');

await ui.clickButton('Dry run');
await bodyHas('executes|runs on this machine|privileges');
await shot('47-harness-dryrun-confirm.png');
await ui.clickButton('Run on this machine', '[role=dialog]');
await bodyHas('exit code|Exit code|Succeeded|BLOCKS', 30000);
await shot('48-harness-dryrun-result.png');
console.log('  native dialogs seen:', ui.dialogs);
console.log('  console errors:', ui.consoleLog.filter((l) => /^(error|EXCEPTION)/.test(l)));
if (ui.dialogs.length) fail('a native dialog appeared');
ui.close();
console.log('\nHARNESS FLOW OK');
