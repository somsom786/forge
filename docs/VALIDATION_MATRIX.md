# Validation matrix

What has been run, where, and with what result, for `cloud/overhaul` (PR https://github.com/somsom786/forge/pull/8).

Labels:

| Label | Meaning |
|---|---|
| **PASS — Linux** | Automated tests passed on Linux: the cloud container and the `ubuntu-24.04` CI runner |
| **PASS — Windows CI** | Passed on GitHub's `windows-2022` runner: a clean Windows Server VM with no WebView2 window, no provider logins and no person using the installer |
| **PASS — Physical Windows** | Passed on the owner's Windows 11 PC. Only rows verified there *before this branch* carry it, marked "earlier" |
| **FAIL** | Ran and failed |
| **BLOCKED** | Could not run here; says what blocks it |
| **NOT RUN** | Not attempted yet |

A Linux pass is never a Windows pass. **Simulated** marks tests whose counterpart is scripted rather than real:

- the scripted agent (`fake` provider);
- the scripted ACP agent (`packages/engine/test/fixtures/acp-agent.mjs`);
- the scripted Codex app-server (`codex.test.ts`);
- the scripted Anthropic-compatible model server (`fake-anthropic.mjs`), which real Claude Code talks to;
- the UI preview's mock backend.

## Commands

| Suite | Command | Runs in CI |
|---|---|---|
| Rust core | `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib` with `FORGE_REQUIRE_SANDBOX_TESTS=1 FORGE_REQUIRE_SCANNER_TESTS=1 FORGE_REQUIRE_CLAUDE_REAL_TESTS=1` (sandbox and scanners on Linux only) | Linux and Windows |
| Engine | `npm run test -w @forge/engine` with `FORGE_ACP_INTEROP_DIR=<dir with claude-agent-acp@0.88.0>` and `FORGE_REQUIRE_ACP_INTEROP=1` | Linux and Windows |
| UI unit and component | `npm run test -w forge-desktop` | Linux and Windows |
| UI end-to-end (simulated) | `npm run e2e -w forge-desktop` | Linux |
| Windows scripts | `.github/workflows/ci.yml` job "windows scripts": parse under PowerShell 5.1 and 7, PSScriptAnalyzer, smoke run | Windows |
| Installer | `npm run package` | Windows (artifact, 7 days) |
| Benchmarks | `FORGE_BENCH=1 cargo test --release --lib bench_ -- --nocapture --test-threads=1` | on demand |
| Code-intelligence evaluation | `scripts/codeintel-eval.sh` | on demand |
| Security | `.github/workflows/security.yml`: npm audit, cargo-audit, cargo-deny, gitleaks (full history), OSV-Scanner, SBOMs, third-party notices | every push |

**CI evidence.** CI was green on both runners at `4eb671a`, `e07f9d3` and `7188f8f`. At `96ad0ff` Windows CI failed one new test, which found the worktree defect fixed in `6626706`. The physical-Windows validation of this branch is **NOT RUN** (`docs/CODEX_WINDOWS_HANDOFF.md`).

## Features

| Area | Tests | Linux | Windows CI | Physical Windows | Remaining limitation |
|---|---|---|---|---|---|
| Missions, worktrees, recovery | `supervisor::tests::*`, `integrity::*`, `upgrade::*` | PASS — Linux | PASS — Windows CI | PASS — Physical Windows (earlier) | Re-validate on this branch |
| Approvals: bound to the exact request, re-checked at decision time, expiring, reused at most once | `approvals::*`, `regressions::*` | PASS — Linux | PASS — Windows CI, including a **junction** swapped in at decision time (`windows::a_junction_swapped_in…`) | NOT RUN | Simulated agent |
| Policy (paths, protected files, Unicode look-alikes, shorthands, Glob/Grep patterns) | `policy::tests` | PASS — Linux | PASS — Windows CI | NOT RUN | Shell rules are heuristics |
| Agent containment (Linux): engine, provider and approved commands in bubblewrap; broad binds refused | `agent_sandbox::*`, `claude_real::an_approved_command…when_contained` | PASS — Linux (with host controls; mutation-checked) | n/a: refused on Windows, never falls back (`windows::agent_isolation…`, PASS — Windows CI) | n/a | Network and environment not restricted |
| Check isolation `linux_namespaces` | `execution_tests::*` | PASS — Linux (9 adversarial tests, mutation-checked) | n/a | n/a | Linux only |
| `windows_sandbox`, `isolated_wsl` backends | — | BLOCKED (not implemented) | BLOCKED | BLOCKED | Designed in `docs/EXECUTION.md` |
| Real Claude Code (bundled binary against a scripted local model): approval-gated write, escape denied, hostile repository locked down, real MCP server as authorized | `claude_real::*` | PASS — Linux | PASS — Windows CI | NOT RUN | The model is scripted; no account used |
| MCP authorization (hash-pinned, allow-list enforced by the core) | `mcp::*`, `claude_real::a_real_mcp_server…` | PASS — Linux | PASS — Windows CI | NOT RUN | Server runs as the user |
| ACP (official SDK; brokered permissions and file access) | `acp.test.ts`, `tests::acp::*` | PASS — Linux | PASS — Windows CI | NOT RUN | Scripted agent |
| ACP interop with a real third-party agent (`claude-agent-acp` 0.88.0) | `acp-interop.test.ts` | PASS — Linux | PASS — Windows CI | NOT RUN | Its model is scripted |
| Codex adapter | `codex.test.ts` | PASS — Linux (simulated app-server) | PASS — Windows CI (simulated) | NOT RUN | No real Codex on this branch |
| Proof Engine: real gitleaks and OSV-Scanner through the check path; real Trivy and Playwright reports | `scanners::*`, `evidence::tests` | PASS — Linux | PASS — Windows CI (report parsing; tools not installed there) | NOT RUN | Trivy and Playwright are not executed in CI |
| Proof Packs (summary strip, "agent ran") | `proof::tests`, `ProofPackPanel.test.tsx` | PASS — Linux | PASS — Windows CI | NOT RUN | |
| Memory and Obsidian (link forms, NFD names, renames, conflicts, read-only notes) | `knowledge::*`, `vault_fixture_tests::*` | PASS — Linux | PASS — Windows CI | NOT RUN (no Obsidian app) | Lexical retrieval |
| Code intelligence | `codeintel::tests`, `eval_oss` (on demand) | PASS — Linux; evaluated on 9 real repositories | PASS — Windows CI (unit) | NOT RUN | Declarations only |
| Harness Studio (Doctor incl. precedence, edits, rollback, dry-runs) | `harness::*` | PASS — Linux | PASS — Windows CI | NOT RUN | Live test ignored |
| Storage usage and clean-up (previewed, token-confirmed, audited) | `storage::*` | PASS — Linux | PASS — Windows CI | NOT RUN | |
| Worktree removal with a file in use (refused before deleting; half-deleted worktree finished after confirmation) | `windows::discarding_while_a_file_is_locked…`, `integrity::a_worktree_an_earlier_removal…` | PASS — Linux | FAIL at `96ad0ff` (found the bug); fix `6626706` pending | NOT RUN | |
| Upgrade from the released schema (v6 with data) to v15; shipped migrations pinned; newer database refused | `upgrade::*`, `db::tests` | PASS — Linux | PASS — Windows CI | NOT RUN | Installer upgrade not tested |
| UI end-to-end (**simulated**: every screen, mission tabs, palette, dialogs, storage; WCAG A/AA via axe at 1100×700) | `apps/desktop/e2e` | PASS — Linux (6 tests, mutation-checked) | n/a | NOT RUN | Says nothing about WebView2 or real data |
| Stress volumes (`docs/PERFORMANCE.md`) | `bench_*` (on demand) | PASS — Linux | NOT RUN | NOT RUN | Cloud numbers only |
| Codex sign-in detection | `providers::tests` | PASS — Linux | PASS — Windows CI | NOT RUN | Needs real `codex login status` |
| Windows validation scripts | `scripts/windows/*.ps1` | n/a | PASS — Windows CI | NOT RUN | |
| NSIS installer (unsigned, with `THIRD_PARTY_NOTICES.txt`) | CI job | n/a | builds | PASS — Physical Windows (earlier: install, launch, mission, uninstall) | Upgrade over an older install untested; unsigned |
| Live provider missions (Claude, Codex) | `live_*` (ignored) | BLOCKED (would spend the owner's quota) | BLOCKED (no logins) | PASS — Physical Windows (earlier) | Handoff item |

## Adversarial regression tests (security list, items 1–18)

| # | Threat | Main tests | Linux | Windows CI | Gap |
|---|---|---|---|---|---|
| 1 | Host-file access from agent-executed code | `agent_sandbox::*`, `claude_real::…when_contained`, `execution_tests::*`, policy | PASS — Linux | PASS (policy; isolation refused) | On Windows, approved actions run on the host |
| 2 | Secrets via environment or mounts | `execution_tests::secrets…`, `acp.test.ts` env stripping, `dryrun::secret…`, `hardening_tests::secrets…` | PASS — Linux | partial | Contained agents and trusted-local checks inherit the environment |
| 3 | Network egress | `execution_tests::…network…`, `providers::non_loopback…` | PASS — Linux | partial | No egress control for agents |
| 4 | Project hooks and MCP at startup | `claude_real::a_hostile_repository…` (mutation-checked), `claudeOptions.test.ts`, `mcp::*`, `review_hardening::a_hostile_git_hook…` | PASS — Linux | PASS — Windows CI | — |
| 5 | Approval replay or request mutation | `approvals::*`, `regressions::a_reask…` | PASS — Linux | PASS — Windows CI | — |
| 6 | Path redirection (links, junctions, 8.3, streams) | `paths::*`, `policy::protection_follows…`, `windows::a_junction_swapped_in…`, ACP root spellings | PASS — Linux | PASS — Windows CI | — |
| 7 | Git config, filters, hooks | `review_hardening::*`, `git::a_worktree_redirected…` | PASS — Linux | PASS — Windows CI | — |
| 8 | Untrusted test scripts | `execution_tests::*`, `phase2::a_check_that_modifies…`, adapter tool-missing | PASS — Linux | PASS (trusted_local) | Checks on Windows are not isolated |
| 9 | Cross-worktree access | policy, `parallel_missions…`, sandbox binds | PASS — Linux | PASS — Windows CI | Reads outside ask (high risk); not blocked |
| 10 | Process escape and orphans | `regressions::a_descendant…`, `proc::*`, `verify::timeout_kills…` | PASS — Linux | PASS — Windows CI (Job Objects) | `setsid` on Unix |
| 11 | Restart with pending approvals | `restart_recovery…`, `upgrade::…` (every pending approval expires) | PASS — Linux | PASS — Windows CI | Real app restart: handoff |
| 12 | Agent crash mid-change | `agent_crash_fails…` | PASS — Linux | PASS — Windows CI | No crash between writes of a multi-file change |
| 13 | Provider sandbox misconfiguration | `codex.test.ts` (simulated), ACP `*_once` only | PASS — Linux | PASS — Windows CI | Real Codex sandbox on Windows untested |
| 14 | Injection through memory or docs | `service_tests::hostile_note…`, `vault_fixture_tests` wrong-page links | PASS — Linux | PASS — Windows CI | Repository text still steers the model |
| 15 | Malicious provider events | `normalize.test.ts`, `regressions::an_unreadable_engine_line…`, store tests | PASS — Linux | PASS — Windows CI | — |
| 16 | Partial writes and crash recovery | harness, knowledge, checks, merges, upgrade | PASS — Linux | PASS — Windows CI | No SQLite power-loss test |
| 17 | Unsafe worktree clean-up | `discard_refuses…`, `integrity::*`, `windows::discarding_while…` | PASS — Linux | fix pending (see above) | — |
| 18 | Upgrade and migration | `upgrade::*`, `db::tests` | PASS — Linux | PASS — Windows CI | Installer upgrade not tested |

## Not run anywhere

- Any live provider mission on this branch: Claude or Codex with a real account.
- Trivy and Playwright executed as FORGE checks; only their real reports are parsed.
- The Tauri window on Windows for this branch's screens.
- Obsidian desktop with a FORGE-managed vault.
- Windows Sandbox and isolated WSL execution (not implemented).
- Installer upgrade over an older install; signed installers.
