// Third-party notices for what the installer ships: the Rust crates compiled into forge-desktop (normal dependencies for
// the target), the npm packages that are not development-only (the UI bundle and the single-file engine bundle draw from
// these), and the Node.js runtime copied next to the engine. Each entry carries its declared licence and the licence and
// notice files found in the package. Licences outside the allow-list are listed first under "Needs a decision".
//
// Usage: node tools/package/notices.mjs [--target x86_64-pc-windows-msvc] [--out THIRD_PARTY_NOTICES.txt] [--summary summary.json]
// It reads cargo metadata and package-lock.json; it downloads nothing (cargo uses the local registry cache).
import { execFileSync } from 'node:child_process';
import { existsSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const repo = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');
const arg = (name, dflt) => { const i = process.argv.indexOf(name); return i > 0 ? process.argv[i + 1] : dflt; };
const target = arg('--target', 'x86_64-pc-windows-msvc');
const out = arg('--out', join(repo, 'THIRD_PARTY_NOTICES.txt'));
const summaryPath = arg('--summary', null);
// Licences whose obligations are met by shipping this file (attribution and licence text).
const ALLOWED = ['MIT', 'Apache-2.0', 'BSD-2-Clause', 'BSD-3-Clause', 'ISC', '0BSD', 'Zlib', 'Unicode-3.0', 'Unicode-DFS-2016', 'Unlicense', 'CC0-1.0', 'BSL-1.0', 'MPL-2.0', 'Apache-2.0 WITH LLVM-exception', 'BlueOak-1.0.0', 'CC-BY-4.0', 'Python-2.0'];
const FILES = /^(licen[cs]e|copying|notice|copyright)([-._].*)?$/i;

function licenceFiles(dir) {
  if (!dir || !existsSync(dir)) return [];
  return readdirSync(dir).filter((f) => FILES.test(f)).sort().map((f) => {
    let text = readFileSync(join(dir, f), 'utf8');
    if (text.length > 40_000) text = `${text.slice(0, 40_000)}\n[… truncated; full text in the package]`;
    return { file: f, text };
  });
}

/** True when every alternative (OR) is allowed or every conjunct (AND) is: an SPDX expression, read simply. */
function allowed(expr) {
  if (!expr) return false;
  const e = expr.replace(/[()]/g, ' ').replace(/\s+/g, ' ').trim();
  if (e.includes(' OR ')) return e.split(' OR ').some((x) => allowed(x.trim()));
  if (e.includes(' AND ')) return e.split(' AND ').every((x) => allowed(x.trim()));
  if (e.includes('/')) return e.split('/').some((x) => allowed(x.trim()));
  return ALLOWED.includes(e);
}

// ---- Rust: the dependency graph forge-desktop is compiled with for the target (normal edges only) ----------------
const meta = JSON.parse(execFileSync('cargo', ['metadata', '--format-version', '1', '--filter-platform', target, '--manifest-path', join(repo, 'apps/desktop/src-tauri/Cargo.toml')], { maxBuffer: 1 << 28 }).toString());
const byId = new Map(meta.packages.map((p) => [p.id, p]));
const nodes = new Map(meta.resolve.nodes.map((n) => [n.id, n]));
const rootId = meta.resolve.root;
const seen = new Set();
const stack = [rootId];
while (stack.length) {
  const id = stack.pop();
  if (seen.has(id)) continue;
  seen.add(id);
  for (const d of nodes.get(id)?.deps ?? []) {
    if (d.dep_kinds.some((k) => k.kind === null)) stack.push(d.pkg);
  }
}
seen.delete(rootId);
const crates = [...seen].map((id) => byId.get(id)).map((p) => ({
  ecosystem: 'crate', name: p.name, version: p.version, licence: p.license ?? (p.license_file ? `see ${p.license_file}` : null),
  source: p.repository ?? `https://crates.io/crates/${p.name}`, dir: dirname(p.manifest_path),
}));

// ---- npm: everything in the lockfile that is not development-only, for the target OS ----------------------------
const os = target.includes('windows') ? 'win32' : target.includes('apple') ? 'darwin' : 'linux';
const lock = JSON.parse(readFileSync(join(repo, 'package-lock.json'), 'utf8'));
const npm = Object.entries(lock.packages)
  .filter(([path, p]) => path.includes('node_modules/') && !p.dev && !p.link)
  .filter(([, p]) => !p.os || p.os.includes(os))
  .map(([path, p]) => {
    const dir = join(repo, path);
    let licence = p.license ?? null;
    if (!licence && existsSync(join(dir, 'package.json'))) licence = JSON.parse(readFileSync(join(dir, 'package.json'), 'utf8')).license ?? null;
    return { ecosystem: 'npm', name: path.slice(path.lastIndexOf('node_modules/') + 'node_modules/'.length), version: p.version, licence, source: p.resolved ?? '', dir };
  });

// ---- the Node.js runtime shipped next to the engine ------------------------------------------------------------
const nodeDir = dirname(process.execPath);
const runtime = [{ ecosystem: 'runtime', name: 'Node.js', version: process.version, licence: 'MIT (and the licences of its bundled components, listed in its LICENSE)', source: `https://github.com/nodejs/node/blob/${process.version}/LICENSE`, dir: [nodeDir, join(nodeDir, '..')].find((d) => existsSync(join(d, 'LICENSE'))) }];

const all = [...runtime, ...crates.sort((a, b) => a.name.localeCompare(b.name)), ...npm.sort((a, b) => a.name.localeCompare(b.name))];
const attention = all.filter((p) => p.ecosystem !== 'runtime' && !allowed(p.licence));
const missingText = all.filter((p) => licenceFiles(p.dir).length === 0);

let txt = `THIRD-PARTY NOTICES FOR FORGE\n\nGenerated by tools/package/notices.mjs for ${target} from Cargo metadata and package-lock.json.\n` +
  `${crates.length} Rust crates, ${npm.length} npm packages, the Node.js runtime.\n` +
  `The npm list is a superset: it is every non-development package in the lockfile, so it also names packages used only at\n` +
  `build time (for example Tailwind's lightningcss) and platform binaries the installer does not copy (for example the Claude\n` +
  `Agent SDK's native claude executables; FORGE uses the Claude Code the user installed). Listing them is harmless.\n\n`;
txt += `NEEDS A DECISION (licence outside the allow-list or not declared): ${attention.length}\n`;
for (const p of attention) txt += `  - ${p.ecosystem} ${p.name} ${p.version}: ${p.licence ?? 'no licence declared'}\n`;
txt += `\nNo licence file found in the package (the declared licence still applies; its standard text governs): ${missingText.length}\n\n`;
for (const p of all) {
  txt += `${'='.repeat(100)}\n${p.name} ${p.version} (${p.ecosystem})\nLicence: ${p.licence ?? 'not declared'}\nSource: ${p.source}\n`;
  for (const f of licenceFiles(p.dir)) txt += `\n--- ${f.file} ---\n${f.text.trim()}\n`;
  txt += '\n';
}
writeFileSync(out, txt);
const byLicence = {};
for (const p of all) byLicence[p.licence ?? 'none'] = (byLicence[p.licence ?? 'none'] ?? 0) + 1;
const summary = { target, crates: crates.length, npm: npm.length, needsDecision: attention.map((p) => `${p.ecosystem}:${p.name}@${p.version} (${p.licence})`), withoutLicenceFile: missingText.length, byLicence };
if (summaryPath) writeFileSync(summaryPath, JSON.stringify(summary, null, 2));
console.log(`notices -> ${out} (${(txt.length / 1048576).toFixed(1)} MiB); ${attention.length} need a decision; ${missingText.length} without a licence file`);
