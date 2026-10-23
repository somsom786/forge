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
| Process containment: Windows Job Object `KILL_ON_JOB_CLOSE` per engine; on Unix each engine, check and hook dry-run leads its own process group and dropping its containment kills the group (does not survive a crash of FORGE itself; a descendant calling `setsid` escapes); engine exits when stdin closes; forced tree kill after 8 s on stop. | `proc.rs`, `main.ts`, `supervisor.rs` | `stopping_a_hung_agent…`, `proc::unix_tests`, host tests |
| Worktree safety: removal refuses when uncommitted/unmerged work exists unless the user confirms the stated loss. | `git.rs`, `supervisor.rs` | `discard_refuses…` |
| Restart honesty: active missions become `interrupted`; pending approvals `expired`. | `supervisor.rs::recover` | `restart_recovery…` |
| Webview CSP; minimal Tauri capabilities (`core:default`, `dialog:allow-open`); no shell/fs/opener plugins exposed to the webview. | `tauri.conf.json`, `capabilities/` | — |

## Additional controls added after the independent review (see ENGINEERING_REVIEWS.md)

| Control | Where | Test |
|---|---|---|
| Repo-defined execution surfaces are neutralised: project hooks and `.mcp.json` servers do not run (`allowManagedHooksOnly`, `strictMcpConfig`, and `mcpServers` limited to what the user authorized; see MCP authorization), while CLAUDE.md/rules/skills still load and FORGE's own hook still fires. Verified with a hostile-repo experiment (control vs locked). | `adapters/claude.ts` | `claudeOptions.test.ts`, `scripts/lockdown-experiment.mjs` (live) |
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
| Hook dry-run runs only on an explicit command from the app's webview, in bash only (no fallback shell), refuses to run uncontained (on Windows the process is created suspended and resumed only after it is in the Job; compiled, not yet run on Windows), scrubs secret-looking env, has a timeout, caps output, kills the whole job and joins its readers; every run is audited with a redacted command and its hash. Hooks FORGE cannot reproduce faithfully (`dryRunSupported:false`) are not run. | `harness/dryrun.rs`, `supervisor/harness.rs` | `dryrun::tests`, `the_dry_run_audit_never_stores_a_secret…` |
| Shipped hook templates are executed in the real shell by tests; the formatter never uses a shell. | `harness/hooks.rs`, `harness/dryrun.rs` | `every_shipped_template_runs…`, `the_formatter_passes_hostile_file_names…` |
| Inventory and Doctor are bounded (entry budget, skipped dependency trees, bounded reads, per-kind finding cap) and never follow links or junctions, including a linked `.claude`. | `harness/inventory.rs`, `harness/doctor.rs` | `scanning_is_bounded…`, `a_linked_dot_claude_directory…`, `a_repetitive_hostile_pair…` |

## Codex missions (Phase 5; review 5 in ENGINEERING_REVIEWS.md)

* Every command and file change is an approval request decided by FORGE (policy, then a human); Codex's `workspace-write` sandbox stays on as defence in depth.
* **Effective settings are checked, not assumed.** `thread/start`/`thread/resume` answer with the sandbox, approval policy, approval reviewer, cwd and provider Codex actually applied after merging the user's config. FORGE records those values in `session.started` (`effective`, next to `requested`) and refuses the session before any turn starts if they are broader than requested: full access or an external sandbox, network access, writable roots outside the worktree, any approval policy other than `untrusted`, a reviewer other than `user`, another cwd or provider, or a missing field. (Engine tests against a scripted app-server; the response shape is from the Codex 0.163.0 protocol schema. Not yet observed against a real Codex whose user config widens the sandbox.)
* **Approvals are scoped.** A command or file-change approval must name this session's thread and the current turn, and refer to an item announced in that turn with the matching type; otherwise it is declined without asking. An item re-announced with different content cannot be approved (an approval given while it changed is withdrawn); if it changes *after* approval, or a completed file change touches a path FORGE did not approve, the session is aborted.
* **Items with no approval gate abort the session.** `mcpToolCall`, `dynamicToolCall`, `webSearch`, `imageGeneration`, `collabAgentToolCall`, `subAgentActivity` and `hookPrompt` items are shown to the user and the session is interrupted and killed. Their prerequisites are disabled, so this is a tripwire, not the primary control; it acts after the item has started.
* **Codex starts MCP servers from a repository's `.codex/config.toml` even though it calls project config untrusted** (reproduced). FORGE refuses to run Codex in a worktree with project Codex config, disables every other server by name, disables hooks/plugins/apps/computer-use/browser/multi-agent, sets `notify=[]` and `web_search="disabled"`, and aborts on any MCP/hook startup.
* **User exec allow-rules bypass approvals** (reproduced: an allowed command ran with no request). A Codex mission refuses to start while the Codex home contains allow rules unless `FORGE_CODEX_ALLOW_USER_RULES=1`.
* Only a ChatGPT login is accepted (`account/read`); API-key/custom-provider logins and API-key environment variables are refused/scrubbed (case-insensitive). FORGE never reads `~/.codex/auth.json`.
* Cleanup on Windows runs `%SystemRoot%\System32\taskkill.exe` (absolute; a non-absolute `SystemRoot` falls back to `C:\Windows`) with `SystemRoot` as cwd; tested with an injected spawn and a hostile worktree full of `taskkill` sentinels, and on Linux with real processes. The Windows branch has **not been run on Windows** by this change.
* Not verified on a real Codex: that a widened user config is reported in the `thread/start` response the way the schema says (the refusal logic is unit-tested only).

## Experiments (Phase 6, backend only)

* **Running an experiment executes the user's command against code from every revision it compares** (any branch, tag or commit of the
  project, which may contain agent-written code) with the user's privileges, exactly like a verification check (limit 4). It only runs
  on an explicit user action. Revisions are pinned to the commit they resolved to when the experiment was defined.
* Each revision runs in its own detached worktree under the FORGE home, never in the user's checkout; repositories configuring external
  git programs are refused before any checkout; git runs with the same hardening as missions. References are validated (no options,
  ranges or reflog expressions) before they reach git.
* Run output is redacted (tail and log) like checks; containment is the check runner's (best-effort, limit 5).
* A mission used as a variant is snapshotted with `commit-tree` from its worktree state (temporary index, as for review fingerprints) and the
  commit is kept alive by a ref `refs/forge/snapshots/<sha>` in the project repository. These refs are FORGE's own namespace (not branches)
  and are never pushed by FORGE; nothing removes them yet. The UI asks for confirmation and states the commands before a run.

## Approval binding (Milestone 2)

| Control | Where | Test |
|---|---|---|
| Every approval request is bound to a digest of mission, session, provider, tool, canonical input, worktree, permission profile, policy revision (`policy::REVISION`) and where it executes (`agent_host` today). The UI sends the digest back; a decision on a different request is refused. | `supervisor.rs` (`approval_binding`, `decide_approval_checked`), migration 9 | `approvals::an_approval_is_bound…` |
| Requests expire (30 min). A sweeper expires overdue ones and tells the agent "denied", so nothing waits forever and nothing old can be approved. | `supervisor.rs` (`expire_overdue`) | `approvals::an_undecided_request_expires…` |
| Approving re-runs FORGE policy on the exact original (unredacted, in-memory) request. A target that became protected or was redirected outside the worktree while the human read the prompt is denied by policy. Before this, such an approval went through (shown by a mutation test). | `decide_approval_checked` | `approvals::policy_is_rechecked_at_decision_time…` (Unix; a Windows junction variant is in the Windows handoff) |
| Reuse is narrow: only an identical call (same provider tool-use id and input digest) in the same session reuses an answer. | `on_permission` | `regressions::a_reask_with_changed_input…` |

Remaining window: between FORGE's decision-time check and the agent's own write there is still a small gap that only a brokered
file layer or an OS sandbox closes (limit 3).

## MCP authorization (Milestone 2)

| Control | Where | Test |
|---|---|---|
| A repository's `.mcp.json` gives a mission no servers. The Claude adapter keeps `strictMcpConfig` and receives **only** the servers the user authorized for that project, built by the core from the stored definitions. Codex missions receive none. | `mcp.rs`, `supervisor/mcp.rs`, `spawn_session`, `adapters/claude.ts` | `tests::mcp::a_repository_mcp_json_alone…`, `claudeOptions.test.ts` |
| Authorizing pins the exact definition FORGE read and showed (command, args, URL, env *names*) by SHA-256; the UI must echo the hash, so a file changed after review cannot be authorized. Literal env values in `.mcp.json` are never read into FORGE; values come from FORGE's environment at mission start and are never stored. | `mcp::from_project_file`, `Core::mcp_authorize` | `tests::mcp::only_the_authorized_definition…`, `mcp::tests` |
| Editing `.mcp.json` after authorization changes nothing that runs; the UI flags "changed since authorized". | `mcp_servers_for_mission`, `mcp_authorizations` | `tests::mcp::only_the_authorized_definition…` |
| The core denies, without asking, any `mcp__server__tool` call for a server not authorized for the mission's project or a tool outside the server's allow-list. Allowed calls still go through policy (Ask). | `Core::mcp_gate`, `on_permission` | both tests above |
| Servers that would receive `ANTHROPIC_API_KEY`, `*_BASE_URL` and similar, names with `__`, multi-line args or non-loopback plain-http URLs are refused. Authorize/revoke are audited. | `McpDefinition::validate` | `tests::mcp::servers_that_would_redirect…`, `mcp::tests` |

Honest limits: an authorized stdio server is a program running **on the host with the user's privileges** from the moment the
mission starts (not in an execution backend, not before any approval); FORGE cannot restrict what that program does, only which
of its tools the agent may call and with whose approval. Package-runner commands (`npx`, `uvx`) without a pinned version can
change between runs; the dialog says so.

## Verification adapters (Proof Engine 2.0, Milestone 4)

| Control | Where | Test |
|---|---|---|
| A check may name an adapter (`gitleaks`, `osv-scanner`, `trivy`, `playwright`). The command stays user-authored and runs on the project's execution backend like any check; FORGE only suggests the command, and only when the tool is installed (Playwright: when the project declares it). | `evidence.rs`, `detect::adapter_checks`, `contract.rs` (unknown adapters refused) | `detect::tests`, `tests::evidence::unknown_adapters…` |
| The adapter reads **stdout only** (captured separately from stderr, redacted line by line, capped at 16 MiB) and the report, not the exit code, decides the result. Unparseable output, or a non-zero exit with a clean report, is a *tool error*; a missing tool is *tool missing*; neither is ever a pass, and a required check in either state blocks approval. | `verify::run_check_capture`, `evidence::evaluate` | `evidence::tests` (9), `verify::tests::stdout_can_be_captured_alone…`, `tests::evidence::a_clean_report_passes…` |
| Secret-scan findings keep rule, file and line only; `Secret`/`Match` are never stored or shown, even redacted. At most 200 findings are kept per run (counts cover all). Unknown severity is treated as blocking. | `evidence::gitleaks`, `MAX_FINDINGS` | `evidence::tests::gitleaks_findings…`, `findings_are_capped…` |
| Findings appear in the Verification view and the Proof Pack (JSON + markdown). | `proof.rs` | `tests::evidence::findings_from_a_secret_scan…` |

Honest limits: the parsers were tested against **hand-written fixtures** built from each tool's documented JSON format
(`apps/desktop/src-tauri/fixtures/evidence/README.md`), not against real tool runs; the suggested command lines (notably
`gitleaks … --report-path -` and `osv-scanner scan source`) must be confirmed against the tool versions on the user's machine.
Trivy downloads its vulnerability database, so it fails (tool error) on the `linux_namespaces` backend, which has no network.

## ACP agents (Milestone 3)

FORGE is the ACP **client** (official SDK `@agentclientprotocol/sdk` 1.8.0); an ACP agent is a program the user registered.

| Control | Where | Test |
|---|---|---|
| Agents run only if registered by the user; the command is pinned to an absolute, existing file at registration; `.cmd`/`.bat`/`.ps1` wrappers are refused (no shell); registration/removal audited. Missions name a registered agent. | `acp.rs`, `supervisor/acp.rs`, `create_mission` | `acp::tests`, `tests::acp::acp_missions_need_a_registered_agent` |
| Every `session/request_permission` is mapped onto FORGE policy (`edit`/`delete`/`move` → one `Edit` per location, `read`/`search` → `Read`, `execute` → `Bash`, `fetch` → `WebFetch`, anything else → `acp_<kind>`, which asks). A write-like call without a location is denied by policy. | `adapters/acp.ts` `forgeRequests` | `acp.test.ts`, `tests::acp::policy_denies…` |
| FORGE answers only with *once* options (`allow_once`, `reject_once`); if the agent offers no `allow_once`, the call is refused rather than granting standing permission. | `chooseOption` | `acp.test.ts` (never grants standing permission) |
| File access the agent asks for (`fs/read_text_file`, `fs/write_text_file`) is brokered: policy decides (reads inside the worktree are allowed, outside ask; writes ask unless covered by the edit just approved, consumed once), and FORGE performs the write after re-checking the **real** path is inside the worktree and not a link. A target swapped for a link after approval is not written through (mutation-checked). | `writeTextFile`, `safeTarget` | `acp.test.ts`, `tests::acp::an_acp_agents_edit…` |
| No terminal capability is offered. MCP servers: only the project's authorized ones (http only if the agent supports it). | `initialize`, `toAcpMcpServers` | `acp.test.ts` |
| The agent process gets FORGE's environment minus API keys/base URLs of every provider and minus `FORGE_*`; FORGE never calls `authenticate` (an agent that needs sign-in fails with a message telling the user to sign in outside FORGE). A protocol version other than 1 is refused. | `acpEnv`, `runAcp` | `acp.test.ts` |

**Limit (important):** ACP permission requests are *agent-initiated*. An agent that runs a tool or writes a file itself without
asking is not mediated by FORGE (same trust class as limit 1). The worktree diff remains the authoritative record and is
reviewed before merge. FORGE prepends its mission guidance to the prompt (ACP has no system prompt).

## Fixes from the independent cloud reviews (2026-10-23)

Found by independent security, protocol and data-integrity reviews of `cloud/overhaul`; each fix has a regression test that
fails without it (mutation-checked where noted).

| Finding | Severity | Fix | Test |
|---|---|---|---|
| A **dangling** symlink/junction in the worktree let a Write escape the worktree or reach a protected file: `exists()` follows links, so the link's own (inside) path was checked | **P0** | `resolve_within` refuses any component that exists as a link but whose target does not | `paths::a_broken_link_is_refused_wherever_it_points`, `approvals::a_dangling_link_*` (mutation-checked) |
| Redaction could hide part of a shell command from the approver (`TOKEN=$(… \| sh)` shown as `TOKEN=[REDACTED]`) while the digest covered the real command | P1 | A shell command whose text redaction would change is refused, not asked; other tools get High risk + a warning | `approvals::a_command_whose_text_would_be_hidden…` |
| An approved call could be replayed indefinitely with the same tool-use id (ACP agents choose their ids) without a policy re-check | P1 | A decided allow is reused once (the provider prompt after FORGE's hook), after re-running policy; further repeats are denied | `approvals::an_approval_is_reused_at_most_once…` |
| `core.hooksPath` pointed at a shared, pre-creatable directory under the temp dir | P1 | Points at the null device (`/dev/null`, `NUL`), which cannot hold hooks | `review_hardening::a_hostile_git_hook_never_runs…` |
| Unicode look-alikes (`.claude/ſettings.json`, zero-width characters) passed the protected-name check on case-insensitive file systems | P1 (unconfirmed on macOS/Windows) | Protected names are compared after folding characters such file systems fold onto ASCII | `policy::unicode_lookalikes_of_protected_names_are_denied` |
| `~`, `$VAR`, `%VAR%` in a write path were checked literally | P2 | Denied | `policy::home_and_variable_shorthands_are_denied` |
| MCP server names with edge underscores made tool names ambiguous; `http://localhost.evil.com` counted as loopback | P2 | Names may not start/end with `_`; URL host parsed exactly | `mcp::tests::dangerous_or_malformed_definitions_are_refused` |
| Approving without the card's binding skipped the binding check | P2 | Approving a bound request requires the binding; denying does not | `approvals::approving_without_the_cards_binding_is_refused` |
| A `Read` naming no file was allowed silently | P2 | Asked | `policy::write_without_path_is_denied_and_unknown_tools_ask` |

| Data integrity: a squash merge that reached git before a crash left the mission "approved" forever (every retry: "nothing to commit") | P1 | Merge intent recorded before git runs (migration 13) and reconciled with the repository on restart and on retry | `integrity::a_squash_merge_that_landed…`, `…interrupted_before_its_commit…` |
| Data integrity: a crash between `git worktree add` and the database write bricked the mission | P1 | An orphaned worktree that is provably the mission's own and untouched is adopted; a failure after `add` undoes it | `integrity::a_worktree_created_before_a_crash_is_adopted_on_start` |
| Data integrity: a second FORGE on the same data folder "recovered" the first one's live missions, approvals and checks | P1 | Exclusive OS lock on `forge.lock` at startup (released by the OS on exit or crash) | `instance_lock_tests` |
| Discard/cleanup deleted whatever path the database named: a link to another mission's worktree deleted that worktree's work | P2 | Before deleting: not a link, inside FORGE's worktrees folder, and the registered worktree of this repo on this mission's branch | `integrity::discarding_through_a_link_to_another_worktree_is_refused` |
| `git worktree remove` silently deleted ignored files (a local `.env`) | P2 | Ignored entries count as potential loss (confirmation required; merge cleanup keeps such worktrees) | `integrity::ignored_files_need_confirmation…` |
| Recovery only saw the 500 most recently updated missions; a failed write stopped the rest of recovery | P2 | Queried by state; best effort per item, failures audited | `integrity::recovery_finds_active_missions_beyond_any_listing_limit` |
| Changing the success criteria after approval kept the approval | P2 | Returns the mission to review | `integrity::changing_the_success_criteria_after_approval…` |
| Discard of a finished mission deleted the worktree before failing | P2 | Refused before anything is deleted | `integrity::a_finished_mission_cannot_be_discarded…` |

Policy revision bumped to `policy-2026-10-23.2` (approvals requested under the old revision are never reused).

## Execution backends (Stage B; docs/EXECUTION.md)

* Checks and experiment runs execute through the project's **execution backend**. `trusted_local` (the default, unchanged
  behaviour) is labelled "Not isolated" everywhere. `linux_namespaces` (bubblewrap) is implemented and adversarially tested on
  Linux: no host files outside the workspace, no home directory, cleared environment, no network, no surviving processes,
  read-only git directory. Windows Sandbox and isolated WSL are designed and reported **not implemented**.
* A selected backend that is unavailable is an error; FORGE never falls back to a less isolated one. Changes are audited, and
  every verification run, experiment run and Proof Pack records the backend it ran on.
* The **agents** and the commands they run after approval are not yet routed through a backend (limit 1 stands).

## Known limits (do not assume these are solved)

1. **An approved agent shell command runs with the user's privileges and no sandbox.** (Checks and experiments can now be isolated on Linux; see Execution backends.) FORGE shows risk warnings but cannot
   confine it: an approved command could read or modify `~/.forge/forge.db` or files outside the worktree, push to a
   remote using the user's credentials, or run package scripts. Mitigation path: provider sandboxing (Claude `sandbox`
   option, Codex sandbox) and brokered git — not enabled yet.
2. **Shell-pattern rules (push, destructive commands, protected files via shell) are heuristics**, not a security
   boundary; `git p""ush` style obfuscation defeats them. The human reading each shell prompt is the real control, which
   is why every shell call asks. Path rules for the *file tools* are firm (enforced on the resolved real path).
3. **Check-then-use races on the filesystem** (e.g. a junction swapped between policy check and tool execution) cannot be
   closed without a brokered file layer or OS sandbox. This now also applies to Harness Studio's write path: FORGE re-verifies the
   path and the previous content immediately before each write/delete and reports a swap afterwards, but a concurrent local
   process that wins that microsecond window could still redirect a write; closing it needs handle-pinned directory identities. Creating a file
   that did not exist is an exclusive create (it fails instead of replacing a file that appeared after the check); replacing an existing
   file is not.
4. **Running a verification check executes code from the mission worktree** (unsandboxed on `trusted_local`; isolated on `linux_namespaces`) (including agent-written code, or a
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
15. Harness backups are stored unencrypted under the FORGE home. They are kept until the user prunes them (`harness_prune_backups`; `harness_backup_usage` shows the space and what would go). A prune never removes the backup of an unresolved change, of the newest N applied changes per file (default 10), or of any change that can be rolled back right now; a pruned change can no longer be rolled back and says so. There is no automatic pruning and no UI for it yet.

## Provider authentication

FORGE uses the user's existing locally authenticated CLI (`claude`), launched via the official Agent SDK with
`pathToClaudeCodeExecutable`. It never reads, copies or stores provider tokens. Personal-subscription use is for local
development; a distributed commercial/enterprise edition must use provider-approved integration and billing and is
not claimed here.

## Enterprise

Not implemented. Schemas and boundaries are designed for it (see DATA_MODEL.md, ROADMAP.md Phase 7). No SOC 2, SSO, RBAC,
tenant isolation or remote execution exists, and none should be advertised until built and tested.
