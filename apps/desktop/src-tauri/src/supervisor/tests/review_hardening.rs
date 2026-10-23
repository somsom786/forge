//! Regression tests for the third independent review (verification, merge and review integrity). Each test names the
//! attack it closes.

use super::*;
use crate::contract::{CheckKind, CheckSpec, Contract, Criterion, CriterionStatus};
use crate::supervisor::{ReviewDecision, Seen};

const FILE_EXISTS: &str = "node -e \"process.exit(require('fs').existsSync('FORGE_TEST.md')?0:1)\"";

fn contract() -> Contract {
    Contract {
        version: 1,
        criteria: vec![Criterion { id: "file".into(), text: "The note file exists".into(), verified_by: vec!["exists".into()], manual: false, scope_rule: false }],
        checks: vec![CheckSpec { id: "exists".into(), name: "Note exists".into(), command: FILE_EXISTS.into(), kind: CheckKind::Test, timeout_secs: 60, required: true }],
        ..Default::default()
    }
}

fn in_review(f: &Fixture) -> Mission {
    let m = new_mission(f, "make a note", PermissionProfile::AcceptEdits);
    f.core.start_mission(&m.id, None).unwrap();
    wait_until("review", || state(f, &m.id) == "awaiting_review");
    wait_until("finalised", || f.core.live_session_count() == 0);
    m
}

fn seen(f: &Fixture, id: &str) -> Seen {
    let r = f.core.verification_report(id).unwrap();
    Seen { fingerprint: r.fingerprint, contract_sha256: r.contract_sha256 }
}

fn worktree(f: &Fixture, id: &str) -> PathBuf {
    PathBuf::from(f.core.store.get_mission(id).unwrap().worktree_path.unwrap())
}

fn run_check_to_end(f: &Fixture, id: &str) {
    let run = f.core.start_check(id, "exists").unwrap();
    wait_until("check done", || f.core.store.get_run(&run.id).unwrap().state != crate::contract::RunState::Running);
    wait_until("slot released", || !lock(&f.core.checks).contains_key(id));
}

#[test]
fn an_approval_names_what_the_reviewer_saw_and_nothing_else_is_accepted() {
    let f = fixture();
    let m = in_review(&f);
    f.core.set_contract(&m.id, contract()).unwrap();
    run_check_to_end(&f, &m.id);

    // The reviewer's screen shows state A...
    let looked_at = seen(&f, &m.id);
    // ...and then something changes the files behind their back (an editor, a script, an agent with shell access).
    std::fs::write(worktree(&f, &m.id).join("sneaky.txt"), "added after the reviewer looked\n").unwrap();

    let err = f.core.review(&m.id, ReviewDecision::Approve, None, Some("override anyway"), Some(&looked_at)).unwrap_err().to_string();
    assert!(err.contains("not the ones you reviewed"), "{err}");
    assert_eq!(state(&f, &m.id), "awaiting_review", "nothing was approved");
    assert!(f.core.store.review_decisions(&m.id).unwrap().is_empty(), "and nothing was recorded as approved");

    // An approval that names nothing is refused outright.
    let err = f.core.review(&m.id, ReviewDecision::Approve, None, Some("x"), None).unwrap_err().to_string();
    assert!(err.contains("must name"), "{err}");

    // Changing the success criteria after looking also invalidates the approval.
    let fresh = seen(&f, &m.id);
    let mut other = contract();
    other.criteria[0].text = "Something far easier to satisfy".into();
    f.core.set_contract(&m.id, other).unwrap();
    assert!(f.core.review(&m.id, ReviewDecision::Approve, None, Some("x"), Some(&fresh)).unwrap_err().to_string().contains("not the ones you reviewed"));

    // Judging the current state works.
    let now = seen(&f, &m.id);
    f.core.review(&m.id, ReviewDecision::Approve, None, Some("reviewed the new state"), Some(&now)).unwrap();
    assert_eq!(state(&f, &m.id), "approved");
}

#[test]
fn a_manual_verdict_is_refused_if_the_changes_moved_since_the_reviewer_looked() {
    let f = fixture();
    let m = in_review(&f);
    let mut c = contract();
    c.criteria[0] = Criterion { id: "human".into(), text: "Reads well".into(), verified_by: vec![], manual: true, scope_rule: false };
    c.checks.clear();
    f.core.set_contract(&m.id, c).unwrap();
    let looked_at = seen(&f, &m.id).fingerprint;
    std::fs::write(worktree(&f, &m.id).join("later.txt"), "x").unwrap();
    let err = f.core.set_manual_verdict(&m.id, "human", true, None, &looked_at).unwrap_err().to_string();
    assert!(err.contains("not the ones you were looking at"), "{err}");
    f.core.set_manual_verdict(&m.id, "human", true, None, &seen(&f, &m.id).fingerprint).unwrap();
}

#[test]
fn editing_a_checks_command_after_it_passed_voids_the_evidence() {
    // The attack: the agent (or anyone) rewrites the check to `exit 0` after a legitimate pass, keeping its id.
    let f = fixture();
    let m = in_review(&f);
    f.core.set_contract(&m.id, contract()).unwrap();
    run_check_to_end(&f, &m.id);
    let before = f.core.verification_report(&m.id).unwrap();
    assert!(before.unresolved.is_empty() && before.checks[0].fresh, "{:?}", before.unresolved);

    let mut weakened = contract();
    weakened.checks[0].command = "node -e \"process.exit(0)\"".into();
    f.core.set_contract(&m.id, weakened).unwrap();
    let after = f.core.verification_report(&m.id).unwrap();
    assert!(matches!(after.criteria[0].status, CriterionStatus::Inconclusive { .. }), "{:?}", after.criteria[0].status);
    assert_eq!(after.blocking_checks, vec!["Note exists"], "the old pass does not satisfy the new check");
    assert!(!after.checks[0].fresh);
    assert!(after.unresolved.iter().any(|u| u.contains("edited after its last run")), "{:?}", after.unresolved);
    assert!(f.core.review(&m.id, ReviewDecision::Approve, None, None, Some(&seen(&f, &m.id))).is_err(), "needs a recorded override now");
}

#[test]
fn changes_to_files_that_define_a_check_are_called_out() {
    let f = fixture();
    let m = in_review(&f);
    let mut c = contract();
    c.checks[0].command = "npm test".into();
    f.core.set_contract(&m.id, c).unwrap();
    std::fs::write(worktree(&f, &m.id).join("package.json"), "{\"scripts\":{\"test\":\"exit 0\"}}").unwrap();
    let r = f.core.verification_report(&m.id).unwrap();
    assert_eq!(r.checks[0].touched_inputs, vec!["package.json"]);
    assert!(r.unresolved.iter().any(|u| u.contains("define or configure it")), "{:?}", r.unresolved);
}

#[test]
fn a_force_staged_ignored_file_does_not_ride_along_into_the_merge() {
    let f = fixture();
    let m = in_review(&f);
    let wt = worktree(&f, &m.id);
    std::fs::write(wt.join(".gitignore"), "smuggled.txt\n").unwrap();
    std::fs::write(wt.join("smuggled.txt"), "never reviewed\n").unwrap();
    git::run(&wt, &["add", "-f", "smuggled.txt"]).unwrap(); // sits in the REAL index, invisible to the reviewed tree
    f.core.review(&m.id, ReviewDecision::Approve, None, None, Some(&seen(&f, &m.id))).unwrap();
    f.core.merge_mission(&m.id, false).unwrap();
    assert!(f.repo.path().join("FORGE_TEST.md").exists());
    assert!(!f.repo.path().join("smuggled.txt").exists(), "only the reviewed tree is merged");
    let tracked = git::run(f.repo.path(), &["ls-files"]).unwrap();
    assert!(!tracked.contains("smuggled.txt"), "{tracked}");
}

#[test]
fn merging_never_overwrites_an_ignored_local_file_in_the_main_checkout() {
    let f = fixture();
    let m = in_review(&f); // the mission adds FORGE_TEST.md; at review time nothing ignores it
    // Afterwards the user starts ignoring that name in main and keeps their own local file under it.
    std::fs::write(f.repo.path().join(".gitignore"), "FORGE_TEST.md\n").unwrap();
    git::run(f.repo.path(), &["add", "-A"]).unwrap();
    git::run(f.repo.path(), &["commit", "-q", "-m", "ignore the note file locally"]).unwrap();
    std::fs::write(f.repo.path().join("FORGE_TEST.md"), "MY OWN LOCAL NOTES\n").unwrap();
    f.core.review(&m.id, ReviewDecision::Approve, None, None, Some(&seen(&f, &m.id))).unwrap();
    let err = f.core.merge_mission(&m.id, false).map(|_| ()).unwrap_err().to_string();
    assert!(err.contains("not tracked by git") || err.contains("overwrite"), "{err}");
    assert_eq!(std::fs::read_to_string(f.repo.path().join("FORGE_TEST.md")).unwrap(), "MY OWN LOCAL NOTES\n");
    assert_eq!(state(&f, &m.id), "approved");
}

#[test]
fn cleanup_after_a_merge_protects_commits_added_to_the_branch_afterwards() {
    let f = fixture();
    let m = in_review(&f);
    f.core.review(&m.id, ReviewDecision::Approve, None, None, Some(&seen(&f, &m.id))).unwrap();
    // main moves on so a squash merge keeps the mission branch around (its tree no longer equals main's)
    std::fs::write(f.repo.path().join("elsewhere.txt"), "x\n").unwrap();
    git::run(f.repo.path(), &["add", "-A"]).unwrap();
    git::run(f.repo.path(), &["commit", "-q", "-m", "unrelated work on main"]).unwrap();
    let out = f.core.merge_mission(&m.id, true).unwrap();
    assert!(!out.cleaned_up, "{:?}", out.notes);
    assert_eq!(state(&f, &m.id), "merged");

    // Later, someone commits more work on the leftover branch.
    let branch = f.core.store.get_mission(&m.id).unwrap().branch.unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let wt = tmp.path().join("later");
    git::run(f.repo.path(), &["worktree", "add", wt.to_str().unwrap(), &branch]).unwrap();
    std::fs::write(wt.join("later-work.txt"), "valuable\n").unwrap();
    git::run(&wt, &["add", "-A"]).unwrap();
    git::run(&wt, &["-c", "user.name=t", "-c", "user.email=t@e", "commit", "-q", "-m", "work after the merge"]).unwrap();
    git::run(f.repo.path(), &["worktree", "remove", "--force", wt.to_str().unwrap()]).unwrap();

    let err = f.core.cleanup_workspace(&m.id, false).unwrap_err().to_string();
    assert!(err.contains("unmerged commit"), "a merged mission is not exempt: {err}");
    assert!(git::run(f.repo.path(), &["rev-parse", "--verify", &format!("refs/heads/{branch}")]).is_ok(), "branch survives the refusal");
    f.core.cleanup_workspace(&m.id, true).unwrap();
}

#[test]
fn a_check_cannot_start_while_another_operation_owns_the_mission() {
    let f = fixture();
    let m = in_review(&f);
    f.core.set_contract(&m.id, contract()).unwrap();
    let guard = f.core.reserve(&m.id, false).unwrap(); // e.g. a merge or discard is in flight
    let err = f.core.start_check(&m.id, "exists").unwrap_err().to_string();
    assert!(err.contains("another operation owns this mission") || err.contains("stop the agent first"), "{err}");
    drop(guard);
    run_check_to_end(&f, &m.id);
}

#[test]
fn a_hostile_git_hook_never_runs_during_review_or_merge() {
    let f = fixture();
    let marker = f.repo.path().join("hook-ran.txt");
    let hooks = f.repo.path().join(".git").join("hooks");
    std::fs::create_dir_all(&hooks).unwrap();
    for name in ["pre-commit", "post-merge", "post-checkout", "reference-transaction", "prepare-commit-msg", "commit-msg", "pre-merge-commit"] {
        std::fs::write(hooks.join(name), format!("#!/bin/sh\necho ran >> \"{}\"\n", marker.to_string_lossy().replace('\\', "/"))).unwrap();
    }
    let m = in_review(&f);
    f.core.set_contract(&m.id, contract()).unwrap();
    run_check_to_end(&f, &m.id);
    f.core.generate_proof_pack(&m.id).unwrap();
    f.core.review(&m.id, ReviewDecision::Approve, None, None, Some(&seen(&f, &m.id))).unwrap();
    f.core.merge_mission(&m.id, false).unwrap();
    assert!(!marker.exists(), "a repository hook ran as a side effect of FORGE's own git operations");
}

#[test]
fn a_repository_configuring_an_external_git_filter_is_refused_before_anything_executes() {
    let f = fixture();
    let m = in_review(&f);
    git::run(f.repo.path(), &["config", "filter.evil.clean", "powershell -NoProfile -Command exit"]).unwrap();
    let err = f.core.verification_report(&m.id).unwrap_err().to_string();
    assert!(err.contains("external git programs") && err.contains("filter.evil.clean"), "{err}");
}
