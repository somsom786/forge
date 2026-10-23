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
| Merge safety: clean main checkout on the base branch, no in-progress merge/rebase, conflicts detected with `git merge-tree` before touching anything, failed merge restored, approval pinned to the exact change fingerprint, never pushes. | `merge.rs`, `supervisor/review.rs` | `merge::tests`, `supervisor::tests::phase2` |

## Known limits (do not assume these are solved)

1. **An approved shell command runs with the user's privileges and no sandbox.** FORGE shows risk warnings but cannot
   confine it: an approved command could read or modify `~/.forge/forge.db` or files outside the worktree, push to a
   remote using the user's credentials, or run package scripts. Mitigation path: provider sandboxing (Claude `sandbox`
   option, Codex sandbox) and brokered git — not enabled yet.
2. **Shell-pattern rules (push, destructive commands, protected files via shell) are heuristics**, not a security
   boundary; `git p""ush` style obfuscation defeats them. The human reading each shell prompt is the real control, which
   is why every shell call asks. Path rules for the *file tools* are firm (enforced on the resolved real path).
3. **Check-then-use races on the filesystem** (e.g. a junction swapped between policy check and tool execution) cannot be
   closed without a brokered file layer or OS sandbox.
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
9. Not yet reviewed independently: Phase 2 merge/verification code and anything built after Phase 2 (see
   ENGINEERING_REVIEWS.md for what has been).

## Provider authentication

FORGE uses the user's existing locally authenticated CLI (`claude`), launched via the official Agent SDK with
`pathToClaudeCodeExecutable`. It never reads, copies or stores provider tokens. Personal-subscription use is for local
development; a distributed commercial/enterprise edition must use provider-approved integration and billing and is
not claimed here.

## Enterprise

Not implemented. Schemas and boundaries are designed for it (see DATA_MODEL.md, ROADMAP.md Phase 7). No SOC 2, SSO, RBAC,
tenant isolation or remote execution exists, and none should be advertised until built and tested.
