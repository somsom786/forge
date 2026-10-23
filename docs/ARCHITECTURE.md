# FORGE — Architecture

FORGE orchestrates existing coding agents; it does not reimplement them. It owns orchestration, policy, persistence,
evidence and UX. Providers own the agent loop, tools and model access.

## Process model

```
┌──────────────────────── Tauri app (single window) ────────────────────────┐
│ React UI  ──invoke / forge://event──►  Rust core (src-tauri)              │
│                                          ├ store.rs      SQLite (WAL, FTS5 ready)
│                                          ├ supervisor.rs sessions, approvals, recovery
│                                          ├ policy.rs     allow / deny / ask (pure)
│                                          ├ paths.rs      containment (pure)
│                                          ├ git.rs        Git CLI: status, worktrees, diff
│                                          ├ proc.rs       Job Objects, tree kill
│                                          └ providers.rs  CLI detection, capability matrix
└───────────────────────────────┬────────────────────────────────────────────┘
                                │ JSONL over stdio (protocol.ts)   one process per session
                          ┌─────▼──────┐
                          │ engine     │  Node. Adapter per provider.
                          │ (packages/engine)  claude.ts → Claude Agent SDK → user's `claude`
                          └────────────┘  (codex.ts: planned, Codex app-server)
```

**Why a Node engine at all?** The supported programmatic Claude surface is the Claude Agent SDK (TypeScript); Codex's
rich surface is a JSON-RPC server that is equally easy from either side. Everything privileged — persistence, policy,
approvals, git, process containment — stays in Rust. The engine is deliberately stateless and untrusted-adjacent: it holds no
authority, can only *request* decisions, and dies with its session.

**Why one engine per session?** Cancelling a session is exactly "kill this process tree"; a crash cannot affect other
sessions; Job Objects map 1:1 to sessions.

## Control flow of a mission

1. `mission_create` → row in `ready`.
2. `mission_start`: `ready→preparing`; create worktree `~/.forge/worktrees/<proj>/<slug>-<id>` on branch
   `forge/<slug>-<id>` from the project's current HEAD; spawn engine inside a Job Object; send `start`.
3. Engine emits `ready` → `preparing→running`. Provider events are normalised to `ActivityKind`s; each carries a
   per-session `seq`. Rust redacts, persists (`INSERT OR IGNORE` on `(session_id, seq)`), *then* pushes to the UI.
4. Every tool call → `permission` message → `policy::evaluate`:
   `Allow` (reply), `Deny` (persist `auto_denied`, reply), `Ask` (persist `pending`, mission → `awaiting_approval`, wait).
   Only `decide_approval` (UI command, single-shot) can answer an `Ask`.
5. `turn_complete` → `awaiting_review` (success) / `failed` / `interrupted`. Process exit triggers one `finalize`:
   ends the session row, expires unanswered approvals, drops the Job Object.
6. Follow-up turns resume the provider session in the same worktree (`Preparing` again).

## Honesty rules enforced in code

* A mission is `running` in the UI only while `Core.sessions` holds an attached process (`MissionDetail.running`).
* On startup `Core::recover` marks any mission that should have a live session as `interrupted`; nothing is re-attached
  or pretended.
* Unsupported provider capabilities are `supported:false` in the matrix and must not render as controls.
* Usage figures are provider estimates and labelled so; subscription utilisation appears only from provider
  `rate_limit_event`s.

## Event ordering and idempotency

Engine events have strictly increasing `seq` per session. Core-originated events (approvals, turn completion) use
negative `seq` so the namespaces never collide. Duplicates are ignored by the unique key; display order is the row id.

## Verification, review and merge (Phase 2)

`contract.rs` (pure: scope globs, criterion status derivation) → `verify.rs` (check runner: timeout, tree kill, bounded
output, log file) → `proof.rs` (pure: verification report, Proof Pack JSON/Markdown) → `merge.rs` (clean-checkout
precondition, `git merge-tree` conflict detection, restore-on-failure) → orchestration in `supervisor/review.rs`.
A criterion's status is *derived from evidence* (Not checked / Check pending / Verified by checks / Verified manually /
Within scope / Failed / Inconclusive / Requires manual review); an agent saying "done" changes nothing. Evidence is tied to
the change-set **fingerprint**; any change after a check makes its result stale, and merge refuses anything that differs
from what was approved.

## Memory (Phase 3)

```
 page files (managed folder | Obsidian vault)  ──scan/hash──►  SQLite index (pages, FTS5, links, sources)
        ▲ atomic write under writer lock                              │  trust = review records bound to content hash
        │ (revisions, journaled)                                      ▼
   propose / edit  ◄── human verify (exact revision) ──►  retrieve.rs: verified-only, re-hash at packaging,
                                                           sanitise, budget, provenance ──► context_package
                                                                                              │ persisted bytes
                                                           first user turn (never system prompt) ◄┘
```
* `knowledge/page.rs` strict frontmatter subset (no YAML anchors/tags), slugs, wikilinks, normalisation, atomic writes.
* `knowledge/vault.rs` filesystem boundary: reparse points, hardlinks, `.obsidian`/`.git` refused; per-root file lock.
* `knowledge/service.rs` scan, create, revisions, sources + freshness, generated `index.md`/`log.md`, crash recovery.
* `knowledge/retrieve.rs` ranking (BM25 + file overlap + recency − staleness), one page per topic, budget, injection text.
* `knowledge/lint.rs` advisory quality checks. `supervisor/memory.rs` ties it to missions; failures never block a mission.

## Harness Studio (Phase 4)

Modules (`src-tauri/src/harness/`): `inventory` (what configures an agent in this project: instruction files, settings, rules, skills,
subagents, commands, MCP; each item says whether a FORGE mission loads it), `doctor` (findings: stale paths, contradictions, context bloat,
risky or broken hooks, broad permissions), `hooks` (command/settings validation, add/remove, templates, event interpretation),
`edit` (allowlist, link-free path resolution, diff preview, line-ending preservation), `dryrun` (run one hook against a synthetic event),
`store` (journal). `supervisor/harness.rs` composes them under a per-project OS file lock.

Write path: `harness_preview` (never writes) → `harness_apply(expectedSha256, expectedAfterSha256, acknowledgeRisks)` → under the lock:
re-read, compare hash, re-validate the exact bytes, back up the previous content (verified), journal `started`, atomic write, read back
and verify, journal `applied`, record the new content as reviewed, audit, push `harness` event. `harness_rollback` restores the backup only
if the file still has the hash the edit produced; otherwise it refuses and explains. Startup recovery resolves any `started` row from the
file on disk.

What this does *not* do: FORGE missions still never run project hooks or `.mcp.json` (see SECURITY.md). Hooks edited here take effect in the
user's own Claude Code sessions.

## Planned domains (not yet built)

Harness configuration service (Phase 4), experiment runner (Phase 6), second provider adapter — Codex (Phase 5): each a
separate module behind `Core`-level commands; see ROADMAP.md. Tables are added by forward-only migrations.
