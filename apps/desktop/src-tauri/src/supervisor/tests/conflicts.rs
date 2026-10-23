//! Phase 5: conflict report across a project's missions, against real worktrees.

use super::*;

/// A mission with a real worktree on its own branch from the current HEAD of main (as a started mission has).
fn mission_with_worktree(f: &Fixture, name: &str) -> (Mission, PathBuf) {
    let m = new_mission(f, name, PermissionProfile::Default);
    let repo = f.repo.path();
    let base = git::head_commit(repo).unwrap();
    let wt = f._home.path().join("wt").join(name);
    let branch = format!("forge/{name}");
    git::worktree_add(repo, &wt, &branch, &base).unwrap();
    f.core.store.set_mission_workspace(&m.id, "main", &base, &branch, &wt.to_string_lossy()).unwrap();
    (f.core.store.get_mission(&m.id).unwrap(), wt)
}

#[test]
fn the_project_report_names_overlaps_conflicts_drift_and_skipped_missions() {
    let f = fixture();
    let (a, wa) = mission_with_worktree(&f, "a");
    let (b, wb) = mission_with_worktree(&f, "b");
    let (c, wc) = mission_with_worktree(&f, "c");
    let (gone, wg) = mission_with_worktree(&f, "gone");
    let (merged, _) = mission_with_worktree(&f, "merged");
    std::fs::write(wa.join("src/lib.txt"), "one-a\ntwo\n").unwrap();
    std::fs::write(wb.join("src/lib.txt"), "one-b\ntwo\n").unwrap();
    std::fs::write(wc.join("notes.md"), "c\n").unwrap();
    git::worktree_remove(f.repo.path(), &wg, true).unwrap();
    // A merged mission's work has landed; it is not part of the report.
    f.core.store.transition(&merged.id, MissionState::Ready, "test").unwrap();
    for s in [MissionState::Preparing, MissionState::Running, MissionState::AwaitingReview, MissionState::Approved, MissionState::Merged] {
        f.core.store.transition(&merged.id, s, "test").unwrap();
    }
    // main moves after the missions started, touching b's line.
    std::fs::write(f.repo.path().join("src/lib.txt"), "one-main\ntwo\n").unwrap();
    git::run(f.repo.path(), &["commit", "-qam", "main moves"]).unwrap();

    let r = f.core.mission_conflicts(&f.project.id).unwrap();
    let ids: Vec<&str> = r.missions.iter().map(|m| m.mission_id.as_str()).collect();
    assert_eq!(ids, vec![a.id.as_str(), b.id.as_str(), c.id.as_str()]);
    assert_eq!(r.skipped.len(), 1);
    assert_eq!(r.skipped[0].mission_id, gone.id);
    assert!(r.skipped[0].reason.contains("no longer exists"), "{:?}", r.skipped);

    let ab = r.pairs.iter().find(|p| p.a == a.id && p.b == b.id).unwrap();
    assert_eq!(ab.overlapping_files, vec!["src/lib.txt"]);
    assert_eq!(ab.prediction.outcome, "conflicts");
    let ac = r.pairs.iter().find(|p| p.a == a.id && p.b == c.id).unwrap();
    assert!(ac.overlapping_files.is_empty());
    assert_eq!(ac.prediction.outcome, "clean");
    assert_eq!(r.pairs.len(), 3);

    let db = r.drift.iter().find(|d| d.mission_id == b.id).unwrap();
    assert_eq!((db.commits_behind, db.overlapping_files.clone()), (1, vec!["src/lib.txt".to_string()]));
    assert_eq!(db.prediction.as_ref().unwrap().outcome, "conflicts");
    let dc = r.drift.iter().find(|d| d.mission_id == c.id).unwrap();
    assert_eq!((dc.commits_behind, dc.prediction.as_ref().unwrap().outcome.as_str()), (1, "clean"));
}
