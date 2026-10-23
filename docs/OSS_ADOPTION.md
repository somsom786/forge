# Open-source adoption register

Policy: **DEPEND** (use a package), **INTEGRATE** (call an external program over a stable interface), **FORK** (copy/modify
code; isolated, with provenance and an upstream-review process), **STUDY** (learn from it, import nothing). Prefer upstream
dependencies to vendoring. Licences below were read from the installed artefacts (`package.json`, crate metadata, Debian
`copyright` files) on 2026-10-23, not assumed from a repository being public.

## Decisions taken in this programme

| Component | Upstream | Version | Licence | Strategy | Why | Security notes | Update process | Attribution | Local changes |
|---|---|---|---|---|---|---|---|---|---|
| bubblewrap | github.com/containers/bubblewrap | 0.9.0 (as installed by the distribution) | LGPL-2.0-or-later | **INTEGRATE** (executed as `bwrap`, not linked, not bundled) | The namespace sandbox behind the `linux_namespaces` execution backend; used by Flatpak, maintained, small | Security depends on the kernel's user namespaces and on bubblewrap itself; FORGE probes it and reports it unavailable rather than falling back | The user's distribution updates it; FORGE does not ship it | None required for invoking an LGPL program; name it in third-party notices if it is ever bundled | None |

Evaluated, not adopted yet (to be decided in the stage that needs them): ACP SDKs, tree-sitter, ast-grep, ripgrep, Gitleaks,
OSV-Scanner, Trivy, Playwright, hyperfine, Monaco, xyflow, xterm.js, Graphiti. Each will get a row with the fields above
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

### npm

| Package | Version | Licence |
|---|---|---|
| **@anthropic-ai/claude-agent-sdk** | 0.3.309 | **Proprietary**: "© Anthropic PBC. All rights reserved. Use is subject to the Legal Agreements" (code.claude.com/docs/en/legal-and-compliance). Bundled into the engine and the installer. **Redistribution in a commercial FORGE release must be checked against those terms before release.** |
| react, react-dom, zustand, cmdk, @radix-ui/*, tailwindcss, vite, vitest, esbuild | see lockfile | MIT |
| lucide-react | 1.61.0 | ISC |
| @tauri-apps/api, @tauri-apps/plugin-dialog | 2.x | MIT OR Apache-2.0 |
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
