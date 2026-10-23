# Validation matrix

What has been run, where, and with what result, for the `cloud/overhaul` branch. Labels:

| Label | Meaning |
|---|---|
| **PASS — Linux** | Automated tests passed on Linux (the cloud container and/or the `ubuntu-24.04` CI runner) |
| **PASS — Windows CI** | Automated tests passed on GitHub's `windows-2022` runner: a clean Windows Server VM, no WebView2 window, no provider logins, no installer use by a person |
| **PASS — Physical Windows** | Passed on the owner's Windows 11 PC. Only rows verified there before this branch carry it, and they are marked as earlier evidence |
| **FAIL** | Ran and failed |
| **BLOCKED** | Could not run here (tool, login or platform missing); says what blocks it |
| **NOT RUN** | Not attempted yet |

A Linux pass is never a Windows pass. "Mock" marks tests whose counterpart is scripted rather than the real program: the
scripted agent (`fake` provider), the scripted ACP agent (`packages/engine/test/fixtures/acp-agent.mjs`), the scripted Codex
app-server (`packages/engine/test/codex.test.ts`), or hand-written scanner reports (`apps/desktop/src-tauri/fixtures/evidence`).

CI evidence: CI run 32 (head `14f289f`) was green on `ubuntu-24.04` and `windows-2022`, including the Windows script job
(parse under 5.1/7, PSScriptAnalyzer, smoke) and the NSIS installer build; the security workflow was green at `08aadaf` and
`14f289f`. Code intelligence was merged afterwards (`cd75d4c`): its Windows CI result is on the PR
(https://github.com/somsom786/forge/pull/8), not claimed here.
Physical Windows validation of this branch: **NOT RUN** (planned; `docs/CODEX_WINDOWS_HANDOFF.md`).

## Features

| Area | Linux | Windows CI | Physical Windows | Notes |
|---|---|---|---|---|
| Mission control, worktrees, policy, approvals, recovery (existing) | PASS — Linux | PASS — Windows CI | PASS — Physical Windows (earlier, before this branch) | Re-validate on this branch: approvals changed |
| Approval binding: digest of full context, 30-min expiry, decision-time policy re-check | PASS — Linux | PASS — Windows CI (binding, expiry) | NOT RUN | The TOCTOU test uses a Unix symlink; the Windows junction variant is a handoff item. Mock: scripted agent |
| MCP authorization (hash-pinned, only authorized servers reach missions, core denies the rest) | PASS — Linux | PASS — Windows CI | NOT RUN | Mock: scripted agent asks for `mcp__` tools. **No real MCP server was started**; Claude SDK accepting the `mcpServers` shape is not exercised live |
| ACP agents (official SDK client; brokered permissions and file access) | PASS — Linux | PASS — Windows CI | NOT RUN | Mock: scripted ACP agent built on the real SDK. **No third-party ACP agent run.** Symlink-swap test may skip on Windows without symlink privilege |
| Proof Engine adapters (gitleaks, OSV-Scanner, Trivy, Playwright) | PASS — Linux | PASS — Windows CI | NOT RUN | Mock: hand-written reports. **None of the tools was run**; suggested command lines unconfirmed |
| Execution backend `trusted_local` | PASS — Linux | PASS — Windows CI | PASS — Physical Windows (earlier, as the only runner) | Not isolated, labelled so |
| Execution backend `linux_namespaces` (bubblewrap) | PASS — Linux (9 adversarial tests, mutation-checked; CI lifts Ubuntu 24.04's AppArmor userns restriction) | n/a | n/a | Linux only |
| Execution backends `windows_sandbox`, `isolated_wsl` | BLOCKED (not implemented) | BLOCKED (not implemented) | BLOCKED (not implemented) | Designed in EXECUTION.md; never selectable |
| Codex sign-in detection fix ("Not logged in") | PASS — Linux (unit) | PASS — Windows CI (unit) | NOT RUN | Needs a real `codex login status` on a logged-out and a logged-in machine |
| Windows validation scripts (`scripts/windows/*.ps1`) | n/a | PASS — Windows CI (parse 5.1/7, PSScriptAnalyzer, smoke with expensive steps skipped) | NOT RUN | The first smoke run found a report-encoding bug in the CI validator (fixed in `14f289f`) |
| NSIS installer build | n/a | CI job builds and uploads it (artifact, 7 days) | PASS — Physical Windows (earlier: silent install, launch, scripted mission, uninstall) | Upgrade over an older install never tested; unsigned |
| Security workflow (npm audit, cargo-audit, cargo-deny, gitleaks full history, OSV-Scanner, SBOMs) | PASS — Linux (first run) | n/a | n/a | OSV-Scanner informational; FORGE's own crate is unlicensed (accepted exception until the owner chooses a licence) |
| Experiments, conflicts, Agents screen | PASS — Linux | PASS — Windows CI | NOT RUN | UI seen only in the browser preview with mock data |
| Code intelligence (tree-sitter index, Code Map, relevant code in context) | PASS — Linux | see PR checks for `cd75d4c` | NOT RUN | Measured only on a synthetic 36-file fixture (top-3 F1 0.48 grep → 0.59 index); Code Map seen in the browser preview only |
| Live provider missions (Claude, Codex) | BLOCKED (would spend the owner's quota; never run by the cloud agent) | BLOCKED (no logins) | PASS — Physical Windows (earlier, before this branch) | Re-run on this branch is a handoff item |

## Adversarial regression tests (security acceptance list, items 1–18)

Tests named here assert the restriction itself (the file is not read, the process is gone), not only that policy said "deny".

| # | Threat | Tests | Linux | Windows CI | Gaps |
|---|---|---|---|---|---|
| 1 | Host-file access from agent-executed code | `execution_tests::a_command_cannot_read_files_outside_its_workspace`, `…cannot_write_outside…`; policy `write_outside_worktree_is_denied…`, `reads_outside_worktree_escalate`; ACP `safeTarget` link test | PASS — Linux | PASS — Windows CI (policy only) | **Agent tool calls approved by a human run on the host** (SECURITY limit 1); isolation covers checks/experiments only |
| 2 | Secrets via env and mounted paths | `execution_tests::secrets_in_forges_environment_do_not_reach_the_command` (unredactable value + control); `acp.test.ts` env stripping; `harness::dryrun::secret_looking_environment_is_not_passed_to_the_hook`; `knowledge::hardening_tests::secrets_in_imported_notes…` | PASS — Linux | partial | Trusted-local checks see the user's environment by design |
| 3 | Unauthorized network egress | `execution_tests::a_command_cannot_reach_the_network_or_host_loopback_services`; `providers::non_loopback_urls_are_never_contacted` | PASS — Linux | partial | No egress control for agents or trusted-local checks |
| 4 | Project hook and MCP startup execution | `claudeOptions.test.ts` (lockdown), `tests::mcp::a_repository_mcp_json_alone…`, `…hostile rewrite…`, Codex project-config refusal (`codex.test.ts`), `review_hardening::a_hostile_git_hook_never_runs…` | PASS — Linux | PASS — Windows CI | Live lockdown experiment (`scripts/lockdown-experiment.mjs`) was last run before this branch |
| 5 | Approval replay or request mutation | `approvals::an_approval_is_bound…`, `regressions::a_reask_with_changed_input…`, `identical_requests_share_one_key…`, `store::approvals_can_be_decided_exactly_once` | PASS — Linux | PASS — Windows CI | — |
| 6 | Windows path redirection and junction replacement | `paths::*` (short names, streams, devices), `policy::protection_follows_the_real_path…` (cfg windows), `knowledge::vault::*junction*`, `harness::edit::links_in_the_path_are_refused` | PASS — Linux (Unix links) | PASS — Windows CI | Decision-time re-check with a **junction** swapped in: handoff item (P0) |
| 7 | Modified git config and external filters | `review_hardening::a_repository_configuring_an_external_git_filter_is_refused…`, `…hostile_git_hook…`, `git::a_worktree_redirected_to_another_repository_is_detected` | PASS — Linux | PASS — Windows CI | — |
| 8 | Untrusted test scripts | checks run only on explicit user action; `execution_tests::*` on `linux_namespaces`; `phase2::a_check_that_modifies_the_worktree_is_flagged_not_trusted`; evidence adapters never pass on unreadable output | PASS — Linux | PASS — Windows CI (trusted_local) | On Windows, checks are not isolated (no Windows backend yet) |
| 9 | Cross-worktree access | `policy_blocks_writes_outside_the_worktree…`, `parallel_missions_get_separate_worktrees…`, sandbox mounts only the workspace | PASS — Linux | PASS — Windows CI | Agents can *read* other worktrees after approval (reads outside ask, high risk) |
| 10 | Process escape and orphaned subprocesses | `regressions::a_descendant_holding_the_pipes…`, `proc::dropping_containment_kills_the_whole_process_group`, `verify::timeout_kills_the_whole_process_tree`, `execution_tests::nothing_started…outlives_it` | PASS — Linux | PASS — Windows CI (Job Objects) | Microsecond window between spawn and Job assignment for checks (documented) |
| 11 | App restart during pending approvals | `restart_recovery_marks_active_missions_interrupted_not_running`, `store::expiring_session_approvals_leaves_decided_ones`, `stopping_while_an_approval_is_pending_expires_it`, approval expiry sweeper test | PASS — Linux | PASS — Windows CI | Real app restart with a pending card: handoff item (P1) |
| 12 | Agent crash during filesystem modifications | `agent_crash_fails_the_mission_honestly_with_diagnostics`; the worktree diff remains the record | PASS — Linux | PASS — Windows CI | **Partial**: no test crashes the agent *between* writes of a multi-file change |
| 13 | Provider sandbox misconfiguration | Codex effective-settings refusal (`codex.test.ts`, mock app-server); ACP: no terminal capability offered, only `*_once` answers | PASS — Linux | PASS — Windows CI | Real Codex `workspace-write` sandbox behaviour on Windows not tested |
| 14 | Source/prompt injection through memory or docs | `knowledge::service_tests::hostile_note_text_cannot_forge_the_package_boundary…`, `memory::a_page_edited_after_verification_is_not_injected…`, `knowledge::page::hostile_yaml_constructs…` | PASS — Linux | PASS — Windows CI | Injection through repository files read by the agent is out of FORGE's control (diff review) |
| 15 | Malicious or malformed provider events | `store::duplicate_and_out_of_order_events_are_safe`, engine `normalize.test.ts`, `codex.test.ts`, ACP protocol-version/auth/crash tests, malformed engine lines are dropped | PASS — Linux | PASS — Windows CI | — |
| 16 | Partial DB writes and crash recovery | `harness::a_crash_between_journal_and_write…`, `knowledge::service_tests::crash_recovery_resolves_interrupted_writes…`, `phase2::restart_recovery_resolves_checks_that_were_mid_flight`, `experiment::…a_crash_is_recovered` | PASS — Linux | PASS — Windows CI | No power-loss test of SQLite itself (relies on SQLite WAL) |
| 17 | Unsafe worktree cleanup | `discard_refuses_to_destroy_unsaved_work…`, `regressions::discard_cannot_delete_a_branch_with_unmerged_commits…`, `review_hardening::cleanup_after_a_merge_protects_commits…`, `git::worktree_lifecycle_protects_uncommitted_work` | PASS — Linux | PASS — Windows CI | — |
| 18 | Release upgrade and migration failures | `db::migrations_apply_from_scratch_and_forward_from_every_earlier_version` (now through migration 12) | PASS — Linux | PASS — Windows CI | **Installer upgrade over an older install never tested** (handoff P1) |

## Not run anywhere (honest list)

* Any live provider mission on this branch (Claude, Codex, a real ACP agent).
* A real MCP server started by a mission.
* gitleaks / OSV-Scanner / Trivy / Playwright as FORGE checks (the security workflow runs gitleaks and OSV-Scanner on the
  repository itself, which is different).
* The Tauri window on Windows for any of the new screens (MCP dialog, ACP registration, findings view, Code Map).
* Windows Sandbox / isolated WSL execution (not implemented).
