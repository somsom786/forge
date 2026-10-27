# FORGE

A desktop mission control for AI coding agents. You give FORGE a task. It runs Claude Code, Codex or any agent that
speaks the Agent Client Protocol (ACP) in an isolated git worktree, puts every risky action in front of you for approval,
verifies the result with real checks and scanners, and records what was established in a Proof Pack. It keeps what the
team learned as reviewed project memory.

FORGE does not reimplement agents. It sits around them and owns what they do not: authorization, parallel workspaces,
evidence, review and memory.

> **Status: pre-release.** Built and tested on Linux and on GitHub's Windows runners. The Windows desktop app on a real
> PC has not been re-validated on this branch (see `docs/CODEX_WINDOWS_HANDOFF.md`). Installers are **unsigned** CI
> artifacts, not releases. There is no licence yet (`docs/LICENSING_DECISION.md`).

## Why

Coding agents are capable, but running them on real repositories raises questions their own interfaces leave to you:

- What exactly did the agent touch?
- Did anything run that you did not agree to?
- Do the tests actually pass on *these* changes?
- Can three agents work at once without trampling each other?
- Will the next agent know what the last one learned?

FORGE makes those answers explicit and checkable.

## What it does today

| Area | Implemented | Notes |
|---|---|---|
| **Missions** | One git worktree and branch per mission; parallel missions; restart recovery; merge or discard, guarded against losing unsaved work | Merges are recorded before git runs and reconciled after a crash |
| **Approvals** | Every agent request goes through FORGE's policy (allow / ask / deny) in the Rust core. Approvals are bound to a digest of the exact request, expire, are re-checked at decision time, and are never reused for a changed request | Shell rules are heuristics; the human reading each prompt is the control (`docs/SECURITY.md`) |
| **Agents** | Claude Code (Agent SDK, repository hooks and MCP locked down), Codex (app-server), any ACP agent (official SDK), and a scripted test agent | Uses the subscription logins you already have. No API-key billing path |
| **Isolation** | Checks and experiments can run in a Linux namespace sandbox (bubblewrap). On Linux, **agents themselves** can be contained too: no host files beyond the worktree and runtime | Windows Sandbox and isolated WSL are designed, not implemented; on Windows everything runs as you, labelled "not isolated" |
| **MCP** | Servers run only when authorized, pinned by hash, with tool allow-lists enforced by the core | An authorized stdio server is a program running with your privileges |
| **Proof Engine** | Acceptance criteria linked to checks; freshness against the exact diff; gitleaks, OSV-Scanner, Trivy and Playwright adapters with structured findings; Proof Packs (Markdown and JSON, hashed) | A Proof Pack records evidence; it is not a guarantee |
| **Memory** | Project knowledge pages (and an Obsidian vault, read-only unless you grant write access) with human verification bound to content hashes; only verified pages reach agents, labelled as data | Lexical retrieval (SQLite FTS5); no embeddings |
| **Code intelligence** | tree-sitter symbol index (Rust, TS/TSX, JS, Python), Code Map, relevant code added to mission context | Evaluated on nine real repositories (`docs/EVALUATION.md`) |
| **Harness Studio** | Inventory, Doctor (including settings precedence), safe editing with preview and rollback, hook templates with dry-runs | |
| **Storage** | Usage view and a previewed, confirmed clean-up of old finished missions' logs and transcripts | Nothing is removed automatically |

Not built: enterprise features, cloud sync, a plugin marketplace. No claim is made that FORGE makes agents faster or
better than running them directly: that comparison needs live runs which have not been done (`docs/EVALUATION.md`).

## Platforms

| Platform | State |
|---|---|
| Windows 10/11 (x64) | Primary target. CI builds the NSIS installer and runs the full test suite on `windows-2022`. A physical PC was last validated before this branch |
| Linux | Development and CI platform. All tests run, including agent sandboxing and the real Claude Code binary against a scripted local model |
| macOS | Not built or tested |

## Architecture

```
React UI (Tauri webview) ──IPC──► Rust core: policy, approvals, worktrees, store (SQLite WAL), evidence, memory, isolation
                                      │ JSON lines over stdio, one engine process per session
                                      ▼
                               Node engine (packages/engine): adapters for Claude Agent SDK, Codex app-server, ACP, scripted agent
                                      │
                                      ▼
                               The agent you installed (claude, codex, an ACP agent), signed in with your own account
```

All authority lives in the Rust core. The engine and the agents can only *request*. Details: `docs/ARCHITECTURE.md`,
`docs/SECURITY.md`, `docs/EXECUTION.md`, `docs/DATA_MODEL.md`.

## Development

Requirements:

- Node 22+ with its bundled npm.
- Rust stable.
- Git.
- On Linux, the Tauri system libraries: `libwebkit2gtk-4.1-dev`, `libgtk-3-dev`, `librsvg2-dev`, `libayatana-appindicator3-dev`.
- Optionally `bwrap` (bubblewrap) for the isolation backend.

```sh
npm ci
npm run dev                      # engine build + Tauri dev app
npm run dev -w forge-desktop     # UI only, in a browser, against built-in preview data (no core)
npm run package                  # Windows: NSIS installer (unsigned), with THIRD_PARTY_NOTICES.txt
```

Packaged Claude integration requires a separately installed Agent SDK `0.3.309`; the Providers screen gives the installation command. The installer ships setup instructions and a build manifest, and excludes the SDK's code. Codex and ACP do not require it. Details and remaining terms decisions: `docs/LICENSING_DECISION.md`.

## Tests

```sh
npm run test -w @forge/engine                                       # engine (adapters, protocol, ACP)
npm run test -w forge-desktop                                       # UI unit and component tests
npm run e2e -w forge-desktop                                        # browser end-to-end on the preview (simulated backend)
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib  # Rust core (~430 tests)
```

Some tests need real tools and skip without them; CI sets these variables so that a missing tool fails instead:

| Variable | Requires |
|---|---|
| `FORGE_REQUIRE_SANDBOX_TESTS=1` | bubblewrap |
| `FORGE_REQUIRE_SCANNER_TESTS=1` | gitleaks, osv-scanner |
| `FORGE_REQUIRE_CLAUDE_REAL_TESTS=1` | the Claude Code binary from `node_modules`, run against a scripted local model; no account, no network |
| `FORGE_REQUIRE_ACP_INTEROP=1` | a real third-party ACP agent (`claude-agent-acp`) |

On-demand measurements:

- `FORGE_BENCH=1 cargo test --release --lib bench_` (`docs/PERFORMANCE.md`)
- `scripts/codeintel-eval.sh` (`docs/EVALUATION.md`)

Live provider tests are `#[ignore]`d and never run in CI.

What has been verified where: `docs/VALIDATION_MATRIX.md`.

## Security

FORGE runs coding agents on your machine with your account. Read `docs/SECURITY.md` before relying on it. In particular:

- On Windows, and by default on Linux, **an action you approve runs with your privileges**.
- Shell-command rules are heuristics.
- Contained agents on Linux still have network access.
- The webview is trusted by the core.
- An authorized MCP server is a program running as you.

Known open risks are in `docs/REMAINING_RISKS.md`.

## Contributing

Work on a branch and open a pull request against `main`. CI must be green:

- Linux and Windows tests;
- Windows PowerShell script checks;
- the installer build;
- the security workflow.

New dependencies are recorded in `docs/OSS_ADOPTION.md` with their licence and reason. GPL code is not accepted. Add a
test that fails without your change; for security fixes, a test that reproduces the attack. How contributions will be
licensed (DCO or CLA) depends on the licensing decision.

Roadmap: `docs/ROADMAP.md`. Honest quality assessment: `docs/CLOUD_QUALITY_AUDIT.md`.
