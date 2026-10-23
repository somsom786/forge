//! Mission contracts and criterion evaluation.
//!
//! The central rule of the Proof Engine: a criterion's status is *derived from evidence*, never from
//! what an agent says. An agent claiming "done" changes nothing here.

use crate::error::{ForgeError, Result};
use globset::{GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
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

fn rev_of(parts: &[&str]) -> String {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p.as_bytes());
        h.update([0u8]);
    }
    format!("{:x}", h.finalize())[..24].to_string()
}

impl CheckSpec {
    /// Identity of what this check *means*: changing the command, kind or timeout makes old results about a different check.
    pub fn revision(&self) -> String {
        rev_of(&[&self.id, &self.command, &format!("{:?}", self.kind), &self.timeout_secs.to_string()])
    }
}

impl Criterion {
    /// Identity of what this criterion *requires*: text, linked checks and flags.
    pub fn revision(&self) -> String {
        let mut by = self.verified_by.clone();
        by.sort();
        rev_of(&[&self.id, &self.text, &by.join(","), &self.manual.to_string(), &self.scope_rule.to_string()])
    }
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
    /// Hash of the whole contract: an approval is only valid for the contract the reviewer saw.
    pub fn revision(&self) -> String {
        let json = serde_json::to_string(self).unwrap_or_default();
        rev_of(&[&json])
    }

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
        // Windows paths are case-insensitive: `SRC/**` must match `src/a.rs` there.
        let glob = globset::GlobBuilder::new(&pat)
            .case_insensitive(cfg!(windows))
            .build()
            .map_err(|e| ForgeError::Other(format!("bad scope glob \"{g}\": {e}")))?;
        b.add(glob);
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
    pub fingerprint: String,
    /// Revision of the check definition it ran under.
    pub spec_rev: String,
    /// The check modified the worktree; its result cannot be trusted as evidence about the current changes.
    pub mutated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManualVerdict {
    pub pass: bool,
    pub note: Option<String>,
    pub fingerprint: String,
    /// Revision of the criterion the verdict was given on.
    pub criterion_rev: String,
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
    /// Current revision of every check definition in the contract.
    pub check_revs: &'a HashMap<String, String>,
    pub manual: &'a HashMap<String, ManualVerdict>,
    pub fingerprint: &'a str,
    pub scope: &'a ScopeReport,
}

/// Derive a criterion's status. Precedence: Failed > Inconclusive > CheckPending > RequiresManualReview > Verified.
/// A scope rule is one requirement among others: it can fail the criterion, but it never hides linked checks or a
/// required human verdict.
pub fn evaluate(c: &Criterion, x: &EvalContext) -> CriterionStatus {
    use CriterionStatus::*;

    let mut failed = vec![];
    let mut inconclusive = vec![];
    let mut pending = vec![];
    let mut passed = vec![];

    if c.scope_rule && !x.scope.clean() {
        let mut all: Vec<&str> = x.scope.in_out_of_scope.iter().map(String::as_str).collect();
        all.extend(x.scope.outside_in_scope.iter().map(String::as_str));
        failed.push(format!("{} file(s) violate the contract scope: {}", all.len(), all.iter().take(5).copied().collect::<Vec<_>>().join(", ")));
    }

    for id in &c.verified_by {
        match x.runs.get(id) {
            None => pending.push(id.clone()),
            Some(r) if r.state == RunState::Running => pending.push(id.clone()),
            Some(r) => {
                // Evidence only counts for the exact check definition it ran under.
                if x.check_revs.get(id).map(String::as_str) != Some(r.spec_rev.as_str()) {
                    inconclusive.push(format!("check {id} was changed after it ran; re-run it"));
                    continue;
                }
                match r.state {
                    RunState::Running => {}
                    RunState::Failed => failed.push(id.clone()),
                    RunState::Error | RunState::Timeout | RunState::Cancelled => inconclusive.push(format!(
                        "check {id} {}",
                        match r.state {
                            RunState::Timeout => "timed out",
                            RunState::Cancelled => "was cancelled",
                            _ => "could not run",
                        }
                    )),
                    // A check that changes the files it is judging proves nothing about them, even if the files later
                    // happen to return to the fingerprint it recorded.
                    RunState::Passed if r.mutated => inconclusive.push(format!("check {id} modified the worktree while running; its result is not trusted")),
                    RunState::Passed if r.fingerprint != x.fingerprint => inconclusive.push(format!("check {id} passed on earlier changes; re-run it")),
                    RunState::Passed => passed.push(format!("{id}#{}", &r.run_id[..r.run_id.len().min(8)])),
                }
            }
        }
    }

    // A human verdict only counts for the criterion text it was given on and the changes it was given on.
    let rev = c.revision();
    let verdict = x.manual.get(&c.id).filter(|m| m.criterion_rev == rev);
    let stale_verdict = x.manual.get(&c.id).filter(|m| m.criterion_rev != rev);
    if let Some(m) = verdict {
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
        return match verdict {
            Some(m) if m.pass && m.fingerprint == x.fingerprint => VerifiedManually { note: m.note.clone() },
            Some(m) if m.fingerprint != x.fingerprint => RequiresManualReview { reason: "changes were made after the last manual verdict".into() },
            _ if stale_verdict.is_some() => RequiresManualReview { reason: "this criterion was edited after the last manual verdict".into() },
            _ => RequiresManualReview { reason: "needs a human verdict".into() },
        };
    }
    if !passed.is_empty() {
        return VerifiedByChecks { evidence: passed };
    }
    if let Some(m) = verdict {
        if m.pass && m.fingerprint == x.fingerprint {
            return VerifiedManually { note: m.note.clone() };
        }
    }
    if c.scope_rule {
        return VerifiedByScopeCheck;
    }
    NotChecked
}

/// Required checks that are not passing on the current changes, under their current definition, without having
/// modified the worktree.
pub fn blocking_checks(c: &Contract, runs: &HashMap<String, RunEvidence>, fingerprint: &str) -> Vec<String> {
    c.checks
        .iter()
        .filter(|k| k.required)
        .filter(|k| !matches!(runs.get(&k.id), Some(r) if r.state == RunState::Passed && r.fingerprint == fingerprint && !r.mutated && r.spec_rev == k.revision()))
        .map(|k| k.name.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use CriterionStatus::*;

    fn run(id: &str, state: RunState, fp: &str) -> (String, RunEvidence) {
        (id.into(), RunEvidence { check_id: id.into(), run_id: format!("{id}-run-0001"), state, fingerprint: fp.into(), spec_rev: "r1".into(), mutated: false })
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
        let revs: HashMap<String, String> = c.verified_by.iter().map(|id| (id.clone(), "r1".to_string())).collect();
        evaluate(c, &EvalContext { runs: &runs, check_revs: &revs, manual: &manual, fingerprint: fp, scope: &scope })
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
        let ok = ManualVerdict { pass: true, note: Some("looks right".into()), fingerprint: "f".into(), criterion_rev: c.revision() };
        assert!(matches!(eval(&c, &runs, &[("c1", ok.clone())], "f"), VerifiedManually { .. }));
        assert!(matches!(eval(&c, &runs, &[("c1", ok)], "g"), Inconclusive { .. } | RequiresManualReview { .. }), "verdict goes stale when changes move");
        let bad = ManualVerdict { pass: false, note: Some("wrong".into()), fingerprint: "f".into(), criterion_rev: c.revision() };
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
        let revs = HashMap::new();
        assert_eq!(evaluate(&c, &EvalContext { runs: &runs, check_revs: &revs, manual: &manual, fingerprint: "f", scope: &ok }), VerifiedByScopeCheck);
        assert!(matches!(evaluate(&c, &EvalContext { runs: &runs, check_revs: &revs, manual: &manual, fingerprint: "f", scope: &bad }), Failed { .. }));
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
        let mut passed = run("t", RunState::Passed, "f");
        passed.1.spec_rev = c.checks[0].revision(); // it ran under the check's current definition
        let runs: HashMap<_, _> = [passed].into_iter().collect();
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

    // ---- regression tests for the independent review (evidence is bound to definitions) ----

    fn eval_with(c: &Criterion, runs: &[(String, RunEvidence)], revs: &[(&str, &str)], manual: &[(&str, ManualVerdict)], fp: &str, scope: &ScopeReport) -> CriterionStatus {
        let runs: HashMap<_, _> = runs.iter().cloned().collect();
        let manual: HashMap<_, _> = manual.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
        let revs: HashMap<String, String> = revs.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        evaluate(c, &EvalContext { runs: &runs, check_revs: &revs, manual: &manual, fingerprint: fp, scope })
    }

    #[test]
    fn a_pass_does_not_carry_over_to_a_check_whose_command_was_changed() {
        let c = crit(&["t"]);
        let runs = [run("t", RunState::Passed, "f")]; // ran under revision "r1"
        assert!(matches!(eval_with(&c, &runs, &[("t", "r1")], &[], "f", &clean()), VerifiedByChecks { .. }));
        let s = eval_with(&c, &runs, &[("t", "r2-the-command-was-edited-to-exit-0")], &[], "f", &clean());
        assert!(matches!(s, Inconclusive { ref reason } if reason.contains("changed after it ran")), "{s:?}");
        let failed = [run("t", RunState::Failed, "f")];
        assert!(matches!(eval_with(&c, &failed, &[("t", "r2")], &[], "f", &clean()), Inconclusive { .. }), "a failure under an old definition is not a failure of the new one either");
    }

    #[test]
    fn a_check_that_modified_the_worktree_is_never_evidence_even_if_files_return_to_the_same_fingerprint() {
        let c = crit(&["t"]);
        let mut r = run("t", RunState::Passed, "f");
        r.1.mutated = true;
        let s = eval_with(&c, &[r], &[("t", "r1")], &[], "f", &clean());
        assert!(matches!(s, Inconclusive { ref reason } if reason.contains("modified the worktree")), "{s:?}");
    }

    #[test]
    fn a_manual_verdict_does_not_survive_an_edit_to_the_criterion_it_judged() {
        let mut c = crit(&[]);
        c.manual = true;
        let verdict = ManualVerdict { pass: true, note: None, fingerprint: "f".into(), criterion_rev: c.revision() };
        assert!(matches!(eval_with(&c, &[], &[], &[("c1", verdict.clone())], "f", &clean()), VerifiedManually { .. }));
        let mut edited = c.clone();
        edited.text = "tokens never expire".into();
        let s = eval_with(&edited, &[], &[], &[("c1", verdict)], "f", &clean());
        assert!(matches!(s, RequiresManualReview { ref reason } if reason.contains("edited after")), "{s:?}");
    }

    #[test]
    fn a_scope_rule_cannot_hide_linked_checks_or_a_required_human_verdict() {
        let mut c = crit(&["t"]);
        c.scope_rule = true;
        // scope is clean but the linked check never ran: not verified
        assert!(matches!(eval_with(&c, &[], &[("t", "r1")], &[], "f", &clean()), CheckPending { .. }));
        // scope is clean and the check passed: verified by the check
        assert!(matches!(eval_with(&c, &[run("t", RunState::Passed, "f")], &[("t", "r1")], &[], "f", &clean()), VerifiedByChecks { .. }));
        // scope violated: fails regardless of a passing check
        let bad = ScopeReport { outside_in_scope: vec!["x".into()], in_out_of_scope: vec![] };
        assert!(matches!(eval_with(&c, &[run("t", RunState::Passed, "f")], &[("t", "r1")], &[], "f", &bad), Failed { .. }));
        // scope + manual: still needs the human
        let mut m = crit(&[]);
        m.scope_rule = true;
        m.manual = true;
        assert!(matches!(eval_with(&m, &[], &[], &[], "f", &clean()), RequiresManualReview { .. }));
    }

    #[test]
    fn scope_globs_are_case_insensitive_on_windows() {
        let c = Contract { out_of_scope: vec!["Secrets/**".into()], in_scope: vec!["SRC/**".into()], ..Default::default() };
        let r = check_scope(&c, &["secrets/key.txt".into(), "src/a.rs".into()]).unwrap();
        if cfg!(windows) {
            assert_eq!(r.in_out_of_scope, vec!["secrets/key.txt"], "{r:?}");
            assert_eq!(r.outside_in_scope, vec!["secrets/key.txt"], "src/a.rs matches SRC/** on Windows");
        } else {
            assert!(r.in_out_of_scope.is_empty());
        }
    }

    #[test]
    fn revisions_change_with_meaning_and_not_with_noise() {
        let a = CheckSpec { id: "t".into(), name: "Tests".into(), command: "npm test".into(), kind: CheckKind::Test, timeout_secs: 60, required: true };
        let mut renamed = a.clone();
        renamed.name = "Unit tests".into();
        renamed.required = false;
        assert_eq!(a.revision(), renamed.revision(), "a display name or the required flag does not change what the check does");
        for edit in [|c: &mut CheckSpec| c.command = "npm test -- --passWithNoTests".into(), |c: &mut CheckSpec| c.timeout_secs = 61, |c: &mut CheckSpec| c.kind = CheckKind::Lint] {
            let mut b = a.clone();
            edit(&mut b);
            assert_ne!(a.revision(), b.revision());
        }
        let k1 = Criterion { id: "c".into(), text: "x".into(), verified_by: vec!["b".into(), "a".into()], manual: false, scope_rule: false };
        let mut k2 = k1.clone();
        k2.verified_by = vec!["a".into(), "b".into()];
        assert_eq!(k1.revision(), k2.revision(), "link order is not meaning");
        let ct = Contract { criteria: vec![k1.clone()], checks: vec![a.clone()], ..Default::default() };
        let mut ct2 = ct.clone();
        ct2.criteria[0].text = "y".into();
        assert_ne!(ct.revision(), ct2.revision());
    }

    #[test]
    fn blocking_checks_require_current_definition_and_no_mutation() {
        let mut ct = Contract::default();
        let spec = CheckSpec { id: "t".into(), name: "Tests".into(), command: "x".into(), kind: CheckKind::Test, timeout_secs: 10, required: true };
        ct.checks.push(spec.clone());
        let mut r = RunEvidence { check_id: "t".into(), run_id: "r".into(), state: RunState::Passed, fingerprint: "f".into(), spec_rev: spec.revision(), mutated: false };
        let ok: HashMap<_, _> = [("t".to_string(), r.clone())].into();
        assert!(blocking_checks(&ct, &ok, "f").is_empty());
        r.mutated = true;
        assert_eq!(blocking_checks(&ct, &[("t".to_string(), r.clone())].into(), "f"), vec!["Tests"]);
        r.mutated = false;
        r.spec_rev = "old".into();
        assert_eq!(blocking_checks(&ct, &[("t".to_string(), r)].into(), "f"), vec!["Tests"]);
    }
}
