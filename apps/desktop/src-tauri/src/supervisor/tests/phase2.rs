//! Phase 2 end-to-end: contract -> real check execution -> evidence -> review -> safe merge -> Proof Pack.
//! Real engine process, real git, real check runner; the agent is the scripted test agent.

use super::*;
use crate::contract::{CheckKind, CheckSpec, Contract, Criterion, CriterionStatus};
use crate::supervisor::{ReviewDecision, Seen};

/// What a reviewer's screen would hold right now: the current fingerprint and contract revision.
fn seen(f: &Fixture, id: &str) -> Seen {
    let r = f.core.verification_report(id).unwrap();
    Seen { fingerprint: r.fingerprint, contract_sha256: r.contract_sha256 }
}

fn approve(f: &Fixture, id: &str, note: Option<&str>, override_reason: Option<&str>) -> Result<Mission> {
    f.core.review(id, ReviewDecision::Approve, note, override_reason, Some(&seen(f, id)))
}

fn verdict(f: &Fixture, id: &str, criterion: &str, pass: bool, note: Option<&str>) -> Result<()> {
    f.core.set_manual_verdict(id, criterion, pass, note, &seen(f, id).fingerprint)
}

/// Passes only if the agent's file exists: genuine evidence about the worktree, not a constant.
const FILE_EXISTS: &str = "node -e \"process.exit(require('fs').existsSync('FORGE_TEST.md')?0:1)\"";
const ALWAYS_FAILS: &str = "node -e \"process.exit(1)\"";

fn contract(required: bool) -> Contract {
    Contract {
        version: 1,
        in_scope: vec![],
        out_of_scope: vec![],
        criteria: vec![
            Criterion { id: "file".into(), text: "The note file exists".into(), verified_by: vec!["exists".into()], manual: false, scope_rule: false },
            Criterion { id: "clean".into(), text: "No unrelated modifications".into(), verified_by: vec![], manual: false, scope_rule: true },
        ],
        checks: vec![
            CheckSpec { id: "exists".into(), name: "Note exists".into(), command: FILE_EXISTS.into(), kind: CheckKind::Test, timeout_secs: 60, required, adapter: None },
            CheckSpec { id: "broken".into(), name: "Always fails".into(), command: ALWAYS_FAILS.into(), kind: CheckKind::Custom, timeout_secs: 60, required: false, adapter: None },
        ],
        security_constraints: vec![],
        review_expectations: None,
    }
}

fn mission_in_review(f: &Fixture, objective: &str) -> Mission {
    let m = new_mission(f, objective, PermissionProfile::AcceptEdits);
    f.core.start_mission(&m.id, None).unwrap();
    wait_until("review", || state(f, &m.id) == "awaiting_review");
    wait_until("finalised", || f.core.live_session_count() == 0);
    m
}

fn run_check(f: &Fixture, mid: &str, check: &str) -> crate::store_review::VerificationRun {
    let started = f.core.start_check(mid, check).unwrap();
    assert_eq!(started.state, crate::contract::RunState::Running);
    wait_until("check finished", || f.core.store.get_run(&started.id).unwrap().state != crate::contract::RunState::Running);
    wait_until("check slot released", || f.core.start_check(mid, "nonexistent").map(|_| true).unwrap_or_else(|e| !e.to_string().contains("already running")));
    f.core.store.get_run(&started.id).unwrap()
}

fn crit_status<'a>(r: &'a crate::proof::VerificationReport, id: &str) -> &'a CriterionStatus {
    &r.criteria.iter().find(|c| c.id == id).unwrap().status
}

#[test]
fn full_lifecycle_contract_check_review_merge_proof() {
    let f = fixture();
    let m = mission_in_review(&f, "make a note");
    f.core.set_contract(&m.id, contract(true)).unwrap();

    // Before any check ran: the linked criterion is pending, nothing is "assumed fine".
    let r0 = f.core.verification_report(&m.id).unwrap();
    assert!(matches!(crit_status(&r0, "file"), CriterionStatus::CheckPending { .. }), "{:?}", crit_status(&r0, "file"));
    assert!(matches!(crit_status(&r0, "clean"), CriterionStatus::VerifiedByScopeCheck));
    assert_eq!(r0.blocking_checks, vec!["Note exists"]);
    assert!(r0.needs_override());
    let err = approve(&f, &m.id, None, None).unwrap_err().to_string();
    assert!(err.contains("not established") && err.contains("recorded reason"), "{err}");

    // Run the real check against the real worktree.
    let run = run_check(&f, &m.id, "exists");
    assert_eq!(run.state, crate::contract::RunState::Passed, "{run:?}");
    assert!(!run.mutated);
    let r1 = f.core.verification_report(&m.id).unwrap();
    assert!(matches!(crit_status(&r1, "file"), CriterionStatus::VerifiedByChecks { .. }));
    assert!(r1.unresolved.is_empty(), "{:?}", r1.unresolved);
    assert!(r1.checks[0].fresh);

    // Approve, merge.
    let approved = approve(&f, &m.id, Some("looks right"), None).unwrap();
    assert_eq!(approved.state, "approved");
    let out = f.core.merge_mission(&m.id, false).unwrap();
    assert_eq!(state(&f, &m.id), "merged");
    assert!(f.repo.path().join("FORGE_TEST.md").exists(), "the change is now in the user's checkout");
    assert!(out.cleaned_up, "{:?}", out.notes);
    let wt = PathBuf::from(f.core.store.get_mission(&m.id).unwrap().worktree_path.unwrap());
    assert!(!wt.exists(), "merged worktree removed");
    assert!(!git::status(f.repo.path()).unwrap().dirty, "nothing left uncommitted in the main checkout");

    // The sealed Proof Pack tells the truth, including the evidence.
    let packs = f.core.store.proof_packs(&m.id).unwrap();
    assert_eq!(packs.len(), 1);
    let md = &packs[0].markdown;
    assert!(md.contains("Verified by checks") && md.contains("Note exists") && md.contains("FORGE_TEST.md"), "{md}");
    assert!(md.contains("approve"), "{md}");
    assert_eq!(packs[0].content["mission"]["state"], "merged");
    let diff_path = packs[0].diff_path.clone().unwrap();
    assert!(std::fs::read_to_string(&diff_path).unwrap().contains("FORGE_TEST.md"));
    assert_eq!(f.core.store.audit_count("mission.merge").unwrap(), 1);
}

#[test]
fn evidence_goes_stale_when_the_code_changes_after_the_check() {
    let f = fixture();
    let m = mission_in_review(&f, "make a note");
    f.core.set_contract(&m.id, contract(true)).unwrap();
    assert_eq!(run_check(&f, &m.id, "exists").state, crate::contract::RunState::Passed);
    assert!(f.core.verification_report(&m.id).unwrap().unresolved.is_empty());

    let wt = PathBuf::from(f.core.store.get_mission(&m.id).unwrap().worktree_path.unwrap());
    std::fs::write(wt.join("sneaky.txt"), "added after the check\n").unwrap();

    let r = f.core.verification_report(&m.id).unwrap();
    assert!(matches!(crit_status(&r, "file"), CriterionStatus::Inconclusive { .. }), "{:?}", crit_status(&r, "file"));
    assert_eq!(r.blocking_checks, vec!["Note exists"], "a pass on older changes must not count");
    assert!(!r.checks[0].fresh);
    assert!(approve(&f, &m.id, None, None).is_err());
}

#[test]
fn a_failing_check_is_recorded_as_failed_and_can_only_be_approved_over_with_a_reason() {
    let f = fixture();
    let m = mission_in_review(&f, "make a note");
    let mut c = contract(false);
    c.criteria.push(Criterion { id: "broken".into(), text: "The impossible holds".into(), verified_by: vec!["broken".into()], manual: false, scope_rule: false });
    f.core.set_contract(&m.id, c).unwrap();
    run_check(&f, &m.id, "exists");
    let r = run_check(&f, &m.id, "broken");
    assert_eq!((r.state, r.exit_code), (crate::contract::RunState::Failed, Some(1)));

    let rep = f.core.verification_report(&m.id).unwrap();
    assert!(matches!(crit_status(&rep, "broken"), CriterionStatus::Failed { .. }));
    assert!(rep.unresolved.iter().any(|u| u.contains("FAILED")));
    assert!(approve(&f, &m.id, None, None).is_err());
    let ok = approve(&f, &m.id, None, Some("known flaky infra check; reviewed by hand")).unwrap();
    assert_eq!(ok.state, "approved");
    let d = f.core.store.review_decisions(&m.id).unwrap();
    assert_eq!(d[0].override_reason.as_deref(), Some("known flaky infra check; reviewed by hand"), "the override is on the record");
}

#[test]
fn manual_verdicts_count_only_for_the_changes_they_were_given_on() {
    let f = fixture();
    let m = mission_in_review(&f, "make a note");
    let mut c = contract(false);
    c.criteria = vec![Criterion { id: "human".into(), text: "Reads well".into(), verified_by: vec![], manual: true, scope_rule: false }];
    f.core.set_contract(&m.id, c).unwrap();
    assert!(matches!(crit_status(&f.core.verification_report(&m.id).unwrap(), "human"), CriterionStatus::RequiresManualReview { .. }));
    verdict(&f, &m.id, "human", true, Some("read it, fine")).unwrap();
    assert!(matches!(crit_status(&f.core.verification_report(&m.id).unwrap(), "human"), CriterionStatus::VerifiedManually { .. }));
    let wt = PathBuf::from(f.core.store.get_mission(&m.id).unwrap().worktree_path.unwrap());
    std::fs::write(wt.join("more.txt"), "x").unwrap();
    assert!(matches!(crit_status(&f.core.verification_report(&m.id).unwrap(), "human"), CriterionStatus::RequiresManualReview { .. }), "a verdict is stale once the changes move");
    assert!(verdict(&f, &m.id, "no-such-criterion", true, None).is_err());
}

#[test]
fn changes_after_approval_must_be_reviewed_again_before_merging() {
    let f = fixture();
    let m = mission_in_review(&f, "make a note");
    approve(&f, &m.id, None, None).unwrap();
    let wt = PathBuf::from(f.core.store.get_mission(&m.id).unwrap().worktree_path.unwrap());
    std::fs::write(wt.join("unreviewed.txt"), "slipped in after approval\n").unwrap();
    let err = f.core.merge_mission(&m.id, false).unwrap_err().to_string();
    assert!(err.contains("differ from what was approved"), "{err}");
    assert_eq!(state(&f, &m.id), "awaiting_review", "sent back for review");
    assert!(!f.repo.path().join("unreviewed.txt").exists() && !f.repo.path().join("FORGE_TEST.md").exists(), "nothing reached the main checkout");
}

#[test]
fn merge_refuses_a_dirty_main_checkout_and_leaves_everything_alone() {
    let f = fixture();
    let m = mission_in_review(&f, "make a note");
    approve(&f, &m.id, None, None).unwrap();
    std::fs::write(f.repo.path().join("my-wip.txt"), "uncommitted user work\n").unwrap();
    let err = f.core.merge_mission(&m.id, false).unwrap_err().to_string();
    assert!(err.contains("uncommitted"), "{err}");
    assert_eq!(state(&f, &m.id), "approved");
    assert_eq!(std::fs::read_to_string(f.repo.path().join("my-wip.txt")).unwrap(), "uncommitted user work\n");
    assert!(!f.repo.path().join("FORGE_TEST.md").exists());
    // once the user cleans up, the same approval merges fine
    std::fs::remove_file(f.repo.path().join("my-wip.txt")).unwrap();
    f.core.merge_mission(&m.id, true).unwrap();
    assert_eq!(state(&f, &m.id), "merged");
}

#[test]
fn merge_conflicts_are_reported_before_touching_the_checkout() {
    let f = fixture();
    let m = mission_in_review(&f, "make a note");
    // main moves on with a conflicting change to the same new file
    std::fs::write(f.repo.path().join("FORGE_TEST.md"), "someone else wrote this first\n").unwrap();
    git::run(f.repo.path(), &["add", "-A"]).unwrap();
    git::run(f.repo.path(), &["commit", "-q", "-m", "conflicting change on main"]).unwrap();
    let head = git::head_commit(f.repo.path()).unwrap();
    approve(&f, &m.id, None, None).unwrap();
    let err = f.core.merge_mission(&m.id, false).unwrap_err().to_string();
    assert!(err.contains("conflict") && err.contains("FORGE_TEST.md"), "{err}");
    assert_eq!(git::head_commit(f.repo.path()).unwrap(), head);
    assert!(!git::status(f.repo.path()).unwrap().dirty);
    assert_eq!(state(&f, &m.id), "approved", "still approved; the user decides how to resolve");
}

#[test]
fn request_changes_sends_a_follow_up_turn_and_returns_to_review() {
    let f = fixture();
    let m = mission_in_review(&f, "make a note");
    let back = f.core.review(&m.id, ReviewDecision::RequestChanges, Some("also add a second note please"), None, None).unwrap();
    assert!(["preparing", "running", "awaiting_approval", "awaiting_review"].contains(&back.state.as_str()));
    wait_until("second turn finished", || f.core.store.sessions_for_mission(&m.id).unwrap().len() == 2 && state(&f, &m.id) == "awaiting_review" && f.core.live_session_count() == 0);
    assert_eq!(f.core.store.review_decisions(&m.id).unwrap()[0].decision, "request_changes");
    assert!(f.core.review(&m.id, ReviewDecision::RequestChanges, None, None, None).is_err(), "a request needs text for the agent");
}

#[test]
fn rejecting_is_final_and_keeps_the_record() {
    let f = fixture();
    let m = mission_in_review(&f, "make a note");
    f.core.review(&m.id, ReviewDecision::Reject, Some("wrong approach"), None, None).unwrap();
    assert_eq!(state(&f, &m.id), "rejected");
    assert!(f.core.merge_mission(&m.id, false).is_err());
    assert!(approve(&f, &m.id, None, None).is_err());
    assert_eq!(f.core.store.review_decisions(&m.id).unwrap()[0].note.as_deref(), Some("wrong approach"));
}

#[test]
fn checks_refuse_to_run_while_an_agent_is_working_and_agents_refuse_while_a_check_runs() {
    let f = fixture();
    let m = new_mission(&f, "FAKE:hang", PermissionProfile::Default);
    f.core.start_mission(&m.id, None).unwrap();
    wait_until("running", || state(&f, &m.id) == "running");
    f.core.set_contract(&m.id, contract(false)).unwrap();
    let err = f.core.start_check(&m.id, "exists").unwrap_err().to_string();
    assert!(err.contains("stop the agent first"), "{err}");
    f.core.stop_mission(&m.id).unwrap();
    wait_until("stopped", || f.core.live_session_count() == 0);

    // Now the reverse: a slow check blocks the agent from restarting on the same files.
    let mut c = contract(false);
    c.checks.push(CheckSpec { id: "slow".into(), name: "Slow".into(), command: "node -e \"setTimeout(()=>{},30000)\"".into(), kind: CheckKind::Custom, timeout_secs: 60, required: false, adapter: None });
    f.core.set_contract(&m.id, c).unwrap();
    let slow = f.core.start_check(&m.id, "slow").unwrap();
    let err = f.core.start_mission(&m.id, None).unwrap_err().to_string();
    assert!(err.contains("verification check is running"), "{err}");
    assert!(f.core.start_check(&m.id, "slow").unwrap_err().to_string().contains("already running"));
    f.core.cancel_check(&m.id).unwrap();
    wait_until("cancelled", || f.core.store.get_run(&slow.id).unwrap().state == crate::contract::RunState::Cancelled);
}

#[test]
fn a_check_that_modifies_the_worktree_is_flagged_not_trusted() {
    let f = fixture();
    let m = mission_in_review(&f, "make a note");
    let mut c = contract(true);
    c.checks[0].command = "node -e \"require('fs').writeFileSync('generated-by-check.txt','x');process.exit(0)\"".into();
    f.core.set_contract(&m.id, c).unwrap();
    let r = run_check(&f, &m.id, "exists");
    assert!(r.mutated, "{r:?}");
    let rep = f.core.verification_report(&m.id).unwrap();
    assert!(rep.unresolved.iter().any(|u| u.contains("modified files while running")), "{:?}", rep.unresolved);
    assert!(!rep.checks[0].fresh, "its result is about different changes than exist now");
}

#[test]
fn invalid_contracts_are_rejected_and_finished_missions_are_frozen() {
    let f = fixture();
    let m = mission_in_review(&f, "make a note");
    let mut bad = contract(false);
    bad.criteria[0].verified_by = vec!["no-such-check".into()];
    assert!(f.core.set_contract(&m.id, bad).is_err());
    f.core.review(&m.id, ReviewDecision::Reject, Some("no"), None, None).unwrap();
    assert!(f.core.set_contract(&m.id, contract(false)).unwrap_err().to_string().contains("can no longer be changed"));
}

#[test]
fn restart_recovery_resolves_checks_that_were_mid_flight() {
    let f = fixture();
    let m = new_mission(&f, "x", PermissionProfile::Default);
    let run = crate::store_review::VerificationRun {
        id: crate::store::new_id(), mission_id: m.id.clone(), check_id: "exists".into(), name: "n".into(), command: "c".into(), kind: "test".into(),
        state: crate::contract::RunState::Running, exit_code: None, started_at: crate::store::now(), ended_at: None, duration_ms: None,
        fingerprint: "f".into(), head_commit: None, mutated: false, output_tail: String::new(), output_truncated: false, log_path: None, spec_rev: String::new(), backend: "trusted_local".into(), evidence: None,
    };
    f.core.store.insert_run(&run).unwrap();
    f.core.recover().unwrap();
    assert_eq!(f.core.store.get_run(&run.id).unwrap().state, crate::contract::RunState::Error);
}

#[test]
fn a_rejected_missions_leftovers_are_cleaned_only_with_confirmation() {
    let f = fixture();
    let m = mission_in_review(&f, "make a note");
    let wt = PathBuf::from(f.core.store.get_mission(&m.id).unwrap().worktree_path.unwrap());
    assert!(f.core.cleanup_workspace(&m.id, false).unwrap_err().to_string().contains("only finished"), "not before it is finished");
    f.core.review(&m.id, ReviewDecision::Reject, Some("no"), None, None).unwrap();
    let err = f.core.cleanup_workspace(&m.id, false).unwrap_err().to_string();
    assert!(err.contains("permanently delete") && err.contains("uncommitted"), "{err}");
    assert!(wt.join("FORGE_TEST.md").exists(), "refusal leaves the work intact");
    f.core.cleanup_workspace(&m.id, true).unwrap();
    assert!(!wt.exists());
    assert_eq!(state(&f, &m.id), "rejected", "cleanup does not rewrite history");
}

/// Stage B: a project switched to the Linux-namespace backend runs its checks there; the run records where it ran, the Proof
/// Pack says so, and a check that tries to read a host secret cannot. Backends that cannot run are refused, not downgraded.
#[cfg(target_os = "linux")]
#[test]
fn checks_run_on_the_projects_execution_backend_and_record_it() {
    let f = fixture();
    assert_eq!(f.project.execution_backend, "trusted_local", "existing and new projects keep running as before");
    assert!(f.core.project_set_execution_backend(&f.project.id, "windows_sandbox").unwrap_err().to_string().contains("cannot be used"));
    assert!(f.core.project_set_execution_backend(&f.project.id, "docker").is_err());
    if crate::execution::availability(crate::execution::Backend::LinuxNamespaces).is_err() {
        assert!(std::env::var("FORGE_REQUIRE_SANDBOX_TESTS").as_deref() != Ok("1"), "sandbox required but unavailable");
        return;
    }
    let secret_dir = tempfile::tempdir().unwrap();
    let secret = secret_dir.path().join("credentials");
    std::fs::write(&secret, "TOP-SECRET-CREDENTIAL-99").unwrap();
    let m = mission_in_review(&f, "make a note");
    let mut contract = f.core.get_contract(&m.id).unwrap();
    contract.checks.push(crate::contract::CheckSpec { id: "steal".into(), name: "steal".into(), command: format!("cat '{}' || true; echo still-ran", secret.display()), kind: crate::contract::CheckKind::Test, required: false, timeout_secs: 30, adapter: None });
    f.core.set_contract(&m.id, contract).unwrap();
    f.core.project_set_execution_backend(&f.project.id, "linux_namespaces").unwrap();
    let run = run_check(&f, &m.id, "steal");
    assert_eq!(run.backend, "linux_namespaces");
    assert!(run.output_tail.contains("still-ran"), "{}", run.output_tail);
    assert!(!run.output_tail.contains("TOP-SECRET-CREDENTIAL-99"), "the check read a host secret: {}", run.output_tail);
    assert_eq!(f.core.store.audit_count("project.execution_backend").unwrap(), 1);
    // Switching back records trusted-local runs as such; nothing is hidden.
    f.core.project_set_execution_backend(&f.project.id, "trusted_local").unwrap();
    assert_eq!(run_check(&f, &m.id, "steal").backend, "trusted_local");
}
