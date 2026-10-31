# Windows validation handoff (continuation)

> Continuation update: `codex/overhaul-continuation` starts at `8c3bcc9`; validate its actual HEAD. Results on the available Windows host and CI are in `VALIDATION_MATRIX.md`. A real WebView2 scripted mission completed with disposable data, but an independent local installer longevity check exited cleanly at 63.5 seconds; later 180-second Windows CI checks passed. Investigate the local exit before release. Host physicality and real provider workflows have not been established. Agent instruction changes require fingerprint-bound acknowledgement. Windows agent execution remains trusted-local, not isolated. The installer does not bundle the Claude SDK; see `README.md` for its separately installed dependency.

The portable Linux component probe can be run in a disposable Linux environment with Node, a working bubblewrap binary, and the built engine: `node scripts/linux/probe-isolation.mjs /path/to/bwrap /path/to/bundle/main.js`. It uses temporary harmless fixtures and tests host controls, check containment, agent network behavior and cancellation. It does not validate the Rust supervisor or create a Windows isolation backend. Do not modify an existing personal WSL distribution to turn this into a security claim. Validate filesystem and credential access, children, Git metadata, network, environment, resource limits, cancellation, crashes, provider operation and recovery against any future Windows backend before enabling or describing it as isolated.

For the person validating this branch on Windows, helped by Codex. This continuation has Windows-host and GitHub runner
results; whether the host was a physical PC was not independently established. Status labels and evidence:
`docs/VALIDATION_MATRIX.md`. Open risks: `docs/REMAINING_RISKS.md`.

## A. Where things are

| Item | Value |
|---|---|
| Branch | `codex/overhaul-continuation` |
| Code to validate | Record the actual `git rev-parse HEAD` and compare it with the PR head. Earlier validation at `4c7d5c1` does not establish the result for later changes. |
| PR | draft continuation PR into `cloud/overhaul`: https://github.com/somsom786/forge/pull/9 |
| Base | continuation starts at overhaul revision `8c3bcc9` |
| Contains | Existing overhaul plus the continuation commits; inspect Git history and current PR state before merging anything. |
| Migrations | Forward-only; inspect `db.rs` for the current schema. A database written by a newer FORGE is refused. |
| New dependencies | npm: `@agentclientprotocol/sdk` 1.8.0 (Apache-2.0); dev-only `@playwright/test` 1.56.1 and `@axe-core/playwright` 4.13.0. Rust: tree-sitter 0.27.1 and grammars (MIT), `unicode-normalization` 0.1.26 (MIT/Apache-2.0 + Unicode-3.0). Never bundled: bubblewrap, gitleaks, osv-scanner, trivy, playwright (user-chosen checks). See `docs/OSS_ADOPTION.md` |
| Original installer contents | Historical only: `forge-desktop.exe`, `engine\main.js` (then included the Claude SDK), `node\node.exe`, `THIRD_PARTY_NOTICES.txt`. See the continuation update above for current packaging. |

## B. What changed since the last physical validation

1. **Approvals** are bound to the exact request, its policy revision (`policy-2026-10-24.1`) and where it executes. They expire after 30 minutes, are re-checked at decision time, and are reused at most once.
2. **Isolation:**
   - Checks and experiments can run in `linux_namespaces` on Linux.
   - On Linux, **agents** can be contained too ("Agents run: …").
   - On Windows only `trusted_local` exists, and isolation is refused rather than faked.
3. **MCP:** hash-pinned authorization per project; allow-lists are enforced by the core.
4. **ACP agents:** official SDK; tested against the real `claude-agent-acp`.
5. **Real Claude Code in tests:** the bundled binary talks to a scripted local model. No account is used.
6. **Proof Engine:**
   - Real scanner runs (gitleaks, OSV-Scanner) and real report formats (Trivy, Playwright).
   - The Proof Pack summary strip and "Agent ran: …".
7. **Memory:**
   - Obsidian links resolve like Obsidian: file names, NFD names, case-insensitive. Slug collisions no longer cause wrong-page links.
   - Read-only notes are never rewritten.
   - Sync-conflict copies are flagged.
8. **Code intelligence:** CommonJS symbols are extracted; evaluated on nine real repositories.
9. **Harness Doctor:** explains which settings file wins and that deny/ask beat allow.
10. **UI accessibility:** contrast, keyboard scrolling, palette labels and headings. Storage view and a previewed clean-up.
11. **Data integrity:**
    - Merges are journaled.
    - Orphaned worktrees are adopted.
    - One FORGE per data folder.
    - Every pending approval expires at restart.
    - **A worktree with a file in use is no longer half-deleted on Windows** (found by Windows CI this round).
12. **Release:** third-party notices are generated and shipped; SBOMs come from the security workflow.

## C. Exact commands (PowerShell, from a fresh clone)

```powershell
# 1. Get the branch and record what you validate
git clone https://github.com/somsom786/forge.git forge-validate
cd forge-validate
git checkout codex/overhaul-continuation
git rev-parse HEAD
git status --short --branch

# 2. Allow the scripts in this window only
Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass

# 3. Automated suites, then the focused scripts
.\scripts\windows\verify-windows.ps1
.\scripts\windows\test-provider-adapters.ps1
.\scripts\windows\test-execution-backends.ps1          # elevated window also checks Windows Sandbox availability
.\scripts\windows\test-obsidian.ps1 -Interactive
.\scripts\windows\test-release.ps1 -Launch             # builds the installer, installs to %TEMP%, launches, uninstalls

# 4. The development app for the manual checks
npm run dev
```

Each script prints a table and writes `validation-results\<script>-<timestamp>.json`. Exit codes: 0 means all required
checks passed, 1 means a FAIL, 2 means a required check is BLOCKED. Scripts never run `#[ignore]` live tests or start a
provider mission.

## D. Validation checklist

Severity: **P0** blocks merging, **P1** must be fixed or documented before a release, **P2** nice to know.
Use disposable repositories, vaults and harmless fixture files for every destructive, junction and scanner scenario. Do not log out the user's providers or use personal files. Live-provider tests require intentional, narrowly scoped authorization; scripted fixtures are the default.

| # | Objective | Action | Expected | Failure signs | Where to fix | Sev |
|---|---|---|---|---|---|---|
| D1 | Whole automated suite on the PC (Node 22, Rust MSVC, VS Build Tools, WebView2, Git) | `verify-windows.ps1` | every row PASS — Physical Windows | any FAIL; open its `.log` | where the test lives | P0 |
| D2 | Approval in the real window using an available provider or scripted fixture | `npm run dev`, mission "create NOTE.md with one line" | card shows the expiry; Approve writes the file; the Proof Pack header says "Agent ran: on the host, as the user (not isolated)" | approval does nothing; card refused as "not the one you were shown" | `supervisor.rs`, `ApprovalCard.tsx` | P0 |
| D3 | Junction swapped in at decision time | automated: `windows::a_junction_swapped_in…` (PASS — Windows CI); repeat by hand during D2 with `mklink /J` | refused "at decision time"; nothing written outside | file appears outside the worktree | `paths.rs`, `policy.rs` | P0 |
| D4 | MCP authorization end to end | Harness Studio → `.mcp.json` → authorize with one allow-listed tool → Claude mission using it and another tool | the allowed tool asks; the other is denied without asking; editing `.mcp.json` shows "changed since authorized" | server runs unauthorized | `mcp.rs`, `supervisor/mcp.rs` | P0 |
| D5 | Discard and clean-up with a file open in an editor | open a worktree file in an editor that locks it (or `type con > file` in another window), then Discard | "files … are in use by another program … nothing was deleted"; after closing it, Discard works | half-deleted worktree; "not a git repository" on retry | `supervisor.rs` `remove_mission_worktree` | P0 |
| D6 | Upgrade over the installed build from `main` | install the old installer, make a project and a mission with an approval pending, install this one over it | data intact; the pending approval expired; the mission is "interrupted"; Settings → Storage works | crash on start; "newer version of FORGE" (it must not appear here) | `db.rs`, `supervisor/tests/upgrade.rs` | P1 |
| D7 | ACP agent end to end | register `claude-agent-acp` (real program: `node.exe` + script; `.cmd` shims are refused), run a mission | protocol 1; edits ask; writes land in the worktree | agent asks to sign in (do it in the agent) | `adapters/acp.ts` | P1 |
| D8 | Codex sign-in detection and a tiny Codex mission | Inspect `codex login status`; run one narrowly scoped mission only when intentionally authorized | detection matches reality; approvals as with Claude | logged-out shown as signed in | `providers.rs`, `adapters/codex.ts` | P1 |
| D9 | Real scanners (winget/scoop gitleaks, osv-scanner) | add a gitleaks check, plant `ghp_` + 36 letters, run | Failed with file:line, no secret text, tool version shown; a clean tree passes | "report unreadable" with a working tool | `evidence.rs` | P1 |
| D10 | Installer | `test-release.ps1 -Launch` | starts; `engine\main.js`, `node\node.exe`, `THIRD_PARTY_NOTICES.txt` present; uninstall removes it | missing files | `tauri.bundle.conf.json`, `tools/package/*` | P1 |
| D11 | Agent isolation offer on Windows | Projects → "Agents run: …" | only Trusted local selectable; others say why | an isolated option selectable | `ExecutionBackendPanel.tsx` | P1 |
| D12 | Restart with a pending approval | close FORGE with a card pending, reopen | mission interrupted, approval expired | card still answerable | `supervisor.rs` `recover` | P1 |
| D13 | Accessibility and layout in WebView2 at 1100×700 | Tab through each screen; Ctrl+K "Go to Knowledge"; Settings → Storage preview | the same results as the simulated e2e tests; no clipped text | low contrast, unreachable scroll areas | `apps/desktop/src` | P2 |
| D14 | Obsidian | `test-obsidian.ps1 -Interactive`; also a vault with a note named with accents and one marked read-only | links resolve; read-only note never changed | wrong link targets; a read-only note rewritten | `knowledge/*` | P2 |
| D15 | Harness Doctor with your real `~/.claude/settings.json` | Harness Studio → Doctor | precedence findings name the files; none points at the user file | crash or a misleading winner | `harness/doctor.rs` | P2 |

## E. Codex work queue

**P0**

1. D1: fix any failure. If a test fails only on Windows, fix the code, unless the test's assumption is Unix-only; in that case make it platform-correct and say why.
2. D2, D4, D5 in the real window.

**P1**

3. D6–D12.
4. Record the benchmark numbers on the PC: `FORGE_BENCH=1 cargo test --release --lib bench_ -- --nocapture --test-threads=1`. Put them next to the cloud numbers in `docs/PERFORMANCE.md`, labelled Physical Windows.

**P2**

5. D13–D15.
6. Real screenshots of the main screens for the README, labelled as such.

Windows Sandbox / WSL execution and code signing remain release work. Do not enable or describe a Windows backend as isolated until its actual enforcement is validated. The owner must choose FORGE's licence (`docs/LICENSING_DECISION.md`).

## F. Known limitations

- **Security:**
  - On Windows agents run on the host, and approved actions run with your privileges.
  - The webview is trusted by the core.
  - Shell rules are heuristics.
  - Authorized MCP stdio servers run as you.
  - Instruction-file review uses a path heuristic; unsupported provider conventions may not be flagged.
- **Providers:** no live mission on this branch. Claude Code was tested with a scripted model, ACP with a real agent and a scripted model, and Codex with a scripted app-server only.
- **Proof Engine:** Trivy and Playwright reports are parsed from real output, but the tools are not run in CI.
- **Release:**
  - The installer is unsigned and its upgrade path is untested.
  - The original installer bundled the Claude Agent SDK; continuation packaging excludes it. Vendor terms and use of a separately installed SDK still require review.
  - FORGE has no licence.
- **Linux note:** Ubuntu 24.04 restricts user namespaces for unconfined programs. CI lifts that restriction on its runner; users need an AppArmor profile for `bwrap`, or the backend reports itself unavailable.

## G. Prompt to paste into Codex

```text
You are validating the FORGE branch `codex/overhaul-continuation` on this Windows 11 PC. Read docs/CODEX_WINDOWS_HANDOFF.md,
docs/VALIDATION_MATRIX.md, docs/REMAINING_RISKS.md, docs/SECURITY.md and scripts/windows/README.md first.

Rules:
- Work on a new branch `codex/windows-validation` created from the current `codex/overhaul-continuation`. Never push to main, never force-push,
  never merge PRs.
- Never read or print credentials (~/.codex/auth.json, Claude tokens, env tokens). Never use API-key billing.
- Do not run Rust tests marked #[ignore] and do not start live provider missions yourself; when a checklist item needs a
  live mission (D2, D4, D7, D8), stop and ask me to perform it, then record what I report.
- Never mark something verified unless you ran it on this PC. Use exactly the labels in VALIDATION_MATRIX.md.

Do, in order:
1. Section C. Put each script's summary table into a new file docs/WINDOWS_VALIDATION_REPORT.md with the commit SHA.
2. For each FAIL: diagnose from the .log under validation-results\, fix the root cause with the smallest change, re-run,
   and record before/after in the report.
3. Work the queue in section E, P0 first.
4. Update docs/VALIDATION_MATRIX.md (Physical Windows column) only for what you ran here.
5. Commit with clear messages, push `codex/windows-validation`, and open a PR into `codex/overhaul-continuation` saying exactly what
   was run, what passed, what failed and what is still not run.
```

## H. Independent review topics

These questions can guide a future independent security review. Do not automatically launch additional paid model sessions.

1. `apps/desktop/src-tauri/src/paths.rs` (`resolve_within`), `policy.rs` (`evaluate`) and `supervisor.rs`
   (`decide_approval_checked`): is there any Windows spelling or reparse point that passes `resolve_within` at decision
   time but writes outside the worktree? Consider 8.3 names, `\\?\`, ADS, mount points, hard links, case and Unicode
   normalisation.
2. `execution.rs` (`agent_command`) and `supervisor.rs` (`agent_read_only_paths`, `narrow_enough`, `agent_state_paths`):
   can a contained agent reach a host file it should not through the bind list? Consider `node_modules` ancestors, symlinks
   inside bound directories, and `/proc`.
3. `packages/engine/src/adapters/acp.ts`: can an ACP agent obtain a write outside the worktree, or standing permission,
   through option kinds, multiple locations, relative paths, or `fs/write_text_file` races?
4. `supervisor/storage.rs`: can the clean-up delete anything the preview did not list? Consider runs created between
   preview and apply, and clock changes.
5. `knowledge/retrieve.rs` and `knowledge/store.rs`: can a vault note become injected into an agent prompt without a
   person verifying exactly that content?
