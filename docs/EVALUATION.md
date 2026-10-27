# Evaluation

Continuation 2026-10-27: no retrieval or productivity benchmark was rerun or changed in this increment. The figures below remain historical; held-out measurements and a native-agent comparison are still outstanding.

What has been measured about FORGE's retrieval, how, and what it does **not** show. Nothing here compares FORGE missions
with native Claude Code or Codex sessions: that needs live missions on a provider account and has not been run (see
"Native agent vs FORGE" below).

## Code intelligence: commit localization on real repositories (2026-10-23)

**Question.** Given a task description, does FORGE's symbol index point at the files that actually need changing, and
does it do better than the obvious baseline (keyword grep)?

**Method** (`apps/desktop/src-tauri/src/codeintel/eval_oss.rs`, reproduce with `scripts/codeintel-eval.sh`):

* Nine public repositories, pinned by commit, in four languages. Each is read only (git + tree-sitter); nothing in them
  is executed.
* Ground truth comes from each repository's own history, selected by a fixed rule, not by hand: among the last 600
  non-merge commits, the first 30 whose subject reads like a task (≥ 4 words; not a release, bump, merge, revert,
  format, lint, typo or docs commit) and that change 1–4 indexed source files outside tests/docs/examples.
* The **commit subject is the objective**; the **changed source files are the answer**. Both selectors run on the
  **parent commit's tree**, i.e. before the change existed.
* `grep`: files containing the most distinct objective words (case-insensitive), ties by path — what `grep -il` gives.
  `index`: `select_files`, the ranking FORGE uses to build a mission's "Relevant code" context.
* Metrics: precision, recall and hit rate (any changed file in the top k) at k = 3 and 5; MRR over the top 10; for the
  context block FORGE actually injects (2,000-byte budget), whether it mentions a changed file and its size.

### Results — development set (the five repositories looked at while working)

| Repository (language) | Files / symbols | hit@3 grep → index | R@5 grep → index | MRR@10 grep → index | Context block hits a changed file |
|---|---|---|---|---|---|
| ripgrep (Rust) | 221 / 3,328 | 0.17 → **0.47** | 0.32 → **0.57** | 0.17 → **0.40** | 70% (≈1.96 KB) |
| ky (TypeScript) | 101 / 324 | **0.63** → 0.43 | **0.53** → 0.52 | **0.45** → 0.43 | 87% (≈1.97 KB) |
| click (Python) | 163 / 1,298 | 0.50 → **0.70** | 0.63 → **0.88** | 0.36 → **0.60** | 90% (≈1.83 KB) |
| express (JavaScript, CommonJS) | 221 / 139 | **0.77** → 0.57 | **0.78** → 0.63 | 0.36 → **0.55** | 60% (≈0.32 KB) |
| zod (TypeScript) | 714 / 4,197 | 0.23 → **0.67** | 0.21 → **0.52** | 0.17 → **0.46** | 70% (≈1.89 KB) |

**A bug the evaluation found.** Before this run the extractor ignored CommonJS/prototype assignments
(`res.send = function send(body) {…}`, `exports.parse = …`, `Router.prototype.route = …`). On Express the index then
had 59 symbols and scored hit@3 **0.13**, MRR **0.12** (worse than grep). Extracting those assignments (a general
language-coverage fix, with its own unit test `commonjs_and_prototype_assignments_are_symbols`) gave the numbers above;
the other four repositories did not change.

### Results — held-out set (cloned after the decisions below were made, run once)

| Repository (language) | Files / symbols | hit@3 grep → index | R@5 grep → index | MRR@10 grep → index | Context block hits a changed file |
|---|---|---|---|---|---|
| fd (Rust) | 54 / 407 | 0.43 → **0.47** | 0.60 → 0.60 | 0.31 → **0.39** | 73% |
| axios (JavaScript) | 454 / 985 | 0.00 → **0.73** | 0.08 → **0.60** | 0.04 → **0.61** | 80% |
| flask (Python) | 249 / 902 | 0.10 → **0.47** | 0.13 → **0.43** | 0.10 → **0.38** | 60% |
| hono (TypeScript) | 488 / 1,696 | 0.13 → **0.80** | 0.18 → **0.77** | 0.11 → **0.63** | 87% |

### A tried and rejected change: fusing the index with grep

The development-set misses had two causes: test files ranked high, and objectives naming APIs used *inside* function
bodies (`trimRight`, `req.connection`, `Buffer`), which a declarations-only index cannot see and grep can. Reciprocal rank
fusion of the two rankings (k = 60, untuned) was the best of the three on MRR in all five development repositories
(e.g. ky 0.45/0.43 → 0.73, click → 0.77, express → 0.68). On the held-out set it helped on fd (MRR 0.52) and flask (0.40)
but **lost clearly** on axios (0.61 → 0.41) and hono (0.63 → 0.49), where grep is dominated by large non-source files.
It was therefore **not adopted**; the code stays in the evaluation so the result is reproducible. A fusion restricted to
indexed source files is a plausible next step and must be judged on a fresh held-out set.

### Cost

Index build (whole tree, release build, this container): median 47–614 ms across the nine repositories (zod, 714 files:
≈ 0.6 s); `select_files` median 0.2–4 ms. The index is cached per repository and rebuilt when the tree changes.

### What this does and does not show

* It measures **file localization from a one-line description**. It does not measure whether missions succeed, finish
  faster or cost less with FORGE's context — that needs live agent runs.
* Commit subjects are written after the fact and often name the identifiers involved, which helps both selectors; real
  task descriptions may be vaguer. Expected files exclude tests by construction, which penalises any selector that
  returns test files (both do).
* 30 cases per repository: differences of a few points are noise. The large gaps (axios, hono, zod, ripgrep) are not.
* The two weaknesses above (tests ranked high; no view into function bodies or references) are real and unaddressed.

## Memory retrieval on real documentation (2026-10-23)

**Method** (`apps/desktop/src-tauri/src/knowledge/memory_eval.rs`): FAQ files from two public projects. Each answer
becomes one verified memory page titled only "Answer N" (the question's wording is not in the page); each question is
then used as a mission objective, and the package FORGE would inject (`retrieve::build`, default 6,000-character budget)
is checked for the right answer.

| FAQ (pinned clone as above) | Pairs | hit@1 | hit@3 | MRR | Right answer in the package at all | Raw FTS hit@1 |
|---|---|---|---|---|---|---|
| ripgrep `FAQ.md` | 28 | 0.57 | 0.79 | 0.66 | 0.82 | 0.57 |
| click `docs/faqs.md` | 6 | 0.83 | 1.00 | 0.92 | 1.00 | 0.83 |

Reading: retrieval is lexical (SQLite FTS5/BM25 plus FORGE's trust, file-overlap and freshness adjustments), so a
question and answer that share no words are missed — e.g. "How do I search files that aren't UTF-8?" against an answer
written in terms of *encodings*. FORGE's re-ranking did not change hit@1 over plain BM25 here (it is designed to demote
untrusted, stale and superseded pages, which this set does not contain). The click set is too small to mean much.
The 6,000-character budget covers the notes; a fixed frame of about 420 characters (the data-not-instructions header)
and the separate "Relevant code" block (≤ 2,000 bytes) come on top.

Not measured: retrieval over a user's real, messy vault (duplicates, contradictions); semantic (embedding) retrieval,
which FORGE does not have.

## Native agent vs FORGE

Not run. A fair comparison needs the same tasks given to native Claude Code / Codex and to FORGE missions on the same
model and account, with outcome checks (tests passing, diff review) and cost/time recorded. The cloud environment has no
provider account by design, so this is on the owner's machine (docs/CODEX_WINDOWS_HANDOFF.md). **No claim that FORGE
outperforms native agents is made anywhere in this repository.**

## Continuation measurements (2026-10-27, Windows)

The debug-build measurements below compare the previous selector with the candidate on the same pinned checkouts and commit-derived tasks. They are not productivity measurements. Runs used local clones and no provider or GPU. The nine original repositories are the projects above; the additional held-out repositories were ItsDangerous (`672971d66a2ef9f85151e53283113f33d642dabd`), p-limit (`a8a6fbec4e0e866d6d779b10889bb4f5567e70eb`), and WalkDir (`6fd031c82ba5a4204b4ce6eae73dacb00dc072ec`). There were 30 tasks per repository except p-limit's 24. The selector records bounded identifier mentions inside syntax trees; these are **unresolved syntactic mentions**, not resolved references or a call graph.

| Set, macro mean across repositories | P@3 | Hit@3 | MRR@10 | Context hit | Mean context bytes | Mean median build / query |
|---|---:|---:|---:|---:|---:|---:|
| Original nine, before | .217 | .589 | .493 | .752 | 1,644 | 916.8 / 11.64 ms |
| Original nine, candidate | .232 | .619 | .535 | .811 | 1,762 | 1,065.2 / 15.23 ms |
| New held-out three, before | .235 | .461 | .313 | .417 | 440 | 330 / .695 ms |
| New held-out three, candidate | .287 | .564 | .450 | .564 | 639 | 370 / .937 ms |

Individual regressions include Zod Hit@3 .667 to .567, Click MRR .600 to .568, and Flask Hit@3 .467 to .400. These small samples and debug timings do not establish universal improvement; identifier storage and parsing cost additional memory and time. Run `FORGE_CODEINTEL_EVAL` as described in `codeintel/eval_oss.rs` against the pinned local clones to reproduce; measure release-build latency and resident memory on a representative user's repository before making performance claims.

A subsequent Zod rerun after a memory-allocation refactor reproduced Hit@3 .567 (30 tasks). The median lower-bound estimate for retained identifier strings and their vector/map entries was 1,674,522 bytes; it excludes allocator overhead and transient memory, so it is not peak or process memory. Its debug median build time was 2,573 ms while other tests and packaging ran concurrently, and is not comparable to the quieter runs above.

Memory retrieval compares the existing lexical channel with a separate, lower-weight, bounded related-terminology FTS channel; exact matches retain their original ranking signal. For ripgrep's 28 FAQ questions, Hit@3 stayed .786, MRR rose .661 to .667, precision@3 stayed .262, and answer inclusion rose .821 to .857; mean package size fell from 6,159 to 6,140 characters. Click's six questions were unchanged (Hit@1 .833, Hit@3 1.0, MRR .917). A held-out Black FAQ at `9485ad42045c3b4384beabec7841a7d1b7da27ce`, with headings normalized for the existing FAQ parser, had 13 questions and was unchanged (.769, 1.0, .872 respectively). A lower expansion weight of .45 produced no improvement and was rejected; .9 produced the small measured gain. These sets are small, package sizes include the context frame, and retrieval memory consumption was not measured. See `knowledge/memory_eval.rs` for baseline/candidate evaluation and latency output. This is local lexical expansion, not semantic retrieval; verified-only injection and provenance are unchanged.

On this Windows debug run, median per-query memory retrieval was 250.1 to 245.1 ms for ripgrep, 218.1 to 227.0 ms for Click, and 236.7 to 226.4 ms for Black. Maxima were 366.5 to 305.3 ms, 343.2 to 279.8 ms, and 316.8 to 244.1 ms respectively. These single-run timings are noisy and include fixture/database work; they are not a performance claim.
