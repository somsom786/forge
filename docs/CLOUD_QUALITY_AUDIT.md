# Cloud quality audit

An evidence-backed assessment of `cloud/overhaul` (PR https://github.com/somsom786/forge/pull/8) at the end of the cloud
programme, 2026-10-24. Scores are 0–5:

- **5**: done and proven where it matters, including the target platform.
- **4**: solid, with known gaps.
- **3**: works and is tested in the cloud, with significant unverified areas.
- **2**: partial.
- **1**: a sketch.

Nothing scores 5: no area has been validated on a physical Windows PC on this branch.

Test inventory at the end of this round:

- Rust core: 435 passing, 6 ignored. The ignored ones are live tests that need accounts or local models.
- Engine: 88 passing, 3 skipped. The skipped ones are ACP interop tests that run when their agent is installed; CI requires them.
- UI: 126 unit and component tests.
- 6 browser end-to-end tests, labelled simulated.

All of this runs on `ubuntu-24.04` and `windows-2022` in CI, except the e2e tests (Linux only) and the sandbox and
scanner tests (Linux only, required there).

## Scorecard

### Security — 3/5

- **Condition:**
  - Every agent request goes through one Rust authorization path. Policy is re-checked at decision time.
  - Approvals are bound to a digest of the exact request, its policy revision and where it executes. They expire, are reused at most once, and are never reused for a changed request.
  - MCP is hash-pinned and allow-listed. Repository hooks and MCP are locked down in real Claude Code.
- **Issues found and fixed this programme:**
  - **P0:** a dangling-link escape (`paths::resolve_within`).
  - Approval replay and post-completion answers.
  - Commands whose redacted text hid their payload.
  - Unicode look-alike protected paths and home/variable shorthands.
  - ACP replay, pathless reads and title-as-command.
  - A sandbox exposure through MCP/ACP arguments (P1, final review).
  - Unvalidated provider state folders.
  - Glob patterns reaching outside the worktree.
  - Full list: `docs/SECURITY.md` and the commit log.
- **Evidence:**
  - Adversarial tests for items 1–18 of the security list (`docs/VALIDATION_MATRIX.md`).
  - Real Claude Code against a hostile repository, mutation-checked.
  - A Windows junction swapped in at decision time is denied (Windows CI).
  - Two independent reviews each round; findings fixed with regression tests.
- **Remaining:**
  - On Windows, and by default on Linux, an approved action runs as the user.
  - The webview is trusted.
  - Shell rules are heuristics.
  - Contained agents keep network access.
  - Agent-configuration edits are not flagged for review (`docs/REMAINING_RISKS.md`).
- **Readiness:** suitable for supervised use on the user's own repositories. Not for running agents on untrusted repositories on Windows.

### Architecture — 4/5

- **Condition:**
  - Privileged logic lives only in Rust: policy, approvals, git, store, isolation and evidence.
  - The Node engine is stateless and can only request.
  - JSON-lines protocol with bounded lines; forward-only migrations (15), with the shipped ones pinned by hash.
- **Issues found and fixed:**
  - Column-list drift between session queries (now one list).
  - A database written by a newer FORGE is refused, not misread.
  - Removal paths unified in `remove_mission_worktree`.
- **Evidence:** upgrade simulation from the released v6 schema with real data; restart-recovery and crash-window tests.
- **Remaining:**
  - `supervisor.rs` is large (about 1,400 lines plus submodules).
  - An older FORGE opening an upgraded database still ignores newer settings: the refusal only exists from now on.
- **Readiness:** good foundation.

### Agent integrations — 3/5

- **Condition:**
  - Claude Code (Agent SDK, lockdown).
  - Codex (app-server).
  - ACP (official SDK), tested against a real third-party agent (`claude-agent-acp`).
  - Scripted test agent.
  - Linux containment of the agent process.
- **Issues found and fixed:**
  - Windows 8.3 path mismatch in ACP file checks.
  - The scripted model truncated Windows paths.
  - Billing-variable stripping for ACP.
  - Ungated Codex commands are flagged.
- **Evidence:**
  - The real Claude Code binary against a scripted Anthropic-compatible server: approval-gated write, escape denied, hostile repository, real MCP server, contained approved command.
  - ACP interop on Linux and Windows CI.
- **Remaining:**
  - No live provider mission on this branch (by design: no accounts in the cloud).
  - Codex is tested against a scripted app-server only.
  - Other ACP agents are untested.
- **Readiness:** needs the live checks in the Windows handoff.

### Memory — 3/5

- **Condition:**
  - Verified-only injection, bound to content hashes.
  - Untrusted frontmatter; read-only vault by default.
  - Lint for duplicates, staleness and topic conflicts.
- **Issues found and fixed:**
  - Non-ASCII wikilinks resolved to the wrong note (slug collisions).
  - Read-only notes were rewritten or deleted.
  - Sync-conflict copies were not called out.
- **Evidence:**
  - Fixture vault tests: link forms, NFD names, renames, deletions, conflicts, read-only files.
  - Retrieval evaluation on real FAQs: ripgrep hit@3 0.79 (`docs/EVALUATION.md`).
- **Remaining:**
  - Lexical retrieval misses paraphrases.
  - No real Obsidian app test.
- **Readiness:** usable; retrieval quality is modest.

### Code intelligence — 3/5

- **Condition:** tree-sitter declarations index for four languages, Code Map, relevant code in context.
- **Issues found and fixed:** CommonJS/prototype assignments were not extracted. Express hit@3 went from 0.13 to 0.57.
- **Evidence:**
  - Commit-localization evaluation on nine pinned repositories, five development and four held out. The index matches or beats grep on MRR in 8 of 9.
  - A fusion that lost on the held-out set was rejected.
  - A 20k-file index builds in about 0.5 s.
- **Remaining:** no references or bodies; test files rank high.
- **Readiness:** helpful pointers, honestly labelled.

### Proof Engine — 4/5

- **Condition:**
  - Criteria linked to checks; freshness against the exact diff.
  - gitleaks, OSV-Scanner, Trivy and Playwright adapters, evaluated on real tool output.
  - Proof Packs, hashed, now recording where the agent ran.
  - A summary strip in the UI.
- **Issues found and fixed:**
  - The evaluator assumed exit codes the tools don't use.
  - A missing tool vs a findings result.
  - A check runner dying left its slot held.
- **Evidence:**
  - Real gitleaks and osv-scanner run through the check path in CI.
  - Real report fixtures from Trivy and Playwright.
  - The Proof Pack panel has component tests.
- **Remaining:**
  - Trivy and Playwright are not executed in CI, only their real reports parsed.
  - A pack is evidence, not a guarantee.
- **Readiness:** strong.

### Harness Studio — 3/5

- **Condition:** inventory, Doctor, previewed and journaled edits with rollback, hook templates and dry-runs.
- **Issues found and fixed:**
  - Settings precedence and permission conflicts are now explained.
  - Findings no longer point at the user's global file.
- **Evidence:** 72 harness tests, including crash recovery between journal and write.
- **Remaining:**
  - The live test with real Claude Code and a local model is ignored in CI.
  - Dry-runs have not run on a Windows desktop.
- **Readiness:** good in the cloud; needs Windows validation.

### UX — 3/5

- **Condition:** every screen and the mission workspace pass WCAG A/AA checks (axe) at the minimum window size, with keyboard paths tested.
- **Issues found and fixed:**
  - Low-contrast text on every screen.
  - Keyboard-unreachable scroll regions.
  - The command palette ignored visible labels.
  - Heading order.
  - An empty table header.
  - The 500-mission cap was not stated.
- **Evidence:** six simulated Playwright tests in CI, mutation-checked.
- **Remaining:**
  - Not verified in WebView2 or with real data.
  - No screen-reader walkthrough by a person.
- **Readiness:** good in a browser; the Windows look and feel is unverified.

### Performance — 3/5

- **Condition:** measured at volumes well above normal use (`docs/PERFORMANCE.md`).
- **Issues found and fixed:**
  - Unbounded growth of transcripts and logs now has a previewed clean-up.
  - Storage usage no longer walks experiment checkouts.
- **Evidence:**
  - 200k events at about 15.7k/s.
  - Recovery over 5k missions.
  - A 3k-note vault.
  - Clean-up at scale; each scenario also asserts correctness.
- **Remaining:**
  - Measured only in the cloud container.
  - No `VACUUM`; experiments are not cleaned up.
- **Readiness:** adequate.

### Testing — 4/5

- **Condition:**
  - Real binaries where possible: Claude Code, an MCP server, an ACP agent, scanners, bubblewrap.
  - Hosts as controls for isolation tests; mutation checks on the critical tests.
  - Linux and Windows CI with pinned actions.
- **Issues found and fixed:** six real defects this round came from new tests rather than from review:
  - Windows half-deleted worktree.
  - Wrong-page wikilinks.
  - Read-only notes.
  - CommonJS symbols.
  - Pending approvals surviving restart.
  - Windows path truncation in a fixture.
- **Remaining:**
  - Physical Windows validation.
  - No fuzzing.
  - UI e2e runs against mocks only.
- **Readiness:** strong for a pre-release.

### Release engineering — 2/5

- **Condition:**
  - CI builds an unsigned NSIS installer with a SHA-256.
  - The security workflow runs audits and produces SBOMs.
  - Third-party notices are generated and shipped in the installer.
- **Issues found:**
  - The installer shipped no licence notices (fixed).
  - It bundles the proprietary Claude Agent SDK JavaScript, with redistribution unconfirmed.
- **Remaining:**
  - No licence.
  - No code signing.
  - No update channel.
  - Installer upgrade over an older install untested.
- **Readiness:** not releasable until the licensing items and signing are resolved.

## Final security and quality review (section 18)

Answers are as of this commit. "Mitigated" means a test demonstrates the protection; "open" points to
`docs/REMAINING_RISKS.md`.

1. **Can an untrusted agent bypass authorization?**
   - Not through FORGE's request path. All decisions are made in the core, bound to the exact request and re-checked at decision time. The engine cannot approve anything.
   - The tests include replay, re-asks with changed input, answers after completion, redaction-hidden commands and link swaps (Unix links; a Windows junction on Windows CI).
   - Open: an *approved* shell command runs as the user (except in a contained agent on Linux), and shell rules are heuristics.
2. **Can an MCP server gain unauthorized capabilities?**
   - Only authorized, hash-pinned definitions reach missions, and calls outside the allow-list are denied by the core. ACP missions never receive allow-listed servers they could bypass.
   - Open: an authorized stdio server runs with the user's privileges, inside the sandbox only when the agent is contained.
3. **Can a malicious repository influence privileged execution?**
   - Repository hooks, `.mcp.json`, settings that widen permissions, git hooks and external git filters do not run. This is tested against real Claude Code and real git.
   - Open:
     - Repository text (`CLAUDE.md`, rules) still steers the model.
     - Changes an agent makes to those files are not singled out for review.
4. **Can stale verification evidence be reused?**
   - No. Results are bound to the diff fingerprint, the check definition revision and the contract hash.
   - Editing a check, the changes or the criteria voids the earlier result or approval (tested).
5. **Can the database become inconsistent after interruption?**
   - Transitions are transactional, and merges are journaled and reconciled.
   - Restart recovery covers sessions, missions, approvals (now all pending ones), checks, knowledge writes, harness edits and experiments.
   - Open: no SQLite power-loss test.
6. **Can worktrees be deleted incorrectly?**
   - Deletion requires the path to be inside FORGE's worktrees folder, not a link, verified as the mission's worktree, and the loss confirmed.
   - On Windows a worktree with a file in use is refused before anything is deleted, and a half-deleted one can be finished. That fix came from a failure in this round's Windows CI and has a test.
   - Storage clean-up never deletes worktrees.
7. **Can memory silently become authoritative?**
   - No. Trust lives in FORGE's review records, bound to content hashes; frontmatter cannot grant it.
   - Edited pages drop back to unverified. Only verified or pinned pages are injected, labelled as data.
   - Wrong-page link resolution was found and fixed.
8. **Can secrets leak through logs or subprocess environments?**
   - Partly mitigated:
     - Redaction of logs and events.
     - Billing variables are stripped for ACP.
     - Secret-looking environment is scrubbed for hook dry-runs and isolated checks.
     - gitleaks evidence never stores secret material.
   - Open:
     - Redaction is pattern based.
     - Trusted-local checks and contained agents inherit FORGE's environment.
9. **Can an agent escape the documented execution restrictions?**
   - On Linux with a contained agent: not to the host filesystem beyond the documented binds (tested with host controls and real Claude Code; broad binds are refused).
   - Elsewhere the documented restriction is policy plus approval, not a sandbox, and that is stated everywhere.
   - Open: network egress; `setsid` escaping the Unix process group.
10. **Can an unavailable provider leave the UI in a false state?**
    - Providers are re-detected and capability-reported.
    - A missing provider or backend fails the start with a reason and leaves no session behind.
    - Crashes surface as failed or interrupted with diagnostics (tested).
    - Live-provider behaviour on Windows is a handoff item.
11. **Can event streams overwhelm the frontend or database?**
    - Engine lines are capped at 16 MiB, and an unreadable line ends the session.
    - Events are paginated (first page in under 1 ms at 200k events).
    - Logs are capped per run; transcripts have a clean-up.
    - Open: no UI rendering stress test with a very long live transcript.
12. **Can unsupported platform features appear enabled?**
    - No. Unimplemented backends are reported as such and cannot be selected.
    - Agent isolation is refused on Windows and never falls back (tested on Windows CI).
    - Scanners report "tool missing" instead of passing.
13. **Are migrations safe for existing users?**
    - Forward-only and transactional, with the shipped migrations hash-pinned.
    - Upgrading from the released v6 schema with data is tested.
    - A database from a newer FORGE is refused.
    - Open: downgrade to a build older than this change.
14. **Are new open-source dependencies maintainable and legally compatible?**
    - Each is recorded in `docs/OSS_ADOPTION.md` with its licence and reason, and all are permissive.
    - New this round: `unicode-normalization` (runtime), and `@playwright/test` and `@axe-core/playwright` (MPL-2.0, dev only).
    - The only incompatibility is FORGE's own missing licence plus the proprietary SDK redistribution question (`docs/LICENSING_DECISION.md`).
15. **Are all claimed features actually implemented?**
    - The README and validation matrix claim only what tests demonstrate, and label simulations and mocks.
    - Designed-but-unbuilt items (Windows Sandbox and WSL backends, enterprise features) are labelled as such.
    - Claims that need live or physical validation are marked NOT RUN or BLOCKED.
