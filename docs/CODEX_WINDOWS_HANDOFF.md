# Windows validation handoff (cloud/overhaul)

For the person validating this branch on the physical Windows 11 PC, helped by Codex. Everything here was built and
tested in a Linux cloud container and on GitHub's `windows-2022` runner. **Nothing on this branch has run on the physical
PC yet.** Status labels and evidence: `docs/VALIDATION_MATRIX.md`. Open risks: `docs/REMAINING_RISKS.md`.

## A. Where things are

| Item | Value |
|---|---|
| Branch | `cloud/overhaul` |
| Code to validate | **`6626706`**. Later commits on the branch change documentation only: `git diff --stat 6626706 HEAD -- . ':!docs' ':!README.md'` must print nothing. If it prints something, validate HEAD and say so in the report |
| PR | draft PR `cloud/overhaul` → `main`: https://github.com/somsom786/forge/pull/8 |
| Base | `main` at `fdb16e7` (database schema v6) |
| Contains | PRs #1–#7 plus the overhaul commits. Merging PR 8 supersedes them; do not merge them separately |
| Migrations | 7–15, forward-only. 1–6 are pinned to what `main` ships. A database written by a newer FORGE is refused |
| New dependencies | npm: `@agentclientprotocol/sdk` 1.8.0 (Apache-2.0); dev-only `@playwright/test` 1.56.1 and `@axe-core/playwright` 4.13.0. Rust: tree-sitter 0.27.1 and grammars (MIT), `unicode-normalization` 0.1.26 (MIT/Apache-2.0 + Unicode-3.0). Never bundled: bubblewrap, gitleaks, osv-scanner, trivy, playwright (user-chosen checks). See `docs/OSS_ADOPTION.md` |
| Installer contents | `forge-desktop.exe`, `engine\main.js` (includes the Claude Agent SDK JavaScript), `node\node.exe`, `THIRD_PARTY_NOTICES.txt` (new) |

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
git checkout cloud/overhaul
git rev-parse HEAD
git diff --stat 6626706 HEAD -- . ':!docs' ':!README.md'   # expect no output

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

| # | Objective | Action | Expected | Failure signs | Where to fix | Sev |
|---|---|---|---|---|---|---|
| D1 | Whole automated suite on the PC (Node 22, Rust MSVC, VS Build Tools, WebView2, Git) | `verify-windows.ps1` | every row PASS — Physical Windows | any FAIL; open its `.log` | where the test lives | P0 |
| D2 | Approval in the real window (Claude Code signed in; a tiny task) | `npm run dev`, mission "create NOTE.md with one line" | card shows the expiry; Approve writes the file; the Proof Pack header says "Agent ran: on the host, as the user (not isolated)" | approval does nothing; card refused as "not the one you were shown" | `supervisor.rs`, `ApprovalCard.tsx` | P0 |
| D3 | Junction swapped in at decision time | automated: `windows::a_junction_swapped_in…` (PASS — Windows CI); repeat by hand during D2 with `mklink /J` | refused "at decision time"; nothing written outside | file appears outside the worktree | `paths.rs`, `policy.rs` | P0 |
| D4 | MCP authorization end to end | Harness Studio → `.mcp.json` → authorize with one allow-listed tool → Claude mission using it and another tool | the allowed tool asks; the other is denied without asking; editing `.mcp.json` shows "changed since authorized" | server runs unauthorized | `mcp.rs`, `supervisor/mcp.rs` | P0 |
| D5 | Discard and clean-up with a file open in an editor | open a worktree file in an editor that locks it (or `type con > file` in another window), then Discard | "files … are in use by another program … nothing was deleted"; after closing it, Discard works | half-deleted worktree; "not a git repository" on retry | `supervisor.rs` `remove_mission_worktree` | P0 |
| D6 | Upgrade over the installed build from `main` | install the old installer, make a project and a mission with an approval pending, install this one over it | data intact; the pending approval expired; the mission is "interrupted"; Settings → Storage works | crash on start; "newer version of FORGE" (it must not appear here) | `db.rs`, `supervisor/tests/upgrade.rs` | P1 |
| D7 | ACP agent end to end | register `claude-agent-acp` (real program: `node.exe` + script; `.cmd` shims are refused), run a mission | protocol 1; edits ask; writes land in the worktree | agent asks to sign in (do it in the agent) | `adapters/acp.ts` | P1 |
| D8 | Codex sign-in detection and a tiny Codex mission | `codex logout` / `codex login`; one small mission | detection matches reality; approvals as with Claude | logged-out shown as signed in | `providers.rs`, `adapters/codex.ts` | P1 |
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

Out of scope unless the owner asks: Windows Sandbox / WSL backends (design in `docs/EXECUTION.md`); code signing;
licence choice (`docs/LICENSING_DECISION.md`).

## F. Known limitations

- **Security:**
  - On Windows agents run on the host, and approved actions run with your privileges.
  - The webview is trusted by the core.
  - Shell rules are heuristics.
  - Authorized MCP stdio servers run as you.
  - Edits to `CLAUDE.md`, `.claude/commands` and similar files are not flagged for review.
- **Providers:** no live mission on this branch. Claude Code was tested with a scripted model, ACP with a real agent and a scripted model, and Codex with a scripted app-server only.
- **Proof Engine:** Trivy and Playwright reports are parsed from real output, but the tools are not run in CI.
- **Release:**
  - The installer is unsigned and its upgrade path is untested.
  - The bundled Claude Agent SDK is proprietary, with redistribution unconfirmed.
  - FORGE has no licence.
- **Linux note:** Ubuntu 24.04 restricts user namespaces for unconfined programs. CI lifts that restriction on its runner; users need an AppArmor profile for `bwrap`, or the backend reports itself unavailable.

## G. Prompt to paste into Codex

```text
You are validating the FORGE branch `cloud/overhaul` on this Windows 11 PC. Read docs/CODEX_WINDOWS_HANDOFF.md,
docs/VALIDATION_MATRIX.md, docs/REMAINING_RISKS.md, docs/SECURITY.md and scripts/windows/README.md first.

Rules:
- Work on a new branch `codex/windows-validation` created from `cloud/overhaul`. Never push to main, never force-push,
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
5. Commit with clear messages, push `codex/windows-validation`, and open a PR into `cloud/overhaul` saying exactly what
   was run, what passed, what failed and what is still not run.
```

## H. Review queue for GPT-6 Astra (independent analysis; read-only, one question at a time)

These questions benefit from a reviewer outside this cloud session. They are not blocking; answer them when convenient.

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
