# Windows validation handoff (cloud/overhaul)

For the person validating this branch on the physical Windows 11 PC, helped by Codex. Everything here was built and tested in a
Linux cloud container and on GitHub's Windows CI runner. **Nothing on this branch has run on the physical PC yet.**
Status labels and evidence: `docs/VALIDATION_MATRIX.md`.

## A. Where things are

| Item | Value |
|---|---|
| Branch | `cloud/overhaul` |
| Head SHA | `cd75d4c` at the time of writing; see the PR (the branch moves while CI fixes land; `git rev-parse HEAD` after checkout) |
| PR | draft PR `cloud/overhaul` → `main` (https://github.com/somsom786/forge/pull/8) |
| Base | `main` at `fdb16e7` |
| Contains | PRs #1–#7 (`cloud/codex-hardening`, `harness-hardening`, `experiments-backend`, `mission-conflicts`, `unix-containment`, `product-ui`, `execution-backends`) plus the overhaul commits. Merging this PR supersedes them; do not merge them separately afterwards |
| New dependencies | npm `@agentclientprotocol/sdk` 1.8.0 (Apache-2.0, exact pin); Rust: tree-sitter 0.27.1 + grammar crates for Rust, TypeScript, JavaScript, Python (all MIT; see OSS_ADOPTION.md). External programs, never bundled: bubblewrap (Linux), and optionally gitleaks / osv-scanner / trivy / playwright as user-chosen checks |
| Migrations | 9 approval binding, 10 MCP authorization, 11 ACP agents, 12 structured evidence (forward-only; tested from every earlier version) |

## B. What changed (summary)

1. **Approval binding**: every approval is bound to a digest of mission, session, provider, tool, exact input, worktree,
   profile, policy revision and execution location; it expires after 30 minutes; approving re-runs policy on the original
   request (a target swapped for a link meanwhile is denied).
2. **Execution backends**: checks and experiments run on a per-project backend. `trusted_local` (unchanged, "not isolated")
   and `linux_namespaces` (bubblewrap, Linux) are implemented; Windows Sandbox and isolated WSL are designed, not implemented.
   On Windows only `trusted_local` is available.
3. **MCP authorization**: `.mcp.json` alone does nothing; servers are reviewed, hash-pinned and authorized per project in
   Harness Studio; missions get only stored definitions; unauthorized `mcp__` calls are denied without asking.
4. **ACP agents**: register any Agent Client Protocol agent (Agents screen) and run missions with it; FORGE answers every
   permission request and performs the file reads/writes the agent asks for through policy and approvals.
5. **Proof Engine adapters**: a check can parse gitleaks / OSV-Scanner / Trivy / Playwright JSON into findings (Verification
   view, Proof Pack).
6. **Code intelligence**: tree-sitter symbol index, Code Map view, relevant-code section in context packages.
7. **Fixes**: Codex "Not logged in" was shown as signed in; Unix process containment; 8.3 short-path handling.
8. **Tooling**: `scripts/windows/*.ps1`, CI with SHA-pinned actions, Windows script job, Windows installer build artifact,
   security workflow (audits, licences, gitleaks, OSV, SBOM).

## C. Exact commands (PowerShell, from a fresh clone)

```powershell
# 1. Get the branch
git clone https://github.com/somsom786/forge.git forge-validate
cd forge-validate
git checkout cloud/overhaul
git rev-parse HEAD            # write this down for the report

# 2. Allow the scripts in this window only
Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass

# 3. Everything automated (prereqs, npm ci, engine/UI/Rust tests), then the focused scripts
.\scripts\windows\verify-windows.ps1
.\scripts\windows\test-provider-adapters.ps1
.\scripts\windows\test-execution-backends.ps1          # run from an elevated window to also check Windows Sandbox
.\scripts\windows\test-obsidian.ps1 -Interactive       # opens a temporary fixture vault in Obsidian, asks what you saw
.\scripts\windows\test-release.ps1 -Launch             # builds the NSIS installer, installs to %TEMP%, launches, uninstalls
#    (refuses if FORGE is already installed for this user; add -ReplaceExistingInstall only if that is acceptable)

# 4. Launch the app for manual checks (development build)
npm run dev
```

Each script prints a table and writes `validation-results\<script>-<timestamp>.json` (read it with
`Get-Content -Raw -Encoding UTF8 | ConvertFrom-Json`). Exit code 0 = all required checks passed, 1 = a FAIL, 2 = a required
check BLOCKED. Scripts never run `#[ignore]` live tests and never start a provider mission.

## D. Validation checklist

Severity: **P0** blocks merging, **P1** must be fixed or documented before a release, **P2** nice to know.

| # | Objective | Prerequisites | Command / action | Expected | Failure signs | Files | Fix location | Sev |
|---|---|---|---|---|---|---|---|---|
| D1 | Whole automated suite on the PC | Node 22, Rust MSVC, VS Build Tools, WebView2, Git | `verify-windows.ps1` | Overall PASS, every row `PASS — Physical Windows` | any FAIL row; open its `.log` | `validation-results\` | where the failing test lives | P0 |
| D2 | Approval binding in the real window | a project; Claude Code signed in (spends a little quota: use a tiny task) or the Codex CLI | `npm run dev`, start a mission "create NOTE.md with one line", wait for the approval card | card shows "expires in 29m"; Approve writes the file; leaving a card 30 min (or test with a short TTL build) expires it and the agent says denied | approval does nothing; error "not the one you were shown" on a fresh card | `supervisor.rs` (`decide_approval_checked`), `ApprovalCard.tsx` | same | P0 |
| D3 | Decision-time re-check with a **junction** (Windows TOCTOU) | D2 setup, admin not needed | while the card for `NOTE.md` is open: `cmd /c mklink /J <worktree>\linkdir C:\Users\Public` and change the request to a file under it (or ask the agent to write `linkdir\x.txt`), then Approve | approval refused with "Blocked by FORGE policy at decision time"; nothing written under `C:\Users\Public` | file appears outside the worktree | `policy.rs`, `paths.rs` | `paths::resolve_within` | P0 |
| D4 | MCP authorization end to end | a repo with `.mcp.json` declaring a harmless stdio server (e.g. `npx -y @modelcontextprotocol/server-everything@<pinned version>`) | Harness Studio → `.mcp.json` → Review & authorize (allow-list one tool) → run a Claude mission asking to use that tool and another tool | the allow-listed tool asks for approval; the other is denied without asking; editing `.mcp.json` afterwards shows "changed since authorized" and the old definition still runs | server starts without authorization; denied tool reaches a prompt | `mcp.rs`, `supervisor/mcp.rs`, `adapters/claude.ts` | same | P0 |
| D5 | ACP agent end to end | an ACP-capable agent installed (e.g. an ACP bridge for a CLI you already use); **register the real program** (`node.exe` + script path), `.cmd` shims are refused | Agents → Register agent; New mission with provider "ACP agent" | session.started shows the agent name and protocol 1; edits ask; writes land in the worktree | "script wrapper" error (expected for `.cmd`); agent asks to sign in (do it in the agent, not FORGE) | `adapters/acp.ts`, `acp.rs` | same | P1 |
| D6 | Codex sign-in detection | Codex CLI | `codex logout` → Agents screen shows "not signed in"; `codex login` → "signed in" | matches reality | logged-out shown as signed in | `providers.rs::codex_signed_in` | same | P1 |
| D7 | Proof Engine adapters with real tools | install gitleaks and/or osv-scanner (winget/scoop) | in a mission contract add a check with Report = gitleaks (suggested command), run it on a worktree containing a fake token like `ghp_` + 36 letters | Failed, findings list the file:line, no secret text shown; a clean tree passes; uninstalling the tool gives "tool not installed" | "report unreadable" with a working tool → the command line or output format differs; capture the stdout and add it as a fixture | `evidence.rs`, `fixtures/evidence/` | parser or `ADAPTERS` command | P1 |
| D8 | Execution backend chooser | — | Projects → project → "Checks run: …" | only Trusted local selectable; Windows Sandbox / WSL say "not implemented"; checks show "Not isolated" | an unimplemented backend selectable | `execution.rs`, `ExecutionBackendPanel.tsx` | same | P1 |
| D9 | Installer | — | `test-release.ps1 -Launch` | PASS; installed app starts, engine and node present, uninstall removes it | missing `engine\main.js` or `node\node.exe` | `tauri.bundle.conf.json`, `scripts/bundle.mjs` | same | P1 |
| D10 | Upgrade over the previous installed build | an installer from `main` (older) | install old, create a project+mission, install new over it, open the app | data intact; migrations 9–12 applied silently | crash on start, "database" errors | `db.rs` | migrations | P1 |
| D11 | Restart with a pending approval | D2 | close FORGE while a card is pending, reopen | mission "interrupted", approval expired, recovery toast | mission still "awaiting approval" forever | `supervisor.rs` recovery | same | P1 |
| D12 | Obsidian | Obsidian installed | `test-obsidian.ps1 -Interactive` | the fixture note opens | "vault not found" persists after opening the folder as a vault | `knowledge/vault.rs` | `obsidian_uri` | P2 |
| D13 | Code Map | a real repo | Projects → project → Code Map, search a known function | symbols with correct file:line | empty map for a TS/Rust repo | `codeintel/` | same | P2 |
| D14 | New screens render in the real WebView2 window | — | look at: Harness Studio MCP dialog, Agents → ACP agents, Verification findings, Code Map | as in the browser preview | layout broken, console errors | `apps/desktop/src/components/*` | same | P2 |

## E. Codex work queue

**P0**
1. Run D1; fix any FAIL. If a Rust test fails only on Windows, fix the code, not the test, unless the test's assumption is
   Unix-only (then make it platform-correct and say why in the commit).
2. D3 (junction TOCTOU) — add an automated `#[cfg(windows)]` test next to
   `supervisor::tests::approvals::policy_is_rechecked_at_decision_time…` using `mklink /J` via `std::process::Command`.
3. D2 and D4 in the real window.

**P1**
4. D5–D11. For D7, capture real tool stdout into `apps/desktop/src-tauri/fixtures/evidence/` (scrub anything secret) and
   add tests; correct the suggested commands in `evidence::ADAPTERS` if needed.
5. `README` / `docs/PROGRESS.md`: update levels for every item validated (A = verified on the PC by running it).

**P2**
6. D12–D14. Screenshots of the new screens into the PR (real, from the PC).
7. Windows Sandbox backend prototype only if the owner asks (design in EXECUTION.md).

## F. Known limitations

* **Security**: agents (Claude, Codex, ACP) run on the host; approved tool calls run with the user's privileges. ACP
  permission requests are agent-initiated (a tool run without asking is not seen). Authorized MCP stdio servers run on the
  host from mission start. Checks are isolated only on Linux (`linux_namespaces`).
* **Windows**: no isolated execution backend; `.cmd`/`.bat` agent wrappers refused (register `node.exe` + script); symlink
  tests may skip without the privilege.
* **Providers**: no live mission run on this branch; ACP and MCP exercised only with scripted counterparts.
* **Proof Engine**: scanner parsers tested on hand-written fixtures; Trivy needs network (fails on the sandbox backend).
* **Packaging**: unsigned installer; upgrade path untested; the Claude Agent SDK is proprietary and bundled (redistribution
  terms to check before any public release); FORGE has no licence (owner's decision).
* **Process**: CI's Ubuntu job lifts AppArmor's user-namespace restriction on the runner; users on Ubuntu 24.04 need an
  AppArmor profile for `bwrap` or the backend reports unavailable.

## G. Prompt to paste into Codex

```text
You are validating the FORGE branch `cloud/overhaul` on this Windows 11 PC. Read docs/CODEX_WINDOWS_HANDOFF.md,
docs/VALIDATION_MATRIX.md, docs/SECURITY.md and scripts/windows/README.md first.

Rules:
- Work on a new branch `codex/windows-validation` created from `cloud/overhaul`. Never push to main, never force-push,
  never merge PRs.
- Never read or print credentials (~/.codex/auth.json, Claude tokens, env tokens). Never use API-key billing.
- Do not run Rust tests marked #[ignore] and do not start live provider missions yourself; when a checklist item needs a
  live mission (D2, D4, D5), stop and ask me to perform it, then record what I report.
- Never mark something verified unless you ran it on this PC. Use exactly the labels in VALIDATION_MATRIX.md.

Do, in order:
1. Section C steps 1-3. Paste each script's summary table into a new file docs/WINDOWS_VALIDATION_REPORT.md with the commit SHA.
2. For each FAIL: diagnose from the .log under validation-results\, fix the root cause with the smallest change, re-run the
   failing script, and record the before/after in the report.
3. Work the queue in section E (P0 first). Add the Windows junction test for D3.
4. Update docs/VALIDATION_MATRIX.md (Physical Windows column) and docs/PROGRESS.md levels only for what you ran here.
5. Commit with clear messages, push `codex/windows-validation`, and open a PR into `cloud/overhaul` describing exactly what
   was run, what passed, what failed and what is still not run.
```

Optional independent review questions (ask Codex in read-only mode, one at a time, with the named files attached):
1. `supervisor.rs` `decide_approval_checked` + `policy.rs` + `paths.rs`: is there any Windows path spelling or reparse-point
   case that passes `resolve_within` at decision time but writes outside the worktree?
2. `packages/engine/src/adapters/acp.ts`: can an ACP agent obtain a write outside the worktree, or standing permission, via
   option kinds, multiple locations, relative paths or `fs/write_text_file` races?
3. `mcp.rs` + `supervisor/mcp.rs`: can a repository influence what MCP server actually runs after authorization?
4. `evidence.rs`: can a crafted tool output make a failing scan look like a pass?
