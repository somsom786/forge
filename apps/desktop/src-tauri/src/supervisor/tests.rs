
use super::*;
use std::sync::Mutex as StdMutex;
use std::time::Instant;

struct Fixture {
    core: Arc<Core>,
    repo: tempfile::TempDir,
    _home: tempfile::TempDir,
    events: Arc<StdMutex<Vec<UiEvent>>>,
    project: Project,
}

fn engine_script() -> PathBuf {
    // Lets the whole suite run against the packaged single-file bundle (copied somewhere with no node_modules).
    if let Some(p) = std::env::var_os("FORGE_TEST_ENGINE") {
        return dunce::canonicalize(&p).expect("FORGE_TEST_ENGINE does not exist");
    }
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../packages/engine/dist/main.js");
    dunce::canonicalize(&p).unwrap_or_else(|_| panic!("engine not built at {}; run `npm run build:engine` first", p.display()))
}

fn fixture() -> Fixture {
    let repo = git::testutil::init_repo();
    let home = tempfile::tempdir().unwrap();
    let events: Arc<StdMutex<Vec<UiEvent>>> = Arc::new(StdMutex::new(vec![]));
    let ev2 = events.clone();
    let core = Core::new(
        Store::in_memory(),
        Config { home: home.path().to_path_buf(), node: None, engine_script: Some(engine_script()), claude: None, codex: None, allow_fake: true, max_concurrent: 2, ollama_url: "http://127.0.0.1:11434".into() },
        Arc::new(move |e| ev2.lock().unwrap().push(e)),
    );
    let project = core.add_project(repo.path()).unwrap();
    Fixture { core, repo, _home: home, events, project }
}

fn wait_until(what: &str, mut f: impl FnMut() -> bool) {
    let t0 = Instant::now();
    while !f() {
        assert!(t0.elapsed() < Duration::from_secs(25), "timed out waiting for: {what}");
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn state(f: &Fixture, id: &str) -> String {
    f.core.store.get_mission(id).unwrap().state
}

fn new_mission(f: &Fixture, objective: &str, profile: PermissionProfile) -> Mission {
    f.core.create_mission(&f.project.id, objective, None, "fake", None, profile).unwrap()
}

fn pending(f: &Fixture, id: &str) -> Vec<Approval> {
    f.core.store.pending_approvals(Some(id)).unwrap()
}

#[test]
fn full_mission_with_approval_isolates_changes_in_a_worktree() {
    let f = fixture();
    let m = new_mission(&f, "add a scripted note", PermissionProfile::Default);
    f.core.start_mission(&m.id, None).unwrap();
    wait_until("approval request", || !pending(&f, &m.id).is_empty());
    assert_eq!(state(&f, &m.id), "awaiting_approval");

    let a = pending(&f, &m.id).remove(0);
    assert_eq!(a.tool, "Write");
    let wt = PathBuf::from(f.core.store.get_mission(&m.id).unwrap().worktree_path.unwrap());
    assert!(!wt.join("FORGE_TEST.md").exists(), "nothing may be written before approval");

    f.core.decide_approval(&a.id, true, None).unwrap();
    wait_until("awaiting_review", || state(&f, &m.id) == "awaiting_review");

    assert!(wt.join("FORGE_TEST.md").exists());
    assert!(!f.repo.path().join("FORGE_TEST.md").exists(), "main checkout must stay untouched");
    assert!(!git::status(f.repo.path()).unwrap().dirty);

    let base = f.core.store.get_mission(&m.id).unwrap().base_commit.unwrap();
    let files = git::changed_files(&wt, &base).unwrap();
    assert!(files.iter().any(|c| c.path == "FORGE_TEST.md"));

    // history is persisted and ordered
    let kinds: Vec<String> = f.core.store.events(&m.id, 0, 1000).unwrap().into_iter().map(|e| e.kind).collect();
    for k in ["session.started", "tool.call", "approval.requested", "approval.decided", "file.changed", "turn.completed"] {
        assert!(kinds.iter().any(|x| x == k), "missing {k} in {kinds:?}");
    }
    wait_until("session finalised", || f.core.live_session_count() == 0);
    let sessions = f.core.store.sessions_for_mission(&m.id).unwrap();
    assert_eq!(sessions[0].state, "ended");
    assert!(f.core.store.audit_count("approval.approve").unwrap() == 1);
}

#[test]
fn denying_an_approval_keeps_the_worktree_clean() {
    let f = fixture();
    let m = new_mission(&f, "add a note", PermissionProfile::Default);
    f.core.start_mission(&m.id, None).unwrap();
    wait_until("approval", || !pending(&f, &m.id).is_empty());
    let a = pending(&f, &m.id).remove(0);
    f.core.decide_approval(&a.id, false, Some("not now")).unwrap();
    wait_until("awaiting_review", || state(&f, &m.id) == "awaiting_review");
    let wt = PathBuf::from(f.core.store.get_mission(&m.id).unwrap().worktree_path.unwrap());
    assert!(!wt.join("FORGE_TEST.md").exists());
    assert!(f.core.decide_approval(&a.id, true, None).is_err(), "decisions are single-shot");
}

#[test]
fn policy_blocks_writes_outside_the_worktree_without_asking_the_user() {
    let f = fixture();
    let m = new_mission(&f, "FAKE:deny try to escape", PermissionProfile::AcceptEdits);
    f.core.start_mission(&m.id, None).unwrap();
    wait_until("awaiting_review", || state(&f, &m.id) == "awaiting_review");
    assert!(pending(&f, &m.id).is_empty(), "a policy denial must never reach the human as a prompt");
    let all = f.core.store.approvals_for_mission(&m.id).unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].state, "auto_denied");
    let wt = PathBuf::from(f.core.store.get_mission(&m.id).unwrap().worktree_path.unwrap());
    assert!(!wt.parent().unwrap().join("outside-worktree.txt").exists());
}

#[test]
fn protected_files_stay_protected_even_with_accept_edits() {
    let f = fixture();
    let m = new_mission(&f, "FAKE:env write secrets", PermissionProfile::AcceptEdits);
    f.core.start_mission(&m.id, None).unwrap();
    wait_until("awaiting_review", || state(&f, &m.id) == "awaiting_review");
    let wt = PathBuf::from(f.core.store.get_mission(&m.id).unwrap().worktree_path.unwrap());
    assert!(!wt.join(".env").exists());
    assert_eq!(f.core.store.approvals_for_mission(&m.id).unwrap()[0].state, "auto_denied");
}

#[test]
fn accept_edits_profile_auto_allows_ordinary_edits() {
    let f = fixture();
    let m = new_mission(&f, "make a note", PermissionProfile::AcceptEdits);
    f.core.start_mission(&m.id, None).unwrap();
    wait_until("awaiting_review", || state(&f, &m.id) == "awaiting_review");
    let wt = PathBuf::from(f.core.store.get_mission(&m.id).unwrap().worktree_path.unwrap());
    assert!(wt.join("FORGE_TEST.md").exists());
    assert!(pending(&f, &m.id).is_empty());
}

#[test]
fn stopping_a_hung_agent_marks_it_interrupted_and_cleans_up_the_process() {
    let f = fixture();
    let m = new_mission(&f, "FAKE:hang", PermissionProfile::Default);
    f.core.start_mission(&m.id, None).unwrap();
    wait_until("running", || state(&f, &m.id) == "running");
    let pid = f.core.handle_for_mission(&m.id).unwrap().pid;
    f.core.stop_mission(&m.id).unwrap();
    wait_until("interrupted", || state(&f, &m.id) == "interrupted");
    wait_until("session removed", || f.core.live_session_count() == 0);
    wait_until("process gone", || !proc::is_alive(pid));
}

#[test]
fn stopping_while_an_approval_is_pending_expires_it() {
    let f = fixture();
    let m = new_mission(&f, "add a note", PermissionProfile::Default);
    f.core.start_mission(&m.id, None).unwrap();
    wait_until("approval", || !pending(&f, &m.id).is_empty());
    let a = pending(&f, &m.id).remove(0);
    f.core.stop_mission(&m.id).unwrap();
    wait_until("interrupted", || state(&f, &m.id) == "interrupted");
    wait_until("finalised", || f.core.live_session_count() == 0);
    assert!(["denied", "expired"].contains(&f.core.store.get_approval(&a.id).unwrap().state.as_str()));
    assert!(pending(&f, &m.id).is_empty());
    assert!(f.core.decide_approval(&a.id, true, None).is_err(), "cannot approve after the session ended");
}

#[test]
fn agent_crash_fails_the_mission_honestly_with_diagnostics() {
    let f = fixture();
    let m = new_mission(&f, "FAKE:crash", PermissionProfile::Default);
    f.core.start_mission(&m.id, None).unwrap();
    wait_until("failed", || state(&f, &m.id) == "failed");
    let got = f.core.store.get_mission(&m.id).unwrap();
    assert!(got.last_error.unwrap().contains("exit code 7"));
    wait_until("finalised", || f.core.live_session_count() == 0);
    assert_eq!(f.core.store.sessions_for_mission(&m.id).unwrap()[0].state, "lost");
}

#[test]
fn restart_recovery_marks_active_missions_interrupted_not_running() {
    let f = fixture();
    let m = new_mission(&f, "x", PermissionProfile::Default);
    // Simulate a FORGE that died mid-run: persisted as running with a live session and a pending approval.
    f.core.store.transition(&m.id, MissionState::Preparing, "t").unwrap();
    f.core.store.transition(&m.id, MissionState::Running, "t").unwrap();
    let s = f.core.store.create_session(&m.id, "fake", None).unwrap();
    f.core.store.session_live(&s.id, Some(999_999), None, &json!({})).unwrap();
    f.core.store.create_approval(&m.id, &s.id, "r", "Bash", "x", &json!({}), "low", &[], "pending", None, None).unwrap();

    let r = f.core.recover().unwrap();
    assert_eq!((r.lost_sessions, r.expired_approvals, r.interrupted_missions.len()), (1, 1, 1));
    assert_eq!(state(&f, &m.id), "interrupted");
    assert_eq!(f.core.store.get_session(&s.id).unwrap().state, "lost");
    assert!(pending(&f, &m.id).is_empty());
    // idempotent
    assert_eq!(f.core.recover().unwrap().interrupted_missions.len(), 0);
}

#[test]
fn concurrency_limit_is_enforced() {
    let f = fixture();
    let ms: Vec<_> = (0..3).map(|i| new_mission(&f, &format!("FAKE:hang {i}"), PermissionProfile::Default)).collect();
    f.core.start_mission(&ms[0].id, None).unwrap();
    f.core.start_mission(&ms[1].id, None).unwrap();
    let err = f.core.start_mission(&ms[2].id, None).unwrap_err().to_string();
    assert!(err.contains("concurrency limit"), "{err}");
    assert_eq!(state(&f, &ms[2].id), "ready", "a refused start must not change mission state");
    for m in &ms[..2] {
        f.core.stop_mission(&m.id).unwrap();
    }
    wait_until("all stopped", || f.core.live_session_count() == 0);
}

#[test]
fn parallel_missions_get_separate_worktrees_and_branches() {
    let f = fixture();
    let a = new_mission(&f, "FAKE:hang a", PermissionProfile::Default);
    let b = new_mission(&f, "FAKE:hang b", PermissionProfile::Default);
    f.core.start_mission(&a.id, None).unwrap();
    f.core.start_mission(&b.id, None).unwrap();
    let (ma, mb) = (f.core.store.get_mission(&a.id).unwrap(), f.core.store.get_mission(&b.id).unwrap());
    assert_ne!(ma.worktree_path, mb.worktree_path);
    assert_ne!(ma.branch, mb.branch);
    assert!(PathBuf::from(ma.worktree_path.unwrap()).join("README.md").exists());
    for m in [&a, &b] {
        f.core.stop_mission(&m.id).unwrap();
    }
    wait_until("stopped", || f.core.live_session_count() == 0);
}

#[test]
fn discard_refuses_to_destroy_unsaved_work_without_confirmation() {
    let f = fixture();
    let m = new_mission(&f, "make a note", PermissionProfile::AcceptEdits);
    f.core.start_mission(&m.id, None).unwrap();
    wait_until("awaiting_review", || state(&f, &m.id) == "awaiting_review");
    wait_until("finalised", || f.core.live_session_count() == 0);
    let wt = PathBuf::from(f.core.store.get_mission(&m.id).unwrap().worktree_path.unwrap());

    let err = f.core.discard_mission(&m.id, false).unwrap_err().to_string();
    assert!(err.contains("permanently delete") && err.contains("uncommitted"), "{err}");
    assert!(wt.join("FORGE_TEST.md").exists(), "refusal must leave the work intact");
    assert_eq!(state(&f, &m.id), "awaiting_review");

    f.core.discard_mission(&m.id, true).unwrap();
    assert!(!wt.exists());
    assert_eq!(state(&f, &m.id), "cancelled");
}

#[test]
fn ui_receives_events_after_they_are_persisted() {
    let f = fixture();
    let m = new_mission(&f, "make a note", PermissionProfile::AcceptEdits);
    f.core.start_mission(&m.id, None).unwrap();
    wait_until("awaiting_review", || state(&f, &m.id) == "awaiting_review");
    let evs = f.events.lock().unwrap();
    for e in evs.iter() {
        if let UiEvent::Activity { event } = e {
            let stored = f.core.store.events(&event.mission_id, event.id - 1, 1).unwrap();
            assert_eq!(stored.first().map(|s| s.id), Some(event.id), "UI saw an event that is not in the database");
        }
    }
    assert!(evs.iter().any(|e| matches!(e, UiEvent::Delta { .. })), "streaming deltas reach the UI");
}

/// LIVE: drives the real Claude Code CLI through the full FORGE path. Spends a small amount of the
/// signed-in user's provider allowance, so it only runs on request:
///   cargo test --lib live_claude -- --ignored --nocapture
#[test]
#[ignore = "live: spends provider quota"]
fn live_claude_mission_end_to_end() {
    let claude = which::which("claude").expect("claude CLI on PATH");
    let repo = git::testutil::init_repo();
    let home = tempfile::tempdir().unwrap();
    let core = Core::new(
        Store::in_memory(),
        Config { home: home.path().to_path_buf(), node: None, engine_script: Some(engine_script()), claude: Some(claude), codex: None, allow_fake: false, max_concurrent: 1, ollama_url: "http://127.0.0.1:11434".into() },
        Arc::new(|_| {}),
    );
    let project = core.add_project(repo.path()).unwrap();
    let m = core
        .create_mission(&project.id, "Create a file named hello.txt in the repository root containing exactly the text: forge works\nDo nothing else. Do not run any commands.", None, "claude", Some("haiku"), PermissionProfile::Default)
        .unwrap();
    core.start_mission(&m.id, None).unwrap();

    let t0 = Instant::now();
    let mut asked: Vec<String> = vec![];
    loop {
        let st = core.store.get_mission(&m.id).unwrap();
        if matches!(st.state.as_str(), "awaiting_review" | "failed" | "interrupted") {
            break;
        }
        assert!(t0.elapsed() < Duration::from_secs(240), "live mission timed out in state {}", st.state);
        for a in core.store.pending_approvals(Some(&m.id)).unwrap() {
            asked.push(format!("{} :: {}", a.tool, a.summary));
            let ok = matches!(a.tool.as_str(), "Write" | "Edit" | "MultiEdit");
            core.decide_approval(&a.id, ok, None).unwrap();
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    let done = core.store.get_mission(&m.id).unwrap();
    println!("final state: {} error: {:?}", done.state, done.last_error);
    println!("approvals asked: {asked:#?}");
    for e in core.store.events(&m.id, 0, 500).unwrap() {
        println!("{:>4} {:<20} {}", e.seq, e.kind, e.payload.to_string().chars().take(160).collect::<String>());
    }
    assert_eq!(done.state, "awaiting_review", "error: {:?}", done.last_error);
    let wt = PathBuf::from(done.worktree_path.clone().unwrap());
    let content = std::fs::read_to_string(wt.join("hello.txt")).expect("hello.txt created in the worktree");
    assert_eq!(content.trim(), "forge works");
    assert!(!repo.path().join("hello.txt").exists(), "main checkout untouched");

    let ev = core.store.events(&m.id, 0, 500).unwrap();
    let started = ev.iter().find(|e| e.kind == "session.started").expect("session.started");
    println!("credentialSource = {}", started.payload["credentialSource"]);
    assert_eq!(started.payload["credentialSource"], "none", "must run on the subscription login, not an API key");
    assert!(ev.iter().any(|e| e.kind == "turn.completed"));
    wait_until("finalised", || core.live_session_count() == 0);
    let usage = core.store.sessions_for_mission(&m.id).unwrap()[0].usage.clone();
    println!("usage: {usage:?}");
    assert!(usage.is_some(), "provider-reported usage is recorded");
}

#[test]
fn invalid_projects_and_empty_objectives_are_rejected() {
    let f = fixture();
    let plain = tempfile::tempdir().unwrap();
    assert!(f.core.add_project(plain.path()).is_err(), "non-repo");
    assert!(f.core.create_mission(&f.project.id, "   ", None, "fake", None, PermissionProfile::Default).is_err());
    assert!(f.core.create_mission(&f.project.id, "x", None, "no-such-provider", None, PermissionProfile::Default).is_err(), "unknown provider is refused, not faked");
    // A known provider whose executable is missing is refused when it is started, with an actionable message.
    let m = f.core.create_mission(&f.project.id, "x", None, "codex", None, PermissionProfile::Default).unwrap();
    let err = f.core.start_mission(&m.id, None).unwrap_err().to_string();
    assert!(err.contains("Codex was not found"), "{err}");
}

mod regressions;
mod live;
mod phase2;
mod memory;
mod review_hardening;
mod harness;
mod experiment;
mod conflicts;
mod approvals;

#[test]
fn the_agents_overview_lists_running_and_recent_sessions_with_their_missions() {
    let f = fixture();
    let m = new_mission(&f, "FAKE:hang", PermissionProfile::Default);
    f.core.start_mission(&m.id, None).unwrap();
    wait_until("running", || state(&f, &m.id) == "running");
    let o = f.core.agents_overview(7).unwrap();
    assert_eq!(o.sessions.len(), 1);
    assert_eq!((o.sessions[0].mission_title.as_str(), o.sessions[0].project_id.as_str()), (m.title.as_str(), f.project.id.as_str()));
    assert_eq!(o.usage.len(), 1);
    assert_eq!((o.usage[0].provider.as_str(), o.usage[0].live), ("fake", 1));
    f.core.stop_mission(&m.id).unwrap();
    wait_until("interrupted", || state(&f, &m.id) == "interrupted");
    wait_until("finalised", || f.core.live_session_count() == 0);
    let o = f.core.agents_overview(7).unwrap();
    assert_eq!((o.sessions.len(), o.usage[0].live), (1, 0), "an ended session stays listed for the window");
}
