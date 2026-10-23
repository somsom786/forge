//! Regression tests for the independent security/reliability review (see docs/ENGINEERING_REVIEWS.md).

use super::*;

#[test]
fn a_reask_with_changed_input_needs_a_fresh_approval() {
    // Same provider tool_use_id, different input: must not inherit the earlier approval.
    let f = fixture();
    let m = new_mission(&f, "FAKE:replay", PermissionProfile::Default);
    f.core.start_mission(&m.id, None).unwrap();
    wait_until("first approval", || !pending(&f, &m.id).is_empty());
    let first = pending(&f, &m.id).remove(0);
    f.core.decide_approval(&first.id, true, None).unwrap();
    wait_until("second approval", || !pending(&f, &m.id).is_empty());
    let second = pending(&f, &m.id).remove(0);
    assert_ne!(first.id, second.id);
    assert_ne!(first.provider_request_id, second.provider_request_id);
    assert!(first.provider_request_id.starts_with("tu_dup:") && second.provider_request_id.starts_with("tu_dup:"));
    f.core.decide_approval(&second.id, false, Some("different file, no")).unwrap();
    wait_until("review", || state(&f, &m.id) == "awaiting_review");
    let wt = PathBuf::from(f.core.store.get_mission(&m.id).unwrap().worktree_path.unwrap());
    assert!(wt.join("FORGE_TEST.md").exists(), "the approved call ran");
    assert!(!wt.join("FORGE_TEST_2.md").exists(), "the changed call needed (and was refused) its own approval");
}

#[test]
fn identical_requests_share_one_key_regardless_of_json_key_order() {
    let a = request_key("t1", "Bash", &json!({"command": "ls", "cwd": "x"}));
    let b = request_key("t1", "Bash", &json!({"cwd": "x", "command": "ls"}));
    let c = request_key("t1", "Bash", &json!({"command": "ls -la", "cwd": "x"}));
    let d = request_key("t1", "Edit", &json!({"command": "ls", "cwd": "x"}));
    assert_eq!(a, b);
    assert!(a != c && a != d);
}

#[test]
fn only_one_of_many_concurrent_starts_wins() {
    let f = fixture();
    let m = new_mission(&f, "FAKE:hang", PermissionProfile::Default);
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let (core, id) = (f.core.clone(), m.id.clone());
            std::thread::spawn(move || core.start_mission(&id, None).is_ok())
        })
        .collect();
    let results: Vec<bool> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| **r).count(), 1, "exactly one start may succeed: {results:?}");
    assert_eq!(f.core.live_session_count(), 1);
    f.core.stop_mission(&m.id).unwrap();
    wait_until("stopped", || f.core.live_session_count() == 0);
}

#[test]
fn capacity_is_reserved_atomically() {
    let f = fixture(); // max_concurrent = 2
    let ms: Vec<_> = (0..5).map(|i| new_mission(&f, &format!("FAKE:hang {i}"), PermissionProfile::Default)).collect();
    let threads: Vec<_> = ms
        .iter()
        .map(|m| {
            let (core, id) = (f.core.clone(), m.id.clone());
            std::thread::spawn(move || core.start_mission(&id, None).is_ok())
        })
        .collect();
    let ok: Vec<bool> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(ok.iter().filter(|r| **r).count(), 2, "limit is 2: {ok:?}");
    assert!(f.core.live_session_count() <= 2);
    for (m, started) in ms.iter().zip(&ok) {
        if *started {
            f.core.stop_mission(&m.id).unwrap();
        } else {
            assert_eq!(state(&f, &m.id), "ready", "a refused mission must be untouched");
        }
    }
    wait_until("all stopped", || f.core.live_session_count() == 0);
}

#[test]
fn a_start_is_refused_for_states_that_cannot_start() {
    let f = fixture();
    let m = new_mission(&f, "make a note", PermissionProfile::AcceptEdits);
    f.core.start_mission(&m.id, None).unwrap();
    wait_until("review", || state(&f, &m.id) == "awaiting_review");
    wait_until("finalised", || f.core.live_session_count() == 0);
    f.core.discard_mission(&m.id, true).unwrap();
    assert!(f.core.start_mission(&m.id, None).unwrap_err().to_string().contains("cannot be started"));
    let fresh = new_mission(&f, "x", PermissionProfile::Default);
    assert!(f.core.start_mission(&fresh.id, Some("follow up before start")).is_err());
}

#[test]
fn discard_cannot_delete_a_branch_with_unmerged_commits_even_if_the_worktree_is_gone() {
    let f = fixture();
    let m = new_mission(&f, "make a note", PermissionProfile::AcceptEdits);
    f.core.start_mission(&m.id, None).unwrap();
    wait_until("review", || state(&f, &m.id) == "awaiting_review");
    wait_until("finalised", || f.core.live_session_count() == 0);
    let got = f.core.store.get_mission(&m.id).unwrap();
    let (wt, branch) = (PathBuf::from(got.worktree_path.clone().unwrap()), got.branch.clone().unwrap());
    // The agent (or user) committed work on the mission branch...
    git::run(&wt, &["add", "-A"]).unwrap();
    git::run(&wt, &["-c", "user.name=t", "-c", "user.email=t@e", "commit", "-m", "work worth keeping"]).unwrap();
    // ...and the worktree directory then vanished (deleted by hand, antivirus, a failed cleanup...).
    std::fs::remove_dir_all(&wt).unwrap();

    let err = f.core.discard_mission(&m.id, false).unwrap_err().to_string();
    assert!(err.contains("1 unmerged commit"), "{err}");
    let verify = format!("refs/heads/{branch}");
    assert!(git::run(f.repo.path(), &["rev-parse", "--verify", &verify]).is_ok(), "branch must survive the refused discard");
    assert_ne!(state(&f, &m.id), "cancelled");

    f.core.discard_mission(&m.id, true).unwrap();
    assert!(git::run(f.repo.path(), &["rev-parse", "--verify", "--quiet", &verify]).is_err());
}

#[test]
fn an_answer_given_the_instant_a_prompt_appears_is_never_lost() {
    // Tight polling decider: answers before the supervisor has finished its own bookkeeping.
    for round in 0..6 {
        let f = fixture();
        let m = new_mission(&f, &format!("make a note {round}"), PermissionProfile::Default);
        let (core, id) = (f.core.clone(), m.id.clone());
        let stop = Arc::new(AtomicBool::new(false));
        let s2 = stop.clone();
        let decider = std::thread::spawn(move || {
            while !s2.load(Ordering::SeqCst) {
                if let Ok(list) = core.store.pending_approvals(Some(&id)) {
                    for a in list {
                        let _ = core.decide_approval(&a.id, true, None);
                    }
                }
            }
        });
        f.core.start_mission(&m.id, None).unwrap();
        wait_until("review despite instant answers", || state(&f, &m.id) == "awaiting_review");
        stop.store(true, Ordering::SeqCst);
        decider.join().unwrap();
        let wt = PathBuf::from(f.core.store.get_mission(&m.id).unwrap().worktree_path.unwrap());
        assert!(wt.join("FORGE_TEST.md").exists(), "round {round}: approved write must have happened");
    }
}

#[test]
fn a_descendant_holding_the_pipes_cannot_wedge_finalisation_and_is_killed() {
    let f = fixture();
    let m = new_mission(&f, "FAKE:orphan make a note", PermissionProfile::AcceptEdits);
    f.core.start_mission(&m.id, None).unwrap();
    wait_until("review", || state(&f, &m.id) == "awaiting_review");
    let child_pid = f
        .core
        .store
        .events(&m.id, 0, 1000)
        .unwrap()
        .into_iter()
        .find_map(|e| e.payload.get("childPid").and_then(Value::as_u64))
        .expect("the scripted agent reports its lingering descendant") as u32;
    wait_until("session finalised despite open pipes", || f.core.live_session_count() == 0);
    wait_until("descendant terminated with the job", || !proc::is_alive(child_pid));
    assert_eq!(f.core.store.sessions_for_mission(&m.id).unwrap()[0].state, "ended");
}

#[test]
fn resuming_in_a_worktree_that_was_switched_to_another_branch_is_refused() {
    let f = fixture();
    let m = new_mission(&f, "make a note", PermissionProfile::AcceptEdits);
    f.core.start_mission(&m.id, None).unwrap();
    wait_until("review", || state(&f, &m.id) == "awaiting_review");
    wait_until("finalised", || f.core.live_session_count() == 0);
    let wt = PathBuf::from(f.core.store.get_mission(&m.id).unwrap().worktree_path.unwrap());
    git::run(&wt, &["switch", "-c", "sneaky"]).unwrap();
    let err = f.core.start_mission(&m.id, Some("carry on")).unwrap_err().to_string();
    assert!(err.contains("refusing"), "{err}");
    assert_eq!(f.core.live_session_count(), 0);
    assert_eq!(state(&f, &m.id), "failed");
}

#[test]
fn a_provider_withdrawing_a_tool_call_expires_its_prompt() {
    let f = fixture();
    let m = new_mission(&f, "make a note", PermissionProfile::Default);
    f.core.start_mission(&m.id, None).unwrap();
    wait_until("approval", || !pending(&f, &m.id).is_empty());
    let a = pending(&f, &m.id).remove(0);
    let h = f.core.handle_for_mission(&m.id).unwrap();
    let req_id = lock(&h.waiters).get(&a.id).unwrap()[0].clone();
    f.core.on_permission_cancelled(&h, &req_id);
    assert_eq!(f.core.store.get_approval(&a.id).unwrap().state, "expired");
    assert_eq!(state(&f, &m.id), "running", "no pending prompts left, so the mission is running again");
    f.core.stop_mission(&m.id).unwrap();
    wait_until("stopped", || f.core.live_session_count() == 0);
}

#[test]
fn a_verbatim_engine_path_still_launches() {
    // Tauri hands out `\?\C:\...` paths; Node's loader fails on them (EISDIR: lstat 'C:'). The supervisor must normalise.
    let repo = git::testutil::init_repo();
    let home = tempfile::tempdir().unwrap();
    let verbatim = std::fs::canonicalize(engine_script()).unwrap(); // std keeps the \?\ prefix on Windows
    #[cfg(windows)]
    assert!(verbatim.to_string_lossy().starts_with(r"\\?\"), "precondition: this is the hostile form");
    let core = Core::new(
        Store::in_memory(),
        Config { home: home.path().to_path_buf(), node: None, engine_script: Some(verbatim), claude: None, codex: None, allow_fake: true, max_concurrent: 1, ollama_url: "http://127.0.0.1:11434".into() },
        Arc::new(|_| {}),
    );
    let project = core.add_project(repo.path()).unwrap();
    let m = core.create_mission(&project.id, "make a note", None, "fake", None, PermissionProfile::AcceptEdits).unwrap();
    core.start_mission(&m.id, None).unwrap();
    let t0 = Instant::now();
    while core.store.get_mission(&m.id).unwrap().state != "awaiting_review" {
        let st = core.store.get_mission(&m.id).unwrap();
        assert!(st.state != "failed", "engine failed to start: {:?}", st.last_error);
        assert!(t0.elapsed() < Duration::from_secs(25), "timed out in {}", st.state);
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// Protocol review: a line the core cannot read used to be skipped silently (a lost permission request would leave the agent
/// waiting forever). Now it ends the session, and says why.
#[test]
fn an_unreadable_engine_line_ends_the_session_with_a_reason() {
    let f = fixture();
    let m = new_mission(&f, "FAKE:garbage", PermissionProfile::Default);
    f.core.start_mission(&m.id, None).unwrap();
    wait_until("session ended", || f.core.live_session_count() == 0 && state(&f, &m.id) != "running" && state(&f, &m.id) != "preparing");
    let ended = f.core.store.events(&m.id, 0, 1000).unwrap();
    let text = serde_json::to_string(&ended).unwrap() + &serde_json::to_string(&f.core.store.get_mission(&m.id).unwrap()).unwrap();
    assert!(text.contains("unreadable protocol line"), "the reason is recorded: {text}");
}
