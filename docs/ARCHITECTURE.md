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

## Planned domains (not yet built)

Memory/indexing, context compiler, harness configuration service, verification engine (Proof Packs), experiment runner —
each a separate Rust module behind `Core`-level commands; see ROADMAP.md. Tables are added by forward-only migrations.
