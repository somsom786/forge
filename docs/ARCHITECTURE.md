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

## Experiments (Phase 6, backend only)

`experiment.rs` (pure: definition validation, metric extraction, statistics, verdict), `store_experiment.rs` (migration 7),
`supervisor/experiment.rs` (runner). `experiment_create` resolves the baseline and every variant to exact commits. `experiment_run`
(explicit user action, blocking) checks each commit out in its own detached worktree under the FORGE home, runs the optional
correctness command once per revision, then warm-ups, then the measured runs interleaved across revisions (rotating who goes first),
all through `verify::run_check`. A run that fails, prints no metric or changes its worktree invalidates that revision; a variant is
*better* only if every measured value beats every baseline value with at least 3 runs on both sides. Worktrees are removed at the end;
every run of every attempt is kept. No UI and no progress events yet.
## Conflicts between missions (Phase 5, backend only)

`conflicts.rs` + `supervisor/conflicts.rs`, command `mission_conflicts(projectId)`. For every mission that still has work to land
(not merged, rejected or cancelled) and a verified worktree: its changed files and the tree of its current state (temporary index,
as for review fingerprints). Pairs report shared paths and a `git merge-tree --write-tree` prediction of landing both; drift reports
how far the base branch moved, which moved files the mission also changed, and whether the mission would conflict with the new base.
Trees are wrapped in unreferenced `commit-tree` objects (older git cannot merge bare trees); no ref, index or file is changed. A git
error is reported as `unknown`, never as a conflict.

## Code intelligence

`codeintel/` (index, Code Map, search, relevance) + `supervisor/code.rs`, commands `code_map(projectId)` and
`code_symbols(projectId, query, file, limit)`. Files come from `git ls-files --cached --others --exclude-standard` in the main
checkout (FORGE's hardened git options), so `.gitignore` is respected; `.git`, `node_modules` and `target` are excluded even if
committed, symlinks are never followed, files over 512 KiB or containing NUL bytes are skipped and counted, at most 20 000 files are
listed, each parse has a 2 s deadline and a build 90 s (a truncated index says so). Rust, TypeScript/TSX, JavaScript and Python are
parsed with tree-sitter grammars compiled into the binary; only declarations are walked (module level, class/impl/trait bodies,
namespaces), giving name, kind, file, line span, one-line signature and container. Nothing from the repository is executed and
nothing touches the network.

No database table: the index is kept in memory per repository root, keyed by a fingerprint of HEAD, the porcelain status and the
size/mtime of every changed file, so a commit, checkout or edit triggers a rebuild on the next request and a stale index is not served.

Context integration: `knowledge::retrieve::build` appends a separate `<<<FORGE_RELEVANT_CODE ... FORGE_RELEVANT_CODE>>>` block after
the knowledge block (task first, knowledge second, code third; never the system prompt). It lists symbols whose name words (light
stemming) match the objective's terms, or that sit under a path the mission names, best first, at most 5 per file and 25 in total,
within 2 000 bytes; every line is sanitised like knowledge text. Each pointer's reasons, the terms, the index commit/dirty flag and
how many matches were left out are stored in the package's `params_json.relevantCode`, and the injected-text hash covers the block;
the Memory tab and the new-mission preview show them. The block is governed by the mission's existing memory switch.

Measured on a synthetic fixture (36 files, written by the test, not a real project), file selection at k=3 for 9 hand-labelled
objectives: keyword grep P 0.33 / R 0.83 (F1 0.48), symbol index P 0.44 / R 0.89 (F1 0.59). The index loses the configuration case
(`config/app.toml` has no symbols). Test: `codeintel::tests::measured_symbol_index_selects_files_at_least_as_well_as_keyword_grep`
(run with `--nocapture` for the per-objective table). Not measured on real repositories.

## Planned domains (not yet built)

Independent review missions and conflict detection between missions (Phase 5), an Experiments screen (Phase 6): each a separate
module behind `Core`-level commands; see ROADMAP.md. Tables are added by forward-only migrations.
