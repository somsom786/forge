//! Experiment runner (Phase 6). Each revision (baseline and variants) is checked out at its recorded commit in its own detached
//! worktree under the FORGE home; the user's command runs there through the check runner, warm-ups first, then measured runs
//! interleaved across revisions. Worktrees are removed when the attempt ends, whatever the outcome.
//!
//! Running an experiment executes the user's command against code from the repository (any branch or commit they named) with
//! the user's privileges, exactly like a verification check, so it only happens on an explicit user action.

use super::{Core, lock};
use crate::contract::RunState;
use crate::error::{ForgeError, Result};
use crate::experiment::{self, Metric, NewExperiment, Observed, Report};
use crate::git;
use crate::store_experiment::{Experiment, ExperimentResult, ExperimentVariant, NewResult};
use crate::verify;
use serde::Serialize;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExperimentDetail {
    pub experiment: Experiment,
    pub variants: Vec<ExperimentVariant>,
    /// Runs of the latest attempt.
    pub results: Vec<ExperimentResult>,
}

/// Tracked and untracked files (not ignored ones): a run that changes this set changed the code later runs measure.
fn worktree_state(wt: &Path) -> String {
    git::run(wt, &["status", "--porcelain=v1", "-uall", "--ignore-submodules=none"]).unwrap_or_else(|e| format!("unreadable: {e}"))
}

fn state_name(s: RunState) -> &'static str {
    match s {
        RunState::Running => "running",
        RunState::Passed => "passed",
        RunState::Failed => "failed",
        RunState::Error => "error",
        RunState::Timeout => "timeout",
        RunState::Cancelled => "cancelled",
    }
}

impl Core {
    fn experiments_dir(&self, experiment_id: &str) -> PathBuf {
        self.cfg.home.join("experiments").join(experiment_id)
    }

    /// Resolve every revision to an exact commit now, so later attempts measure the same code even if a branch moves.
    pub fn experiment_create(&self, project_id: &str, n: NewExperiment) -> Result<Experiment> {
        n.validate()?;
        let project = self.store.get_project(project_id)?;
        let root = PathBuf::from(&project.root_path);
        let resolve = |r: &str| -> Result<String> {
            let spec = format!("{}^{{commit}}", r.trim());
            git::run(&root, &["rev-parse", "--verify", "--quiet", &spec])
                .map(|s| s.trim().to_string())
                .map_err(|_| ForgeError::Other(format!("\"{}\" is not a branch, tag or commit of this repository", r.trim())))
        };
        let mut commits = vec![resolve(&n.baseline_ref)?];
        for v in &n.variants {
            commits.push(resolve(&v.git_ref)?);
        }
        let e = self.store.exp_create(project_id, &n, &commits)?;
        self.store.audit("user", "experiment.create", Some(&e.id), &json!({"project": project_id, "command": e.command, "revisions": commits}));
        Ok(e)
    }

    pub fn experiment_list(&self, project_id: &str) -> Result<Vec<Experiment>> {
        self.store.exp_list(project_id)
    }

    pub fn experiment_get(&self, id: &str) -> Result<ExperimentDetail> {
        let experiment = self.store.exp_get(id)?;
        let variants = self.store.exp_variants(id)?;
        let results = self.store.exp_results(id, experiment.attempt)?;
        Ok(ExperimentDetail { experiment, variants, results })
    }

    pub fn experiment_cancel(&self, id: &str) -> Result<()> {
        match lock(&self.experiments).get(id) {
            Some(flag) => {
                flag.store(true, Ordering::SeqCst);
                Ok(())
            }
            None => Err(ForgeError::InvalidState("this experiment is not running".into())),
        }
    }

    fn environment(&self, project_backend: &str) -> serde_json::Value {
        json!({
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "cpus": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0),
            "git": git::version().ok(),
            "forge": env!("CARGO_PKG_VERSION"),
            "backend": project_backend,
        })
    }

    /// Run one attempt to completion (blocking). Explicit user action only.
    pub fn experiment_run(&self, id: &str) -> Result<ExperimentDetail> {
        let def = self.store.exp_get(id)?;
        let project = self.store.get_project(&def.project_id)?;
        let root = PathBuf::from(&project.root_path);
        // Checkouts run clean/smudge filters: refuse before anything is checked out, as for missions.
        git::ensure_no_external_programs(&root)?;
        // Every run goes to the project's execution backend; one that cannot run here is refused, never downgraded.
        let backend = crate::execution::Backend::parse(&project.execution_backend)?;
        crate::execution::require(backend)?;
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut running = lock(&self.experiments);
            if running.contains_key(id) {
                return Err(ForgeError::InvalidState("this experiment is already running".into()));
            }
            running.insert(id.to_string(), cancel.clone());
        }
        let started = self.store.exp_begin_attempt(id, &self.environment(backend.as_str()));
        let e = match started {
            Ok(e) => e,
            Err(err) => {
                lock(&self.experiments).remove(id);
                return Err(err);
            }
        };
        self.store.audit("user", "experiment.run", Some(id), &json!({"attempt": e.attempt, "command": e.command, "correctness": e.correctness_command, "backend": backend.as_str()}));
        let dir = self.experiments_dir(id).join(e.attempt.to_string());
        let mut worktrees: Vec<PathBuf> = vec![];
        let outcome = self.run_attempt(&e, &root, &dir, &cancel, &mut worktrees, backend);
        for wt in &worktrees {
            if let Err(err) = git::worktree_remove(&root, wt, true) {
                self.notice("warn", format!("could not remove the experiment worktree {}: {err}", wt.display()));
            }
        }
        let _ = git::run(&root, &["worktree", "prune"]);
        lock(&self.experiments).remove(id);
        let finished = match outcome {
            Ok(report) if cancel.load(Ordering::SeqCst) => self.store.exp_finish_attempt(id, "cancelled", Some(&report), Some("cancelled by the user")),
            Ok(report) => self.store.exp_finish_attempt(id, "completed", Some(&report), None),
            Err(err) => self.store.exp_finish_attempt(id, "failed", None, Some(&err.to_string())),
        }?;
        self.store.audit("system", "experiment.finished", Some(id), &json!({"attempt": finished.attempt, "state": finished.state, "winner": finished.report.as_ref().and_then(|r| r.winner.clone())}));
        self.experiment_get(id)
    }

    fn run_attempt(&self, e: &Experiment, root: &Path, dir: &Path, cancel: &Arc<AtomicBool>, worktrees: &mut Vec<PathBuf>, backend: crate::execution::Backend) -> Result<Report> {
        let variants = self.store.exp_variants(&e.id)?;
        // One detached worktree per revision, verified to be at the recorded commit.
        let mut wts = vec![];
        for v in &variants {
            let wt = dir.join(&v.id);
            if wt.exists() {
                return Err(ForgeError::InvalidState(format!("{} already exists; remove it and run again", wt.display())));
            }
            std::fs::create_dir_all(dir)?;
            let p = wt.to_string_lossy().into_owned();
            git::run(root, &["worktree", "add", "--detach", &p, &v.commit_sha])?;
            worktrees.push(wt.clone());
            let head = git::head_commit(&wt)?;
            if head != v.commit_sha {
                return Err(ForgeError::Git(format!("the worktree for \"{}\" is at {head}, not {}", v.label, v.commit_sha)));
            }
            wts.push(wt);
        }
        let timeout = Duration::from_secs(e.timeout_secs);
        let logs = dir.join("logs");
        let mut changed = vec![false; variants.len()];
        let mut correctness: Vec<Option<bool>> = vec![None; variants.len()];

        // Runs one command in revision `i` and records it. Returns the run's state and metric value.
        let mut run_one = |i: usize, phase: &str, iteration: i64, command: &str| -> Result<(String, Option<f64>)> {
            let before = worktree_state(&wts[i]);
            let started_at = crate::store::now();
            let log = logs.join(format!("{}-{}-{phase}-{iteration}.log", variants[i].position, crate::store::new_id()));
            let out = verify::run_check_in(backend, command, &wts[i], timeout, Some(&log), cancel.clone());
            let wt_changed = worktree_state(&wts[i]) != before;
            changed[i] |= wt_changed;
            let mut state = state_name(out.state).to_string();
            let metric = match (&e.metric, out.state) {
                (_, s) if s != RunState::Passed || phase == "correctness" => None,
                (Metric::Duration, _) => Some(out.duration_ms as f64),
                (Metric::Output { name, .. }, _) => {
                    let v = experiment::extract_metric(&out.output_tail, name);
                    if v.is_none() {
                        state = "no_metric".into();
                    }
                    v
                }
            };
            let log_s = out.log_path.as_ref().map(|p| p.to_string_lossy().into_owned());
            self.store.exp_add_result(NewResult {
                experiment_id: &e.id,
                variant_id: &variants[i].id,
                attempt: e.attempt,
                phase,
                iteration,
                state: &state,
                exit_code: out.exit_code,
                duration_ms: out.duration_ms as i64,
                metric_value: metric,
                worktree_changed: wt_changed,
                output_tail: &out.output_tail,
                log_path: log_s.as_deref(),
                started_at: &started_at,
                backend: backend.as_str(),
            })?;
            Ok((state, metric))
        };

        if let Some(cc) = &e.correctness_command {
            for i in 0..variants.len() {
                if cancel.load(Ordering::SeqCst) {
                    break;
                }
                let (state, _) = run_one(i, "correctness", 0, cc)?;
                correctness[i] = Some(state == "passed");
            }
        }
        // A revision whose correctness failed is not measured: its numbers would not count anyway.
        let measured: Vec<usize> = (0..variants.len()).filter(|&i| correctness[i] != Some(false)).collect();
        for w in 0..e.warmup {
            for &i in &measured {
                if cancel.load(Ordering::SeqCst) {
                    break;
                }
                run_one(i, "warmup", w as i64, &e.command)?;
            }
        }
        let mut values: Vec<Vec<f64>> = vec![vec![]; variants.len()];
        let mut failed = vec![0usize; variants.len()];
        for it in 0..e.repetitions as usize {
            // Interleave, rotating which revision goes first, so slow drift (thermal, caches, background load) is spread.
            for k in 0..measured.len() {
                if cancel.load(Ordering::SeqCst) {
                    break;
                }
                let i = measured[(it + k) % measured.len()];
                match run_one(i, "measure", it as i64, &e.command)? {
                    (s, Some(v)) if s == "passed" => values[i].push(v),
                    (s, _) if s == "cancelled" => {}
                    _ => failed[i] += 1,
                }
            }
        }
        let observed: Vec<Observed> = variants
            .iter()
            .enumerate()
            .map(|(i, v)| Observed {
                variant_id: v.id.clone(),
                label: v.label.clone(),
                is_baseline: v.is_baseline,
                commit: v.commit_sha.clone(),
                values: std::mem::take(&mut values[i]),
                failed_runs: failed[i],
                correctness: correctness[i],
                worktree_changed: changed[i],
            })
            .collect();
        Ok(experiment::report(&e.metric, &observed))
    }

    /// After a restart: an experiment left `running` is marked interrupted (its recorded runs stay) and its worktrees removed.
    pub(super) fn recover_experiments(&self) {
        let Ok(running) = self.store.exp_running() else { return };
        for e in running {
            let _ = self.store.exp_finish_attempt(&e.id, "interrupted", None, Some("FORGE stopped while this experiment was running"));
            if let Ok(p) = self.store.get_project(&e.project_id) {
                let root = PathBuf::from(&p.root_path);
                let dir = self.experiments_dir(&e.id).join(e.attempt.to_string());
                if let Ok(entries) = std::fs::read_dir(&dir) {
                    for wt in entries.flatten().map(|d| d.path()).filter(|p| p.join(".git").is_file()) {
                        let _ = git::worktree_remove(&root, &wt, true);
                    }
                }
                let _ = git::run(&root, &["worktree", "prune"]);
            }
            self.store.audit("system", "recovery.experiment", Some(&e.id), &json!({"attempt": e.attempt}));
        }
    }
}
