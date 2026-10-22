# FORGE — Data Model

SQLite at `~/.forge/forge.db` (override `FORGE_HOME`). WAL, foreign keys on, forward-only migrations via
`PRAGMA user_version` (`db.rs`). IDs are UUIDv7 strings (time-sortable). Timestamps are RFC 3339 UTC strings.

## Implemented (migration 1)

| Table | Key columns | Notes |
|---|---|---|
| `project` | `id`, `root_path` UNIQUE, `default_branch`, `archived_at` | Registered local git repo. Re-adding returns the existing row. |
| `mission` | `id`, `project_id`, `state`, `provider`, `permission_profile`, `base_ref`, `base_commit`, `branch`, `worktree_path`, `provider_session_id`, `last_error`, `contract_json` | `contract_json` reserved for Phase 2 mission contracts. |
| `mission_transition` | `mission_id`, `from_state`, `to_state`, `reason`, `at` | Append-only history; written in the same transaction as the state change. |
| `agent_session` | `id`, `mission_id`, `provider_session_id`, `state` (`starting/live/ended/lost`), `pid`, `usage_json`, `end_reason` | One row per engine process. `lost` = process vanished without a result. |
| `activity_event` | `mission_id`, `session_id`, `seq`, `kind`, `payload_json` ; UNIQUE(`session_id`,`seq`) | The flight recorder. Payloads redacted and size-bounded. |
| `approval_request` | `id`, `session_id`, `provider_request_id` (UNIQUE per session), `tool`, `summary`, `risk`, `warnings_json`, `state`, `decided_by`, `decision_note` | States: `pending → approved/denied/expired`, or `auto_denied` (policy). Decided at most once. |
| `audit_event` | `at`, `actor`, `action`, `subject`, `detail_json` | `actor` ∈ `user/system/policy`. Org-scoped audit is a future extension. |

## Mission state machine (`mission.rs`, single source of truth)

```
draft → ready → preparing → running ⇄ awaiting_approval
                    │          ├→ awaiting_review → approved → merged (terminal)
                    │          │        ├→ rejected (terminal)  ├→ running/preparing (follow-up)
                    │          ├→ failed → ready | preparing | awaiting_review
                    │          └→ interrupted → preparing | awaiting_review
 any non-terminal → cancelled (terminal, = discarded)
```
`merged`, `rejected`, `cancelled` have no exits. Illegal transitions are rejected and not recorded; same-state is a no-op.
"Stop" yields `interrupted` (resumable); "Discard" yields `cancelled` and removes the worktree only after a
consequence-aware confirmation.

## Designed, not yet implemented

| Entity | Purpose | Phase |
|---|---|---|
| `mission_contract` (in `mission.contract_json`) | objective, scope, acceptance criteria, required tests, limits | 2 |
| `verification_run`, `verification_evidence`, `proof_pack` | check results tied to criteria + commit | 2 |
| `knowledge_source`, `knowledge_page`, `knowledge_assertion`, `memory_link` | evidence → wiki → assertions with state `proposed/verified/superseded/disputed/archived`, FTS5 index | 3 |
| `harness_configuration`, `hook_definition` | detected CLAUDE.md/skills/hooks, versioned backups | 4 |
| `experiment`, `experiment_result` | baseline, command, env, repetitions, outcome | 6 |
| `organization`, `workspace`, `user`, `role`, `policy` | multi-tenant extension; every row gains `workspace_id` | 7 |

Relationships: project 1─* mission 1─* agent_session 1─* activity_event; mission 1─* approval_request; knowledge
assertions will reference `knowledge_source` rows by id + content hash.
