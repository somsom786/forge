# FORGE Windows validation scripts

These PowerShell scripts check FORGE on a Windows 11 PC. Each one records every check with a status, writes a log per
command and a JSON report, and exits non-zero when something fails. They do the same thing every time and keep no state
between runs. Run them from a clone of the repository.

**What they never do**

* They never run a live provider test or mission. Rust tests marked `#[ignore]` (for example `live_claude_mission_end_to_end`
  and `live_codex_*`) are never selected: no script passes `--ignored` or `--include-ignored`, and `Invoke-CargoTestCheck`
  refuses any argument that contains `ignored`. No provider quota is spent.
* They never read `~/.codex/auth.json`, `~/.claude*` credential files or token environment variables, and they never print a
  token. Sign-in state is asked from the CLIs themselves (see `test-provider-adapters.ps1`), and only a yes/no and the login
  method are reported.
* They download nothing, sign nothing, publish nothing and upload nothing. They never call `Invoke-Expression` and never change
  the execution policy.
* They never touch your real FORGE data or a real Obsidian vault. Fixtures, test installs and the FORGE home of a launched test
  app all live under `%TEMP%` and are deleted at the end.

## Prerequisites

| Tool | Why | Checked by |
|---|---|---|
| Windows 10/11 x64 | the product platform | every script (they refuse to run elsewhere) |
| Windows PowerShell 5.1 (built in) or PowerShell 7 | runs the scripts | `#Requires -Version 5.1` |
| Git for Windows (git + Git Bash) | repository, test repositories; the Rust hook tests run hooks in Git Bash like Claude Code does | `verify-windows.ps1` |
| Node.js 22 (`node`, `npm`) | engine, UI, packaging (`package.json` requires node >= 22; CI uses 22) | `verify-windows.ps1` |
| Rust stable with the MSVC host toolchain (`x86_64-pc-windows-msvc`) | Rust core and the app | `verify-windows.ps1` |
| Visual Studio 2022 or Build Tools 2022 with "Desktop development with C++" | MSVC linker for Rust | `verify-windows.ps1` (vswhere) |
| Microsoft Edge WebView2 Runtime (Evergreen) | the app window | `verify-windows.ps1` (registry) |
| Optional: Claude Code CLI, Codex CLI (`codex.exe`), Obsidian, WSL | reported, not required | the matching script |

## How to run

Open PowerShell in the repository root. If scripts are blocked by the execution policy, allow them for this window only:

```powershell
Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass     # affects this PowerShell window only
```

or start each script with `powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\windows\<script>.ps1 ...` (also limited to that
process). The scripts never change the policy themselves.

Suggested order:

```powershell
.\scripts\windows\verify-windows.ps1                    # prerequisites, npm ci, engine, UI, Rust core (about 15-30 min the first time)
.\scripts\windows\test-provider-adapters.ps1            # Claude Code / Codex CLIs, adapter tests (no live mission)
.\scripts\windows\test-execution-backends.ps1           # run it from an elevated PowerShell to also see the Windows Sandbox state
.\scripts\windows\test-obsidian.ps1 -Interactive        # asks you what Obsidian showed
.\scripts\windows\test-release.ps1 -Launch              # builds the installer, installs to %TEMP%, launches, uninstalls
```

or everything from one command (each other script runs as a child process with its own report):

```powershell
.\scripts\windows\verify-windows.ps1 -All -Interactive -Launch
```

Useful switches: `verify-windows.ps1 -SkipInstall -SkipEngine -SkipUi -SkipRust` (prerequisites only), `-IncludeRelease`
(only the release test on top of the main checks); `test-release.ps1 -InstallerPath <setup.exe>` (test an installer you already
built), `-LaunchSeconds 60`; `test-obsidian.ps1 -KeepFixture`; every script has `Get-Help .\scripts\windows\<script>.ps1 -Full`.

## Status labels and exit codes

Every check gets exactly one of these labels (on screen and in the JSON `status` field):

| Label | Meaning |
|---|---|
| `PASS — Physical Windows` | The check passed on a machine that is **not** GitHub Actions (`$env:GITHUB_ACTIONS` is not `true`). If the machine reports a virtual model the script prints a warning and the JSON has `environment.hostLooksVirtual = true`: say so when you report the result. |
| `PASS — Windows CI` | The check passed inside GitHub Actions. This is evidence on a clean Windows VM, not on a physical PC. |
| `FAIL` | The check ran and failed. Its log (path in the table and JSON) has the full output. |
| `BLOCKED` | A prerequisite is missing, so the check could not run or could not decide (the detail says what is missing). |
| `NOT RUN` | Skipped on purpose by a parameter (or, for the Obsidian question, skipped by the person). |

Checks marked *informational* report a fact about the machine (a CLI that is not installed, WSL state, sign-in state): their
`BLOCKED` does not change the exit code. A cargo test filter that matches no test is a `FAIL` (a green run that tested nothing
proves nothing).

| Exit code | Meaning |
|---|---|
| 0 | every required check passed (or was `NOT RUN` by a parameter) |
| 1 | at least one `FAIL` |
| 2 | no `FAIL`, but at least one required check is `BLOCKED` |

## Reading the JSON report

Each run writes `validation-results\<script>-<yyyyMMdd-HHmmss>.json` and a folder of the same name with one `.log` per command
(the folder `validation-results\` ignores itself in git). Shape:

```json
{
  "script": "verify-windows",
  "startedAt": "2026-10-24T09:12:03.1234567+02:00",
  "finishedAt": "...",
  "durationSeconds": 1234.5,
  "overall": "PASS | FAIL | INCOMPLETE (required checks BLOCKED)",
  "exitCode": 0,
  "counts": { "pass": 14, "fail": 0, "blockedRequired": 0, "blockedInformational": 1, "notRun": 4 },
  "environment": { "githubActions": false, "hostLooksVirtual": false, "os": "Windows 11 Pro 24H2 (build 26100)", "powershell": "5.1.26100.1", "powershellEdition": "Desktop" },
  "repository": { "root": "C:\\Users\\me\\forge", "commit": "<sha>", "branch": "cloud/overhaul", "trackedChanges": false },
  "logDirectory": "C:\\Users\\me\\forge\\validation-results\\verify-windows-20261024-091203",
  "statusLabels": ["PASS — Physical Windows", "FAIL", "BLOCKED", "NOT RUN"],
  "notes": ["No live provider test is run: ..."],
  "checks": [
    {
      "name": "Rust core: cargo test --lib (ignored live tests stay ignored)",
      "status": "PASS — Physical Windows",
      "outcome": "PASS",
      "required": true,
      "durationSeconds": 512.3,
      "command": "C:\\Users\\me\\.cargo\\bin\\cargo.exe test --lib --locked",
      "logPath": "C:\\...\\Rust-core-cargo-test-lib-ignored-live-tests-stay-ignored.log",
      "detail": "<n> passed, 0 failed, <m> ignored (live tests, not run), 0 filtered out"
    }
  ]
}
```

`status` is the label above; `outcome` is the same without the environment suffix (`PASS`, `FAIL`, `BLOCKED`, `NOT RUN`) for
tools. `repository.trackedChanges = true` means the run tested a working tree with uncommitted changes to tracked files. For a
check that ran another script, `logPath` is that script's JSON report.

## What each script checks (exact commands)

All `cargo` commands run in `apps\desktop\src-tauri`; all `npm` commands run in the repository root (as `npm.cmd`, so the
execution policy does not matter for npm).

### `verify-windows.ps1` (main entry point)

| Check | Command | Required |
|---|---|---|
| Prerequisite: git | `git --version` | yes |
| Prerequisite: Git Bash (hook tests) | `bash.exe` in `CLAUDE_CODE_GIT_BASH_PATH`, `%ProgramFiles%`, `%ProgramFiles(x86)%`, `%LOCALAPPDATA%` (`Git\bin` / `Programs\Git\bin`), the same search as FORGE's `find_bash()` | yes |
| Prerequisite: Node.js 22+ | `node --version` (major >= 22) | yes |
| Prerequisite: npm | `npm --version` | yes |
| Prerequisite: Rust (cargo, rustc) | `cargo --version`, `rustc -vV` | yes |
| Prerequisite: Rust MSVC target | `rustc -vV` host is `*-pc-windows-msvc`; `rustup target list --installed` shown | yes |
| Prerequisite: Visual Studio Build Tools (C++) | `vswhere.exe -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -latest -format value -property displayName` | yes |
| Prerequisite: WebView2 runtime | registry value `pv` under `EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}` (HKLM WOW6432Node, HKLM, HKCU) | yes |
| Prerequisite: git identity | `git config --get user.name`, `git config --get user.email` | informational |
| Install | `npm ci` | yes |
| Engine build / typecheck / tests | `npm run build -w @forge/engine`, `npm run typecheck -w @forge/engine`, `npm run test -w @forge/engine` | yes |
| UI typecheck / tests | `npm run typecheck -w forge-desktop`, `npm run test -w forge-desktop` | yes |
| Rust core | `cargo test --lib --locked` | yes |
| Other scripts | with `-All`: the four scripts below; with `-IncludeRelease`: `test-release.ps1` | yes when requested |

### `test-provider-adapters.ps1`

| Check | Command | Required |
|---|---|---|
| Claude Code CLI installed | `claude --version` | informational |
| Claude Code sign-in | `claude auth status` (JSON; only `loggedIn`, `authMethod`, `subscriptionType` are read; output never shown or logged), the same call as FORGE's `providers.rs` | informational |
| Codex CLI installed | `codex --version`; `FORGE_CODEX_PATH` overrides `PATH` and only a real `codex.exe` counts, as in FORGE | informational |
| Codex sign-in | `codex login status` (output never shown or logged). Note: FORGE's own check treats any output containing "logged in" as signed in, which includes "Not logged in"; this script treats "not logged in" or a non-zero exit as *no* | informational |
| Engine adapter tests | `npm run test -w @forge/engine` (builds first; Codex adapter against a scripted app-server, ACP against a scripted agent, Claude options/env scrubbing) | yes |
| Rust, scripted agents | `cargo test --lib --locked supervisor::tests::acp`, `... supervisor::tests::mcp`, `... supervisor::tests::approvals`, `... acp::tests`, `... mcp::tests`, `... providers::tests` | yes |

There is no Rust test that drives a scripted Codex app-server; the Codex adapter's scripted tests live in
`packages/engine/test/codex.test.ts`.

### `test-execution-backends.ps1`

Only `trusted_local` (the user's shell, **not isolated**) runs on Windows. `windows_sandbox` and `isolated_wsl` are designed
(`docs/EXECUTION.md`) but **not implemented**; the Sandbox and WSL states are informational.

| Check | Command | Required |
|---|---|---|
| Windows Sandbox feature | `Get-WindowsOptionalFeature -Online -FeatureName Containers-DisposableClientVM` (needs an elevated prompt, otherwise `BLOCKED` with that reason) | informational |
| WSL status | `wsl.exe --status` (with `WSL_UTF8=1`) | informational |
| Rust | `cargo test --lib --locked <filter>` for each filter: `execution`, `execution::tests::unimplemented_backends_are_never_available_and_say_so`, `paths::`, `proc::`, `policy::` | yes |
| Rust, Windows-only (`#[cfg(windows)]`) | `paths::tests::windows_case_insensitive_and_rejects_streams_and_devices`, `paths::tests::a_root_spelled_with_a_short_name_still_contains_its_files_but_short_names_below_it_are_refused`, `policy::tests::protection_follows_the_real_path_not_the_spelling`, `proc::tests::a_suspended_process_runs_only_after_it_is_contained_and_resumed` | yes |
| Rust, real junctions / Job Objects on Windows | `paths::tests::resolve_within_catches_directory_link_escape`, `knowledge::vault::tests::a_directory_junction_inside_the_tree_is_refused`, `knowledge::vault::tests::a_junction_that_points_elsewhere_inside_the_same_tree_is_still_refused`, `harness::edit::tests::links_in_the_path_are_refused`, `supervisor::tests::stopping_a_hung_agent_marks_it_interrupted_and_cleans_up_the_process`, `supervisor::tests::regressions::a_descendant_holding_the_pipes_cannot_wedge_finalisation_and_is_killed`, `harness::dryrun::tests` | yes |

### `test-obsidian.ps1`

| Check | Command | Required |
|---|---|---|
| Fixture vault | creates `%TEMP%\forge-obsidian-fixture-<timestamp>\FORGE Validation Vault\` with `.obsidian\app.json` and one note containing a random token | yes |
| Obsidian installed | `HKCU\Software\Classes\obsidian` or `HKCR\obsidian` exists (FORGE's own check) | informational |
| Rust | `cargo test --lib --locked obsidian`, `... vault`, `... knowledge::` | yes |
| Human confirmation (`-Interactive`) | `Start-Process 'obsidian://open?path=<note>'` with FORGE's encoding (`knowledge\vault.rs` `obsidian_uri`), then asks what you saw. FORGE documents that the link works for vaults Obsidian already knows; if Obsidian says the vault is unknown you are guided to "Open folder as vault" for the fixture and the link is opened again (remove the fixture from Obsidian's vault list afterwards) | yes when `-Interactive` |

### `test-release.ps1`

| Check | Command | Required |
|---|---|---|
| No existing FORGE installation | uninstall entries named `FORGE` in HKCU/HKLM. If one exists the install steps are `BLOCKED` unless `-ReplaceExistingInstall` (the test uninstall would remove that registration) | yes |
| Build installer | `npm run package` (= `tauri build --config src-tauri/tauri.bundle.conf.json`: NSIS, `installMode: currentUser`, engine bundle and `node.exe` staged by `tools/package/prepare.mjs`); expects exactly one new `<target>\release\bundle\nsis\*-setup.exe` | yes |
| SHA-256 | `Get-FileHash -Algorithm SHA256` of the setup exe and `<target>\release\forge-desktop.exe`, saved as `SHA256SUMS.txt` in the log folder | yes |
| Silent install | `<setup.exe> /S /D=%TEMP%\forge-release-test-<timestamp>\FORGE` (NSIS: `/D=` last and unquoted) | yes |
| Installed files | app exe, `uninstall.exe`, `engine\main.js`, `node\node.exe`; an uninstall entry pointing at that folder | yes |
| Launch (`-Launch`) | starts the installed exe with `FORGE_HOME` and `WEBVIEW2_USER_DATA_FOLDER` inside the temporary folder; must still run after `-LaunchSeconds` (default 20); reports whether a main window exists; then `taskkill /T /F` | yes when `-Launch` |
| Silent uninstall | `<install dir>\uninstall.exe /S`, then the app exe and the uninstall entry must be gone (polled up to 180 s, because the NSIS uninstaller relaunches itself from `%TEMP%`) | yes |

The installer is unsigned (SmartScreen will warn when it is opened by hand). Upgrade over an older installation is not tested.

## What these scripts cannot verify

* Real Claude Code or Codex missions (they would spend quota; run the `live_*` tests by exact name, by hand, when you decide to).
* Windows Sandbox / isolated WSL execution (not implemented in FORGE).
* The real app window flows (`tools/e2e/*.mjs` drive the real WebView2 window; see `codex-handoff.md`).
* Code signing, auto-update, upgrade over an existing installation.

## CI (`.github/workflows/`)

| Workflow / job | Runner | What it does | Fails the build on |
|---|---|---|---|
| `ci.yml` / `test` | ubuntu-24.04, windows-2022 | engine, UI and Rust tests (live tests stay ignored) | any test failure |
| `ci.yml` / `windows-scripts` | windows-2022 | scripts must be ASCII; parse every `.ps1`/`.psm1` with Windows PowerShell 5.1 and PowerShell 7 (`[System.Management.Automation.Language.Parser]::ParseFile`); PSScriptAnalyzer 1.25.0; smoke-runs all five scripts with the expensive steps skipped and checks each JSON report (labels from the set above, `Windows CI` only) | parse errors, analyzer findings of severity `Error`, `PSUseCompatibleSyntax` findings for 5.1/7.0, a smoke exit code of 1, a malformed report |
| `ci.yml` / `tauri-windows-build` | windows-2022 | `npm run package`; uploads the NSIS installer and its `SHA256SUMS.txt` as a workflow artifact (7 days). No signing, no release | build failure, not exactly one installer |
| `security.yml` / `npm-audit` | ubuntu-24.04 | `npm audit --omit=dev --audit-level=high`; then all dependencies (informational) | high/critical advisory in production dependencies |
| `security.yml` / `cargo-audit` | ubuntu-24.04 | `cargo audit --file apps/desktop/src-tauri/Cargo.lock` (cargo-audit 0.22.2) | any vulnerability (unmaintained/unsound warnings are printed only) |
| `security.yml` / `cargo-deny` | ubuntu-24.04 | `cargo deny --locked check licenses sources` with `apps/desktop/src-tauri/deny.toml` (cargo-deny 0.20.2) | a licence outside the allow-list, a non-crates.io source. The root crate being unlicensed is the one accepted finding (FORGE has no licence yet) |
| `security.yml` / `gitleaks` | ubuntu-24.04 | gitleaks 8.30.1 (SHA-256 verified) `git` scan of the full history with `.gitleaks.toml` (default rules + a narrow path-and-value allowlist for the fake tokens in tests), redacted output | any finding |
| `security.yml` / `osv-scanner` | ubuntu-24.04 | osv-scanner 2.7.0 (SHA-256 verified) on `package-lock.json` and `Cargo.lock` | nothing (informational; overlaps npm/cargo audit) |
| `security.yml` / `sbom` | ubuntu-24.04 | `npm sbom --sbom-format cyclonedx --omit dev --omit optional --package-lock-only`; `cargo cyclonedx` 0.5.9 for `x86_64-pc-windows-msvc` and `x86_64-unknown-linux-gnu`; uploaded as an artifact (30 days) | SBOM generation failure |

All workflows have `permissions: contents: read` and no secrets. Every action is pinned to a full commit SHA with its tag in a
comment; `security.yml` also runs weekly so new advisories appear without a push.

### TODO for maintainers

* Newer major versions exist of `actions/checkout` (v7), `actions/setup-node` (v7) and `actions/upload-artifact` (v7); the
  pins stay on the v4 line the workflow already used (they run on the node20 actions runtime). Move deliberately, one at a time.
* `dtolnay/rust-toolchain` has no release tags; it is pinned to the commit of its `stable` branch on 2026-10-23.
* When FORGE gets a licence, set `license` (or `publish = false`) in `apps/desktop/src-tauri/Cargo.toml` and remove the
  unlicensed-root exception in `security.yml`.
* The first gitleaks run over the full history may report findings nobody has seen yet. Check each one; only fake test values
  belong in `.gitleaks.toml`, and only with both the file and the exact value.
* `npm sbom` omits optional dependencies (see the comment in `security.yml`); they are build-time platform binaries.
