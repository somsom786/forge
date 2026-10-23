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

## Implemented (migrations 4–5 — evidence revisions, Harness Studio)

| Change | Notes |
|---|---|
| Migration 4: `verification_run.spec_rev`, `manual_verdict.criterion_rev` | Evidence is bound to the *definition* it was gathered for (check command/timeout/kind revision; criterion text revision). Editing a check or criterion makes earlier evidence stale instead of silently reusing it. |
| `harness_change` (migration 5) | Journal of every edit FORGE made to an agent configuration file: `kind` (edit/create/rollback), `before_sha256` / `after_sha256` (NULL = file absent), `backup_path` (relative to the FORGE home; content *before* the change), `rolls_back_id`, `state` (started → applied/failed). A row is written `started` before the file is touched, so a crash is resolved by comparing the file on disk to `after_sha256`. |
| `harness_review` (migration 5) | The content hash of each configuration file the user last reviewed (set by saving through FORGE or by "Mark reviewed"). Doctor reports files that differ from it ("changed outside FORGE since you last reviewed it"). |
| Files outside SQLite | `<home>/harness-backups/<project>/<change>.bak` (exact previous bytes, hash-verified before any restore) and `<home>/harness-locks/<project>/.forge-lock` (OS file lock serialising writers across FORGE processes). Backups are kept indefinitely (each is at most 256 KiB); there is no retention policy yet. |

## Implemented (migration 7 — experiments, Phase 6; backend only)

| Table | Notes |
|---|---|
| `experiment` | Definition (name, objective, `command`, optional `correctness_command`, `metric_json` = duration or a `FORGE_METRIC <name>=<n>` value with a direction, `repetitions`, `warmup`, `timeout_secs`), `state` (draft → running → completed/failed/cancelled/interrupted), `attempt` (incremented per run), `env_json` (OS, arch, CPU count, git and FORGE versions of the latest attempt), `report_json` (verdict of the latest finished attempt), `last_error`. |
| `experiment_variant` | The revisions compared: position 0 is the baseline. `git_ref` is what the user named (or `mission:<id>` for a mission snapshot); `commit_sha` is what it resolved to at creation, and every attempt measures that commit even if the branch moves or the agent keeps editing. Mission snapshots are pinned by `refs/forge/snapshots/<sha>`. |
| `experiment_result` | One row per run of every attempt (`phase` correctness/warmup/measure, `iteration`, `state` incl. `no_metric`, `exit_code`, `duration_ms`, `metric_value`, `worktree_changed`, redacted `output_tail`, `log_path`). Failed and inconclusive attempts are never deleted. |
| Files outside SQLite | `<home>/experiments/<experiment>/<attempt>/<variant>` detached worktrees (removed when the attempt ends, or at startup after a crash) and `.../logs/*.log` (redacted, capped run logs, kept). |

## Implemented (migration 8 — execution backends)

| Change | Notes |
|---|---|
| `project.execution_backend` | `trusted_local` (default, also for every existing project) or `linux_namespaces`. Changed only by the user; audited. |
| `verification_run.backend`, `experiment_result.backend` | Where each run actually executed. Existing rows read `trusted_local`, which is what they were. |

## Implemented (migration 9 — approval binding)

`approval_request.binding_sha256`, `policy_rev`, `executes_on`, `expires_at`. NULL on rows created before migration 9 (their
sessions have ended; they can no longer be decided).

## Implemented (migration 10 — MCP authorization)

`mcp_authorization(id, project_id, name, definition_json, definition_sha256, tool_allowlist_json, authorized_at, revoked_at)`;
at most one active (unrevoked) row per project and server name (partial unique index). Re-authorizing revokes the previous row,
so history is kept. `definition_json` holds env variable *names* only.

## Implemented (migration 11 — ACP agents)

`acp_agent(id, name, command, args_json, added_at, removed_at)`; at most one active agent per name. `command` is the absolute
path resolved at registration. An ACP mission has `provider = 'acp'` and `model = <agent name>`.

## Implemented (migration 12 — structured evidence)

`verification_run.evidence_json`: the adapter's verdict (`outcome`: passed / findings / tool_missing / tool_error / incomplete),
counts by severity and up to 200 findings (rule, severity, title, location, package, fixed-in, blocking). NULL for plain checks.
`CheckSpec.adapter` lives in the contract JSON; a check's revision includes it only when set, so existing evidence keeps its meaning.

## Implemented (migration 13 — merge records)

`mission_merge(mission_id, target_oid, branch_oid, predicted_tree, squash, message, started_at, commit_oid, finished_at)`:
written *before* git merges, finished after the mission becomes Merged. On restart (and on a retried merge) an unfinished
record is reconciled with the repository: the merge commit landed (complete it), a squash stopped before its commit (finish
FORGE's own commit, after checking the index holds exactly the predicted tree), or nothing happened (forget it). The recorded
`branch_oid` also tells cleanup which commits a squash contained.

## Implemented (migrations 14–15 — where agents run)

* `project.agent_backend` (default `trusted_local`): where the project's agent sessions run, chosen by the user and
  audited (`project.agent_backend`); an unavailable backend refuses to start a mission instead of falling back.
* `agent_session.executes_on` (default `agent_host`, which is where every session before this migration ran): recorded
  when the session starts, carried into approval bindings and Proof Packs ("Agent ran: …").

## Designed, not yet implemented

| Entity | Purpose | Phase |
|---|---|---|
| `organization`, `workspace`, `user`, `role`, `policy` | multi-tenant extension; every row gains `workspace_id` | 7 |

Relationships: project 1─* mission 1─* agent_session 1─* activity_event; mission 1─* approval_request / verification_run /
review_decision / proof_pack / context_package; project 1─* knowledge_page *─* knowledge_source.
