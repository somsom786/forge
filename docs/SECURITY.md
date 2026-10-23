# FORGE — Security Model

Scope: a **local, single-user** desktop application. There is no multi-user or enterprise security claim, no
certification, and no remote service. Treat everything below as the implemented local design plus its known limits.

## Trust boundaries

* **Authority** lives in the Rust core only: policy, approvals, persistence, git, process control.
* The **engine** and the **agent** are untrusted-adjacent. The engine can request decisions; it cannot make them.
* **Data is not authority.** Repository files, tool output, web content, memory pages and agent text may contain
  instructions; FORGE renders them as plain text and the agent is told to treat them as data. Nothing in them can
  change policy.
* Approvals can only be answered through the `approval_decide` command from the app's own webview. The agent has no path
  to Tauri IPC. Decisions are single-shot (`UPDATE … WHERE state='pending'`), attributable (`decided_by`, audit event),
  and expire when the session ends.

## Controls implemented (with tests)

| Control | Where | Test |
|---|---|---|
| Every tool call is gated by an SDK `PreToolUse` hook; `canUseTool` is a fail-safe fallback. User-level Claude settings are excluded (`settingSources:['project']`), so a user's global allow-rules/bypass defaults cannot widen what FORGE approved. | `adapters/claude.ts` | live E2E (`live_claude_mission_end_to_end`) |
| Writes outside the worktree and to protected paths (`.git`, `.claude/settings*|hooks|agents|skills`, `.codex`, `.forge`, `.env*`) are **denied by policy**, never prompted, even in `accept_edits`. | `policy.rs` | `policy::tests`, `supervisor::tests::policy_blocks…` |
| `git push` is denied for agents (publishing is a FORGE-controlled step). Best-effort pattern; the real control is human review of shell prompts. | `policy.rs` | `git_push_is_denied_not_asked` |
| Path containment: component-wise, case-insensitive on Windows, `..` and junction/symlink escape, alternate data streams, reserved device names. | `paths.rs` | `paths::tests` (incl. real junction) |
| `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`, Bedrock/Vertex/Foundry switches are stripped from the engine env, so missions cannot silently bill the API. A non-subscription `credentialSource` is surfaced in the UI. | `adapters/claude.ts` | `normalize.test.ts` |
| Secrets redaction (common token shapes, `key=value` assignments, PEM blocks) before persistence; stderr tails redacted; payloads size-bounded. | `redact.rs`, `util.ts` | `redact::tests` |
| Process containment: Windows Job Object `KILL_ON_JOB_CLOSE` per engine; engine exits when stdin closes; forced tree kill after 8 s on stop. | `proc.rs`, `main.ts`, `supervisor.rs` | `stopping_a_hung_agent…`, host tests |
| Worktree safety: removal refuses when uncommitted/unmerged work exists unless the user confirms the stated loss. | `git.rs`, `supervisor.rs` | `discard_refuses…` |
| Restart honesty: active missions become `interrupted`; pending approvals `expired`. | `supervisor.rs::recover` | `restart_recovery…` |
| Webview CSP; minimal Tauri capabilities (`core:default`, `dialog:allow-open`); no shell/fs/opener plugins exposed to the webview. | `tauri.conf.json`, `capabilities/` | — |

## Additional controls added after the independent review (see ENGINEERING_REVIEWS.md)

| Control | Where | Test |
|---|---|---|
| Repo-defined execution surfaces are neutralised: project hooks and `.mcp.json` servers do not run (`allowManagedHooksOnly`, `strictMcpConfig`, empty `mcpServers`), while CLAUDE.md/rules/skills still load and FORGE's own hook still fires. Verified with a hostile-repo experiment (control vs locked). | `adapters/claude.ts` | `claudeOptions.test.ts`, `scripts/lockdown-experiment.mjs` (live) |
| Approvals are bound to `tool_use_id` + digest of the exact tool input; a changed re-ask is a new request; hooks never rewrite input. | `supervisor.rs` | `a_reask_with_changed_input…` |
| Path identity: policy evaluates the *real* path (junctions, short names resolved); trailing-dot/space and 8.3 spellings rejected; comparison is case-insensitive. | `paths.rs`, `policy.rs` | `protection_follows_the_real_path…` |
| Atomic per-mission reservation: one start/discard/merge owner, concurrency counted for in-flight starts. | `supervisor.rs` | `only_one_of_many_concurrent_starts_wins` |
| Containment is mandatory; finalisation is driven by process exit and terminates the whole Job (lingering descendants die). | `proc.rs`, `supervisor.rs` | `a_descendant_holding_the_pipes…` |
| Git hardening: worktree identity verified before use; previews use a temporary index (the worktree's real index is never touched); redirecting env vars stripped; `core.fsmonitor=false`. | `git.rs` | `previewing_a_diff…`, `a_worktree_redirected…` |
| Discard/cleanup safety counts commits from the branch ref, not just the worktree directory. | `git.rs`, `supervisor.rs` | `discard_cannot_delete_a_branch…` |
| Local-model endpoints must be plain `http` on loopback (checked in Rust and the engine); billing credentials are stripped for local missions; no cost is ever reported for a local model. | `providers.rs`, `adapters/claude.ts` | `only_plain_http_loopback…`, `claudeOptions.test.ts` |
| Merge safety: clean main checkout on the base branch, no in-progress merge/rebase, the **reviewed tree** (not the worktree's index) is what gets committed, compare-and-swap ref update, merge predicted with `git merge-tree` against object ids and the result checked against the prediction, untracked/ignored files in the checkout never overwritten, failures restored with `merge --abort`/`reset --merge` (never `reset --hard`), never pushes. | `merge.rs`, `supervisor/review.rs` | `merge::tests`, `review_hardening` |
| Review integrity: approving requires the fingerprint **and contract revision** the reviewer saw; manual verdicts require the expected fingerprint; evidence is bound to the check/criterion *definition* it was gathered for; a check that modified the worktree is never evidence. | `supervisor/review.rs`, `contract.rs`, `proof.rs` | `review_hardening`, `contract::tests` |
| Git execution safety: `core.hooksPath` is empty for every supervised call (no repository hook ever runs); repositories configuring non-system clean/smudge/process filters or merge drivers are refused (Git LFS recognised); diffs always use `--no-textconv --no-ext-diff`. | `git.rs` | `merge::tests::repository_hooks_never_run…`, `repositories_configuring_external_git_programs_are_refused` |
| Memory trust: authority is a review record bound to the content hash; only human-verified pages are injected (into the *user* turn); injected bytes are the same bytes whose hash was compared; the stored/indexed copy and check logs are redacted. | `knowledge/*`, `verify.rs` | `knowledge::store_tests`, `service_tests`, `hardening_tests`, `supervisor::tests::memory` |
| Filesystem boundary for the wiki: the *spelled* path is checked for links before canonicalisation, hardlinks and `.obsidian`/`.git` refused, writer lock per root, crash recovery touches only writable roots under the lock. | `knowledge/vault.rs`, `knowledge/service.rs` | `vault::tests`, `hardening_tests` |

## Harness Studio controls (Phase 4; independently reviewed — review 4 in ENGINEERING_REVIEWS.md)

| Control | Where | Test |
|---|---|---|
| Editable set is an allowlist of project files (instruction files, `.claude/settings.json`, rules/commands/agents/skills). User-level files, `.mcp.json`, `.git`, `.forge`, hooks scripts and `settings.local.json` are refused at preview *and* apply. | `harness/edit.rs`, `inventory::editable_rel` | `only_allowlisted_project_files_are_editable`, `nothing_outside_the_allowlist_can_be_written` |
| Path resolution is link-free: the spelled path and the real path are both walked; junctions/symlinks/reparse points and hard-linked files are refused. | `harness/edit.rs` | `links_in_the_path_are_refused` (real junction) |
| A save is a compare-and-swap under a per-project OS file lock: the file must still have the hash the user opened, and the text must hash to the preview they approved. | `supervisor/harness.rs` | `a_file_that_changed_after_it_was_opened…`, `what_is_written_is_exactly…`, `of_many_simultaneous_saves…` |
| Every write backs up the previous bytes first (verified), is journaled before it happens, is atomic, is read back, and is reversible. Rollback refuses if the file changed after the edit, and never restores a backup whose hash does not match the journal. | `supervisor/harness.rs`, `harness/store.rs` | `an_edit_is_previewed_journaled…`, `rollback_refuses_when…`, `a_tampered_backup…`, `a_crash_between_journal_and_write…` |
| Hooks: exact event names, single-line visible commands (hidden/bidi characters flagged and shown escaped), mandatory timeout. A new or changed hook may only be a plain command hook (no `args`/`shell`/HTTP/prompt forms), and **every** new or changed hook needs an explicit acknowledgement, whatever the heuristics say. Unchanged hooks are grandfathered by (event, matcher, definition, multiplicity); widening a matcher, copying a hook or re-enabling hooks makes them new. | `harness/hooks.rs`, `harness/edit.rs` | `hooks::tests`, `risky_hooks_need_an_explicit_acknowledgement…` |
| Other settings that run commands or redirect traffic (`apiKeyHelper`, `awsAuthRefresh`, `statusLine`, `fileSuggestion`, …, `enableAllProjectMcpServers`, `env` entries such as `*_BASE_URL`/`*_PROXY`) are listed as risks and need the same acknowledgement. | `harness/hooks.rs` | `other_command_running_settings_need_acknowledgement` |
| Hook dry-run runs only on an explicit command from the app's webview, in bash only (no fallback shell), refuses to run uncontained, scrubs secret-looking env, has a timeout, caps output, kills the whole job and joins its readers; every run is audited with a redacted command and its hash. Hooks FORGE cannot reproduce faithfully (`dryRunSupported:false`) are not run. | `harness/dryrun.rs`, `supervisor/harness.rs` | `dryrun::tests`, `the_dry_run_audit_never_stores_a_secret…` |
| Shipped hook templates are executed in the real shell by tests; the formatter never uses a shell. | `harness/hooks.rs`, `harness/dryrun.rs` | `every_shipped_template_runs…`, `the_formatter_passes_hostile_file_names…` |
| Inventory and Doctor are bounded (entry budget, skipped dependency trees, bounded reads, per-kind finding cap) and never follow links or junctions, including a linked `.claude`. | `harness/inventory.rs`, `harness/doctor.rs` | `scanning_is_bounded…`, `a_linked_dot_claude_directory…`, `a_repetitive_hostile_pair…` |

## Known limits (do not assume these are solved)

1. **An approved shell command runs with the user's privileges and no sandbox.** FORGE shows risk warnings but cannot
   confine it: an approved command could read or modify `~/.forge/forge.db` or files outside the worktree, push to a
   remote using the user's credentials, or run package scripts. Mitigation path: provider sandboxing (Claude `sandbox`
   option, Codex sandbox) and brokered git — not enabled yet.
2. **Shell-pattern rules (push, destructive commands, protected files via shell) are heuristics**, not a security
   boundary; `git p""ush` style obfuscation defeats them. The human reading each shell prompt is the real control, which
   is why every shell call asks. Path rules for the *file tools* are firm (enforced on the resolved real path).
3. **Check-then-use races on the filesystem** (e.g. a junction swapped between policy check and tool execution) cannot be
   closed without a brokered file layer or OS sandbox. This now also applies to Harness Studio's write path: FORGE re-verifies the
   path and the previous content immediately before each write/delete and reports a swap afterwards, but a concurrent local
   process that wins that microsecond window could still redirect a write; closing it needs handle-pinned directory identities.
4. **Running a verification check executes code from the mission worktree** (including agent-written code, or a
   modified `npm test` script) with the user's privileges. Checks only run on an explicit user action and their commands
   come from the user, but the code they invoke is not sandboxed. A check that dirties the worktree is flagged and its
   result is not treated as fresh.
5. Containment of *checks* is best-effort (a descendant created between spawn and job assignment could escape); the
   engine's containment is race-free because the engine does nothing until it receives `start`.
6. Repo-committed `CLAUDE.md`, rules and skills are still loaded as project instructions. They are text the model reads,
   so a hostile repository can try to steer the agent (prompt injection); policy and approvals still apply to what it
   then tries to do. The mission guidance tells the agent to treat file contents as data.
7. Redaction is best-effort pattern matching.
8. The scripted test agent exists only in debug builds / when `FORGE_ALLOW_FAKE_PROVIDER=1`.
9. **A passing check is evidence, not proof.** A check's exit code 0 does not show the tests assert anything, and the agent
   can edit what a check runs. FORGE flags edits to files that define a check and excludes checks that modified the
   worktree, but it cannot tell a vacuous pass from a meaningful one.
10. **Proof Pack diffs are byte-exact and therefore contain your code verbatim** (including any secret already in it), so
    that their hash stays verifiable. They are stored under `~/.forge/proofs` and are not redacted.
11. Prompt injection through memory is mitigated (only reviewed text, user turn, sanitised, framed as data) but not solved: no
    delimiter can stop a model from being persuaded by reviewed text that itself contains instructions. FORGE's approval
    policy, not the model's compliance, remains the control over what an agent can do.
12. Reviewed independently: Phase 1 supervisor, the memory design, and the Phase 2/3 implementation (three reviews, all
    findings dispositioned in ENGINEERING_REVIEWS.md). Harness Studio is covered by review 4 (all findings dispositioned in
    ENGINEERING_REVIEWS.md; two items deliberately not done). Anything built after it (packaging, the Codex adapter) is not reviewed.
13. **Harness edits change the user's real checkout**, not a worktree: a saved CLAUDE.md or settings.json is immediately what the
    user's own Claude Code sessions read. Hooks written here run with the user's privileges in those sessions. FORGE shows the diff and
    risks, but it cannot judge whether a command is *wise*.
14. **A hook dry-run executes real code** (the hook command) with the user's privileges in the project folder. Env scrubbing is by name
    pattern and is best-effort; the run is not sandboxed.
15. Harness backups are not pruned and are stored unencrypted under the FORGE home.

## Provider authentication

FORGE uses the user's existing locally authenticated CLI (`claude`), launched via the official Agent SDK with
`pathToClaudeCodeExecutable`. It never reads, copies or stores provider tokens. Personal-subscription use is for local
development; a distributed commercial/enterprise edition must use provider-approved integration and billing and is
not claimed here.

## Enterprise

Not implemented. Schemas and boundaries are designed for it (see DATA_MODEL.md, ROADMAP.md Phase 7). No SOC 2, SSO, RBAC,
tenant isolation or remote execution exists, and none should be advertised until built and tested.
