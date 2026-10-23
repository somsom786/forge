# FORGE — handoff to Codex

You are taking over a half-built, real, working project. Read this file first, then `docs/ORIGINAL_PROMPT.md` (the user's full
master directive, verbatim, ~40 KB) and `docs/ARCHITECTURE.md`, `docs/SECURITY.md`, `docs/DECISIONS.md`.
Work in `C:\Users\raynh\forge` on Windows 11 (Git Bash and PowerShell both available; Tauri WebView2 app).

## 1. The ultimate goal (the user's original prompt, condensed)

The user gave a very large "FORGE — The Agentic Engineering Operating System / MASTER BUILD DIRECTIVE" and asked the assistant to act
as founding CTO, architect, security engineer and lead developer and to **build FORGE as real, working software, not a plan**:

* A **Windows-first desktop app** (Tauri 2: React/TypeScript/Tailwind UI, Rust core, Node "provider engine" sidecar, SQLite + FTS5,
  Git CLI) that **orchestrates existing coding agents** (Claude Code now, Codex next) **without reimplementing them**.
* Product pillars:
  1. **Mission Control** — missions run in isolated git worktrees; streaming, approvals, cancel, recovery.
  2. **Mission Contracts + Proof Engine** — acceptance criteria, checks, Proof Packs, safe merge.
  3. **FORGE Memory** — an LLM-wiki-style, provenance-tracked knowledge base (Obsidian integration, context compiler/inspector).
  4. **Harness Studio** — inventory/safe-edit of CLAUDE.md, AGENTS.md, skills, subagents, hooks; Doctor; preview/backup/rollback.
  5. Multi-agent, experiments, and enterprise foundations (**design only — never claim enterprise features that do not exist**).
* **Hard rules from the directive:** no fake functionality; real provider integrations; real git; real verification; no hidden API
  billing; no unsafe hooks; honest reporting of what is and is not verified; maintain docs (PRODUCT, ARCHITECTURE, DATA_MODEL,
  SECURITY, ROADMAP, PROGRESS, DECISIONS, ENGINEERING_REVIEWS); never expose credentials; do not use separately billed API auth;
  do not capture provider OAuth tokens; use GPT-6 Astra (High) through the local Codex CLI for **selective** independent reviews
  (read-only, ephemeral, no API billing) and never accept either model's advice blindly.
* Phased delivery with verification gates (Phases 0–7). Prioritise **completed, verified functionality over breadth**.

### Instructions the user gave during the session (still binding)

* "Don't ask me if you want to download or install something. Do whatever you want. You have all privilege." — act, don't ask.
* "Use High reasoning effort for architecture, complex integrations, security and difficult debugging. Delegate isolated,
  straightforward implementation to Medium-effort subagents. Consult GPT-6 Astra High selectively. Prioritise completed, verified
  functionality over breadth."
* "If you want to test the app with the agents, use my **local models** so we save my Claude limits." The user's Claude usage was
  ~96% of the 7-day limit. **Never run the live Claude test** (`supervisor::tests::live_claude_mission_end_to_end`) and never use
  `cargo test --include-ignored` with a broad filter (it runs that test). Run live tests by exact name; local-model ones are
  `live_local_*` and `live_harness_*`.
* The user is now **short on limits**; they want this repository handed over to Codex and published publicly.

## 2. What exists (state at hand-off)

Repo: `C:\Users\raynh\forge` (npm workspaces: `apps/desktop`, `packages/engine`). ~15 commits on `main`.

```
apps/desktop/               React + Vite + Tailwind UI (src/), Tauri app (src-tauri/)
  src-tauri/src/            Rust core: supervisor, store (SQLite), policy, git, merge, verify, contract, proof, knowledge/, harness/
  src/                      screens/, components/, lib/ (types.ts + api.ts = the UI<->core contract)
packages/engine/            Node engine: one process per agent session, JSONL over stdio; adapters: claude, fake (scripted test agent)
tools/e2e/                  CDP drivers for the REAL window (smoke, mission-flow, knowledge-flow, harness-flow) — see §4
tools/package/prepare.mjs   stages engine bundle + node.exe for the installer
docs/                       PRODUCT, ARCHITECTURE, DATA_MODEL, SECURITY, DECISIONS, ROADMAP, ENGINEERING_REVIEWS, ORIGINAL_PROMPT
research/                   codex-probe.mjs (Codex app-server exploration script); schema-codex/ is git-ignored (regenerate, §6)
```

Architecture in one paragraph: the Rust core owns persistence, policy, approvals, git and process control. For each agent session it
spawns one Node engine process (`packages/engine/dist/main.js`, or the esbuild bundle when packaged) and speaks newline-delimited JSON
(`protocol.ts`). The Claude adapter uses the official Agent SDK pointed at the user's installed `claude`; every tool call goes through an
SDK `PreToolUse` hook to the core's policy (Allow/Deny/Ask) and, when needed, a human approval card. Repo-defined hooks/MCP are
neutralised (`allowManagedHooksOnly`, `strictMcpConfig`, empty `mcpServers`) while CLAUDE.md etc. still load. Windows Job Objects contain
processes. Missions run in git worktrees under the FORGE home; review binds approvals to a change-set fingerprint and merges the
*reviewed tree* via `commit-tree` + CAS `update-ref`.

### Phase status (honest)

| Phase | Status | Evidence |
|---|---|---|
| 1 Mission Control (core, Claude adapter, UI) | **Done** | live real-Claude mission (haiku) and real local-model mission through FORGE; real-window E2E |
| 2 Contracts, checks, review, Proof Pack, safe merge | **Done** | `tools/e2e/mission-flow.mjs` in the real window incl. the Proof Pack appearing after merge; 3 independent reviews fixed |
| 3 Memory (knowledge, vault, retrieval, inspector) | **Done** | `tools/e2e/knowledge-flow.mjs`; live gate with a real local agent: with memory → correct private facts, without → hallucination |
| 4 Harness Studio (inventory, Doctor, safe edit, hooks, dry-run, rollback) | **Done** | `tools/e2e/harness-flow.mjs`; live gate: `live_harness_hook_and_instruction_written_by_forge_take_effect_in_real_claude_code` (real Claude Code, local model) saw the hook fire and obeyed the written instruction; independent review 4 fixed (2 items deliberately open, §5) |
| 5 Multi-agent: parallel worktrees + concurrency limit | **Done** (core, tested with scripted agent) | |
| 5 Codex adapter | **Not started in code** — protocol explored, key findings in §6 | `research/codex-probe.mjs` |
| 6 Experiments | Not started | |
| 7 Enterprise | Design only; no claims | |
| Windows installer | **Built and install-tested** (silent install, real mission, bundled Node used): `FORGE_0.1.0_x64-setup.exe` (25.75 MiB, NSIS, per-user) via `npm run package` | built at `apps/desktop/src-tauri/target/release/bundle/nsis/` (git-ignored); the engine bundle was verified separately by running the Rust supervisor suite against a copy of `main.js` with no `node_modules` |

Last recorded test counts (re-run to confirm): Rust ~277 passing + ~20 later harness tests (4 live tests ignored), engine 36, UI 100
(vitest). `docs/PROGRESS.md` is **missing** — write it (the directive requires it) with exact counts and evidence levels.
`docs/ROADMAP.md` statuses are stale (still show Phase 2–4 as not started) — update them.

## 3. Hard invariants — do not weaken these (each has tests)

* The agent never decides: approvals are single-shot, bound to `tool_use_id:sha256(tool+canonical input)`; policy lives in Rust.
* Writes outside the worktree and to protected paths (`.git`, `.claude/settings*|hooks|agents|skills`, `.codex`, `.forge`, `.env*`) are denied by policy.
* Paths are compared on the **real** path (junctions, 8.3 names, ADS, device names rejected). Node cannot load verbatim `\\?\` paths — use `dunce`.
* No hidden API billing: `ANTHROPIC_API_KEY` & co are stripped from engine env. Local-model endpoints must be plain `http` on loopback.
* Git runs with `core.hooksPath` empty; repos configuring non-system filters/merge drivers are refused; never `reset --hard`; merge commits the reviewed tree.
* Memory: trust lives in SQLite bound to the content hash; only human-verified pages are injected, into the **first user turn**, never the system prompt.
* Harness edits: allowlist of project files only (never `~/.claude`, `.mcp.json`); preview → compare-and-swap → backup → journal → atomic write → read-back; every new/changed hook requires explicit acknowledgement; rollback refuses on conflict.
* Do not add false security/enterprise claims. `docs/SECURITY.md` lists known limits honestly — keep it honest.

## 4. How to build, run and test

```powershell
npm install                                   # root, workspaces
npm run build:engine                          # tsc -> packages/engine/dist (the Rust tests need this)
cd apps/desktop/src-tauri; cargo test --lib   # Rust suite (needs `node`, `git`; Git Bash for hook tests; Ollama NOT needed)
npm run test -w @forge/engine                 # engine tests
npm run test -w forge-desktop                 # UI tests (vitest) ; npm run typecheck -w forge-desktop
npm run package                               # installer (stages engine bundle + node.exe; tauri.bundle.conf.json)
```

Run the real app with remote debugging so the E2E scripts can drive the real WebView2 window:

```bash
FORGE_HOME="C:\\Users\\raynh\\forge\\.forge-tmp\\home" FORGE_ALLOW_FAKE_PROVIDER=1 \
WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222" npm run tauri -w forge-desktop -- dev
node tools/e2e/mission-flow.mjs  .forge-tmp/shots     # also: knowledge-flow.mjs, harness-flow.mjs, smoke.mjs
```

Use a fresh `FORGE_HOME` per run. Kill only your own dev processes (filter by command line); the user has other processes running.

Local-model live tests (free; Ollama must be running with the tag `forge-qwen3.5-9b-32k`, i.e. `qwen3.5:9b` with `num_ctx 32768`):
`cargo test --lib live_local_model_mission_end_to_end -- --ignored --nocapture`, `live_local_memory_gate_with_a_real_agent`,
`live_harness_hook_and_instruction_written_by_forge_take_effect_in_real_claude_code`. **Never** run `live_claude_mission_end_to_end`.

### Windows / tooling lessons (cost real time)

* Real-window clicks need **real mouse events** (CDP `Input.dispatchMouseEvent`), not `el.click()` (Radix). `tools/e2e/cdp.mjs` has `realClick`, `clickButton`, `clickTab`.
* Tauri blocks `window.confirm/alert` — the UI must use in-app Modals (a test forbids native dialogs).
* **A test that only checks a command validates is not a test** — execute it in the real shell. (A template quoting bug and a Windows `shell:true` file-name injection were both found this way / by review.) Hook commands run in Git Bash; avoid backslashes inside `node -e "..."`.
* Shell tool quirks: heredocs with quotes/backslashes get mangled — write patch scripts to files; commit before any mutation experiment; avoid `git checkout -- file` on uncommitted work.
* `*.log` and `.forge-tmp/` are git-ignored on purpose (probe logs contain the user's MCP config and paths). Do not commit them.

## 5. Known gaps and open items (priority order)

1. **Codex adapter (Phase 5)** — see §6; this was the work in progress when the session ended.
2. **Install-test the installer** (`FORGE_0.1.0_x64-setup.exe`): install, launch, run a scripted mission, confirm the bundled `node.exe` and engine are located (`locate_node`/`locate_engine` in `lib.rs`). Not done.
3. **Write `docs/PROGRESS.md`**; fix stale `docs/ROADMAP.md`; add the `harness_*` rows and migrations 5–6 to `docs/DATA_MODEL.md` if anything changed since.
4. Harness items deliberately **not** done (documented in ENGINEERING_REVIEWS.md review 4 and SECURITY.md limits 3, 13–15): handle-pinned directory identities / exclusive-create for the write path (a concurrent local process could still win a microsecond race), spawning hook dry-runs suspended before Job assignment, backup retention/pruning.
5. UI polish: the mission workspace has 7 tabs (the strip scrolls; verify it looks right); `inventory.rs` has an unused-`mut` warning.
6. Phase 6 (experiments) and Phase 7 (design only).
7. A further independent review (Astra, read-only) of the Codex adapter before relying on it. Consultation budget used: 4 of 4–6 (reviews 1–4 in `docs/ENGINEERING_REVIEWS.md`). Invoke with: `codex exec -m gpt-6-astra -c model_reasoning_effort=high -s read-only --ephemeral -C <repo> -o <out.txt> -` and a prompt on stdin; evaluate every finding yourself (reproduce, then adopt/adapt/decline).

## 6. Codex adapter — everything learned so far (do not re-derive)

Codex CLI 0.163.0 is installed as `C:\Users\raynh\AppData\Local\Programs\OpenAI\Codex\bin\codex.exe` (a real exe, so it can be spawned
without a shell; npm `.cmd` shims cannot be spawned directly by modern Node). Auth is the user's existing ChatGPT login in `~/.codex`; FORGE
must never read/copy tokens. Strip `OPENAI_API_KEY`/`CODEX_API_KEY`/`OPENAI_BASE_URL` from the child env so a mission cannot silently bill an API.
Regenerate the protocol schema if needed: `codex app-server generate-json-schema --out research/schema-codex`.
`research/codex-probe.mjs` is a working JSON-RPC client (env: `CODEX_EXE`, `PROBE_ARGS`, `PROBE_THREAD_CONFIG`, `PROBE_EFFORT`,
`PROBE_CONFIGREAD`, `PROBE_LIST`; usage `node research/codex-probe.mjs <cwd> "<prompt>" [model] [modelProvider]`).

**Protocol (stdio JSONL JSON-RPC, `codex app-server`):** `initialize{clientInfo}` → notify `initialized` → `thread/start{cwd, approvalPolicy,
sandbox, model?, modelProvider?, ephemeral?, config?, developerInstructions?}` (response `thread.id`) → `turn/start{threadId, input:[{type:'text',text,text_elements:[]}], effort?}`.
`thread/resume{threadId,…}` resumes; `turn/interrupt{threadId,turnId}` cancels; `config/read{cwd}` returns the *effective* config; `model/list` lists models
(`gpt-6.1-sol`, `gpt-6-astra`, `gpt-6-sol`, `gpt-6-luna`, `gpt-5.6-*` …; low effort via `effort:'low'`).
Notifications: `item/started|completed` (item types: `userMessage`, `agentMessage` {text, phase}, `reasoning` {summary}, `commandExecution` {command, cwd, status, exitCode, aggregatedOutput},
`fileChange` {changes:[{path, kind:{type:'add'|'update'|'delete'}, diff}], status}, `mcpToolCall`, `webSearch`, …), `item/agentMessage/delta`, `turn/started|completed` (turn.status `completed|interrupted|failed`, `turn.error.message`),
`thread/tokenUsage/updated` (`tokenUsage.total|last` {inputTokens, cachedInputTokens, outputTokens, reasoningOutputTokens}), `error{willRetry}`, `warning`, `mcpServer/startupStatus/updated`, `hook/*`.
**Server→client requests that need an answer** (never leave one hanging): `item/commandExecution/requestApproval` (params: itemId, command, cwd, reason, networkApprovalContext…; result `{decision:'accept'|'acceptForSession'|'decline'|'cancel'|…}`),
`item/fileChange/requestApproval` (params carry only itemId/reason/grantRoot — the file paths are in the earlier `item/started` fileChange item with the same id; result `{decision:'accept'|'acceptForSession'|'decline'|'cancel'}`),
plus `item/permissions/requestApproval`, `item/tool/requestUserInput`, `mcpServer/elicitation/request`, `item/tool/call`, `account/chatgptAuthTokens/refresh`, `attestation/generate`, legacy `applyPatchApproval`/`execCommandApproval` → answer with a JSON-RPC error / decline.
Verified live (gpt-5.6-luna, low effort): with `approvalPolicy:'untrusted'`, `sandbox:'workspace-write'` a file creation produced `fileChange` approval requests and a shell command a `commandExecution` approval (the command arrives wrapped as `"powershell.exe" -Command "…"`); declining made the item `declined` and the agent continued.
Local models via Codex (`modelProvider:'ollama'`, model `forge-qwen3.5-9b-32k`) run and answer, but the model gets **no shell/file tools**, so approvals cannot be exercised locally — use a small OpenAI model sparingly (user's Codex quota is limited too).

**SECURITY FINDING (reproduced): a hostile repo's `.codex/config.toml` MCP server RUNS.** Codex prints "Project-local config, hooks, and exec policies are disabled … until the project is trusted", but an
`[mcp_servers.evil] command="node" args=["-e","…writeFileSync('PWNED_MCP.txt')"]` in the repo still started and executed code (project `hooks.json` hooks did not run). Also the user's *global* Codex config loads ~10 MCP servers and plugin hooks into every session.
What works (verified): `-c mcp_servers={}` and a thread-level `config:{mcp_servers:{}}` do **not** remove servers; **per-name `{enabled:false}` does**. So the adapter must (1) call `config/read{cwd: worktree}` first, (2) read `config.mcp_servers` keys (this includes repo-defined servers — origins show `mcp_servers.evil.*`), (3) pass
`config:{mcp_servers:{<every name>:{enabled:false}}}` in `thread/start`, (4) start `codex app-server` with `--disable hooks --disable plugins --disable apps --disable computer_use --disable browser_use --disable browser_use_external --disable multi_agent --disable remote_plugin`
(plugin-provided servers `chatcut`/`cua_repl` and `codex_apps` still started when only per-name disabling was used — the last experiment, combining the flags **and** per-name disabling, was about to be run when the session ended: **run it and confirm zero `mcpServer/startupStatus` lines and no `PWNED_*` files** using `.forge-tmp/codexhostile`-style fixtures),
(5) after `thread/start` call `mcpServerStatus/list` as a tripwire and abort the session if anything is running. Check that `--disable <unknown-feature>` does not break future Codex versions.

**Adapter design to implement** (mirror `adapters/claude.ts`; contract in `adapters/types.ts`, `protocol.ts`):
`packages/engine/src/adapters/codex.ts` (+ `codexRpc.ts` JSON-RPC client over child stdio, injectable spawn for tests). Add `codexPath` to `StartCommand`, `adapterFor('codex')` in `main.ts`.
Map: `agentMessage` delta → `ctx.delta`, completed → `agent.text`; `reasoning` summary → `agent.thinking`; `commandExecution` started → `tool.call` (name `PowerShell`/`Bash`, input `{command}`), completed → `tool.result`;
`fileChange` → `tool.call` name `Write`/`Edit`, completed → `tool.result` + `file.changed`; usage from `thread/tokenUsage/updated`; `turn/completed` → `ctx.turnComplete` (`failed`→`error`, `interrupted`→`interrupted`); `ctx.providerSession(threadId)`.
Approvals: ask `ctx.requestPermission` — one request per changed path for file changes (tool `Write`/`Edit`, `input.file_path`) so the Rust policy's path/protected-file rules apply; `commandExecution` → tool `PowerShell` with the full command (policy's push/risky-shell regexes match on it); network approvals → `WebFetch`. allow → `accept`, deny → `decline` (not `cancel`, so the agent can continue). Unknown server requests → JSON-RPC error. `cancel` → `turn/interrupt` then close; kill the child tree on engine exit.
Thread settings: `approvalPolicy:'untrusted'`, `sandbox:'workspace-write'` (Codex's own sandbox is defence in depth that the Claude path lacks — verify it actually applies on this machine: user config has `[windows] sandbox="elevated"`), `approvalsReviewer:'user'`, `developerInstructions` = mission guidance (not the user turn), memory block in the first user turn as for Claude.
Rust side: `Config.codex: Option<PathBuf>` (update all `Config {…}` literals incl. tests), resolve the real exe (not a shim), pass `codexPath` in the start JSON (`supervisor.rs` `spawn_session`, ~line 583), `providers.rs`: real capability matrix and `adapter_available` for codex (currently hard-coded false), `commands.rs` `providers_detect`, mission-create provider validation, UI new-mission dialog provider choice (UI contract in `apps/desktop/src/lib/types.ts`/`api.ts`).
Tests: engine unit tests against a fake app-server script (allow/deny/cancel/unknown request/crash); one live test with a small model by exact name, ignored by default; add a hostile-repo regression test like `packages/engine/scripts/lockdown-experiment.mjs` for Codex. Update `docs/SECURITY.md` (Codex controls + the MCP finding), `docs/ARCHITECTURE.md`, `docs/DECISIONS.md`, `docs/ROADMAP.md`.

## 7. Conventions

* Match surrounding code style and comment density; Rust errors via `ForgeError`; every state change persisted before it is pushed to the UI; new UiEvents are added in `supervisor.rs` + `types.ts` + `reducers.ts`.
* Migrations are forward-only (`db.rs` `MIGRATIONS`, currently v1–v6). Never edit a released migration; add a new one.
* The UI/core contract is `apps/desktop/src/lib/types.ts` + `api.ts`; keep both in sync with `commands.rs` (camelCase via serde).
* After UI changes run typecheck + vitest + build, and **verify in the real window** with the CDP scripts (unit tests missed three real bugs earlier).
* Commit messages: describe the why; end with the attribution line your tooling requires.
* Report honestly: say what is verified, by what evidence, and what is not. Never write "done" for something only compiled.
