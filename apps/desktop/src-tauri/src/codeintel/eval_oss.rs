//! Code-intelligence evaluation on real open-source repositories, using their own history as ground truth.
//!
//! For each repository, recent commits are selected by a fixed rule (no hand-picking): non-merge commits that change 1–4
//! indexed source files outside tests/docs/examples, whose subject reads like a task (not a release, bump, merge, format
//! or typo commit). The **commit subject is the objective**, the **changed source files are the answer**, and both
//! selectors run on the **parent commit's tree** (before the change existed):
//!
//! * `grep`: files matching the most distinct objective words (the baseline `grep -il` gives), ties by path;
//! * `index`: [`select_files`], what FORGE's symbol index points a mission at;
//! * `fused`: reciprocal rank fusion of the two. **Evaluated and not adopted**: it helped on the five repositories it was
//!   tried on first, then lost to the index alone on two of four held-out repositories (docs/EVALUATION.md). Kept here so
//!   the negative result stays reproducible.
//!
//! Also measured: index build time, files and symbols, query latency, and whether the "Relevant code" context block
//! (2000-byte budget) mentions a changed file, with its size.
//!
//! Not part of the normal suite: it needs local clones. `scripts/codeintel-eval.sh` clones the pinned repositories and runs
//! it; or run with
//! `FORGE_CODEINTEL_EVAL=/path/repo1,/path/repo2 [FORGE_CODEINTEL_EVAL_OUT=out.json] cargo test --release --lib eval_oss -- --nocapture`.
//! The clones are not modified: each parent tree is checked out into a temporary detached worktree that is removed after.

use super::*;
use std::time::Instant;

const PER_REPO: usize = 30;
const SCAN: usize = 600;
const KS: [usize; 2] = [3, 5];

fn indexed_source(p: &str) -> bool {
    let lower = p.to_lowercase();
    Lang::from_path(p).is_some()
        && !lower.ends_with(".d.ts")
        && !lower.split('/').any(|seg| {
            matches!(seg, "test" | "tests" | "__tests__" | "spec" | "specs" | "docs" | "doc" | "examples" | "example" | "benches" | "bench" | "fixtures" | "testdata" | "playground")
        })
        && !lower.rsplit('/').next().unwrap_or("").contains(".test.")
        && !lower.rsplit('/').next().unwrap_or("").contains(".spec.")
        && !lower.rsplit('/').next().unwrap_or("").starts_with("test_")
}

fn task_like(subject: &str) -> bool {
    let s = subject.to_lowercase();
    let words = s.split_whitespace().count();
    let noise = ["release", "bump", "merge", "version", "changelog", "typo", "fmt", "format", "lint", "chore", "deps", "dependabot", "revert", "readme", "ci:", "ci ", "wip", "prepare", "publish"];
    words >= 4 && !noise.iter().any(|n| s.starts_with(n) || s.contains(&format!(" {n}")) || s.contains(&format!("({n}")))
        && !s.chars().next().is_some_and(|c| c.is_ascii_digit())
}

struct Case {
    sha: String,
    subject: String,
    expected: Vec<String>,
}

fn cases(repo: &Path) -> Vec<Case> {
    let log = git::run(repo, &["log", "--no-merges", "-n", &SCAN.to_string(), "--format=%x1e%H%x1f%s", "--name-only", "--no-renames", "HEAD"]).unwrap();
    let mut out = vec![];
    for rec in log.split('\u{1e}').filter(|r| !r.trim().is_empty()) {
        let mut lines = rec.lines();
        let Some((sha, subject)) = lines.next().and_then(|h| h.split_once('\u{1f}')) else { continue };
        let changed: Vec<String> = lines.map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect();
        let src: Vec<String> = changed.iter().filter(|p| indexed_source(p)).cloned().collect();
        if src.is_empty() || src.len() > 4 || !task_like(subject) {
            continue;
        }
        out.push(Case { sha: sha.to_string(), subject: subject.to_string(), expected: src });
        if out.len() == PER_REPO {
            break;
        }
    }
    out
}

fn grep_select(root: &Path, terms: &[String], k: usize) -> Vec<String> {
    let listing = git::run(root, &["ls-files", "-z"]).unwrap();
    let mut scored: Vec<(String, usize)> = listing
        .split('\0')
        .filter(|p| !p.is_empty())
        .filter_map(|p| {
            let meta = std::fs::metadata(root.join(p)).ok()?;
            if meta.len() > MAX_FILE_BYTES {
                return None;
            }
            let text = std::fs::read_to_string(root.join(p)).ok()?.to_lowercase();
            let n = terms.iter().filter(|t| text.contains(t.as_str())).count();
            (n > 0).then(|| (p.to_string(), n))
        })
        .collect();
    scored.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    scored.into_iter().take(k).map(|(p, _)| p).collect()
}

#[derive(Default, Clone, Copy)]
struct Score {
    p: f64,
    r: f64,
    hit: f64,
}

fn score(sel: &[String], expected: &[String]) -> Score {
    let n = sel.iter().filter(|s| expected.contains(s)).count() as f64;
    Score { p: if sel.is_empty() { 0.0 } else { n / sel.len() as f64 }, r: n / expected.len() as f64, hit: if n > 0.0 { 1.0 } else { 0.0 } }
}

/// Reciprocal rank fusion (k = 60, the usual constant; not tuned here) of two rankings.
fn rrf(a: &[String], b: &[String], k: usize) -> Vec<String> {
    let mut score: BTreeMap<&String, f64> = BTreeMap::new();
    for list in [a, b] {
        for (i, f) in list.iter().enumerate() {
            *score.entry(f).or_default() += 1.0 / (60.0 + i as f64 + 1.0);
        }
    }
    let mut v: Vec<(&String, f64)> = score.into_iter().collect();
    v.sort_by(|x, y| y.1.partial_cmp(&x.1).unwrap().then(x.0.cmp(y.0)));
    v.into_iter().take(k).map(|(f, _)| f.clone()).collect()
}

fn rr(sel: &[String], expected: &[String]) -> f64 {
    sel.iter().position(|s| expected.contains(s)).map(|i| 1.0 / (i + 1) as f64).unwrap_or(0.0)
}

#[test]
fn eval_oss_commit_localization() {
    let Ok(list) = std::env::var("FORGE_CODEINTEL_EVAL") else {
        eprintln!("SKIPPED: set FORGE_CODEINTEL_EVAL to a comma-separated list of local clones");
        return;
    };
    let mut report = vec![];
    for repo in list.split(',').filter(|s| !s.is_empty()).map(PathBuf::from) {
        let name = repo.file_name().unwrap().to_string_lossy().to_string();
        let head = git::run(&repo, &["rev-parse", "HEAD"]).unwrap().trim().to_string();
        let cs = cases(&repo);
        let wt = tempfile::tempdir().unwrap();
        let wt_path = wt.path().join("wt");
        git::run(&repo, &["worktree", "add", "-q", "--detach", wt_path.to_str().unwrap(), &format!("{}^", cs[0].sha)]).unwrap();
        // [grep, index, fused] x KS, plus MRR@10 and the context-block hit.
        let mut sums: Vec<[Score; 3]> = vec![[Score::default(); 3]; KS.len()];
        let (mut mrr, mut ctx_hit, mut ctx_bytes, mut build_ms, mut query_ms, mut files, mut symbols) = ([0.0f64; 3], 0.0, 0usize, vec![], vec![], 0, 0);
        let mut rows = vec![];
        for c in &cs {
            git::run(&wt_path, &["checkout", "-q", "--detach", &format!("{}^", c.sha)]).unwrap();
            let t = Instant::now();
            let ix = build(&wt_path).unwrap();
            build_ms.push(t.elapsed().as_secs_f64() * 1000.0);
            files = ix.files.len();
            symbols = ix.symbols.len();
            let terms = objective_terms(&c.subject);
            let g20 = grep_select(&wt_path, &terms, 20);
            let t = Instant::now();
            let x20 = select_files(&ix, &c.subject, &[], 20);
            query_ms.push(t.elapsed().as_secs_f64() * 1000.0);
            let f10 = rrf(&x20, &g20, 10);
            let (g10, x10) = (g20[..g20.len().min(10)].to_vec(), x20[..x20.len().min(10)].to_vec());
            for (i, k) in KS.iter().enumerate() {
                let g = score(&g10[..g10.len().min(*k)], &c.expected);
                let x = score(&x10[..x10.len().min(*k)], &c.expected);
                let f = score(&f10[..f10.len().min(*k)], &c.expected);
                for (s, v) in sums[i].iter_mut().zip([g, x, f]) {
                    s.p += v.p;
                    s.r += v.r;
                    s.hit += v.hit;
                }
            }
            mrr[0] += rr(&g10, &c.expected);
            mrr[1] += rr(&x10, &c.expected);
            mrr[2] += rr(&f10, &c.expected);
            let rc = relevant(&ix, &c.subject, &[], CODE_BUDGET_BYTES);
            let hit = rc.items.iter().any(|i| c.expected.contains(&i.file));
            ctx_hit += if hit { 1.0 } else { 0.0 };
            ctx_bytes += rc.used_bytes;
            rows.push(serde_json::json!({"commit": c.sha, "subject": c.subject, "expected": c.expected, "grepTop5": &g10[..g10.len().min(5)], "indexTop5": &x10[..x10.len().min(5)], "fusedTop5": &f10[..f10.len().min(5)], "contextHit": hit}));
        }
        git::run(&repo, &["worktree", "remove", "--force", wt_path.to_str().unwrap()]).unwrap();
        let n = cs.len() as f64;
        build_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
        query_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let med = |v: &Vec<f64>| v[v.len() / 2];
        println!("\n== {name} (HEAD {}, {} cases; last parent tree: {files} files, {symbols} symbols)", &head[..12], cs.len());
        println!("index build median {:.0} ms (max {:.0}); select_files median {:.2} ms", med(&build_ms), build_ms.last().unwrap(), med(&query_ms));
        let mut by_k = serde_json::Map::new();
        for (i, k) in KS.iter().enumerate() {
            let [g, x, f] = sums[i];
            println!(
                "k={k}: grep P {:.2} R {:.2} hit {:.2} | index P {:.2} R {:.2} hit {:.2} | fused P {:.2} R {:.2} hit {:.2}",
                g.p / n, g.r / n, g.hit / n, x.p / n, x.r / n, x.hit / n, f.p / n, f.r / n, f.hit / n
            );
            by_k.insert(
                k.to_string(),
                serde_json::json!({"grep": {"precision": g.p / n, "recall": g.r / n, "hit": g.hit / n}, "index": {"precision": x.p / n, "recall": x.r / n, "hit": x.hit / n}, "fused": {"precision": f.p / n, "recall": f.r / n, "hit": f.hit / n}}),
            );
        }
        println!("MRR@10: grep {:.2}, index {:.2}, fused {:.2}; context block mentions a changed file in {:.0}% (mean {} bytes)", mrr[0] / n, mrr[1] / n, mrr[2] / n, 100.0 * ctx_hit / n, ctx_bytes / cs.len().max(1));
        report.push(serde_json::json!({
            "repo": name, "head": head, "cases": cs.len(), "files": files, "symbols": symbols,
            "buildMsMedian": med(&build_ms), "buildMsMax": build_ms.last(), "queryMsMedian": med(&query_ms),
            "atK": by_k, "mrr10": {"grep": mrr[0] / n, "index": mrr[1] / n, "fused": mrr[2] / n},
            "contextHitRate": ctx_hit / n, "contextBytesMean": ctx_bytes / cs.len().max(1), "rows": rows,
        }));
        assert!(cs.len() >= 10, "{name}: the selection rule found only {} cases", cs.len());
    }
    if let Ok(out) = std::env::var("FORGE_CODEINTEL_EVAL_OUT") {
        std::fs::write(out, serde_json::to_string_pretty(&report).unwrap()).unwrap();
    }
}
