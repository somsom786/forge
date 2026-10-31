# Validation matrix

2026-10-31: PASS — Windows verification suite, 12 tests, including refusal to execute when its requested log cannot be created. Linux CI pending.
2026-10-31: PASS — Windows verification suite, 11 tests after incomplete-output handling. The inherited-pipe fixture did not reproduce on Windows and is restricted to Unix; Linux CI is required for that regression.
2026-10-31: PASS — Windows verification tests, 11 passed; includes unavailable report path, capped valid-JSON-prefix report, and frozen completed buffer. Linux CI pending. Test subprocesses hidden and below normal priority.
2026-10-31: PASS — Windows verification tests, 8 passed, including timeout and cancellation; Windows suspended-process Job Object test, 1 passed. Hidden, below-normal-priority subprocesses. Linux and Windows CI for this change pending.
2026-10-31: PASS — Windows execution tests, 7 passed, including capability-reporting assertions. This verifies reporting only; it does not validate Linux enforcement.
2026-10-31: PASS — Windows provider tests, 8 passed, including oversized and slow-trickle loopback server fixtures. Linux CI pending. No GPU or provider account used.
2026-10-31: PASS — Windows targeted provider tests, 7 passed, including 200 KB of CLI output and a descendant retaining stdout beyond the deadline. Hidden, below-normal-priority test process; no provider account or GPU used. Linux CI for this change: pending.

## Continuation, 2026-10-31

Git metadata mount validation: PASS — Windows, 7 portable execution tests, including forged pointers and a legitimate linked worktree. The forged-pointer regression failed before the fix. PASS — Linux and Windows CI at `4aaca0e`, including required Linux sandbox enforcement and Unix fixtures; the unsigned installer and security checks also passed. This validation does not establish Windows agent isolation.

## Continuation, 2026-10-28

Installer uninstall polling: FAIL — Windows CI run `40718667010`: the application stayed up for 180 seconds, the install folder was removed, but the test observed the uninstall registry entry immediately afterward. The generated NSIS script deletes that entry after the executable. The updated validation waits for both conditions and still fails on a persistent entry. PASS — Windows PowerShell 5.1 and PowerShell 7 fixtures for delayed removal, persistent entry, and remaining executable; PASS — Windows CI at `46836d6` for the real 180-second launch and uninstall. This evidence suggests a test race but does not establish whether the particular earlier registry entry would eventually have disappeared.

Continuation PR #9 at `46836d6`: PASS — Linux CI, 455 Rust tests passed and 6 ignored; PASS — Windows CI, 442 passed and 6 ignored; PASS — Windows 180-second unsigned installer launch and uninstall, script checks, dependency and secret scans, and SBOM generation.

Cleanup preview freshness: PASS — Windows, 13 storage tests and 1 focused desktop component test; PASS — TypeScript type checking. The new same-size orphan rewrite failed before the fix and passed afterward; a modified mission log also invalidates its preview. The token checks metadata, not file contents, and cannot prevent concurrent path swaps. Linux CI is pending for this revision.

Cleanup preview detail: PASS — Windows targeted component test and TypeScript type checking; fixture checks that mission and unreferenced log paths are present before confirmation. This is a mock-backend UI test, not a real-desktop cleanup run.

Storage failure display: PASS — Windows desktop tests, 128 passed, including a fixture exercising visible failure details and their removal after successful retry; PASS — TypeScript type checking. No GPU or visible subprocess window used.

2026-10-28: PASS — Windows, 12 storage tests, including a real locked mission-log fixture. Before the fix the new regression failed because the transcript was deleted despite the locked log; after the fix it passed, preserved the log reference, and succeeded after release and a fresh preview. Temporary repositories only; hidden, below-normal-priority subprocesses; no GPU or provider account used. Linux CI for this change is pending.

## Continuation, 2026-10-27

Obsidian opening: PASS — Windows URI encoding regression; PASS — Linux CI desktop-handler lookup fixture (success, missing, failure, timeout), NOT RUN locally. Real Linux Obsidian/desktop-handler integration is NOT RUN.

Toolchain mount path filtering: PASS — Windows, 1 portable targeted test including an unverifiable home; Unix symlink-to-home and ancestor regression is present but NOT RUN locally and must pass Linux CI. The change may omit custom toolchains when the home is unset or cannot be resolved.

Agent skill path classification: PASS — Windows, 1 targeted regression including skill instructions and helper scripts under nested, case-varied `.agents/skills` paths. It feeds the existing fingerprint-bound review gate.

Worktree lock-probe collision regression: PASS — Windows, 1 targeted test in a temporary directory; no test subprocess window or GPU was used. The Unix no-rename branch requires the corresponding Linux CI result.

External Claude SDK follow-up: engine 90 passed, 5 skipped; copied bundle runs its scripted provider without SDK, and a Claude start with a missing SDK fails visibly. SDK metadata/version test: 1 passed. Real Claude Code against a scripted local model with `FORGE_REQUIRE_CLAUDE_REAL_TESTS=1`: 4 passed on Windows (no account/quota/GPU). Package staging succeeded; esbuild inputs exclude Anthropic SDK code. An intermediate unsigned installer built, installed and uninstalled in a temporary location (6 checks passed); it predates subsequent code changes. Installer launch/upgrade with separately installed SDK remains NOT RUN.

Storage follow-up: `cargo test --lib --locked storage::` on Windows: 11 passed. Includes experiment artifacts, active-run exclusion, unexpected worktree preservation, junction refusal, locked log retry, durable audit failure, interrupted cleanup, explicit idle-only SQLite compaction on a real temporary database, and lock-order regression. UI 127 passed and typecheck passed. No real user artifacts were deleted.

Schema v16 follow-up: review-hardening tests 12 passed; database tests 5 passed on Windows. New migration preserves legacy review notes while defaulting explicit instruction acknowledgement to false. Older schemas are tested through every forward migration; newer schemas continue to be refused.

Execution hardening follow-up: Windows targeted `execution::tests`: 4 passed; UI 127 passed; typecheck and engine build passed. Linux end-to-end environment isolation regression added but NOT RUN locally (the existing WSL distribution has no bubblewrap or Rust). The test is required through the existing sandbox CI setting. Agent capability reporting now distinguishes shared network/provider state from isolated checks.

Linux component probe: PASS on Ubuntu 24.04.4 under WSL using a temporarily extracted Ubuntu bubblewrap 0.9.0-1ubuntu0.3. Harmless host-control and isolated read/write/environment/network fixtures and scripted engine cancellation passed. This is not the Rust supervisor test and is not a Windows isolation backend. Simulated browser E2E: 6 passed using headless Chrome with GPU disabled; not a real Tauri integration test.

Full Rust suite: PASS on the available Windows host, 439 passed and 6 ignored, `FORGE_REQUIRE_CLAUDE_REAL_TESTS=1` with scripted local model. Some platform-specific cases return early and are not Windows/Linux enforcement evidence. Installed Windows app scripted mission: PASS for mission creation, approval, actual diff, check, review, merge and Proof Pack using isolated temporary data and WebView2, with no console errors. Installer test: FAIL overall: six checks passed, including unsigned build, hashes, temporary install, manifest and uninstall, but the 180-second launch check observed a clean process exit after 63.5 seconds. Cause unknown. The test artifact was built from commit `0531e67` with uncommitted documentation/workflow changes; installer SHA-256 `95795d785d28624ff90a0fbbac3a48cb57cbe0b167a2f2178fccd0c297a40259`. It is not a signed or published release.

Local Windows environment (not CI; whether the host is physical was not independently established): Node 24.19.0, npm 12.2.0, Rust 1.92.0 MSVC. `codex/overhaul-continuation` starts at `8c3bcc9`. No GPU workloads or live paid provider missions were run.

GitHub PR #8 checks queried 2026-10-27: Linux and Windows tests, Windows unsigned build, Windows script checks, dependency/security scans and SBOM jobs reported PASS on the PR's existing revision. These are stale for the continuation commits.

Continuation PR #9, commit `c1aa777`: PASS — Linux CI (451 Rust passed, 6 ignored; required sandbox, scanner and scripted Claude CLI checks enabled), PASS — Windows CI (439 Rust passed, 6 ignored), and PASS — unsigned installer build and Windows script checks. Security, dependency and SBOM jobs passed. Follow-up commit `50c17a9`: PASS — Windows CI installed the unsigned app into temporary storage, kept it running for 90 seconds with GPU disabled, then uninstalled it. This does not explain or invalidate the earlier local 180-second failure.

Continuation PR #9, commit `77b088c`: PASS — Linux CI (453 Rust passed, 6 ignored), Windows CI (440 Rust passed, 6 ignored), security and dependency checks, and unsigned installer build. The clean Windows runner installed, launched for 180 seconds with GPU disabled, and uninstalled the application. The earlier local early exit remains unexplained; this CI pass is not evidence of a real provider, signing, or physical-desktop interoperability.

Continuation PR #9, commit `714679b`: PASS — Linux CI (454 Rust passed, 6 ignored), Windows CI (440 Rust passed, 6 ignored), security and dependency checks, and the 180-second unsigned Windows installer launch and uninstall.

| Check | Result | Evidence and limits |
|---|---|---|
| Initial Windows script | FAIL | First attempt: npm cache EPERM; dependent suites blocked. Retried with a worktree-local cache: one Codex fixture failed due to parent configuration; 425 Rust passed, 6 ignored, and 126 UI passed. Source changed during this run, so it is not a clean final-revision baseline. |
| Engine after fixture fix | PASS — Windows (host type unconfirmed) | 86 passed, 5 skipped; simulated Codex app-server, ACP interoperability not required in this run. |
| UI after review change | PASS — Windows (host type unconfirmed) | 127 passed; unit/component tests, not WebView2 integration. |
| Typecheck | PASS — Windows (host type unconfirmed) | `npm run typecheck`, engine and desktop. |
| Rust core | PASS — Windows (host type unconfirmed) | 427 passed, 6 ignored, 0 failed with four test threads after the tree/diff change. A subsequent path-list expansion was checked by its targeted test (1 passed); the full suite was not repeated after that small expansion. Optional tool tests may return early when tools are unavailable. |
| Instruction review regression | PASS — Windows (host type unconfirmed) | 3 passed; 1 unrelated live test ignored. Includes denial, stale acknowledgement, merge refusal and legacy approval. |
| Formatting | FAIL | `cargo fmt --check` reports extensive repository-wide pre-existing differences; no wholesale formatting applied. `git diff --check` passed. |
| Linux / new Windows CI | NOT RUN | Existing PR #8 CI at `8c3bcc9` was green on 2026-10-24; those results do not cover this continuation. |
| Desktop, actual providers, Windows isolation, installer upgrade | NOT RUN | Automated tests do not establish real UI or provider interoperability; Windows agent isolation remains unimplemented. |

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

**CI evidence.** CI was green on both runners, including the installer build, at `4eb671a`, `e07f9d3`, `7188f8f` and `6e430ac` (the same code as `6626706`). At `96ad0ff` Windows CI failed one new test, which found the worktree defect fixed in `6626706`. The physical-Windows validation of this branch is **NOT RUN** (`docs/CODEX_WINDOWS_HANDOFF.md`).

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
| Worktree removal with a file in use (refused before deleting; half-deleted worktree finished after confirmation) | `windows::discarding_while_a_file_is_locked…`, `integrity::a_worktree_an_earlier_removal…` | PASS — Linux | PASS — Windows CI (failed at `96ad0ff`, which found the bug; fixed in `6626706`, green at `6e430ac`) | NOT RUN | |
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
| 2 | Secrets via environment or mounts | `execution_tests::secrets…`, `acp.test.ts` env stripping, `dryrun::secret…`, `hardening_tests::secrets…` | PASS — Linux | partial | Contained agents use an environment allow-list but have provider state and configured proxy values; trusted-local checks inherit the environment |
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
| 17 | Unsafe worktree clean-up | `discard_refuses…`, `integrity::*`, `windows::discarding_while…` | PASS — Linux | PASS — Windows CI | — |
| 18 | Upgrade and migration | `upgrade::*`, `db::tests` | PASS — Linux | PASS — Windows CI | Installer upgrade not tested |

## Not run anywhere

- Any live provider mission on this branch: Claude or Codex with a real account.
- Trivy and Playwright executed as FORGE checks; only their real reports are parsed.
- The Tauri window on Windows for this branch's screens.
- Obsidian desktop with a FORGE-managed vault.
- Windows Sandbox and isolated WSL execution (not implemented).
- Installer upgrade over an older install; signed installers.
