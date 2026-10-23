# FORGE — Data Model

SQLite at `~/.forge/forge.db` (override `FORGE_HOME`). WAL, foreign keys on, forward-only migrations via
`PRAGMA user_version` (`db.rs`, currently 3). IDs are UUIDv7 strings (time-sortable). Timestamps are RFC 3339 UTC strings.

## Implemented (migration 1 — orchestration)

| Table | Key columns | Notes |
|---|---|---|
| `project` | `id`, `root_path` UNIQUE, `default_branch`, `archived_at`, `vault_path`, `vault_write` | Registered local git repo. Re-adding returns the existing row. |
| `mission` | `id`, `project_id`, `state`, `provider`, `permission_profile`, `base_ref`, `base_commit`, `branch`, `worktree_path`, `provider_session_id`, `last_error`, `contract_json`, `use_memory`, `memory_pins` | `provider` ∈ claude / claude-local / fake. |
| `mission_transition` | `mission_id`, `from_state`, `to_state`, `reason`, `at` | Append-only history; written in the same transaction as the state change. |
| `agent_session` | `id`, `mission_id`, `provider_session_id`, `state` (`starting/live/ended/lost`), `pid`, `usage_json`, `end_reason` | One row per engine process. `lost` = process vanished without a result. |
| `activity_event` | `mission_id`, `session_id`, `seq`, `kind`, `payload_json` ; UNIQUE(`session_id`,`seq`) | The flight recorder. Payloads redacted and size-bounded. Core-originated events use negative `seq`. |
| `approval_request` | `id`, `session_id`, `provider_request_id` (UNIQUE per session = `tool_use_id:digest(tool+input)`), `tool`, `summary`, `risk`, `state`, `decided_by`, `decision_note` | States: `pending → approved/denied/expired`, or `auto_denied` (policy). Decided at most once. |
| `audit_event` | `at`, `actor`, `action`, `subject`, `detail_json` | `actor` ∈ `user/system/policy`. |

## Mission state machine (`mission.rs`, single source of truth)

```
draft → ready → preparing → running ⇄ awaiting_approval
                    │          ├→ awaiting_review → approved → merged (terminal)
                    │          │        ├→ rejected (terminal)  ├→ running/preparing (follow-up / request changes)
                    │          ├→ failed → ready | preparing | awaiting_review
                    │          └→ interrupted → preparing | awaiting_review
 any non-terminal → cancelled (terminal, = discarded)
```
`merged`, `rejected`, `cancelled` have no exits. Illegal transitions are rejected and not recorded; a same-state transition
is a no-op for the store, but starting a mission has stricter state preconditions of its own.
"Stop" yields `interrupted` (resumable); "Discard" yields `cancelled` and removes the worktree only after a
consequence-aware confirmation.

## Implemented (migration 2 — verification, review, proof)

| Table | Notes |
|---|---|
| `verification_run` | One row per check execution: command, state, exit code, duration, **`fingerprint`** of the changes it ran against, `mutated` flag, redacted output tail, log path. Latest run per check is the evidence; history is kept. |
| `manual_verdict` | Human pass/fail per criterion, bound to the fingerprint it was given on (goes stale when changes move). |
| `review_decision` | approve / reject / request_changes with note, optional **override reason**, fingerprint and head commit. A merge requires the latest approve to match the current fingerprint. |
| `proof_pack` | Immutable snapshot (JSON + Markdown + sha256) with the exact diff stored on disk and referenced by hash. |
| `mission.contract_json` | The mission contract: scope globs, criteria, checks (user-authored commands), constraints. |

A **fingerprint** is `sha256(base commit + git write-tree of a temporary index containing the worktree's full current state)`:
it changes whenever any non-ignored byte of the change set changes, and never touches the worktree's real index.

## Implemented (migration 3 — knowledge / memory)

Authority for *trust* lives in SQLite; the page files hold *content*.

| Table | Notes |
|---|---|
| `knowledge_page` | Index row per page file (`managed` FORGE folder or `vault` Obsidian folder). `status` (proposed/verified/disputed/superseded/archived) and `verified_sha256` are authoritative; **frontmatter state/author are ignored on import**. `effective_state` is derived: a verified page whose `current_sha256` differs from `verified_sha256` is `proposed` again. |
| `knowledge_review` | Append-only decision log (created, content_changed, verify, dispute, archive, restore, supersede) with the revision hash it concerned. |
| `knowledge_source` | Provenance: kind (repo_file, commit, mission, check_run, user_note), locator, git blob id, raw + normalised hashes, redacted snapshot, freshness `state` (current/changed/missing/unknown). |
| `knowledge_page_source`, `knowledge_link` | Page↔source and wikilink/file-reference edges; links resolve by title/slug and record ambiguity instead of guessing. |
| `knowledge_revision` | Proposed full-text changes to an existing page (author, base hash, state proposed/applied/rejected/conflict). Applied only under the root's writer lock with a base-hash check. |
| `knowledge_op` | Write journal (started/done/failed with old/new hashes) so a crash can be resolved by inspecting what is on disk. |
| `context_package` | The exact bytes injected into a mission's first prompt, with items, reasons, sources, omitted-with-reasons and budget — history, not a rebuildable index. |
| `knowledge_fts` (FTS5, `unicode61`) | Derived search index (title, body + identifier word-parts, files, tags); rebuildable from the page rows. |

## Designed, not yet implemented

| Entity | Purpose | Phase |
|---|---|---|
| `harness_configuration`, `hook_definition` | detected CLAUDE.md/skills/hooks, versioned backups | 4 |
| `experiment`, `experiment_result` | baseline, command, env, repetitions, outcome | 6 |
| `organization`, `workspace`, `user`, `role`, `policy` | multi-tenant extension; every row gains `workspace_id` | 7 |

Relationships: project 1─* mission 1─* agent_session 1─* activity_event; mission 1─* approval_request / verification_run /
review_decision / proof_pack / context_package; project 1─* knowledge_page *─* knowledge_source.
