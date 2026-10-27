# Licensing decision (for the owner)

## Runtime dependency change, 2026-10-27

FORGE's engine bundle now excludes the Claude Agent SDK. The SDK is an exact-pinned development dependency for tests and a separately installed runtime dependency for Claude users. Packaging checks the esbuild input graph to reject Anthropic SDK code; the installer includes dependency setup instructions and a build manifest. The loader and provider readiness checks require version `0.3.309` and never silently substitute another version. Existing Codex and ACP integrations do not need this SDK.

On Windows, install it yourself with `npm install --prefix "$env:LOCALAPPDATA\FORGEProviderDeps\claude-sdk" --save-exact @anthropic-ai/claude-agent-sdk@0.3.309`. Refresh providers afterward. An alternative install location can be selected with `FORGE_CLAUDE_SDK_PATH` set to the absolute package directory (`.../node_modules/@anthropic-ai/claude-agent-sdk`); restart FORGE after changing environment variables. FORGE performs no automatic package installation or credential copying.

This removes the SDK's code from the shipped engine; it is not a legal conclusion about commercial service use. The [official SDK README](https://github.com/anthropics/claude-agent-sdk-typescript#license-and-terms) points to Anthropic's Commercial Terms for SDK use, including customer-facing products. The owner must still verify the integration's terms and decide FORGE's own license. The table below describes the **previous bundled design**; its SDK redistribution blocker is superseded by the external-dependency implementation.

FORGE has no licence today: there is no `LICENSE` file, `forge-desktop` declares none (cargo-deny reports it as
unlicensed and the security workflow accepts exactly that one finding), and all rights are reserved by default. This
note lays out the realistic options so the owner can decide. **Nothing here is legal advice, and no licence has been
added.** Where it says "confirm", a lawyer or the vendor's terms decide, not this document.

## What FORGE ships, and what that already obliges

From `tools/package/notices.mjs` (Windows target, 2026-10-24), which now also writes `THIRD_PARTY_NOTICES.txt` into
the installer:

| Component | Count | Licences | Obligation when distributing |
|---|---|---|---|
| Rust crates compiled into `forge-desktop.exe` | 261 | MIT, MIT OR Apache-2.0, Unicode-3.0, ISC, BSD, Zlib; **MPL-2.0** (cssparser, selectors, dtoa-short, option-ext, via Tauri/wry) | Keep copyright and licence notices; reproduce Apache `NOTICE` files; MPL-2.0: those files stay MPL and their source must stay available (they are unmodified crates.io sources) |
| npm packages in the UI and the engine bundle | ≤ 202 (superset) | MIT, ISC, Apache-2.0, BSD | Notices, as above |
| Node.js runtime copied next to the engine | 1 | MIT plus its bundled components (OpenSSL Apache-2.0, ICU, …) | Ship Node's `LICENSE` (the generator includes it when present) |
| **`@anthropic-ai/claude-agent-sdk` (JavaScript, inside the engine bundle)** | 1 | "© Anthropic PBC. All rights reserved. Use is subject to the Legal Agreements" | **Confirm** that redistributing the SDK inside a third-party installer is permitted by Anthropic's terms for this kind of product. Until confirmed, this is the one blocker for any public binary release, whichever licence FORGE picks |
| bubblewrap (Linux isolation) | — | LGPL-2.0-or-later | Executed, not linked or shipped: no obligation unless bundled |
| Claude Code, Codex, ACP agents | — | their own | Not shipped: the user installs and signs in to them |

The external-dependency option is now implemented: it loads a separately installed SDK package, rather than assuming the user's Claude Code installation includes the SDK. Direct CLI integration remains an alternative, not implemented.

**Compatibility:** every shipped dependency is compatible with MIT, Apache-2.0 or a proprietary licence for FORGE's own
code. The constraint runs the other way: GPL code must not be imported into FORGE (policy already in
`docs/OSS_ADOPTION.md`).

## The options

| | MIT | Apache-2.0 | Proprietary / commercial (incl. source-available such as BSL/FSL) |
|---|---|---|---|
| Who can use, modify, sell | Anyone, for anything | Anyone, for anything | Only as the owner allows |
| Patents | Silent (an implied licence at best) | Explicit patent grant from contributors; terminates for anyone who sues over patents in the work | Whatever the terms say |
| What users must do | Keep the copyright notice | Keep notices, state changes, reproduce `NOTICE` | Accept the terms; usually a licence key or agreement |
| Enterprise acceptance | High | Highest (legal teams prefer the explicit patent terms) | Depends on terms; procurement involved |
| Contributions | Inbound = outbound works; DCO sign-off is enough | Same; Apache §5 already says contributions are under Apache-2.0 | Needs a CLA or employment/assignment for every contributor, otherwise the owner cannot license their code |
| Owner can later sell or relicense | Yes for own code; others' contributions stay MIT unless a CLA was signed | Same | Yes, if every contribution was assigned or licensed under a CLA |
| Ecosystem fit | Matches much of npm | Matches the ACP SDK (Apache-2.0) and Rust's MIT OR Apache-2.0 norm | Unusual for developer tools that run on users' code; harder to build trust in a security-sensitive tool |
| Competitors can fork it | Yes | Yes | No (BSL/FSL: not until the change date) |

**CLA or DCO.** A CLA (contributors license or assign their rights to the owner) is only needed if the owner wants to
relicense or sell contributed code later. A DCO (`Signed-off-by:` in each commit) records provenance with no paperwork,
and suits an open-source licence where inbound equals outbound.

## Recommendation

1. **Resolve the Claude Agent SDK redistribution question first.** It blocks a public installer under any licence.
2. If FORGE is to be open source: **Apache-2.0 with DCO sign-off.**
   - The patent grant and termination matter for a tool enterprises will run against their own code.
   - It matches the ACP SDK and the Rust ecosystem.
   - Every shipped dependency is compatible with it.
   - Keep the name "FORGE" out of the licence. Common words are hard to protect as trademarks: run a trademark search
     before any branding investment.
3. If there is a real chance of a commercial edition: decide that **before** accepting outside contributions.
   - Either keep the core Apache-2.0 and build paid features in a separate repository,
   - or adopt a CLA from the first external contribution.
   - Relicensing later without one requires every contributor's consent.
4. **Proprietary only** if the owner does not want public contributions and accepts slower adoption and trust building
   for a security-sensitive developer tool.

## Mechanics once decided

These are not done, because they would pre-empt the decision.

- **Apache-2.0:**
  - Add `LICENSE` (full text) and a short `NOTICE`.
  - Set `license = "Apache-2.0"` in `apps/desktop/src-tauri/Cargo.toml` and `"license"` in the three `package.json` files.
  - Remove the cargo-deny exception in `.github/workflows/security.yml`.
  - Add a DCO check, and a `CONTRIBUTING.md` paragraph about it.
- **Proprietary:**
  - Add the terms and `publish = false` plus `[licenses.private] ignore = true` in `deny.toml`.
  - Add a CLA process before taking any contribution.
- **Either way:**
  - Keep `THIRD_PARTY_NOTICES.txt` in the installer (now automatic).
  - Keep the SBOMs from the security workflow with each release.
