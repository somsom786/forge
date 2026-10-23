//! Persistence for verification runs, manual verdicts, review decisions and Proof Packs.

use crate::contract::{Contract, RunState};
use crate::error::{ForgeError, Result};
use crate::store::{Store, new_id, now};
use rusqlite::{OptionalExtension, Row, params};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VerificationRun {
    pub id: String,
    pub mission_id: String,
    pub check_id: String,
    pub name: String,
    pub command: String,
    pub kind: String,
    pub state: RunState,
    pub exit_code: Option<i32>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub duration_ms: Option<i64>,
    /// Fingerprint of the changes the check ran against.
    pub fingerprint: String,
    pub head_commit: Option<String>,
    /// The check modified files in the worktree while running, so its result no longer matches the changes.
    pub mutated: bool,
    pub output_tail: String,
    pub output_truncated: bool,
    pub log_path: Option<String>,
    /// Revision of the check definition this ran under; results never carry over to a changed check.
    pub spec_rev: String,
    /// The execution backend the command ran on (part of the evidence: a pass on the host is not a pass in isolation).
    pub backend: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ManualVerdictRow {
    pub criterion_id: String,
    pub pass: bool,
    pub note: Option<String>,
    pub fingerprint: String,
    /// Revision of the criterion the verdict was given on.
    pub criterion_rev: String,
    pub decided_by: String,
    pub at: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReviewDecisionRow {
    pub id: String,
    pub mission_id: String,
    pub decision: String,
    pub note: Option<String>,
    pub override_reason: Option<String>,
    pub fingerprint: String,
    pub head_commit: Option<String>,
    pub decided_by: String,
    pub at: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProofPackRow {
    pub id: String,
    pub mission_id: String,
    pub created_at: String,
    pub content_sha256: String,
    pub markdown: String,
    pub diff_path: Option<String>,
    pub content: serde_json::Value,
}

pub fn run_state_str(s: RunState) -> &'static str {
    match s {
        RunState::Running => "running",
        RunState::Passed => "passed",
        RunState::Failed => "failed",
        RunState::Error => "error",
        RunState::Timeout => "timeout",
        RunState::Cancelled => "cancelled",
    }
}

fn run_state_parse(s: &str) -> RunState {
    match s {
        "running" => RunState::Running,
        "passed" => RunState::Passed,
        "failed" => RunState::Failed,
        "timeout" => RunState::Timeout,
        "cancelled" => RunState::Cancelled,
        _ => RunState::Error,
    }
}

const RUN_COLS: &str = "id, mission_id, check_id, name, command, kind, state, exit_code, started_at, ended_at, duration_ms, fingerprint, head_commit, mutated, output_tail, output_truncated, log_path, spec_rev, backend";

fn run_from(r: &Row) -> rusqlite::Result<VerificationRun> {
    let state: String = r.get(6)?;
    Ok(VerificationRun {
        id: r.get(0)?,
        mission_id: r.get(1)?,
        check_id: r.get(2)?,
        name: r.get(3)?,
        command: r.get(4)?,
        kind: r.get(5)?,
        state: run_state_parse(&state),
        exit_code: r.get(7)?,
        started_at: r.get(8)?,
        ended_at: r.get(9)?,
        duration_ms: r.get(10)?,
        fingerprint: r.get(11)?,
        head_commit: r.get(12)?,
        mutated: r.get::<_, i64>(13)? != 0,
        output_tail: r.get(14)?,
        output_truncated: r.get::<_, i64>(15)? != 0,
        log_path: r.get(16)?,
        spec_rev: r.get(17)?,
        backend: r.get(18)?,
    })
}

impl Store {
    // ---- contract ------------------------------------------------------------------------

    pub fn set_contract(&self, mission_id: &str, contract: &Contract) -> Result<()> {
        let json = serde_json::to_string(contract)?;
        let n = self.c().execute("UPDATE mission SET contract_json=?2, updated_at=?3 WHERE id=?1", params![mission_id, json, now()])?;
        if n == 0 {
            return Err(ForgeError::NotFound(format!("mission {mission_id}")));
        }
        Ok(())
    }

    pub fn get_contract(&self, mission_id: &str) -> Result<Contract> {
        let json: Option<String> = self
            .c()
            .query_row("SELECT contract_json FROM mission WHERE id=?1", [mission_id], |r| r.get(0))
            .optional()?
            .ok_or_else(|| ForgeError::NotFound(format!("mission {mission_id}")))?;
        match json {
            None => Ok(Contract { version: crate::contract::CONTRACT_VERSION, ..Default::default() }),
            Some(j) => Ok(serde_json::from_str(&j)?),
        }
    }

    // ---- verification runs -------------------------------------------------------------------

    pub fn insert_run(&self, run: &VerificationRun) -> Result<()> {
        self.c().execute(
            "INSERT INTO verification_run(id,mission_id,check_id,name,command,kind,state,started_at,fingerprint,head_commit,spec_rev,backend) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![run.id, run.mission_id, run.check_id, run.name, run.command, run.kind, run_state_str(run.state), run.started_at, run.fingerprint, run.head_commit, run.spec_rev, run.backend],
        )?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn finish_run(&self, id: &str, state: RunState, exit_code: Option<i32>, duration_ms: i64, fingerprint: &str, mutated: bool, output_tail: &str, truncated: bool, log_path: Option<&str>) -> Result<VerificationRun> {
        self.c().execute(
            "UPDATE verification_run SET state=?2, exit_code=?3, ended_at=?4, duration_ms=?5, fingerprint=?6, mutated=?7, output_tail=?8, output_truncated=?9, log_path=?10 WHERE id=?1",
            params![id, run_state_str(state), exit_code, now(), duration_ms, fingerprint, mutated as i64, output_tail, truncated as i64, log_path],
        )?;
        self.get_run(id)
    }

    pub fn get_run(&self, id: &str) -> Result<VerificationRun> {
        self.c()
            .query_row(&format!("SELECT {RUN_COLS} FROM verification_run WHERE id=?1"), [id], run_from)
            .optional()?
            .ok_or_else(|| ForgeError::NotFound(format!("verification run {id}")))
    }

    pub fn runs_for_mission(&self, mission_id: &str) -> Result<Vec<VerificationRun>> {
        let c = self.c();
        let mut s = c.prepare(&format!("SELECT {RUN_COLS} FROM verification_run WHERE mission_id=?1 ORDER BY started_at, id"))?;
        Ok(s.query_map([mission_id], run_from)?.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Latest run per check id.
    pub fn latest_runs(&self, mission_id: &str) -> Result<HashMap<String, VerificationRun>> {
        let mut m = HashMap::new();
        for r in self.runs_for_mission(mission_id)? {
            m.insert(r.check_id.clone(), r);
        }
        Ok(m)
    }

    /// Runs that were mid-flight when FORGE stopped can never finish; say so instead of leaving them "running".
    pub fn fail_orphaned_runs(&self) -> Result<usize> {
        Ok(self.c().execute(
            "UPDATE verification_run SET state='error', ended_at=?1, output_tail='FORGE stopped while this check was running; its result is unknown.' WHERE state='running'",
            [now()],
        )?)
    }

    // ---- manual verdicts ------------------------------------------------------------------------

    pub fn add_manual_verdict(&self, mission_id: &str, criterion_id: &str, pass: bool, note: Option<&str>, fingerprint: &str, criterion_rev: &str) -> Result<()> {
        self.c().execute(
            "INSERT INTO manual_verdict(mission_id, criterion_id, pass, note, fingerprint, criterion_rev, decided_by, at) VALUES (?1,?2,?3,?4,?5,?6,'user',?7)",
            params![mission_id, criterion_id, pass as i64, note, fingerprint, criterion_rev, now()],
        )?;
        Ok(())
    }

    pub fn latest_manual_verdicts(&self, mission_id: &str) -> Result<HashMap<String, ManualVerdictRow>> {
        let c = self.c();
        let mut s = c.prepare("SELECT criterion_id, pass, note, fingerprint, criterion_rev, decided_by, at FROM manual_verdict WHERE mission_id=?1 ORDER BY id")?;
        let rows = s.query_map([mission_id], |r| {
            Ok(ManualVerdictRow { criterion_id: r.get(0)?, pass: r.get::<_, i64>(1)? != 0, note: r.get(2)?, fingerprint: r.get(3)?, criterion_rev: r.get(4)?, decided_by: r.get(5)?, at: r.get(6)? })
        })?;
        let mut m = HashMap::new();
        for r in rows {
            let r = r?;
            m.insert(r.criterion_id.clone(), r);
        }
        Ok(m)
    }

    // ---- review decisions -----------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    pub fn add_review_decision(&self, mission_id: &str, decision: &str, note: Option<&str>, override_reason: Option<&str>, fingerprint: &str, head_commit: Option<&str>) -> Result<ReviewDecisionRow> {
        let id = new_id();
        let at = now();
        self.c().execute(
            "INSERT INTO review_decision(id, mission_id, decision, note, override_reason, fingerprint, head_commit, decided_by, at) VALUES (?1,?2,?3,?4,?5,?6,?7,'user',?8)",
            params![id, mission_id, decision, note, override_reason, fingerprint, head_commit, at],
        )?;
        Ok(ReviewDecisionRow { id, mission_id: mission_id.into(), decision: decision.into(), note: note.map(Into::into), override_reason: override_reason.map(Into::into), fingerprint: fingerprint.into(), head_commit: head_commit.map(Into::into), decided_by: "user".into(), at })
    }

    pub fn review_decisions(&self, mission_id: &str) -> Result<Vec<ReviewDecisionRow>> {
        let c = self.c();
        let mut s = c.prepare("SELECT id, mission_id, decision, note, override_reason, fingerprint, head_commit, decided_by, at FROM review_decision WHERE mission_id=?1 ORDER BY at, id")?;
        Ok(s.query_map([mission_id], |r| {
            Ok(ReviewDecisionRow { id: r.get(0)?, mission_id: r.get(1)?, decision: r.get(2)?, note: r.get(3)?, override_reason: r.get(4)?, fingerprint: r.get(5)?, head_commit: r.get(6)?, decided_by: r.get(7)?, at: r.get(8)? })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    // ---- proof packs ------------------------------------------------------------------------------

    pub fn insert_proof_pack(&self, mission_id: &str, content: &serde_json::Value, sha256: &str, markdown: &str, diff_path: Option<&str>) -> Result<ProofPackRow> {
        let id = new_id();
        let created_at = now();
        self.c().execute(
            "INSERT INTO proof_pack(id, mission_id, created_at, content_json, content_sha256, markdown, diff_path) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![id, mission_id, created_at, content.to_string(), sha256, markdown, diff_path],
        )?;
        Ok(ProofPackRow { id, mission_id: mission_id.into(), created_at, content_sha256: sha256.into(), markdown: markdown.into(), diff_path: diff_path.map(Into::into), content: content.clone() })
    }

    pub fn proof_packs(&self, mission_id: &str) -> Result<Vec<ProofPackRow>> {
        let c = self.c();
        let mut s = c.prepare("SELECT id, mission_id, created_at, content_sha256, markdown, diff_path, content_json FROM proof_pack WHERE mission_id=?1 ORDER BY created_at DESC, id DESC")?;
        Ok(s.query_map([mission_id], |r| {
            let content: String = r.get(6)?;
            Ok(ProofPackRow { id: r.get(0)?, mission_id: r.get(1)?, created_at: r.get(2)?, content_sha256: r.get(3)?, markdown: r.get(4)?, diff_path: r.get(5)?, content: serde_json::from_str(&content).unwrap_or(serde_json::Value::Null) })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{CheckKind, CheckSpec};
    use crate::store::NewMission;

    fn seeded() -> (Store, String) {
        let s = Store::in_memory();
        let p = s.add_project("p", "C:/p", "main").unwrap();
        let m = s.create_mission(&NewMission { project_id: &p.id, title: "t", objective: "o", provider: "fake", model: None, permission_profile: "default" }).unwrap();
        (s, m.id)
    }

    fn run(mid: &str, check: &str, fp: &str) -> VerificationRun {
        VerificationRun { id: new_id(), mission_id: mid.into(), check_id: check.into(), name: check.into(), command: "x".into(), kind: "test".into(), state: RunState::Running, exit_code: None, started_at: now(), ended_at: None, duration_ms: None, fingerprint: fp.into(), head_commit: None, mutated: false, output_tail: String::new(), output_truncated: false, log_path: None, spec_rev: "r1".into(), backend: "trusted_local".into() }
    }

    #[test]
    fn contract_roundtrips_and_defaults_to_empty() {
        let (s, id) = seeded();
        assert!(s.get_contract(&id).unwrap().criteria.is_empty());
        let c = Contract {
            version: 1,
            checks: vec![CheckSpec { id: "t".into(), name: "Tests".into(), command: "npm test".into(), kind: CheckKind::Test, timeout_secs: 60, required: true }],
            in_scope: vec!["src/".into()],
            ..Default::default()
        };
        s.set_contract(&id, &c).unwrap();
        assert_eq!(s.get_contract(&id).unwrap(), c);
        assert!(s.set_contract("missing", &c).is_err());
    }

    #[test]
    fn runs_track_state_and_latest_wins_per_check() {
        let (s, id) = seeded();
        let r1 = run(&id, "t", "f1");
        s.insert_run(&r1).unwrap();
        let done = s.finish_run(&r1.id, RunState::Failed, Some(1), 1200, "f1", false, "boom", false, None).unwrap();
        assert_eq!((done.state, done.exit_code, done.duration_ms), (RunState::Failed, Some(1), Some(1200)));
        std::thread::sleep(std::time::Duration::from_millis(3));
        let r2 = run(&id, "t", "f2");
        s.insert_run(&r2).unwrap();
        s.finish_run(&r2.id, RunState::Passed, Some(0), 900, "f2", true, "ok", true, Some("/log")).unwrap();
        let latest = s.latest_runs(&id).unwrap();
        assert_eq!(latest.len(), 1);
        assert_eq!(latest["t"].id, r2.id);
        assert!(latest["t"].mutated && latest["t"].output_truncated);
        assert_eq!(s.runs_for_mission(&id).unwrap().len(), 2, "history is kept");
    }

    #[test]
    fn orphaned_running_checks_become_errors_not_forever_running() {
        let (s, id) = seeded();
        s.insert_run(&run(&id, "t", "f")).unwrap();
        assert_eq!(s.fail_orphaned_runs().unwrap(), 1);
        let r = &s.runs_for_mission(&id).unwrap()[0];
        assert_eq!(r.state, RunState::Error);
        assert!(r.output_tail.contains("result is unknown"));
        assert_eq!(s.fail_orphaned_runs().unwrap(), 0);
    }

    #[test]
    fn latest_manual_verdict_per_criterion_and_review_history() {
        let (s, id) = seeded();
        s.add_manual_verdict(&id, "c1", true, Some("fine"), "f1", "cr1").unwrap();
        s.add_manual_verdict(&id, "c1", false, Some("no, regressed"), "f2", "cr1").unwrap();
        s.add_manual_verdict(&id, "c2", true, None, "f2", "cr2").unwrap();
        let v = s.latest_manual_verdicts(&id).unwrap();
        assert!(!v["c1"].pass && v["c1"].fingerprint == "f2" && v["c2"].pass);
        s.add_review_decision(&id, "request_changes", Some("add tests"), None, "f1", Some("abc")).unwrap();
        s.add_review_decision(&id, "approve", None, Some("CI is down"), "f2", Some("def")).unwrap();
        let d = s.review_decisions(&id).unwrap();
        assert_eq!(d.iter().map(|x| x.decision.as_str()).collect::<Vec<_>>(), vec!["request_changes", "approve"]);
        assert_eq!(d[1].override_reason.as_deref(), Some("CI is down"));
    }

    #[test]
    fn proof_packs_are_kept_newest_first() {
        let (s, id) = seeded();
        s.insert_proof_pack(&id, &serde_json::json!({"v": 1}), "aaa", "# one", None).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(3));
        s.insert_proof_pack(&id, &serde_json::json!({"v": 2}), "bbb", "# two", Some("/d.diff")).unwrap();
        let p = s.proof_packs(&id).unwrap();
        assert_eq!(p.iter().map(|x| x.content_sha256.as_str()).collect::<Vec<_>>(), vec!["bbb", "aaa"]);
        assert_eq!(p[0].content["v"], 2);
    }
}
