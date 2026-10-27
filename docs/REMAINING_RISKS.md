# Remaining risks

What is known to be wrong, missing or risky, as of `cloud/overhaul` 2026-10-24, most important first. "Confirmed"
means reproduced or read in the code; "limitation" means designed-in and documented; "hypothetical" means plausible but
not demonstrated. Everything fixed this round is in `docs/SECURITY.md` ("Fixes from the independent cloud reviews") and
the commit log, not here.

## Release blockers

| # | Risk | Kind | Where | What would close it |
|---|---|---|---|---|
| R1 | The installer bundles the Claude Agent SDK's JavaScript, whose terms read "© Anthropic PBC. All rights reserved. Use is subject to the Legal Agreements". Redistribution inside a third-party installer is not confirmed | Confirmed (licence) | `packages/engine/scripts/bundle.mjs`; `THIRD_PARTY_NOTICES.txt` "Needs a decision" | Owner confirms with Anthropic's terms, or the engine loads the SDK from the user's own installation (`docs/LICENSING_DECISION.md`) |
| R2 | No licence for FORGE itself | Confirmed | repository root | Owner's decision (`docs/LICENSING_DECISION.md`) |
| R3 | This branch has not run on a physical Windows PC: WebView2 rendering, real provider logins, Obsidian, Windows Defender interaction, installer upgrade over an older install, unsigned-installer SmartScreen flow | Not run | — | `docs/CODEX_WINDOWS_HANDOFF.md` |
| R4 | Installer is unsigned; no update channel | Confirmed | `.github/workflows/ci.yml` (tauri windows build) | Code-signing certificate and a signed update feed (design only; not started) |

## Security (in priority order)

1. **An approved agent action runs with the user's privileges on Windows, and on Linux unless the project's agent
   backend is `linux_namespaces`** (limitation; `docs/SECURITY.md` limit 1). Approval prompts and policy reduce, not
   remove, the risk. Windows isolation (Windows Sandbox / isolated WSL) is designed in `docs/EXECUTION.md`, not
   implemented.
2. **Contained agents (Linux) keep network access and provider state** (limitation; `execution.rs`
   `agent_command`). Anything the agent can read (the worktree, its own provider login and transcripts of the user's
   other projects in `~/.claude`/`~/.codex`) can leave the machine. Runtime/provider/proxy settings are allow-listed;
   arbitrary inherited environment secrets and loader injection are removed. Proxy settings can contain credentials.
   Directory arguments of MCP servers are not exposed, so filesystem-style servers do not work when contained.
3. **The webview is trusted by the core** (limitation, from the first security review, F9). Any script running in the
   UI can call every command, including approving requests. Mitigations: strict CSP, no remote content, no HTML
   injection (`SafeMarkdown`), minimal Tauri capabilities. There is no second factor for approvals.
4. **Shell rules are heuristics** (limitation; `policy.rs`). `git p""ush`-style obfuscation defeats pattern rules;
   every shell call asks, and the person reading it is the control.
5. **Agent instruction detection is path-based and cannot detect every provider convention** (limitation). The review
   gate and Proof Pack flag supported instruction and configuration paths, and approval requires an explicit acknowledgement
   bound to the changes fingerprint. Older approvals of such changes return to review before merge. The exact diff is in
   the Changes view and the Proof Pack's saved diff. A differently named instruction file may not be recognized; the
   trusted webview can still submit acknowledgements, and a checkbox cannot establish that a person actually read a diff.
6. **A file write by the provider itself, after approval, has a microsecond check-then-use window** (limitation;
   `docs/SECURITY.md` limit 3). Decision-time re-checks close the human-latency window (tested with Unix links; Windows
   junction test added this round, result in CI); the provider's own write is outside FORGE's control.
7. **Older FORGE builds opening an upgraded database silently ignore new settings** (confirmed, for builds before this
   change only). From this change on, a build refuses a database written by a newer one (`db.rs` `migrate`). A user who
   reinstalls `main` after this branch would run agents on the host even if the project says contained.
8. **Unix process containment does not survive a descendant calling `setsid`** (limitation; `proc.rs`). Windows Job
   Objects are not affected.
9. **Redaction is pattern based** (limitation; `redact.rs`). Unusual secret formats reach logs and transcripts.

## Reliability and data

| # | Issue | Kind | Where | Notes / fix |
|---|---|---|---|---|
| D1 | SQLite compaction can be slow on a large database | Limitation | `supervisor/storage.rs` | Explicit idle-only action, never automatic; temporary-database reclamation and busy refusal tested on Windows |
| D2 | Leftover experiment worktrees and unexpected files need manual inspection | Limitation | `supervisor/storage.rs` | Finished experiment logs and empty folders are previewed and cleaned; linked or unexpected entries are preserved and explained |
| D3 | Mission lists load the 500 most recent missions | Limitation | `store.rs` `list_missions` | The screen says so; recovery and clean-up are not limited |
| D4 | No power-loss test of SQLite itself | Not run | — | Relies on SQLite WAL + `synchronous=NORMAL` (a power cut can lose the last transactions, not corrupt the database) |

## Product quality

| # | Issue | Kind | Evidence |
|---|---|---|---|
| Q1 | Memory retrieval is lexical: questions that use different words from the note are missed (ripgrep FAQ: right answer in the package 82%, first 57%) | Measured | `docs/EVALUATION.md` |
| Q2 | Code index sees declarations only: tasks that name an API used *inside* functions are missed; test files rank high | Measured | `docs/EVALUATION.md` |
| Q3 | No live comparison of FORGE against native agents; no claim of benefit can be made | Not run | `docs/EVALUATION.md` |
| Q4 | UI verified only in Chromium against mock data; WebView2 and real data not exercised by automation | Limitation | `apps/desktop/e2e` |
| Q5 | Codex: real `workspace-write` sandbox behaviour on Windows, and live missions, not run on this branch | Not run | `docs/VALIDATION_MATRIX.md` |
| Q6 | Third-party ACP agents other than `claude-agent-acp` untested; agents that keep their login outside `~/.claude`/`~/.codex` fail when contained | Limitation | `docs/EXECUTION.md` |
