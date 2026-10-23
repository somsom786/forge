# FORGE — Roadmap

Status key: ✅ done & verified · 🟡 partly done · ⬜ not started. Detail and evidence live in PROGRESS.md.

## Phase 1 — Working daily driver (P0) ✅
Core, Claude adapter, desktop UI done and verified in the real window. 🟡 Installer built, **not install-tested**.

## Phase 2 — Verification (P0) ✅
Contracts, check runner, review with fingerprint-bound approval, Proof Pack, safe merge. Verified in the real window.

## Phase 3 — Memory & Obsidian (P0) ✅
Knowledge store, vault boundary, verified-only retrieval, inspector, lint. Verified in the real window; Obsidian opening untested (not installed).

## Phase 4 — Harness Studio (P1) ✅
Inventory, Doctor, safe edit with preview/backup/rollback, hook templates and dry-run. Verified in the real window and against real Claude Code.

## Phase 5 — Multi-agent (P1) 🟡
✅ Codex adapter via `codex app-server` (real mission verified; independent review 5) · ✅ parallel worktrees + concurrency limit ·
⬜ conflict detection between missions · ⬜ independent review missions.

## Phase 6 — Experiments (P2)  ⬜
## Phase 7 — Enterprise extensions (future)  ⬜ — nothing implemented; no claims.

## Deferred / explicit non-goals for now
Embeddings (only if FTS5 measurably under-performs on real queries), ACP (no concrete benefit identified yet),
MCP integrations, CI providers, remote runners, auto-updater, macOS/Linux packaging.
