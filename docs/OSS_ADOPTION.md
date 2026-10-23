# Open-source adoption register

Policy: **DEPEND** (use a package), **INTEGRATE** (call an external program over a stable interface), **FORK** (copy/modify
code; isolated, with provenance and an upstream-review process), **STUDY** (learn from it, import nothing). Prefer upstream
dependencies to vendoring. Licences below were read from the installed artefacts (`package.json`, crate metadata, Debian
`copyright` files) on 2026-10-23, not assumed from a repository being public.

## Decisions taken in this programme

| Component | Upstream | Version | Licence | Strategy | Why | Security notes | Update process | Attribution | Local changes |
|---|---|---|---|---|---|---|---|---|---|
| bubblewrap | github.com/containers/bubblewrap | 0.9.0 (as installed by the distribution) | LGPL-2.0-or-later | **INTEGRATE** (executed as `bwrap`, not linked, not bundled) | The namespace sandbox behind the `linux_namespaces` execution backend; used by Flatpak, maintained, small | Security depends on the kernel's user namespaces and on bubblewrap itself; FORGE probes it and reports it unavailable rather than falling back | The user's distribution updates it; FORGE does not ship it | None required for invoking an LGPL program; name it in third-party notices if it is ever bundled | None |
| @agentclientprotocol/sdk | github.com/agentclientprotocol/typescript-sdk | 1.8.0 (exact; published 2026-10-09; 2.x was days old and skipped) | Apache-2.0 (LICENSE file in the package) | **DEPEND** (npm, bundled into the engine by esbuild) | The official ACP implementation: FORGE is the client; the test fixture agent uses its agent side | Peer dependency `zod` already present (MIT, via the Claude Agent SDK); no install scripts; protocol version pinned to 1 and checked at `initialize` | Bump deliberately; re-run `packages/engine/test/acp.test.ts` and the Rust `tests::acp` suite | Apache-2.0 NOTICE/attribution in third-party notices when distributed | None |
| gitleaks, OSV-Scanner, Trivy, Playwright | github.com/gitleaks/gitleaks, github.com/google/osv-scanner, github.com/aquasecurity/trivy, github.com/microsoft/playwright | whatever the user installed (none bundled) | MIT (gitleaks), Apache-2.0 (the others), per their upstream repositories; not read from an installed artefact because none is installed here | **INTEGRATE** (FORGE parses their JSON reports; the user installs and runs them as checks) | Proof Engine 2.0 evidence adapters (`evidence.rs`) | FORGE never downloads or runs them on its own; output is untrusted input to a bounded parser | Re-validate the parsers against real reports when the user's versions change (Windows handoff) | None needed: not distributed | None |
| tree-sitter (Rust binding + C runtime) | github.com/tree-sitter/tree-sitter | 0.27.1 (published 2026-10-08) | MIT (crate `Cargo.toml` + LICENSE file) | **DEPEND** (crates.io; C runtime compiled by `cc` at build time) | Incremental parser library behind the code index (`codeintel/`); used by GitHub, Neovim, Zed | Parses untrusted repository files in-process (C code): bounded by a 512 KiB file cap, a 2 s per-file deadline via the progress callback, and a declarations-only walk. Default features only (`std`); the optional `wasm`/wasmtime feature is not enabled. Pulls in `tree-sitter-language` 0.1.9 (MIT) and `streaming-iterator` 0.1.9 (MIT OR Apache-2.0) | Bump deliberately; re-run `cargo test --lib codeintel` and `cargo check --target x86_64-pc-windows-gnu` | MIT notice in third-party notices | None |
| tree-sitter-rust | github.com/tree-sitter/tree-sitter-rust | 0.24.2 | MIT (crate `Cargo.toml` + LICENSE file) | **DEPEND** | Rust grammar (generated C parse tables + scanner) | Grammar code only; no queries are executed | As above | MIT notice | None |
| tree-sitter-typescript | github.com/tree-sitter/tree-sitter-typescript | 0.23.2 | MIT (declared in the crate `Cargo.toml`; the published crate ships no LICENSE file, the upstream repository's LICENSE is MIT) | **DEPEND** | TypeScript and TSX grammars | As above | As above | MIT notice | None |
| tree-sitter-javascript | github.com/tree-sitter/tree-sitter-javascript | 0.25.0 | MIT (crate `Cargo.toml` + LICENSE file) | **DEPEND** | JavaScript/JSX grammar | As above | As above | MIT notice | None |
| tree-sitter-python | github.com/tree-sitter/tree-sitter-python | 0.25.0 | MIT (crate `Cargo.toml` + LICENSE file) | **DEPEND** | Python grammar | As above | As above | MIT notice | None |

Evaluated, not adopted yet (to be decided in the stage that needs them): ast-grep, ripgrep, hyperfine, Monaco, xyflow,
xterm.js, Graphiti. Each will get a row with the fields above
before any code depends on it.

## Existing dependencies (inventory, not newly adopted)

### Rust (448 crates in `Cargo.lock` for `apps/desktop/src-tauri`)

| Licence (as declared) | Crates |
|---|---|
| MIT and/or Apache-2.0 (incl. "MIT/Apache-2.0", "Zlib OR Apache-2.0 OR MIT", LLVM-exception variants) | ~400 |
| Unicode-3.0 | 18 (ICU data via `url`/`idna`) |
| Unlicense OR MIT | 10 |
| **MPL-2.0** | `cssparser`, `cssparser-macros`, `dtoa-short`, `selectors`, `option-ext` (via Tauri/wry). File-level copyleft: distributable in a closed product; modified MPL files must be published |
| MIT OR Apache-2.0 OR LGPL-2.1-or-later | `r-efi` (choose MIT/Apache) |
| BSD-3-Clause | 2 |

Direct dependencies: chrono, dunce, globset, regex, rusqlite (+ bundled SQLite, public domain), serde, serde_json, sha2, similar,
tauri, tauri-plugin-dialog, thiserror, uuid, which, win32job, windows-sys. All MIT and/or Apache-2.0 (dunce adds CC0/MIT-0).
Added for code intelligence (rows above): tree-sitter and four grammar crates, all MIT.

### npm

| Package | Version | Licence |
|---|---|---|
| **@anthropic-ai/claude-agent-sdk** | 0.3.309 | **Proprietary**: "© Anthropic PBC. All rights reserved. Use is subject to the Legal Agreements" (code.claude.com/docs/en/legal-and-compliance). Bundled into the engine and the installer. **Redistribution in a commercial FORGE release must be checked against those terms before release.** |
| react, react-dom, zustand, cmdk, @radix-ui/*, tailwindcss, vite, vitest, esbuild | see lockfile | MIT |
| lucide-react | 1.61.0 | ISC |
| @tauri-apps/api, @tauri-apps/plugin-dialog | 2.x | MIT OR Apache-2.0 |
| @agentclientprotocol/sdk | 1.8.0 | Apache-2.0 (adopted in Milestone 3, row above) |
| typescript | 6.0.3 | Apache-2.0 |

### External programs FORGE invokes (not bundled)

| Program | Licence | Note |
|---|---|---|
| git | GPL-2.0 | Invoked as a separate program; not linked, not bundled. Fine to depend on. |
| Claude Code CLI | Proprietary (Anthropic) | The user's own installation and login |
| Codex CLI | Apache-2.0 (openai/codex) | The user's own installation and login |
| Node.js (bundled `node.exe` in the installer) | MIT (+ bundled third-party notices in its distribution) | Its LICENSE must ship with the installer |

## Obligations before a public release

1. Generate a full third-party notice file from `Cargo.lock` and `package-lock.json` (planned with the SBOM step, Stage H).
2. Resolve the Claude Agent SDK redistribution question with Anthropic's terms.
3. Ship Node.js's licence with the bundled `node.exe`.
4. FORGE itself has **no licence yet**; see BASELINE.md §5. The owner decides.
