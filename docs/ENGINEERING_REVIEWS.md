# FORGE — Engineering Reviews

Independent reviews by GPT-6 Astra (High reasoning, via the user's locally authenticated Codex CLI with a read-only,
ephemeral sandbox and no API billing). Claude remains the implementing engineer: every finding below was evaluated
independently, reproduced or checked against the code, and only then adopted, adapted or declined.

## Review 1 — Phase 1 core: security and reliability  (2026-10-23)

Scope: `supervisor.rs`, `policy.rs`, `paths.rs`, `store.rs`, `proc.rs`, `git.rs`, `mission.rs`, and the engine
(`main.ts`, `adapters/claude.ts`, `claudeNormalize.ts`, `fake.ts`). Six questions: approval boundary, concurrency,
Windows containment, path security, data loss, anything else.

| # | Astra's finding | Severity (Astra) | My assessment | Resolution | Evidence |
|---|---|---|---|---|---|
| 1 | A committed `.claude/settings.json` (SessionStart/PreToolUse command hooks) and `.mcp.json` (stdio MCP servers) execute code before any tool approval; SDK sessions skip the interactive trust prompt. | Critical | **Confirmed — and I had under-rated it** as a "known limit". | Adopted. `managedSettings:{allowManagedHooksOnly:true}`, `strictMcpConfig:true`, `mcpServers:{}`; `settingSources:['project']` kept so CLAUDE.md/rules/skills still load. | Controlled experiment (`packages/engine/scripts/lockdown-experiment.mjs`): control run executed the hostile SessionStart hook, PreToolUse hook **and** MCP server; locked run executed none, our own programmatic hook still fired once, CLAUDE.md still loaded. Unit tests in `claudeOptions.test.ts`. |
| 2 | `resolve_within` returned the lexical path; junction into `.claude`, 8.3 aliases, trailing dots, different root casing could evade protected-name matching; `strip_prefix` is case-sensitive. | High | Confirmed (the `strip_prefix` case bug is plain wrong). | Adopted. `resolve_within` now returns the real path (canonical ancestor + tail); ambiguous Windows spellings (trailing dot/space, 8.3 aliases) rejected; relative components computed case-insensitively; protection evaluated on the real path. | `policy::tests::protection_follows_the_real_path_not_the_spelling` (real junction, `.claude.`, `CLAUDE~1`, upper-cased root). **Residual:** a junction swapped between check and execution (TOCTOU) cannot be closed without a filesystem broker/sandbox. |
| 3 | Shell obfuscation (`git p""ush`, PowerShell string building, modified `npm test` scripts) defeats the push regex and protected-file rules. | High | Accepted as a limit of heuristics; correctly says my claim was too strong. | Adopted as **documentation + policy wording**: shell rules are heuristics, not a boundary; every shell call asks a human. Stronger control (provider sandbox / brokered git) is a roadmap item. | `policy.rs` module docs; `SECURITY.md` limit 2. |
| 4 | Approval bound to `tool_use_id` only; a re-ask with changed input (or `updatedInput` from a hook) inherits the earlier approval. | High | Confirmed. | Adopted. Request key = `tool_use_id : sha256(tool + canonical-JSON(input))[..16]`; hooks never return `updatedInput`; with repo hooks neutralised (#1) mutation surfaces are closed. | `a_reask_with_changed_input_needs_a_fresh_approval`, `identical_requests_share_one_key_regardless_of_json_key_order`. |
| 5 | Discard: ignored files invisible; missing worktree ⇒ zero commits ⇒ branch deleted with `-D`; branch switched in worktree ⇒ wrong commits inspected. | High | Partly. Ignored files in a FORGE-created worktree are agent/test artefacts (not user work) so I declined that sub-point; the missing-worktree/branch-switch data-loss path is **real**. | Adopted (except ignored files). Safety counts commits from the *branch ref* in the main repo plus the worktree HEAD, independent of directory existence; `-D` only after explicit confirmation; resume refuses a worktree on another branch. | `discard_cannot_delete_a_branch_with_unmerged_commits_even_if_the_worktree_is_gone`, `resuming_in_a_worktree_that_was_switched_to_another_branch_is_refused`, `git::tests::safety_counts_commits_from_the_branch_even_without_a_worktree`. |
| 6 | Worktree directory replaced by a junction / tampered `.git` file redirects git; `git add -A -N` for diffs mutates the real index (stages deletions); inherited git env/config/hooks. | High | Confirmed (index mutation is real; I had assumed it harmless). | Adopted. `verify_worktree` (toplevel, common-dir, branch identity) before any resume/check/merge; previews use a **temporary index** (`GIT_INDEX_FILE`), never the real one; git runs with redirecting env vars removed and `core.fsmonitor=false`, `--no-ext-diff --no-textconv`. | `previewing_a_diff_never_touches_the_worktrees_real_index`, `a_worktree_redirected_to_another_repository_is_detected`. |
| 7 | Concurrent starts both pass checks; `Preparing→Preparing` is an idempotent success ⇒ duplicate engines, limit exceeded; discard races preparation. | High | Confirmed (my idempotent-transition choice made it worse). | Adopted. Atomic per-mission **reservation** (one owner, capacity counted incl. in-flight starts); strict state preconditions; discard/merge/review take the same reservation. | `only_one_of_many_concurrent_starts_wins` (8 threads), `capacity_is_reserved_atomically` (5 missions, limit 2). |
| 8 | Containment assignment failure only logs; assignment after spawn; PID-based taskkill can hit reused PIDs. | High | Mostly. The engine is idle until `start` is sent, which is after assignment, so descendants cannot predate it; but "log and continue" was wrong. | Adopted: containment is **mandatory** (refuse to run otherwise), terminate through the retained Job handle, PID kill only as fallback. Suspended-creation not adopted (needs raw `CreateProcess`; handshake ordering already closes the window for the engine). | Code review; `stopping_a_hung_agent…`. Check runner remains best-effort (documented). |
| 9 | A descendant holding stdout prevents EOF ⇒ finalisation never runs; stderr thread keeps the handle (and Job) alive; thread-start failures leave partial sessions; full stdin pipe blocks cancel. | High | Confirmed by reading my code (the Job lived inside an `Arc` shared with threads). | Adopted. Finalisation is driven by **process exit** (a waiter thread), reader gets a bounded 2 s drain, Job explicitly terminated at finalisation, startup failures unwind fully. | `a_descendant_holding_the_pipes_cannot_wedge_finalisation_and_is_killed` (real lingering grandchild, killed). |
| 10 | Lost wake-up: UI can decide between DB insert and waiter registration; approval emitted before `AwaitingApproval`; finalise races delivery. | Medium | Confirmed. | Adopted. Per-session lifecycle lock around register/deliver/withdraw/finalise; state set before announce. | `an_answer_given_the_instant_a_prompt_appears_is_never_lost` (6 rounds, 0 ms polling decider). |
| 11 | Ignored SDK cancellation signals leave stale prompts; callback exceptions; hook failure semantics unverified. | Medium | Partly: hook timeouts/errors fail closed per Astra's own reading; stale prompts real. | Adopted: hook/`canUseTool` catch → explicit deny; provider `signal` aborts withdraw the prompt (`permission_cancelled`). | `claudeOptions.test.ts` ("FAILS CLOSED"), `a_provider_withdrawing_a_tool_call_expires_its_prompt`. |

Astra's "top 3": untrusted project execution; path identity/filesystem enforcement; destructive-discard safety. All three
addressed within the limits above. **Not fixed, by design or deferral:** filesystem/network sandboxing of approved shell
commands (SECURITY.md limit 1), TOCTOU junction swaps, ignored-file reporting.

Validation: Rust 127 tests + 1 live-ignored (all pass), engine 36 tests (all pass), plus two live runs (below).

## Live validation of the whole path (not a review finding, recorded for honesty)

* Real Claude Code via the Agent SDK through FORGE, `haiku`, one Write approved by hand: file created in the
  worktree, subscription login (`credentialSource:"none"`), usage recorded. Provider reported **seven-day utilisation
  0.96**; live Claude testing was therefore stopped and replaced by local models.
* Real Claude Code harness driving `qwen3.5:9b` (local Ollama, 32k-context tag) through FORGE: full mission in ~15 s,
  `credentialSource:"local-endpoint"`, no provider cost reported.

## Consultation budget
1 of the planned 4–6 consultations used. Candidates next: Memory provenance/consistency design (Phase 3), hook
execution security for Harness Studio (Phase 4), merge/verification review (Phase 2 — in progress).
