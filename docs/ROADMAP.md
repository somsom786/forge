# FORGE — Roadmap

Status key: ✅ done & verified · 🟡 partly done · ⬜ not started. Detail and evidence live in PROGRESS.md.

## Phase 1 — Working daily driver (P0)
✅ core (Rust supervisor, store, policy, worktrees, recovery, process containment) · ✅ Claude adapter (live-verified) ·
🟡 desktop UI (in progress) · ⬜ packaged installer.
**Gate:** select a project, run a real mission, see what happened, return later.

## Phase 2 — Verification (P0)
⬜ mission contracts (lightweight → strict) · ⬜ review UX (diff already exists read-only) · ⬜ test/lint/build runner
capturing evidence · ⬜ Proof Pack + requirement→code→check graph · ⬜ approve/reject/request-changes ·
⬜ safe merge (clean tree check, conflict detection, FORGE-authored commit, branch/worktree cleanup).
**Gate:** decide whether a change satisfies explicit acceptance criteria without reading terminal history.

## Phase 3 — Memory & Obsidian (P0)
⬜ sources + hashes · ⬜ Markdown wiki + index/log · ⬜ FTS5 retrieval + ranking · ⬜ context package + inspector ·
⬜ lifecycle (proposed/verified/superseded/disputed/archived) · ⬜ lint (duplicates, contradictions, staleness, orphans) ·
⬜ Obsidian vault (read-only default, scoped writes, conflict display, optional).
**Gate:** a verified discovery from mission A is retrieved with provenance in mission B.

## Phase 4 — Harness Studio (P1)
⬜ inventory · ⬜ hook templates with preview/backup/rollback · ⬜ skills browser · ⬜ Harness Doctor ·
⬜ project-settings trust gate (see SECURITY.md limit 4).

## Phase 5 — Multi-agent (P1)
⬜ Codex adapter via app-server (protocol schema already generated and read) · ✅ parallel worktrees + concurrency limit
(core, tested with the scripted agent) · ⬜ conflict detection between missions · ⬜ independent review missions.

## Phase 6 — Experiments (P2)  ⬜
## Phase 7 — Enterprise extensions (future)  ⬜ — nothing implemented; no claims.

## Deferred / explicit non-goals for now
Embeddings (only if FTS5 measurably under-performs on real queries), ACP (no concrete benefit identified yet),
MCP integrations, CI providers, remote runners, auto-updater, macOS/Linux packaging.
