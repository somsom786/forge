//! Mission contracts and criterion evaluation.
//!
//! The central rule of the Proof Engine: a criterion's status is *derived from evidence*, never from
//! what an agent says. An agent claiming "done" changes nothing here.

use crate::error::{ForgeError, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

pub const CONTRACT_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckKind {
    Test,
    Build,
    Lint,
    Typecheck,
    Security,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckSpec {
    pub id: String,
    pub name: String,
    /// Shell command line, run in the mission worktree. Authored or confirmed by the user, never by the agent.
    pub command: String,
    pub kind: CheckKind,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    /// A required check that is not passing on the current changes blocks approval (unless overridden with a reason).
    #[serde(default)]
    pub required: bool,
}

fn default_timeout() -> u64 {
    600
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Criterion {
    pub id: String,
    pub text: String,
    /// Ids of checks whose passing result is evidence for this criterion.
    #[serde(default)]
    pub verified_by: Vec<String>,
    /// Needs a human verdict (in addition to any checks).
    #[serde(default)]
    pub manual: bool,
    /// Evaluated automatically: changed files must respect the contract's scope.
    #[serde(default)]
    pub scope_rule: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Contract {
    #[serde(default)]
    pub version: u32,
    /// Globs (relative to the repo root). Empty = no restriction.
    #[serde(default)]
    pub in_scope: Vec<String>,
    #[serde(default)]
    pub out_of_scope: Vec<String>,
    #[serde(default)]
    pub criteria: Vec<Criterion>,
    #[serde(default)]
    pub checks: Vec<CheckSpec>,
    #[serde(default)]
    pub security_constraints: Vec<String>,
    #[serde(default)]
    pub review_expectations: Option<String>,
}

impl Contract {
    pub fn validate(&self) -> Result<()> {
        let mut seen = HashSet::new();
        for c in &self.checks {
            if c.id.trim().is_empty() || c.command.trim().is_empty() {
                return Err(ForgeError::Other(format!("check \"{}\" needs an id and a command", c.name)));
            }
            if !seen.insert(c.id.as_str()) {
                return Err(ForgeError::Other(format!("duplicate check id \"{}\"", c.id)));
            }
            if c.timeout_secs == 0 || c.timeout_secs > 4 * 3600 {
                return Err(ForgeError::Other(format!("check \"{}\": timeout must be between 1 second and 4 hours", c.id)));
            }
        }
        let mut cseen = HashSet::new();
        for cr in &self.criteria {
            if cr.id.trim().is_empty() || cr.text.trim().is_empty() {
                return Err(ForgeError::Other("every criterion needs an id and text".into()));
            }
            if !cseen.insert(cr.id.as_str()) {
                return Err(ForgeError::Other(format!("duplicate criterion id \"{}\"", cr.id)));
            }
            for v in &cr.verified_by {
                if !seen.contains(v.as_str()) {
                    return Err(ForgeError::Other(format!("criterion \"{}\" references unknown check \"{v}\"", cr.id)));
                }
            }
        }
        scope_sets(self)?;
        Ok(())
    }
}

fn build_set(globs: &[String]) -> Result<GlobSet> {
    let mut b = GlobSetBuilder::new();
    for g in globs {
        // `dir/` means everything beneath it.
        let pat = if g.ends_with('/') { format!("{g}**") } else { g.clone() };
        b.add(Glob::new(&pat).map_err(|e| ForgeError::Other(format!("bad scope glob \"{g}\": {e}")))?);
    }
    b.build().map_err(|e| ForgeError::Other(format!("bad scope globs: {e}")))
}

fn scope_sets(c: &Contract) -> Result<(GlobSet, GlobSet)> {
    Ok((build_set(&c.in_scope)?, build_set(&c.out_of_scope)?))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeReport {
    pub outside_in_scope: Vec<String>,
    pub in_out_of_scope: Vec<String>,
}

impl ScopeReport {
    pub fn clean(&self) -> bool {
        self.outside_in_scope.is_empty() && self.in_out_of_scope.is_empty()
    }
}

/// Check changed paths (repo-relative, `/` separators) against the contract's scope.
pub fn check_scope(c: &Contract, changed: &[String]) -> Result<ScopeReport> {
    let (inside, outside) = scope_sets(c)?;
    let restrict = !c.in_scope.is_empty();
    let mut r = ScopeReport { outside_in_scope: vec![], in_out_of_scope: vec![] };
    for p in changed {
        let p = p.replace('\\', "/");
        if outside.is_match(&p) {
            r.in_out_of_scope.push(p.clone());
        }
        if restrict && !inside.is_match(&p) {
            r.outside_in_scope.push(p);
        }
    }
    Ok(r)
}

// ---- evidence & status ------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Running,
    Passed,
    Failed,
    Error,
    Timeout,
    Cancelled,
}

/// The latest recorded run of one check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunEvidence {
    pub check_id: String,
    pub run_id: String,
    pub state: RunState,
    /// Fingerprint of the mission's changes when the check ran.
    pub fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManualVerdict {
    pub pass: bool,
    pub note: Option<String>,
    pub fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum CriterionStatus {
    /// Nothing is linked to this criterion that could verify it.
    NotChecked,
    /// Linked checks exist but at least one has not run (or is still running).
    CheckPending { pending: Vec<String> },
    /// All linked checks passed on the current changes (and any required manual verdict is in).
    VerifiedByChecks { evidence: Vec<String> },
    VerifiedManually { note: Option<String> },
    VerifiedByScopeCheck,
    Failed { reason: String },
    /// Evidence exists but cannot be trusted: error/timeout/cancelled run, or it ran on different changes.
    Inconclusive { reason: String },
    RequiresManualReview { reason: String },
}

pub struct EvalContext<'a> {
    /// Latest run per check id.
    pub runs: &'a HashMap<String, RunEvidence>,
    pub manual: &'a HashMap<String, ManualVerdict>,
    pub fingerprint: &'a str,
    pub scope: &'a ScopeReport,
}

/// Derive a criterion's status. Precedence: Failed > Inconclusive > CheckPending > RequiresManualReview > Verified.
pub fn evaluate(c: &Criterion, x: &EvalContext) -> CriterionStatus {
    use CriterionStatus::*;

    if c.scope_rule {
        return if x.scope.clean() {
            VerifiedByScopeCheck
        } else {
            let mut all: Vec<&str> = x.scope.in_out_of_scope.iter().map(String::as_str).collect();
            all.extend(x.scope.outside_in_scope.iter().map(String::as_str));
            Failed { reason: format!("{} file(s) violate the contract scope: {}", all.len(), all.iter().take(5).copied().collect::<Vec<_>>().join(", ")) }
        };
    }

    let mut failed = vec![];
    let mut inconclusive = vec![];
    let mut pending = vec![];
    let mut passed = vec![];
    for id in &c.verified_by {
        match x.runs.get(id) {
            None => pending.push(id.clone()),
            Some(r) => match r.state {
                RunState::Running => pending.push(id.clone()),
                RunState::Failed => failed.push(id.clone()),
                RunState::Error | RunState::Timeout | RunState::Cancelled => inconclusive.push(format!("check {id} {}", match r.state {
                    RunState::Timeout => "timed out",
                    RunState::Cancelled => "was cancelled",
                    _ => "could not run",
                })),
                RunState::Passed if r.fingerprint != x.fingerprint => inconclusive.push(format!("check {id} passed on earlier changes; re-run it")),
                RunState::Passed => passed.push(format!("{id}#{}", &r.run_id[..r.run_id.len().min(8)])),
            },
        }
    }
    let manual = x.manual.get(&c.id);
    if let Some(m) = manual {
        if !m.pass && m.fingerprint == x.fingerprint {
            failed.push(format!("manual review: {}", m.note.clone().unwrap_or_else(|| "marked failed".into())));
        }
    }
    if !failed.is_empty() {
        return Failed { reason: format!("failed: {}", failed.join(", ")) };
    }
    if !inconclusive.is_empty() {
        return Inconclusive { reason: inconclusive.join("; ") };
    }
    if !pending.is_empty() {
        return CheckPending { pending };
    }
    if c.manual {
        return match manual {
            Some(m) if m.pass && m.fingerprint == x.fingerprint => VerifiedManually { note: m.note.clone() },
            Some(m) if m.fingerprint != x.fingerprint => RequiresManualReview { reason: "changes were made after the last manual verdict".into() },
            _ => RequiresManualReview { reason: "needs a human verdict".into() },
        };
    }
    if !passed.is_empty() {
        return VerifiedByChecks { evidence: passed };
    }
    if let Some(m) = manual {
        if m.pass && m.fingerprint == x.fingerprint {
            return VerifiedManually { note: m.note.clone() };
        }
    }
    NotChecked
}

/// Required checks that are not currently passing on the current changes.
pub fn blocking_checks(c: &Contract, runs: &HashMap<String, RunEvidence>, fingerprint: &str) -> Vec<String> {
    c.checks
        .iter()
        .filter(|k| k.required)
        .filter(|k| !matches!(runs.get(&k.id), Some(r) if r.state == RunState::Passed && r.fingerprint == fingerprint))
        .map(|k| k.name.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use CriterionStatus::*;

    fn run(id: &str, state: RunState, fp: &str) -> (String, RunEvidence) {
        (id.into(), RunEvidence { check_id: id.into(), run_id: format!("{id}-run-0001"), state, fingerprint: fp.into() })
    }
    fn crit(by: &[&str]) -> Criterion {
        Criterion { id: "c1".into(), text: "tokens expire".into(), verified_by: by.iter().map(|s| s.to_string()).collect(), manual: false, scope_rule: false }
    }
    fn clean() -> ScopeReport {
        ScopeReport { outside_in_scope: vec![], in_out_of_scope: vec![] }
    }
    fn eval(c: &Criterion, runs: &[(String, RunEvidence)], manual: &[(&str, ManualVerdict)], fp: &str) -> CriterionStatus {
        let runs: HashMap<_, _> = runs.iter().cloned().collect();
        let manual: HashMap<_, _> = manual.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
        let scope = clean();
        evaluate(c, &EvalContext { runs: &runs, manual: &manual, fingerprint: fp, scope: &scope })
    }

    #[test]
    fn nothing_linked_means_not_checked_regardless_of_agent_claims() {
        assert_eq!(eval(&crit(&[]), &[], &[], "f1"), NotChecked);
    }

    #[test]
    fn passing_check_on_current_changes_verifies() {
        let s = eval(&crit(&["t"]), &[run("t", RunState::Passed, "f1")], &[], "f1");
        assert!(matches!(s, VerifiedByChecks { ref evidence } if evidence[0].starts_with("t#")), "{s:?}");
    }

    #[test]
    fn stale_pass_is_inconclusive_not_verified() {
        let s = eval(&crit(&["t"]), &[run("t", RunState::Passed, "old")], &[], "new");
        assert!(matches!(s, Inconclusive { .. }), "{s:?}");
    }

    #[test]
    fn precedence_failed_over_inconclusive_over_pending() {
        let c = crit(&["a", "b", "c"]);
        let runs = [run("a", RunState::Passed, "f"), run("b", RunState::Timeout, "f")];
        assert!(matches!(eval(&c, &runs, &[], "f"), Inconclusive { .. }));
        let runs = [run("a", RunState::Failed, "f"), run("b", RunState::Timeout, "f")];
        assert!(matches!(eval(&c, &runs, &[], "f"), Failed { .. }));
        let runs = [run("a", RunState::Passed, "f"), run("b", RunState::Passed, "f")];
        assert!(matches!(eval(&c, &runs, &[], "f"), CheckPending { ref pending } if pending == &["c".to_string()]));
    }

    #[test]
    fn running_check_is_pending() {
        assert!(matches!(eval(&crit(&["t"]), &[run("t", RunState::Running, "f")], &[], "f"), CheckPending { .. }));
    }

    #[test]
    fn manual_criteria_need_a_fresh_human_verdict_even_if_checks_pass() {
        let mut c = crit(&["t"]);
        c.manual = true;
        let runs = [run("t", RunState::Passed, "f")];
        assert!(matches!(eval(&c, &runs, &[], "f"), RequiresManualReview { .. }));
        let ok = ManualVerdict { pass: true, note: Some("looks right".into()), fingerprint: "f".into() };
        assert!(matches!(eval(&c, &runs, &[("c1", ok.clone())], "f"), VerifiedManually { .. }));
        assert!(matches!(eval(&c, &runs, &[("c1", ok)], "g"), Inconclusive { .. } | RequiresManualReview { .. }), "verdict goes stale when changes move");
        let bad = ManualVerdict { pass: false, note: Some("wrong".into()), fingerprint: "f".into() };
        assert!(matches!(eval(&c, &runs, &[("c1", bad)], "f"), Failed { .. }));
    }

    #[test]
    fn scope_rule_is_evaluated_automatically() {
        let mut c = crit(&[]);
        c.scope_rule = true;
        let contract = Contract { in_scope: vec!["src/**".into()], out_of_scope: vec!["src/legacy/**".into(), "*.lock".into()], ..Default::default() };
        let ok = check_scope(&contract, &["src/auth/reset.rs".into()]).unwrap();
        let bad = check_scope(&contract, &["src/auth/reset.rs".into(), "README.md".into(), "src/legacy/old.rs".into(), "Cargo.lock".into()]).unwrap();
        assert!(ok.clean());
        assert_eq!(bad.outside_in_scope, vec!["README.md", "Cargo.lock"]);
        assert_eq!(bad.in_out_of_scope, vec!["src/legacy/old.rs", "Cargo.lock"]);
        let runs = HashMap::new();
        let manual = HashMap::new();
        assert_eq!(evaluate(&c, &EvalContext { runs: &runs, manual: &manual, fingerprint: "f", scope: &ok }), VerifiedByScopeCheck);
        assert!(matches!(evaluate(&c, &EvalContext { runs: &runs, manual: &manual, fingerprint: "f", scope: &bad }), Failed { .. }));
    }

    #[test]
    fn scope_normalises_windows_separators_and_dir_globs() {
        let c = Contract { in_scope: vec!["src/".into()], ..Default::default() };
        let r = check_scope(&c, &[r"src\a\b.rs".into(), r"docs\x.md".into()]).unwrap();
        assert_eq!(r.outside_in_scope, vec!["docs/x.md"]);
    }

    #[test]
    fn empty_scope_restricts_nothing() {
        let r = check_scope(&Contract::default(), &["anything/at/all".into()]).unwrap();
        assert!(r.clean());
    }

    #[test]
    fn validation_catches_broken_contracts() {
        let mut c = Contract::default();
        c.checks.push(CheckSpec { id: "t".into(), name: "tests".into(), command: "npm test".into(), kind: CheckKind::Test, timeout_secs: 60, required: true });
        c.criteria.push(Criterion { id: "a".into(), text: "x".into(), verified_by: vec!["missing".into()], manual: false, scope_rule: false });
        assert!(c.validate().unwrap_err().to_string().contains("unknown check"));
        c.criteria[0].verified_by = vec!["t".into()];
        assert!(c.validate().is_ok());
        c.checks.push(c.checks[0].clone());
        assert!(c.validate().unwrap_err().to_string().contains("duplicate check"));
        c.checks.pop();
        c.checks[0].command = "  ".into();
        assert!(c.validate().is_err());
        let bad_glob = Contract { in_scope: vec!["[".into()], ..Default::default() };
        assert!(bad_glob.validate().is_err());
    }

    #[test]
    fn blocking_checks_lists_required_checks_not_passing_now() {
        let mut c = Contract::default();
        for (id, req) in [("t", true), ("l", true), ("o", false)] {
            c.checks.push(CheckSpec { id: id.into(), name: id.into(), command: "x".into(), kind: CheckKind::Custom, timeout_secs: 10, required: req });
        }
        let runs: HashMap<_, _> = [run("t", RunState::Passed, "f")].into_iter().collect();
        assert_eq!(blocking_checks(&c, &runs, "f"), vec!["l"]);
        assert_eq!(blocking_checks(&c, &runs, "g"), vec!["t", "l"], "a pass on older changes does not count");
    }

    #[test]
    fn contract_json_roundtrip_is_camel_case_and_tolerates_missing_fields() {
        let c: Contract = serde_json::from_str(r#"{"inScope":["src/"],"criteria":[{"id":"a","text":"t","verifiedBy":["x"]}]}"#).unwrap();
        assert_eq!(c.criteria[0].verified_by, vec!["x"]);
        assert!(c.checks.is_empty());
        let s = serde_json::to_string(&c).unwrap();
        assert!(s.contains("inScope"));
    }
}
