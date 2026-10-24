//! Storage clean-up: only finished missions past the chosen age lose their raw logs and transcript; everything that is
//! a record stays; nothing is deleted that the preview did not list.

use super::phase2::{contract, mission_in_review, run_check};
use super::*;

fn age(f: &Fixture, mission_id: &str) {
    f.core.store.c().execute("UPDATE mission SET updated_at='2020-01-01T00:00:00.000Z' WHERE id=?1", [mission_id]).unwrap();
}

fn old_file(p: &Path) {
    std::fs::write(p, "orphan").unwrap();
    let f = std::fs::File::options().write(true).open(p).unwrap();
    f.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(90 * 86_400)).unwrap();
}

#[test]
fn a_cleanup_removes_only_old_finished_missions_raw_data_and_keeps_the_record() {
    let f = fixture();
    // An old, discarded mission with a transcript and a check run (log + captured output on disk).
    let old = mission_in_review(&f, "make a note");
    f.core.set_contract(&old.id, contract(false)).unwrap();
    let run = run_check(&f, &old.id, "exists");
    assert!(run.log_path.is_some());
    f.core.generate_proof_pack(&old.id).unwrap();
    f.core.discard_mission(&old.id, true).unwrap();
    age(&f, &old.id);
    // A recent discarded mission, and an old mission that is still open: neither is eligible.
    let recent = mission_in_review(&f, "make a note");
    f.core.discard_mission(&recent.id, true).unwrap();
    let open = mission_in_review(&f, "make a note");
    age(&f, &open.id);
    // Orphan files in runs/: an old one is eligible, a fresh one (a run being written) is not.
    let runs = f.core.cfg.home.join("runs");
    std::fs::create_dir_all(&runs).unwrap();
    old_file(&runs.join("deadbeef.log"));
    std::fs::write(runs.join("fresh.stdout"), "in progress").unwrap();

    let events_before = |id: &str| f.core.store.events(id, 0, 10_000).unwrap().len();
    assert!(events_before(&old.id) > 0 && events_before(&open.id) > 0);
    let plan = f.core.storage_cleanup_preview(30).unwrap();
    assert_eq!(plan.missions.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), vec![old.id.as_str()], "{plan:?}");
    assert_eq!(plan.missions[0].files, vec![format!("{}.log", run.id)], "the check log (captured output exists only for scanner checks): {plan:?}");
    assert_eq!(plan.orphan_files, vec!["deadbeef.log".to_string()]);

    // A stale or invented token deletes nothing.
    assert!(f.core.storage_cleanup_apply(30, "not-the-token").is_err());
    old_file(&runs.join("cafe.log"));
    assert!(f.core.storage_cleanup_apply(30, &plan.token).unwrap_err().to_string().contains("changed since the preview"));
    assert!(runs.join(format!("{}.log", run.id)).exists() && events_before(&old.id) > 0);

    let plan = f.core.storage_cleanup_preview(30).unwrap();
    let res = f.core.storage_cleanup_apply(30, &plan.token).unwrap();
    assert!(res.failures.is_empty(), "{res:?}");
    assert_eq!(res.files_deleted, 3, "the run log and two orphans");
    assert!(!runs.join(format!("{}.log", run.id)).exists() && !runs.join("deadbeef.log").exists());
    assert!(runs.join("fresh.stdout").exists(), "a fresh file may belong to a run in progress");

    // The transcript is gone but explained; the record stays.
    let left = f.core.store.events(&old.id, 0, 10_000).unwrap();
    assert_eq!(left.len(), 1, "only the clean-up marker: {left:?}");
    assert!(left[0].payload["message"].as_str().unwrap().contains("Transcript removed by a storage clean-up"));
    let kept = f.core.store.get_run(&run.id).unwrap();
    assert!(kept.log_path.is_none(), "the dead path is not kept");
    assert_eq!((kept.state, kept.exit_code, kept.output_tail), (run.state, run.exit_code, run.output_tail), "the result and its output tail are");
    assert!(!f.core.store.proof_packs(&old.id).unwrap().is_empty(), "Proof Packs are kept");
    assert_eq!(state(&f, &old.id), "cancelled");
    assert!(events_before(&open.id) > 0 && events_before(&recent.id) > 0, "other missions untouched");

    // Nothing left to do, and the marker is not counted as something to remove.
    let again = f.core.storage_cleanup_preview(30).unwrap();
    assert!(again.missions.is_empty() && again.orphan_files.is_empty(), "{again:?}");
    let usage = f.core.storage_usage().unwrap();
    assert!(usage.untracked_worktrees.is_empty(), "{usage:?}");
}

#[test]
fn worktrees_no_mission_refers_to_are_listed_never_deleted() {
    let f = fixture();
    let stray = f.core.cfg.home.join("worktrees").join("proj").join("left-behind");
    std::fs::create_dir_all(&stray).unwrap();
    std::fs::write(stray.join("unsaved.txt"), "work").unwrap();
    let usage = f.core.storage_usage().unwrap();
    assert_eq!(usage.untracked_worktrees, vec![stray.to_string_lossy().into_owned()]);
    let plan = f.core.storage_cleanup_preview(0).unwrap();
    f.core.storage_cleanup_apply(0, &plan.token).unwrap();
    assert!(stray.join("unsaved.txt").exists());
}
