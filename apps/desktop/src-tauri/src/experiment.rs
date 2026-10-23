//! Experiments (Phase 6): compare a baseline revision with one or more variant revisions by running one user-authored command
//! several times in isolated, detached git worktrees through the check runner (`verify::run_check`), and report medians, spread
//! and a winner only when the evidence supports one.
//!
//! This module is pure: definitions, validation, metric extraction, statistics and the verdict. Running lives in
//! `supervisor/experiment.rs`, persistence in `store_experiment.rs`.
//!
//! Integrity rules (from the product directive, "Experimental integrity"):
//!   * every revision runs in its own worktree checked out at an exact commit, recorded when the experiment is created;
//!   * an optional correctness command must pass before a revision's measurements count;
//!   * warm-up runs are recorded but not measured; measured runs are interleaved across revisions to spread drift;
//!   * a run that fails, prints no metric, or changes its worktree invalidates that revision's result instead of being dropped;
//!   * no winner is named with fewer than `MIN_RUNS_FOR_VERDICT` measured runs per side, or when the ranges overlap;
//!   * every attempt, including failed and inconclusive ones, stays recorded.

use crate::error::{ForgeError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Fewer measured runs than this per revision never produce a winner.
pub const MIN_RUNS_FOR_VERDICT: usize = 3;
pub const MAX_VARIANTS: usize = 8;
pub const MAX_REPETITIONS: u32 = 100;
pub const MAX_WARMUP: u32 = 10;
pub const MAX_TIMEOUT_SECS: u64 = 4 * 3600;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Direction {
    Lower,
    Higher,
}

/// What is measured.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Metric {
    /// Wall-clock duration of the command in milliseconds; lower is better.
    Duration,
    /// A number the command prints on a line `FORGE_METRIC <name>=<number>` (the last such line of a run counts).
    Output { name: String, direction: Direction },
}

impl Metric {
    pub fn direction(&self) -> Direction {
        match self {
            Metric::Duration => Direction::Lower,
            Metric::Output { direction, .. } => *direction,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VariantSpec {
    pub label: String,
    /// A branch, tag or commit of the project repository.
    pub git_ref: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NewExperiment {
    pub name: String,
    #[serde(default)]
    pub objective: Option<String>,
    /// The benchmark, run in each revision's worktree with the user's privileges.
    pub command: String,
    /// Must pass (exit 0) in a revision's worktree before its measurements count.
    #[serde(default)]
    pub correctness_command: Option<String>,
    pub metric: Metric,
    pub repetitions: u32,
    #[serde(default)]
    pub warmup: u32,
    pub timeout_secs: u64,
    pub baseline_ref: String,
    pub variants: Vec<VariantSpec>,
}

fn metric_name_ok(n: &str) -> bool {
    !n.is_empty() && n.len() <= 64 && n.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

fn ref_ok(r: &str) -> bool {
    // Passed to git as one argument after validation; never an option, never a revision range or reflog expression.
    !r.is_empty() && r.len() <= 200 && !r.starts_with('-') && !r.contains("..") && !r.contains('@') && !r.chars().any(|c| c.is_whitespace() || c.is_control() || matches!(c, '~' | '^' | ':' | '?' | '*' | '[' | '\\'))
}

impl NewExperiment {
    pub fn validate(&self) -> Result<()> {
        let bad = |m: String| Err(ForgeError::Other(m));
        if self.name.trim().is_empty() || self.name.len() > 200 {
            return bad("an experiment needs a name (at most 200 characters)".into());
        }
        if self.command.trim().is_empty() {
            return bad("an experiment needs a command to measure".into());
        }
        for (what, c) in [("command", Some(&self.command)), ("correctness command", self.correctness_command.as_ref())] {
            if let Some(c) = c {
                if c.contains('\n') || c.contains('\r') || c.contains('\0') {
                    return bad(format!("the {what} must be a single line"));
                }
            }
        }
        if self.correctness_command.as_deref().is_some_and(|c| c.trim().is_empty()) {
            return bad("the correctness command is empty; leave it out instead".into());
        }
        if let Metric::Output { name, .. } = &self.metric {
            if !metric_name_ok(name) {
                return bad("a metric name is 1-64 letters, digits, '_', '-' or '.'".into());
            }
        }
        if self.repetitions == 0 || self.repetitions > MAX_REPETITIONS {
            return bad(format!("repetitions must be between 1 and {MAX_REPETITIONS}"));
        }
        if self.warmup > MAX_WARMUP {
            return bad(format!("at most {MAX_WARMUP} warm-up runs"));
        }
        if self.timeout_secs == 0 || self.timeout_secs > MAX_TIMEOUT_SECS {
            return bad("the timeout must be between 1 second and 4 hours".into());
        }
        if self.variants.is_empty() || self.variants.len() > MAX_VARIANTS {
            return bad(format!("compare the baseline with 1 to {MAX_VARIANTS} variants"));
        }
        if !ref_ok(self.baseline_ref.trim()) {
            return bad(format!("\"{}\" is not a branch, tag or commit FORGE accepts", self.baseline_ref));
        }
        let mut labels = HashSet::from(["baseline".to_string()]);
        for v in &self.variants {
            let l = v.label.trim();
            if l.is_empty() || l.len() > 80 {
                return bad("every variant needs a label (at most 80 characters)".into());
            }
            if !labels.insert(l.to_lowercase()) {
                return bad(format!("duplicate variant label \"{l}\" (\"baseline\" is reserved)"));
            }
            if !ref_ok(v.git_ref.trim()) {
                return bad(format!("\"{}\" is not a branch, tag or commit FORGE accepts", v.git_ref));
            }
        }
        Ok(())
    }
}

/// The value of `FORGE_METRIC <name>=<number>` on the last such line, if any. Non-finite numbers do not count.
pub fn extract_metric(output: &str, name: &str) -> Option<f64> {
    let prefix = format!("FORGE_METRIC {name}=");
    output.lines().rev().find_map(|l| l.trim().strip_prefix(&prefix).and_then(|v| v.trim().parse::<f64>().ok()).filter(|v| v.is_finite()))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Stats {
    pub n: usize,
    pub median: f64,
    pub min: f64,
    pub max: f64,
    /// Median absolute deviation from the median: the spread, robust to one outlier.
    pub mad: f64,
}

fn median_of(sorted: &[f64]) -> f64 {
    let n = sorted.len();
    if n % 2 == 1 { sorted[n / 2] } else { (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0 }
}

pub fn stats(values: &[f64]) -> Option<Stats> {
    if values.is_empty() {
        return None;
    }
    let mut v = values.to_vec();
    v.sort_by(|a, b| a.total_cmp(b));
    let median = median_of(&v);
    let mut dev: Vec<f64> = v.iter().map(|x| (x - median).abs()).collect();
    dev.sort_by(|a, b| a.total_cmp(b));
    Some(Stats { n: v.len(), median, min: v[0], max: v[v.len() - 1], mad: median_of(&dev) })
}

/// What was observed for one revision in one attempt (input to the verdict).
#[derive(Debug, Clone, PartialEq)]
pub struct Observed {
    pub variant_id: String,
    pub label: String,
    pub is_baseline: bool,
    pub commit: String,
    /// Metric values of measured runs that succeeded.
    pub values: Vec<f64>,
    /// Measured runs that failed, timed out, were cancelled or printed no metric.
    pub failed_runs: usize,
    /// None: no correctness command, or it never ran.
    pub correctness: Option<bool>,
    /// A run (of any phase) changed tracked or untracked files in the worktree.
    pub worktree_changed: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Baseline,
    Better,
    Worse,
    NoClearDifference,
    InsufficientRuns,
    Invalid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VariantReport {
    pub variant_id: String,
    pub label: String,
    pub is_baseline: bool,
    pub commit: String,
    pub stats: Option<Stats>,
    pub failed_runs: usize,
    pub correctness: Option<bool>,
    pub worktree_changed: bool,
    /// Relative change of the median against the baseline's median (e.g. -0.27 = 27 % lower).
    pub change_vs_baseline: Option<f64>,
    pub verdict: Verdict,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub metric: Metric,
    pub direction: Direction,
    pub min_runs_for_verdict: usize,
    pub variants: Vec<VariantReport>,
    /// Variant id of the winner, when there is one.
    pub winner: Option<String>,
    /// winner | no_winner | inconclusive
    pub conclusion: String,
    pub explanation: String,
}

/// Why a revision's measurements cannot be used, if they cannot.
fn invalid_reasons(o: &Observed) -> Vec<String> {
    let mut r = vec![];
    if o.correctness == Some(false) {
        r.push("the correctness command failed".into());
    }
    if o.worktree_changed {
        r.push("a run changed files in its worktree, so later runs did not measure the same code".into());
    }
    if o.failed_runs > 0 {
        r.push(format!("{} measured run(s) failed or printed no metric", o.failed_runs));
    }
    r
}

/// Compare every variant with the baseline. A variant is *better* only when all its measured values beat all of the baseline's
/// (non-overlapping ranges), with at least `MIN_RUNS_FOR_VERDICT` runs on both sides and both sides valid.
pub fn report(metric: &Metric, observed: &[Observed]) -> Report {
    let dir = metric.direction();
    let base = observed.iter().find(|o| o.is_baseline);
    let base_stats = base.and_then(|b| stats(&b.values));
    let base_invalid = base.map(invalid_reasons).unwrap_or_else(|| vec!["there is no baseline".into()]);
    let mut out = vec![];
    for o in observed {
        let s = stats(&o.values);
        let mut reasons = invalid_reasons(o);
        let change = match (&s, &base_stats) {
            (Some(s), Some(b)) if !o.is_baseline && b.median != 0.0 => Some((s.median - b.median) / b.median.abs()),
            _ => None,
        };
        let verdict = if o.is_baseline {
            if !reasons.is_empty() {
                Verdict::Invalid
            } else if s.as_ref().map_or(0, |s| s.n) < MIN_RUNS_FOR_VERDICT {
                reasons.push(format!("fewer than {MIN_RUNS_FOR_VERDICT} measured runs"));
                Verdict::InsufficientRuns
            } else {
                Verdict::Baseline
            }
        } else if !reasons.is_empty() {
            Verdict::Invalid
        } else if !base_invalid.is_empty() {
            reasons.push(format!("the baseline is not usable ({})", base_invalid.join("; ")));
            Verdict::Invalid
        } else {
            match (&s, &base_stats) {
                (Some(v), Some(b)) if v.n >= MIN_RUNS_FOR_VERDICT && b.n >= MIN_RUNS_FOR_VERDICT => {
                    let (beats, loses) = match dir {
                        Direction::Lower => (v.max < b.min, v.min > b.max),
                        Direction::Higher => (v.min > b.max, v.max < b.min),
                    };
                    if beats {
                        Verdict::Better
                    } else if loses {
                        Verdict::Worse
                    } else {
                        reasons.push("its range of results overlaps the baseline's".into());
                        Verdict::NoClearDifference
                    }
                }
                _ => {
                    reasons.push(format!("fewer than {MIN_RUNS_FOR_VERDICT} measured runs on this side or the baseline's"));
                    Verdict::InsufficientRuns
                }
            }
        };
        out.push(VariantReport {
            variant_id: o.variant_id.clone(),
            label: o.label.clone(),
            is_baseline: o.is_baseline,
            commit: o.commit.clone(),
            stats: s,
            failed_runs: o.failed_runs,
            correctness: o.correctness,
            worktree_changed: o.worktree_changed,
            change_vs_baseline: change,
            verdict,
            reasons,
        });
    }
    let better: Vec<&VariantReport> = out.iter().filter(|v| v.verdict == Verdict::Better).collect();
    let winner = better
        .iter()
        .min_by(|a, b| {
            let (x, y) = (a.stats.as_ref().map_or(0.0, |s| s.median), b.stats.as_ref().map_or(0.0, |s| s.median));
            match dir {
                Direction::Lower => x.total_cmp(&y),
                Direction::Higher => y.total_cmp(&x),
            }
        })
        .map(|v| (*v).clone());
    let decided = out.iter().filter(|v| !v.is_baseline).all(|v| matches!(v.verdict, Verdict::Better | Verdict::Worse | Verdict::NoClearDifference));
    let (conclusion, explanation) = match &winner {
        Some(w) => ("winner", format!("\"{}\" beat the baseline in every measured run (median change {:+.1} %). Whether to keep it is your decision.", w.label, w.change_vs_baseline.unwrap_or(0.0) * 100.0)),
        None if decided => ("no_winner", "No variant beat the baseline in every measured run.".to_string()),
        None => ("inconclusive", "The results cannot name a winner: see the reasons on each revision.".to_string()),
    };
    Report { metric: metric.clone(), direction: dir, min_runs_for_verdict: MIN_RUNS_FOR_VERDICT, variants: out, winner: winner.map(|w| w.variant_id), conclusion: conclusion.into(), explanation }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(id: &str, base: bool, values: &[f64]) -> Observed {
        Observed { variant_id: id.into(), label: id.into(), is_baseline: base, commit: "c".into(), values: values.to_vec(), failed_runs: 0, correctness: Some(true), worktree_changed: false }
    }

    fn spec() -> NewExperiment {
        NewExperiment {
            name: "speed".into(),
            objective: None,
            command: "node bench.js".into(),
            correctness_command: Some("npm test".into()),
            metric: Metric::Duration,
            repetitions: 5,
            warmup: 1,
            timeout_secs: 60,
            baseline_ref: "main".into(),
            variants: vec![VariantSpec { label: "fast".into(), git_ref: "feature/fast".into() }],
        }
    }

    #[test]
    fn statistics_are_robust_and_exact() {
        let s = stats(&[3.0, 1.0, 2.0, 100.0]).unwrap();
        assert_eq!((s.n, s.median, s.min, s.max), (4, 2.5, 1.0, 100.0));
        assert_eq!(s.mad, 1.0); // |x-2.5| = 1.5, 0.5, 0.5, 97.5 -> median 1.0
        assert_eq!(stats(&[5.0]).unwrap().mad, 0.0);
        assert!(stats(&[]).is_none());
    }

    #[test]
    fn the_metric_is_the_last_well_formed_line() {
        let out = "warming up\nFORGE_METRIC latency_ms=180.5\nnoise FORGE_METRIC latency_ms=1\nFORGE_METRIC latency_ms=132\nFORGE_METRIC other=9\n";
        assert_eq!(extract_metric(out, "latency_ms"), Some(132.0));
        assert_eq!(extract_metric(out, "missing"), None);
        assert_eq!(extract_metric("FORGE_METRIC x=NaN\n", "x"), None);
        assert_eq!(extract_metric("FORGE_METRIC x=inf\n", "x"), None);
    }

    #[test]
    fn no_winner_with_fewer_than_three_runs_even_when_every_run_is_faster() {
        let r = report(&Metric::Duration, &[obs("base", true, &[100.0, 101.0]), obs("v", false, &[50.0, 51.0])]);
        assert_eq!(r.winner, None);
        assert_eq!(r.conclusion, "inconclusive");
        assert_eq!(r.variants[1].verdict, Verdict::InsufficientRuns);
        // One run short on the baseline only is still not enough.
        let r = report(&Metric::Duration, &[obs("base", true, &[100.0, 101.0]), obs("v", false, &[50.0, 51.0, 52.0])]);
        assert_eq!(r.variants[1].verdict, Verdict::InsufficientRuns);
        // Three on each side is.
        let r = report(&Metric::Duration, &[obs("base", true, &[100.0, 101.0, 99.0]), obs("v", false, &[50.0, 51.0, 52.0])]);
        assert_eq!(r.winner.as_deref(), Some("v"));
        assert_eq!(r.conclusion, "winner");
        assert!((r.variants[1].change_vs_baseline.unwrap() + 0.49).abs() < 1e-9);
    }

    #[test]
    fn overlapping_ranges_are_no_clear_difference_and_direction_is_respected() {
        let r = report(&Metric::Duration, &[obs("base", true, &[100.0, 110.0, 120.0]), obs("v", false, &[95.0, 105.0, 101.0])]);
        assert_eq!((r.variants[1].verdict, r.winner.clone(), r.conclusion.as_str()), (Verdict::NoClearDifference, None, "no_winner"));
        let higher = Metric::Output { name: "ops".into(), direction: Direction::Higher };
        let r = report(&higher, &[obs("base", true, &[10.0, 11.0, 12.0]), obs("a", false, &[20.0, 21.0, 22.0]), obs("b", false, &[30.0, 31.0, 32.0]), obs("c", false, &[1.0, 2.0, 3.0])]);
        assert_eq!(r.winner.as_deref(), Some("b"), "the best of the better variants wins");
        assert_eq!(r.variants[3].verdict, Verdict::Worse);
    }

    #[test]
    fn failures_changes_and_failed_correctness_invalidate_instead_of_being_dropped() {
        let mut v = obs("v", false, &[1.0, 1.0, 1.0]);
        v.correctness = Some(false);
        let mut w = obs("w", false, &[1.0, 1.0, 1.0]);
        w.worktree_changed = true;
        let mut x = obs("x", false, &[1.0, 1.0, 1.0]);
        x.failed_runs = 1;
        let r = report(&Metric::Duration, &[obs("base", true, &[9.0, 9.0, 9.0]), v, w, x]);
        assert!(r.variants[1..].iter().all(|v| v.verdict == Verdict::Invalid), "{r:#?}");
        assert_eq!((r.winner, r.conclusion.as_str()), (None, "inconclusive"));
        // A broken baseline makes every comparison unusable.
        let mut b = obs("base", true, &[9.0, 9.0, 9.0]);
        b.failed_runs = 2;
        let r = report(&Metric::Duration, &[b, obs("v", false, &[1.0, 1.0, 1.0])]);
        assert_eq!(r.variants[0].verdict, Verdict::Invalid);
        assert_eq!(r.variants[1].verdict, Verdict::Invalid);
        assert!(r.variants[1].reasons[0].contains("baseline"));
    }

    #[test]
    fn the_ui_contract_shape_deserialises() {
        // What `api.experimentCreate` sends (apps/desktop/src/lib/types.ts `NewExperiment`), with the optional fields left out.
        let e: NewExperiment = serde_json::from_str(r#"{"name":"n","command":"node b.js","metric":{"kind":"output","name":"ops","direction":"higher"},"repetitions":3,"timeoutSecs":5,"baselineRef":"main","variants":[{"label":"a","gitRef":"feat"}]}"#).unwrap();
        assert_eq!((e.warmup, e.objective, e.correctness_command), (0, None, None));
        assert_eq!(e.metric, Metric::Output { name: "ops".into(), direction: Direction::Higher });
        let d: NewExperiment = serde_json::from_str(r#"{"name":"n","command":"c","metric":{"kind":"duration"},"repetitions":3,"warmup":1,"timeoutSecs":5,"baselineRef":"main","variants":[],"correctnessCommand":null}"#).unwrap();
        assert_eq!(d.metric, Metric::Duration);
        let r = report(&Metric::Duration, &[obs("base", true, &[1.0, 1.0, 1.0])]);
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!((v["variants"][0]["verdict"].as_str(), v["minRunsForVerdict"].as_u64()), (Some("baseline"), Some(3)));
    }

    #[test]
    fn definitions_are_validated() {
        assert!(spec().validate().is_ok());
        let cases: Vec<(&str, Box<dyn Fn(&mut NewExperiment)>)> = vec![
            ("name", Box::new(|e| e.name = " ".into())),
            ("command", Box::new(|e| e.command = "".into())),
            ("multi-line", Box::new(|e| e.command = "a\nb".into())),
            ("empty correctness", Box::new(|e| e.correctness_command = Some(" ".into()))),
            ("reps 0", Box::new(|e| e.repetitions = 0)),
            ("reps big", Box::new(|e| e.repetitions = MAX_REPETITIONS + 1)),
            ("warmup", Box::new(|e| e.warmup = MAX_WARMUP + 1)),
            ("timeout", Box::new(|e| e.timeout_secs = 0)),
            ("no variants", Box::new(|e| e.variants.clear())),
            ("option ref", Box::new(|e| e.baseline_ref = "--output=/tmp/x".into())),
            ("range", Box::new(|e| e.variants[0].git_ref = "main..evil".into())),
            ("reflog", Box::new(|e| e.variants[0].git_ref = "main@{1}".into())),
            ("space", Box::new(|e| e.variants[0].git_ref = "a b".into())),
            ("reserved label", Box::new(|e| e.variants[0].label = "Baseline".into())),
            ("dup label", Box::new(|e| e.variants.push(VariantSpec { label: "FAST".into(), git_ref: "x".into() }))),
            ("metric name", Box::new(|e| e.metric = Metric::Output { name: "a b".into(), direction: Direction::Lower })),
        ];
        for (what, f) in cases {
            let mut e = spec();
            f(&mut e);
            assert!(e.validate().is_err(), "{what}");
        }
    }
}
