//! Persistence for experiments (Phase 6): definitions, the revisions they compare, and every recorded run of every attempt.

use crate::error::{ForgeError, Result};
use crate::experiment::{Metric, NewExperiment, Report};
use crate::store::{Store, new_id, now};
use rusqlite::{OptionalExtension, Row, params};
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Experiment {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub objective: Option<String>,
    pub command: String,
    pub correctness_command: Option<String>,
    pub metric: Metric,
    pub repetitions: u32,
    pub warmup: u32,
    pub timeout_secs: u64,
    /// draft | running | completed | failed | cancelled | interrupted
    pub state: String,
    /// Number of times it has been started; results belong to one attempt.
    pub attempt: i64,
    /// OS, architecture, CPU count and tool versions recorded when the latest attempt started.
    pub env: Option<serde_json::Value>,
    /// The verdict of the latest finished attempt.
    pub report: Option<Report>,
    pub last_error: Option<String>,
    pub created_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExperimentVariant {
    pub id: String,
    pub experiment_id: String,
    pub position: i64,
    pub label: String,
    pub is_baseline: bool,
    /// What the user named (branch, tag or commit).
    pub git_ref: String,
    /// The exact commit it resolved to when the experiment was created; every attempt measures this commit.
    pub commit_sha: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExperimentResult {
    pub id: String,
    pub experiment_id: String,
    pub variant_id: String,
    pub attempt: i64,
    /// correctness | warmup | measure
    pub phase: String,
    pub iteration: i64,
    /// passed | failed | error | timeout | cancelled | no_metric
    pub state: String,
    pub exit_code: Option<i32>,
    pub duration_ms: i64,
    pub metric_value: Option<f64>,
    pub worktree_changed: bool,
    pub output_tail: String,
    pub log_path: Option<String>,
    pub started_at: String,
}

pub struct NewResult<'a> {
    pub experiment_id: &'a str,
    pub variant_id: &'a str,
    pub attempt: i64,
    pub phase: &'a str,
    pub iteration: i64,
    pub state: &'a str,
    pub exit_code: Option<i32>,
    pub duration_ms: i64,
    pub metric_value: Option<f64>,
    pub worktree_changed: bool,
    pub output_tail: &'a str,
    pub log_path: Option<&'a str>,
    pub started_at: &'a str,
}

const EXP_COLS: &str = "id, project_id, name, objective, command, correctness_command, metric_json, repetitions, warmup, timeout_secs, state, attempt, env_json, report_json, last_error, created_at, started_at, finished_at";

fn exp_from(r: &Row) -> rusqlite::Result<Experiment> {
    let json_col = |i: usize| -> rusqlite::Result<Option<String>> { r.get(i) };
    let metric_json: String = r.get(6)?;
    Ok(Experiment {
        id: r.get(0)?,
        project_id: r.get(1)?,
        name: r.get(2)?,
        objective: r.get(3)?,
        command: r.get(4)?,
        correctness_command: r.get(5)?,
        metric: serde_json::from_str(&metric_json).map_err(|e| rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(e)))?,
        repetitions: r.get::<_, i64>(7)? as u32,
        warmup: r.get::<_, i64>(8)? as u32,
        timeout_secs: r.get::<_, i64>(9)? as u64,
        state: r.get(10)?,
        attempt: r.get(11)?,
        env: json_col(12)?.and_then(|s| serde_json::from_str(&s).ok()),
        // A report written by an older build that no longer parses is shown as absent rather than failing the read.
        report: json_col(13)?.and_then(|s| serde_json::from_str(&s).ok()),
        last_error: r.get(14)?,
        created_at: r.get(15)?,
        started_at: r.get(16)?,
        finished_at: r.get(17)?,
    })
}

fn variant_from(r: &Row) -> rusqlite::Result<ExperimentVariant> {
    Ok(ExperimentVariant { id: r.get(0)?, experiment_id: r.get(1)?, position: r.get(2)?, label: r.get(3)?, is_baseline: r.get::<_, i64>(4)? != 0, git_ref: r.get(5)?, commit_sha: r.get(6)? })
}

fn result_from(r: &Row) -> rusqlite::Result<ExperimentResult> {
    Ok(ExperimentResult {
        id: r.get(0)?,
        experiment_id: r.get(1)?,
        variant_id: r.get(2)?,
        attempt: r.get(3)?,
        phase: r.get(4)?,
        iteration: r.get(5)?,
        state: r.get(6)?,
        exit_code: r.get(7)?,
        duration_ms: r.get(8)?,
        metric_value: r.get(9)?,
        worktree_changed: r.get::<_, i64>(10)? != 0,
        output_tail: r.get(11)?,
        log_path: r.get(12)?,
        started_at: r.get(13)?,
    })
}

impl Report {
    fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

impl Store {
    /// Insert an experiment and its revisions (baseline first) in one transaction. `commits` are resolved by the caller, in the
    /// order baseline, variants...
    pub fn exp_create(&self, project_id: &str, n: &NewExperiment, commits: &[String]) -> Result<Experiment> {
        if commits.len() != n.variants.len() + 1 {
            return Err(ForgeError::Other("every revision needs a resolved commit".into()));
        }
        let id = new_id();
        let mut c = self.c();
        let tx = c.transaction()?;
        tx.execute(
            "INSERT INTO experiment(id, project_id, name, objective, command, correctness_command, metric_json, repetitions, warmup, timeout_secs, state, attempt, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'draft',0,?11)",
            params![
                id,
                project_id,
                n.name.trim(),
                n.objective.as_deref().map(str::trim).filter(|o| !o.is_empty()),
                n.command.trim(),
                n.correctness_command.as_deref().map(str::trim),
                serde_json::to_string(&n.metric).unwrap_or_default(),
                n.repetitions as i64,
                n.warmup as i64,
                n.timeout_secs as i64,
                now()
            ],
        )?;
        let revisions = std::iter::once(("baseline", n.baseline_ref.trim(), true)).chain(n.variants.iter().map(|v| (v.label.trim(), v.git_ref.trim(), false)));
        for (pos, ((label, git_ref, base), commit)) in revisions.zip(commits).enumerate() {
            tx.execute(
                "INSERT INTO experiment_variant(id, experiment_id, position, label, is_baseline, git_ref, commit_sha) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![new_id(), id, pos as i64, label, base as i64, git_ref, commit],
            )?;
        }
        tx.commit()?;
        drop(c);
        self.exp_get(&id)
    }

    pub fn exp_get(&self, id: &str) -> Result<Experiment> {
        self.c()
            .query_row(&format!("SELECT {EXP_COLS} FROM experiment WHERE id=?1"), [id], exp_from)
            .optional()?
            .ok_or_else(|| ForgeError::NotFound(format!("experiment {id}")))
    }

    pub fn exp_list(&self, project_id: &str) -> Result<Vec<Experiment>> {
        let c = self.c();
        let mut s = c.prepare(&format!("SELECT {EXP_COLS} FROM experiment WHERE project_id=?1 ORDER BY created_at DESC, id DESC"))?;
        Ok(s.query_map([project_id], exp_from)?.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn exp_variants(&self, experiment_id: &str) -> Result<Vec<ExperimentVariant>> {
        let c = self.c();
        let mut s = c.prepare("SELECT id, experiment_id, position, label, is_baseline, git_ref, commit_sha FROM experiment_variant WHERE experiment_id=?1 ORDER BY position")?;
        Ok(s.query_map([experiment_id], variant_from)?.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Claim the experiment for a new attempt. Exactly one caller wins; a running experiment cannot be started again.
    pub fn exp_begin_attempt(&self, id: &str, env: &serde_json::Value) -> Result<Experiment> {
        let n = self.c().execute(
            "UPDATE experiment SET state='running', attempt=attempt+1, env_json=?2, started_at=?3, finished_at=NULL, last_error=NULL WHERE id=?1 AND state<>'running'",
            params![id, env.to_string(), now()],
        )?;
        if n != 1 {
            let _ = self.exp_get(id)?; // NotFound if it does not exist
            return Err(ForgeError::InvalidState("this experiment is already running".into()));
        }
        self.exp_get(id)
    }

    /// End the running attempt. Only a running experiment can be finished.
    pub fn exp_finish_attempt(&self, id: &str, state: &str, report: Option<&Report>, error: Option<&str>) -> Result<Experiment> {
        let n = self.c().execute(
            "UPDATE experiment SET state=?2, report_json=COALESCE(?3, report_json), last_error=?4, finished_at=?5 WHERE id=?1 AND state='running'",
            params![id, state, report.map(Report::to_json), error, now()],
        )?;
        if n != 1 {
            return Err(ForgeError::InvalidState(format!("experiment {id} is not running")));
        }
        self.exp_get(id)
    }

    pub fn exp_add_result(&self, r: NewResult) -> Result<ExperimentResult> {
        let id = new_id();
        self.c().execute(
            "INSERT INTO experiment_result(id, experiment_id, variant_id, attempt, phase, iteration, state, exit_code, duration_ms, metric_value, worktree_changed, output_tail, log_path, started_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
            params![id, r.experiment_id, r.variant_id, r.attempt, r.phase, r.iteration, r.state, r.exit_code, r.duration_ms, r.metric_value, r.worktree_changed as i64, r.output_tail, r.log_path, r.started_at],
        )?;
        Ok(self.c().query_row(
            "SELECT id, experiment_id, variant_id, attempt, phase, iteration, state, exit_code, duration_ms, metric_value, worktree_changed, output_tail, log_path, started_at FROM experiment_result WHERE id=?1",
            [&id],
            result_from,
        )?)
    }

    /// Every recorded run of one attempt, in the order they ran.
    pub fn exp_results(&self, experiment_id: &str, attempt: i64) -> Result<Vec<ExperimentResult>> {
        let c = self.c();
        let mut s = c.prepare("SELECT id, experiment_id, variant_id, attempt, phase, iteration, state, exit_code, duration_ms, metric_value, worktree_changed, output_tail, log_path, started_at FROM experiment_result WHERE experiment_id=?1 AND attempt=?2 ORDER BY started_at, rowid")?;
        Ok(s.query_map(params![experiment_id, attempt], result_from)?.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Experiments a crash left `running`.
    pub fn exp_running(&self) -> Result<Vec<Experiment>> {
        let c = self.c();
        let mut s = c.prepare(&format!("SELECT {EXP_COLS} FROM experiment WHERE state='running'"))?;
        Ok(s.query_map([], exp_from)?.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}
