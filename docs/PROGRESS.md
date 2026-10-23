# FORGE — Progress

Evidence levels: **A** = verified by a real run against the real thing (real window / real agent / real git); **B** = automated tests only;
**C** = implemented, not verified; **—** = not built. Last full run: 2026-10-23. Rust 295 passing (6 live tests ignored by default), engine 45,
UI 100 (vitest); `npm run typecheck` clean. Live tests that spend provider quota are never run in bulk (see `codex-handoff.md`).

| Area | Level | Evidence |
|---|---|---|
| Mission Control: worktrees, supervisor, policy, approvals, cancel, recovery, process containment | A | real Claude mission (haiku), real local-model mission (qwen3.5:9b via the Claude Code harness), real Codex mission (gpt-5.6-luna); real-window flow `tools/e2e/mission-flow.mjs` |
| Mission Contracts, check runner, review (fingerprint-bound approval), safe merge, Proof Pack | A | `mission-flow.mjs` incl. Proof Pack appearing after merge; 3 independent reviews fixed |
| Memory: managed folder / Obsidian vault, trust bound to content hash, verified-only injection in the first user turn, context inspector | A | `tools/e2e/knowledge-flow.mjs`; live gate with a real local agent (with memory → private facts correct; without → hallucinated). **Obsidian opening on a machine with Obsidian installed: not tested** (not installed here) |
| Harness Studio: inventory, Doctor, previewed/CAS/journaled/backed-up/reversible edits, hook templates, dry-run, history/rollback | A | `tools/e2e/harness-flow.mjs`; live: a hook and an instruction written through FORGE fired/obeyed in real Claude Code; review 4 fixed (2 items open, see below) |
| Codex adapter (`codex app-server`) | A for the happy path and the refusal, B otherwise | real mission: FORGE approved a write, file created in the worktree, turn succeeded; hostile `.codex/config.toml` refused before anything starts; engine tests (25 Codex tests). review 5 fixed; follow-up on `cloud/codex-hardening` (Linux, unit tests only): effective sandbox/approval settings recorded and constrained, approval scope validated, ungated items abort, taskkill sentinel test. Open: allow-rules refuse the session unless `FORGE_CODEX_ALLOW_USER_RULES=1` |
| Parallel missions + concurrency limit | B | tests with the scripted agent |
| Windows installer (`npm run package`, NSIS, per-user, bundled Node + engine bundle) | A | silent install to a scratch folder, launched, a full scripted mission through the real window incl. merge and Proof Pack; the engine ran on the bundled Node with Node removed from PATH (a user's own *check* commands still need their tools on PATH). Uninstalled afterwards. Not tested: upgrade over an older install, code signing (unsigned: SmartScreen will warn) |
| Experiments (Phase 6) | — | |
| Enterprise (Phase 7) | — design only | no claims made |

## Platforms

Windows is the product platform; every level-A row above was verified on the owner's Windows 11 machine. The Rust suite also passes
on Linux since `cloud/unix-containment` (before it, stopping an agent on Linux/macOS did not kill it: the Unix containment was an
empty placeholder). CI (`.github/workflows/ci.yml`) runs engine, UI and Rust tests on `ubuntu-24.04` and on a GitHub-hosted
`windows-2022` runner; a green Windows CI run is evidence on a clean Windows VM, not on the owner's machine (no WebView2 window, no
providers, no installer). No macOS build or test exists.

## Honest list of what is not verified or not done

* Installer: unsigned; no auto-update; upgrade-over-existing not tested.
* Obsidian live opening, Obsidian write-back with a real vault in use.
* Codex: the effective-settings refusal and approval-scope checks (`cloud/codex-hardening`) are tested only against a scripted app-server on Linux; the Windows `taskkill` branch is tested with an injected spawn, not on Windows; no real Codex run with every permission denied. Resume of a thread, command approvals through FORGE (verified with a probe against the real app-server, and by unit tests, but not in a FORGE mission), Windows sandbox behaviour (`workspace-write` is requested; whether the user's `elevated` sandbox actually confines a command was not tested), usage numbers.
* Harness: handle-pinned write path (a concurrent local process could win a microsecond race), dry-run processes are not created suspended, backups are never pruned.
* The mission workspace tab strip scrolls horizontally with 7 tabs; it was not re-screenshotted after the last change.
* Multi-agent review missions, conflict detection between concurrent missions, experiments.

## History of findings by running things (not by reading code)

Verbatim engine path crash (`\\?\` and Node); `window.confirm` blocked by Tauri; stale Proof Pack tab; a shipped hook template that did not run in Git Bash;
a stray wrong-project selection in an E2E script; Codex project MCP servers running although Codex calls project config untrusted; `config/read` in a git
worktree omitting servers that `thread/start` still starts. Independent reviews (docs/ENGINEERING_REVIEWS.md): 1 Phase 1 core, 2 memory design, 3 verification/merge/memory, 4 Harness Studio, 5 Codex adapter.
