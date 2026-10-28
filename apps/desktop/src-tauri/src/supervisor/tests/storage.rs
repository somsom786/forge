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
fn explicit_compaction_reclaims_pages_and_refuses_active_work() {
    let f = fixture();
    let store = Store::open(&f.core.cfg.home.join("compact-fixture.db")).unwrap();
    let core = Core::new(store, f.core.cfg.clone(), Arc::new(|_| {}));
    core.store.c().execute_batch("CREATE TABLE compact_fixture(payload BLOB); INSERT INTO compact_fixture VALUES (zeroblob(1048576)); DELETE FROM compact_fixture;").unwrap();
    let result = core.storage_compact().unwrap();
    assert!(result.free_pages_before > 0 && result.pages_after < result.pages_before);
    assert_eq!(core.store.audit_count("storage.compact_finished").unwrap(), 1);
    let m = new_mission(&f, "FAKE:hang", PermissionProfile::Default);
    f.core.start_mission(&m.id, None).unwrap();
    assert!(f.core.storage_compact().unwrap_err().to_string().contains("idle"));
    f.core.stop_mission(&m.id).unwrap();
    wait_until("stopped", || f.core.live_session_count() == 0);
}

#[test]
fn cleanup_does_not_hold_the_experiment_lock_while_waiting_for_the_registry() {
    let f = fixture();
    let (_, dir) = finished_experiment(&f);
    let preview = f.core.storage_cleanup_preview(30).unwrap();
    let reg = f.core.reg();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let core = f.core.clone();
    std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        done_tx.send(core.storage_cleanup_apply(30, &preview.token)).unwrap();
    });
    started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    std::thread::sleep(Duration::from_millis(150));
    let experiment_lock_was_free = f.core.experiments.try_lock().is_ok();
    drop(reg);
    let result = done_rx.recv_timeout(Duration::from_secs(10)).expect("cleanup did not finish").unwrap();
    assert!(experiment_lock_was_free, "cleanup must acquire the registry before experiments, as compaction does");
    assert!(result.failures.is_empty(), "{result:?}");
    assert!(!dir.exists());
}

fn finished_experiment(f: &Fixture) -> (String, PathBuf) {
    use crate::experiment::{Metric, NewExperiment, VariantSpec};
    let e = f.core.experiment_create(&f.project.id, NewExperiment {
        name: "cleanup fixture".into(), objective: None, command: "node -e 0".into(), correctness_command: None,
        metric: Metric::Duration, repetitions: 3, warmup: 0, timeout_secs: 10, baseline_ref: "main".into(),
        variants: vec![VariantSpec { label: "variant".into(), git_ref: "main".into(), mission_id: None }],
    }).unwrap();
    f.core.store.exp_begin_attempt(&e.id, &json!({})).unwrap();
    f.core.store.exp_finish_attempt(&e.id, "completed", None, None).unwrap();
    f.core.store.c().execute("UPDATE experiment SET finished_at='2020-01-01T00:00:00.000Z' WHERE id=?1", [&e.id]).unwrap();
    let dir = f.core.cfg.home.join("experiments").join(&e.id);
    std::fs::create_dir_all(dir.join("1/logs")).unwrap();
    std::fs::write(dir.join("1/logs/fixture.log"), "fixture output").unwrap();
    (e.id, dir)
}

#[test]
fn experiment_cleanup_is_previewed_and_rejects_changes_before_removing_logs_and_empty_folders() {
    let f = fixture();
    let (id, dir) = finished_experiment(&f);
    let preview = f.core.storage_cleanup_preview(30).unwrap();
    assert_eq!(preview.experiments.len(), 1);
    assert_eq!(preview.experiments[0].files[0].path, "1/logs/fixture.log");
    std::fs::write(dir.join("1/logs/later.log"), "new output").unwrap();
    assert!(f.core.storage_cleanup_apply(30, &preview.token).is_err());
    assert!(dir.join("1/logs/fixture.log").exists());
    let preview = f.core.storage_cleanup_preview(30).unwrap();
    let result = f.core.storage_cleanup_apply(30, &preview.token).unwrap();
    assert!(result.failures.is_empty(), "{result:?}");
    assert_eq!(result.files_deleted, 2);
    assert!(!dir.exists());
    assert_eq!(f.core.store.exp_get(&id).unwrap().state, "completed");
    assert_eq!(f.core.store.audit_count("storage.cleanup_started").unwrap(), 1);
    assert_eq!(f.core.store.audit_count("storage.cleanup_finished").unwrap(), 1);
}

#[test]
fn cleanup_preserves_active_experiments_and_leftover_worktrees() {
    let f = fixture();
    let (id, dir) = finished_experiment(&f);
    lock(&f.core.experiments).insert(id.clone(), Arc::new(AtomicBool::new(false)));
    assert!(f.core.storage_cleanup_preview(30).unwrap().experiments.is_empty());
    lock(&f.core.experiments).remove(&id);
    std::fs::create_dir_all(dir.join("1/leftover")).unwrap();
    std::fs::write(dir.join("1/leftover/.git"), "unowned worktree marker").unwrap();
    let preview = f.core.storage_cleanup_preview(30).unwrap();
    assert!(preview.experiments.is_empty());
    assert!(!preview.skipped_experiments.is_empty());
    f.core.storage_cleanup_apply(30, &preview.token).unwrap();
    assert!(dir.join("1/logs/fixture.log").exists());
    assert!(dir.join("1/leftover/.git").exists());
}

#[test]
fn experiment_cleanup_refuses_links_and_requires_a_durable_intent() {
    let f = fixture();
    let (_, dir) = finished_experiment(&f);
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("keep.log"), "personal fixture").unwrap();
    crate::paths::test_dir_link(&dir.join("1").join("linked"), outside.path());
    let preview = f.core.storage_cleanup_preview(30).unwrap();
    assert!(preview.experiments.is_empty());
    f.core.storage_cleanup_apply(30, &preview.token).unwrap();
    assert!(outside.path().join("keep.log").exists());
    let (_, safe) = finished_experiment(&f);
    let preview = f.core.storage_cleanup_preview(30).unwrap();
    f.core.store.c().execute_batch("CREATE TRIGGER no_cleanup_audit BEFORE INSERT ON audit_event WHEN NEW.action='storage.cleanup_started' BEGIN SELECT RAISE(FAIL,'audit unavailable'); END;").unwrap();
    assert!(f.core.storage_cleanup_apply(30, &preview.token).is_err());
    assert!(safe.join("1/logs/fixture.log").exists());
}

#[test]
fn interrupted_cleanup_is_recorded_without_claiming_completion_and_can_be_previewed_again() {
    let f = fixture();
    let (_, dir) = finished_experiment(&f);
    std::fs::write(dir.join("1/logs/second.log"), "second").unwrap();
    let preview = f.core.storage_cleanup_preview(30).unwrap();
    f.core.store.audit_required("user", "storage.cleanup_started", Some("interrupted-fixture"), &json!(preview)).unwrap();
    std::fs::remove_file(dir.join("1/logs/fixture.log")).unwrap();
    f.core.recover_storage_cleanup();
    f.core.recover_storage_cleanup();
    assert_eq!(f.core.store.audit_count("storage.cleanup_interrupted").unwrap(), 1);
    assert_eq!(f.core.store.audit_count("storage.cleanup_finished").unwrap(), 0);
    let remaining = f.core.storage_cleanup_preview(30).unwrap();
    assert_eq!(remaining.experiments[0].files.len(), 1);
    assert!(f.core.storage_cleanup_apply(30, &remaining.token).unwrap().failures.is_empty());
    assert!(!dir.exists());
}

#[cfg(windows)]
#[test]
fn a_locked_experiment_log_is_reported_and_cleanup_can_be_retried() {
    use std::os::windows::fs::OpenOptionsExt;
    let f = fixture();
    let (_, dir) = finished_experiment(&f);
    let held = std::fs::OpenOptions::new().read(true).share_mode(3).open(dir.join("1/logs/fixture.log")).unwrap();
    let preview = f.core.storage_cleanup_preview(30).unwrap();
    let result = f.core.storage_cleanup_apply(30, &preview.token).unwrap();
    assert!(!result.failures.is_empty());
    assert!(dir.exists());
    drop(held);
    let preview = f.core.storage_cleanup_preview(30).unwrap();
    assert!(f.core.storage_cleanup_apply(30, &preview.token).unwrap().failures.is_empty());
    assert!(!dir.exists());
}

#[cfg(windows)]
#[test]
fn a_locked_mission_log_preserves_its_transcript_until_cleanup_can_be_retried() {
    use std::os::windows::fs::OpenOptionsExt;
    let f = fixture();
    let mission = mission_in_review(&f, "make a note");
    f.core.set_contract(&mission.id, contract(false)).unwrap();
    let run = run_check(&f, &mission.id, "exists");
    f.core.discard_mission(&mission.id, true).unwrap();
    age(&f, &mission.id);
    let log = PathBuf::from(run.log_path.as_ref().unwrap());
    let before = f.core.store.events(&mission.id, 0, 10_000).unwrap().len();
    assert!(before > 0);
    let held = std::fs::OpenOptions::new().read(true).share_mode(3).open(&log).unwrap();
    let plan = f.core.storage_cleanup_preview(30).unwrap();
    let result = f.core.storage_cleanup_apply(30, &plan.token).unwrap();
    assert!(!result.failures.is_empty(), "locked log must be reported");
    assert_eq!(f.core.store.events(&mission.id, 0, 10_000).unwrap().len(), before, "transcript must remain on failed log removal");
    assert!(f.core.store.get_run(&run.id).unwrap().log_path.is_some(), "the still-existing log must remain referenced");
    drop(held);
    let plan = f.core.storage_cleanup_preview(30).unwrap();
    assert!(f.core.storage_cleanup_apply(30, &plan.token).unwrap().failures.is_empty());
    assert!(!log.exists());
    assert!(f.core.store.get_run(&run.id).unwrap().log_path.is_none());
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

/// Reliability review: a failure on one mission used to abort the clean-up after earlier missions had already lost their
/// data, and nothing was audited. Now the rest continue, the failure is reported, and the audit always names what went.
#[test]
fn a_cleanup_that_fails_on_one_mission_finishes_the_others_and_audits_what_it_did() {
    let f = fixture();
    let mut ids = vec![];
    for _ in 0..2 {
        let m = mission_in_review(&f, "make a note");
        f.core.discard_mission(&m.id, true).unwrap();
        age(&f, &m.id);
        ids.push(m.id);
    }
    // The second mission's transcript cannot be deleted (as with a full disk).
    f.core.store.c().execute_batch(&format!("CREATE TRIGGER no_delete BEFORE DELETE ON activity_event WHEN OLD.mission_id = '{}' BEGIN SELECT RAISE(FAIL, 'disk full'); END;", ids[1])).unwrap();
    let plan = f.core.storage_cleanup_preview(30).unwrap();
    let res = f.core.storage_cleanup_apply(30, &plan.token).unwrap();
    assert!(res.failures.iter().any(|x| x.contains(&ids[1]) && x.contains("disk full")), "{res:?}");
    assert!(f.core.store.events(&ids[0], 0, 1000).unwrap().iter().all(|e| e.payload["retention"] == true), "the first mission was cleaned");
    let audit: String = f.core.store.c().query_row("SELECT detail_json FROM audit_event WHERE action='storage.cleanup'", [], |r| r.get(0)).unwrap();
    assert!(audit.contains(&ids[0]) && audit.contains("disk full"), "{audit}");
}

#[test]
fn an_absurd_age_means_everything_and_does_not_crash() {
    let f = fixture();
    let plan = f.core.storage_cleanup_preview(u32::MAX).unwrap();
    assert_eq!(plan.older_than_days, 36_500);
    f.core.storage_cleanup_apply(u32::MAX, &plan.token).unwrap();
}
