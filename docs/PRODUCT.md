# FORGE — Product

**FORGE is an agentic engineering operating system:** one local cockpit that makes existing coding agents (Claude Code
now, Codex next) more effective, observable, verifiable and cumulative. It does not replace the agents; it owns
orchestration, policy, memory, evidence and UX around them.

## The test for every feature
Does it (1) reduce repeated developer work, (2) improve agent correctness/reliability, (3) preserve knowledge across
sessions, (4) make complex work easier to understand, (5) improve control/verification/security, or (6) enable a
workflow existing tools handle poorly? If none, it does not ship.

## Differentiators (in build order)
1. **Mission Control** — missions in isolated git worktrees, real streaming activity, enforceable approvals, cancel,
   restart-safe history. *(Phase 1 — implemented, see PROGRESS.md.)*
2. **Proof Engine** — mission contracts with acceptance criteria; a Proof Pack ties each criterion to changed code,
   a check, and its result. No invented confidence scores. *(Phase 2.)*
3. **FORGE Memory** — raw evidence → interlinked Markdown wiki → proposed executable knowledge, with provenance,
   staleness and conflict handling; SQLite FTS5 first; Obsidian as an optional view. *(Phase 3.)*
4. **Harness Studio** — inspect/edit CLAUDE.md, AGENTS.md, skills, subagents and hooks safely (preview, backup,
   rollback). *(Phase 4.)*
5. **Multi-agent** and **Experiments** — second provider, parallel missions, controlled optimisation runs. *(5, 6.)*

## Principles distilled from the primary sources read so far
* *Karpathy's LLM Wiki (read):* knowledge should be **compiled once and kept current**, not rediscovered per query:
  immutable raw sources → LLM-maintained interlinked wiki → a schema file telling the maintainer how to behave; with
  ingest / query / **lint** operations, an `index.md` catalogue and an append-only `log.md`. FORGE Memory adopts that
  structure but adds what a team needs: provenance hashes, verification states, human promotion, and the rule that
  generated memory never gains authority to execute.
* The other sources named in the brief (AutoResearch, OpenAI harness engineering, Anthropic customisation guidance,
  Codex-as-platform) are scheduled for reading at the start of the phase that depends on them (6, 4, 4, 5) so decisions
  are made on the text rather than on summaries.

## Non-goals
Another chat UI, an IDE clone, a token dashboard, a terminal wrapper, a reimplementation of any agent harness, or
enterprise claims without enterprise infrastructure.

## V1 definition of done
See the 14-point list in the build brief; PROGRESS.md tracks each with its evidence level
(verified live / verified controlled / not started).
