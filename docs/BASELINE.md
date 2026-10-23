# FORGE — Implementation baseline (audit, 2026-10-23)

Purpose: an honest starting point for the "open-source powerhouse & security hardening" programme. It records what works, with what
evidence, what the open pull requests change, and where the code falls short of what the product directive asks. Re-run the
commands below to confirm; do not trust this file over a test run.

**Where this was produced:** a Linux cloud container (no Windows desktop, no provider logins), plus GitHub Actions on
`ubuntu-24.04` and `windows-2022` runners. "Windows CI" below means a clean GitHub-hosted Windows VM, **not** the owner's
machine, not a real WebView2 window, and no real provider.

## 1. What works (evidence)

| Area | Status | Evidence |
|---|---|---|
| Mission Control: worktrees, supervisor, policy, approvals, cancel, restart recovery | Works | Live missions on the owner's Windows machine (Claude, local model, Codex) per PROGRESS.md; 320+ Rust tests on Linux and Windows CI (`cloud/integration`) |
| Contracts, check runner, fingerprint-bound review, safe merge (`commit-tree` + CAS), Proof Pack | Works | real-window E2E on the owner's machine; `merge::tests`, `review_hardening`, `phase2` on both CI platforms |
| Memory: content-hash trust, verified-only injection in the user turn, Obsidian vault, context inspector | Works (Obsidian opening untested) | `knowledge::*` tests on both platforms; owner's live gate |
| Harness Studio: inventory, Doctor, previewed/CAS/journaled/backed-up edits, rollback, hook dry-run | Works | `harness::*`, `supervisor::tests::harness` on both platforms |
| Codex adapter (`codex app-server`) | Works for the happy path on the owner's machine; hardening in PR #1 unit-tested only | engine tests (61) on both platforms |
| Process containment | Windows: Job Objects (CI-tested). Linux/macOS: **was broken on `main`** (stop never killed the agent); fixed in PR #5 | `stopping_a_hung_agent…`, `proc::unix_tests` |
| Experiments, conflict detection, Agents/Experiments UI | Backend + UI on branches only (#3, #4, #6) | unit/integration tests; UI only in the browser preview |

Commands (repository root): `npm ci`, `npm run build -w @forge/engine`, `npm run test -w @forge/engine`,
`npm run typecheck -w forge-desktop`, `npm run test -w forge-desktop`, `cd apps/desktop/src-tauri && cargo test --lib`.
Live tests are `#[ignore]`d and never run in CI.

## 2. Open pull requests (Stage A review)

All six are by this agent. None is merged into `main`; `cloud/integration` = `main` + #1–#5 with conflicts resolved, green on
Linux and Windows CI at `17fa5e8`.

| PR | Applies to main | Overlap / conflicts | Security effect | Tests | Platform notes | Verdict |
|---|---|---|---|---|---|---|
| #1 Codex hardening | yes | docs only (PROGRESS/SECURITY/ENGINEERING_REVIEWS) | Tightens: refuses broader-than-requested effective sandbox/approval settings, scopes approvals to thread/turn/item, aborts on ungated items | 25 Codex engine tests; Windows CI caught and fixed a test that only passed on Linux | needs one real Codex run on Windows (quota) | Merge |
| #2 Harness hardening | yes | `proc.rs` with #5 (resolved in integration) | Tightens: exclusive create for new files, suspended dry-run start (Windows), backup retention with protection rules | 4 new; Windows code now **run** in Windows CI via integration | — | Merge |
| #5 Unix containment + CI + 8.3 paths | yes | `proc.rs` with #2 | **Fixes a real bug** (stop did not kill agents on Unix) and a real Windows bug (8.3-spelled roots denied all writes; vault-inside-FORGE-home check compared different spellings) | Linux 295/295; Windows CI green | — | Merge first |
| #3 Experiments backend | yes | supervisor/lib/commands with #4 (resolved) | New code execution path (user command × revisions); explicit action only; runs **unsandboxed** like checks | 16 Rust tests incl. migration 7 from every version | — | Merge after #5 |
| #4 Conflict detection | yes | as above | Read-only; one real bug found and fixed (git error read as conflict) | 6 tests | CRLF repos untested | Merge after #5 |
| #6 Product UI (stacked on integration) | via integration | — | Adds `refs/forge/snapshots/*` refs in user repos | UI 110, Rust 323 | preview-only UI | Merge last |

Recommended order: #5 → #1 → #2 → #3 → #4 → #6 (or merge `cloud/integration` then #6). The owner merges; this agent does not.

## 3. Documentation vs. code (findings)

* `docs/PROGRESS.md` said engine tests = 45; `main` has 52.
* SECURITY.md said Linux/macOS sessions "rely on the engine exiting plus an explicit tree kill"; the tree kill was never reached
  (fixed in #5).
* No `LICENSE` file exists. The repository is public but **not** open-source licensed; see §5.
* Approvals are bound to `tool_use_id` + digest(tool, canonical input) and expire with the session. They are **not** bound to a
  policy revision or an execution backend (the directive asks for both). Recorded as Stage B/later work.
* `open_worktree_in_editor` resolves `code` from PATH (on Windows that is `code.cmd`, run through `cmd.exe`); the argument is a
  FORGE-generated path, so not injectable today, but it is the one native command built from a stored path.

## 4. Highest-priority gaps (ordered)

1. **Nothing FORGE runs is isolated from the host.** Checks, experiment runs, hook dry-runs and every approved agent shell
   command execute with the user's full privileges, home directory, credentials and network (SECURITY.md limits 1, 4, 14).
   → Stage B: `ExecutionBackend` with one real isolated backend for checks/experiments first.
2. Approval binding lacks policy revision / backend / explicit expiry.
3. No supply-chain checks in CI (dependency audit, secret scan, licenses, SBOM).
4. No license decision.
5. Agent processes themselves (Claude Code / Codex) run on the host; only their tool calls are gated. Isolating the agent needs
   provider credentials inside a sandbox, which conflicts with "never copy provider tokens"; this needs a design decision.

## 5. Licence options for the owner (not applied)

FORGE has no licence, so nobody may legally reuse it. Options: **Apache-2.0** (permissive, patent grant, common for
commercial open core), **MIT** (permissive, minimal), **AGPL-3.0** (strong copyleft incl. network use; keeps forks open; harder
for some commercial users), or **source-available/proprietary** (e.g. BSL / FSL) if commercial control matters more than
adoption. Third-party notices are needed whatever is chosen (see OSS_ADOPTION.md). The owner must decide.
