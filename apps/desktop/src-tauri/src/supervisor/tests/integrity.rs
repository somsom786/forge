//! Data-integrity regressions from the independent reliability review: interrupted merges, half-created worktrees, deleting
//! the wrong directory, ignored files, recovery limits, approvals that outlive their criteria.

use super::*;
use crate::contract::Contract;
use crate::supervisor::{ReviewDecision, Seen};

fn seen(f: &Fixture, id: &str) -> Seen {
    let r = f.core.verification_report(id).unwrap();
    Seen { fingerprint: r.fingerprint, contract_sha256: r.contract_sha256 }
}

fn approved_mission(f: &Fixture) -> Mission {
    let m = new_mission(f, "make a scripted note", PermissionProfile::AcceptEdits);
    f.core.start_mission(&m.id, None).unwrap();
    wait_until("review", || state(f, &m.id) == "awaiting_review");
    wait_until("finalised", || f.core.live_session_count() == 0);
    f.core.review(&m.id, ReviewDecision::Approve, None, None, Some(&seen(f, &m.id))).unwrap();
    m
}

/// Simulates FORGE crashing right after git merged (squash) and before the database recorded it: the intent is recorded,
/// git runs, nothing else happens. Before the fix the mission stayed "approved" forever and every retry failed.
#[test]
fn a_squash_merge_that_landed_before_a_crash_is_completed_not_retried() {
    let f = fixture();
    let m = approved_mission(&f);
    let mm = f.core.store.get_mission(&m.id).unwrap();
    let (wt, base, br) = (PathBuf::from(mm.worktree_path.unwrap()), mm.base_commit.unwrap(), mm.branch.unwrap());
    let (_, tree) = git::fingerprint_and_tree(&wt, &base).unwrap();
    let (b, _) = crate::merge::commit_reviewed_tree(f.repo.path(), &wt, &br, &tree, "x").unwrap();
    let head = crate::merge::oid(f.repo.path(), "HEAD").unwrap();
    let p = crate::merge::predict(f.repo.path(), &head, &b).unwrap().tree.unwrap();
    f.core.store.begin_merge(&crate::store_review::MergeRecord { mission_id: m.id.clone(), target_oid: head.clone(), branch_oid: b.clone(), predicted_tree: p.clone(), squash: true, message: "x".into(), commit_oid: None, finished_at: None }).unwrap();
    crate::merge::merge(f.repo.path(), &b, "x", true, &head, &p).unwrap(); // ...and FORGE "crashes" here
    assert_eq!(state(&f, &m.id), "approved");
    f.core.recover().unwrap();
    assert_eq!(state(&f, &m.id), "merged", "recovery finishes the merge from the repository");
    let rec = f.core.store.merge_record(&m.id).unwrap().unwrap();
    assert!(rec.finished_at.is_some());
    // The squash-merged branch commit is not "unmerged work" when cleaning up afterwards.
    assert_eq!(f.core.worktree_safety(&m.id).unwrap().unmerged_commits, 0);
}

/// Same, but the crash happened between `git merge --squash` and `git commit`: FORGE finishes its own commit on retry.
#[test]
fn a_squash_interrupted_before_its_commit_is_finished_on_retry() {
    let f = fixture();
    let m = approved_mission(&f);
    let mm = f.core.store.get_mission(&m.id).unwrap();
    let (wt, base, br) = (PathBuf::from(mm.worktree_path.unwrap()), mm.base_commit.unwrap(), mm.branch.unwrap());
    let (_, tree) = git::fingerprint_and_tree(&wt, &base).unwrap();
    let (b, _) = crate::merge::commit_reviewed_tree(f.repo.path(), &wt, &br, &tree, "x").unwrap();
    let head = crate::merge::oid(f.repo.path(), "HEAD").unwrap();
    let p = crate::merge::predict(f.repo.path(), &head, &b).unwrap().tree.unwrap();
    f.core.store.begin_merge(&crate::store_review::MergeRecord { mission_id: m.id.clone(), target_oid: head.clone(), branch_oid: b.clone(), predicted_tree: p, squash: true, message: "x".into(), commit_oid: None, finished_at: None }).unwrap();
    git::run(f.repo.path(), &["merge", "--squash", &b]).unwrap(); // crash before the commit
    let out = f.core.merge_mission(&m.id, true).unwrap();
    assert_eq!(state(&f, &m.id), "merged");
    assert!(out.notes.iter().any(|n| n.contains("already reached git")), "{:?}", out.notes);
    assert!(!git::status(f.repo.path()).unwrap().dirty, "the staged squash was committed, not left behind");
}

/// A crash after `git worktree add` but before the database write used to brick the mission ("path already exists").
#[test]
fn a_worktree_created_before_a_crash_is_adopted_on_start() {
    let f = fixture();
    let m = new_mission(&f, "make a scripted note", PermissionProfile::AcceptEdits);
    let (slug, s8, p8) = (git::slugify(&m.title, 24), &m.id[m.id.len() - 8..], &f.project.id[f.project.id.len() - 8..]);
    let wt = f.core.cfg.home.join("worktrees").join(p8).join(format!("{slug}-{s8}"));
    git::worktree_add(f.repo.path(), &wt, &format!("forge/{slug}-{s8}"), &git::head_commit(f.repo.path()).unwrap()).unwrap();
    f.core.start_mission(&m.id, None).unwrap();
    wait_until("review", || state(&f, &m.id) == "awaiting_review");
    assert!(f.core.store.audit_count("worktree.adopt").unwrap() == 1);
}

/// Mission A's worktree path replaced by a link to mission B's worktree: discarding A must not delete B.
#[cfg(unix)]
#[test]
fn discarding_through_a_link_to_another_worktree_is_refused() {
    let f = fixture();
    let a = new_mission(&f, "first scripted note", PermissionProfile::Default);
    let b = new_mission(&f, "second scripted note", PermissionProfile::Default);
    for m in [&a, &b] {
        f.core.start_mission(&m.id, None).unwrap();
        wait_until("asking", || !pending(&f, &m.id).is_empty());
        f.core.stop_mission(&m.id).unwrap();
    }
    wait_until("stopped", || f.core.live_session_count() == 0);
    let wa = PathBuf::from(f.core.store.get_mission(&a.id).unwrap().worktree_path.unwrap());
    let wb = PathBuf::from(f.core.store.get_mission(&b.id).unwrap().worktree_path.unwrap());
    std::fs::write(wb.join("WIP.txt"), "b's unsaved work").unwrap();
    std::fs::remove_dir_all(&wa).unwrap();
    std::os::unix::fs::symlink(&wb, &wa).unwrap();
    assert!(f.core.discard_mission(&a.id, true).is_err());
    assert_eq!(std::fs::read_to_string(wb.join("WIP.txt")).unwrap(), "b's unsaved work");
}

/// `git worktree remove` deletes ignored files without a word; they now count as potential loss.
#[test]
fn ignored_files_need_confirmation_before_a_worktree_is_removed() {
    let f = fixture();
    let m = new_mission(&f, "make a scripted note", PermissionProfile::Default);
    f.core.start_mission(&m.id, None).unwrap();
    wait_until("asking", || !pending(&f, &m.id).is_empty());
    f.core.stop_mission(&m.id).unwrap();
    wait_until("stopped", || f.core.live_session_count() == 0);
    let wt = PathBuf::from(f.core.store.get_mission(&m.id).unwrap().worktree_path.unwrap());
    // info/exclude lives in the common git dir, so it applies to the mission worktree too.
    std::fs::write(f.repo.path().join(".git/info/exclude"), "local.env\n").unwrap();
    std::fs::write(wt.join("local.env"), "API=1").unwrap();
    let err = f.core.discard_mission(&m.id, false).unwrap_err().to_string();
    assert!(err.contains("ignored"), "{err}");
    assert!(wt.join("local.env").exists());
    f.core.discard_mission(&m.id, true).unwrap();
}

#[test]
fn recovery_finds_active_missions_beyond_any_listing_limit() {
    let f = fixture();
    let old = new_mission(&f, "old", PermissionProfile::Default);
    f.core.store.transition(&old.id, MissionState::Preparing, "test").unwrap();
    f.core.store.c().execute("UPDATE mission SET updated_at='2000-01-01T00:00:00Z' WHERE id=?1", [&old.id]).unwrap();
    for i in 0..510 {
        new_mission(&f, &format!("filler {i}"), PermissionProfile::Default);
    }
    f.core.recover().unwrap();
    assert_eq!(state(&f, &old.id), "interrupted");
}

#[test]
fn changing_the_success_criteria_after_approval_voids_the_approval() {
    let f = fixture();
    let m = approved_mission(&f);
    assert_eq!(state(&f, &m.id), "approved");
    f.core.set_contract(&m.id, Contract { version: 1, ..Default::default() }).unwrap();
    assert_eq!(state(&f, &m.id), "awaiting_review");
}

#[test]
fn a_finished_mission_cannot_be_discarded_so_nothing_is_deleted_first() {
    let f = fixture();
    let m = new_mission(&f, "make a scripted note", PermissionProfile::AcceptEdits);
    f.core.start_mission(&m.id, None).unwrap();
    wait_until("review", || state(&f, &m.id) == "awaiting_review");
    wait_until("finalised", || f.core.live_session_count() == 0);
    f.core.review(&m.id, ReviewDecision::Reject, None, None, None).unwrap();
    let wt = PathBuf::from(f.core.store.get_mission(&m.id).unwrap().worktree_path.unwrap());
    assert!(f.core.discard_mission(&m.id, true).is_err());
    assert!(wt.exists(), "nothing was deleted before the refusal");
}
