//! Approval binding (Milestone 2): what a human approves is bound to the exact request and context, expires, and is re-checked
//! against policy at the moment of the decision.

use super::*;

fn first_pending(f: &Fixture, m: &Mission) -> crate::store::Approval {
    wait_until("approval request", || !pending(f, &m.id).is_empty());
    pending(f, &m.id).remove(0)
}

#[test]
fn approval_expires_at_its_deadline_not_after_it() {
    let deadline = "2026-10-31T12:00:00.000Z";
    assert!(!approval_expired(Some(deadline), "2026-10-31T11:59:59.999Z"));
    assert!(approval_expired(Some(deadline), deadline));
    assert!(approval_expired(Some(deadline), "2026-10-31T12:00:00.001Z"));
    assert!(!approval_expired(None, deadline));
}

#[test]
fn monotonic_deadline_expires_approval_even_if_wall_deadline_is_in_the_future() {
    let f = fixture();
    let m = new_mission(&f, "add a scripted note", PermissionProfile::Default);
    f.core.start_mission(&m.id, None).unwrap();
    let a = first_pending(&f, &m);
    assert!(a.expires_at.as_deref().unwrap() > crate::store::now().as_str());
    let handle = f.core.reg().sessions.get(&a.session_id).cloned().unwrap();
    lock(&handle.approval_deadlines).insert(a.id.clone(), Instant::now().checked_sub(Duration::from_secs(1)).unwrap());
    assert!(f.core.decide_approval_checked(&a.id, true, None, a.binding_sha256.as_deref()).is_err());
    assert_eq!(f.core.store.get_approval(&a.id).unwrap().state, "expired");
}

#[test]
fn an_approval_is_bound_to_its_exact_context_and_a_mismatched_binding_is_refused() {
    let f = fixture();
    let m = new_mission(&f, "add a scripted note", PermissionProfile::Default);
    f.core.start_mission(&m.id, None).unwrap();
    let a = first_pending(&f, &m);
    let binding = a.binding_sha256.clone().expect("bound");
    assert_eq!(a.policy_rev.as_deref(), Some(crate::policy::REVISION));
    assert_eq!(a.executes_on.as_deref(), Some(AGENT_EXECUTES_ON));
    assert!(a.expires_at.as_deref().unwrap() > a.created_at.as_str());
    // The binding is a digest of the context: changing any element changes it.
    let wt = PathBuf::from(f.core.store.get_mission(&m.id).unwrap().worktree_path.unwrap());
    let input = json!({"file_path": wt.join("FORGE_TEST.md").to_string_lossy(), "content": "x"});
    let changed_content = json!({"file_path": wt.join("FORGE_TEST.md").to_string_lossy(), "content": "different"});
    let changed_path = json!({"file_path": wt.join("OTHER.md").to_string_lossy(), "content": "x"});
    let expiry = a.expires_at.as_deref().unwrap();
    let b1 = approval_binding(&m.id, &a.session_id, "fake", "Write", &input, &wt, "default", AGENT_EXECUTES_ON, expiry);
    for other in [
        approval_binding(&m.id, &a.session_id, "fake", "Edit", &input, &wt, "default", AGENT_EXECUTES_ON, expiry),
        approval_binding(&m.id, &a.session_id, "codex", "Write", &input, &wt, "default", AGENT_EXECUTES_ON, expiry),
        approval_binding(&m.id, &a.session_id, "fake", "Write", &input, &wt, "accept_edits", AGENT_EXECUTES_ON, expiry),
        approval_binding(&m.id, &a.session_id, "fake", "Write", &input, &wt, "default", "linux_namespaces", expiry),
        approval_binding(&m.id, "other-session", "fake", "Write", &input, &wt, "default", AGENT_EXECUTES_ON, expiry),
        approval_binding("other-mission", &a.session_id, "fake", "Write", &input, &wt, "default", AGENT_EXECUTES_ON, expiry),
        approval_binding(&m.id, &a.session_id, "fake", "Write", &changed_content, &wt, "default", AGENT_EXECUTES_ON, expiry),
        approval_binding(&m.id, &a.session_id, "fake", "Write", &changed_path, &wt, "default", AGENT_EXECUTES_ON, expiry),
        approval_binding(&m.id, &a.session_id, "fake", "Write", &input, &wt, "default", AGENT_EXECUTES_ON, "2099-01-01T00:00:00.000Z"),
    ] {
        assert_ne!(b1, other);
    }
    let err = f.core.decide_approval_checked(&a.id, true, None, Some("not-the-binding")).unwrap_err().to_string();
    assert!(err.contains("not the one you were shown"), "{err}");
    assert_eq!(f.core.store.get_approval(&a.id).unwrap().state, "pending", "a refused decision changes nothing");
    f.core.decide_approval_checked(&a.id, true, None, Some(&binding)).unwrap();
    wait_until("awaiting_review", || state(&f, &m.id) == "awaiting_review");
}

#[test]
fn an_undecided_request_expires_and_the_agent_is_told_it_was_denied() {
    let f = fixture();
    f.core.set_approval_ttl_for_tests(1);
    let m = new_mission(&f, "add a scripted note", PermissionProfile::Default);
    f.core.start_mission(&m.id, None).unwrap();
    let a = first_pending(&f, &m);
    wait_until("expired", || f.core.store.get_approval(&a.id).unwrap().state == "expired");
    let wt = PathBuf::from(f.core.store.get_mission(&m.id).unwrap().worktree_path.unwrap());
    // The agent got a denial (nothing written) and the mission moved on instead of hanging.
    wait_until("mission no longer waiting", || state(&f, &m.id) != "awaiting_approval");
    assert!(!wt.join("FORGE_TEST.md").exists());
    assert!(f.core.decide_approval(&a.id, true, None).is_err(), "an expired request cannot be approved");
    assert!(f.core.store.audit_count("approval.expired").unwrap() >= 1);
}

#[test]
fn a_stopping_session_cannot_approve_a_waiting_request() {
    let f = fixture();
    let m = new_mission(&f, "add a scripted note", PermissionProfile::Default);
    f.core.start_mission(&m.id, None).unwrap();
    let a = first_pending(&f, &m);
    let handle = f.core.reg().sessions.get(&a.session_id).cloned().unwrap();
    handle.stopping.store(true, Ordering::SeqCst);
    f.core.on_permission(&handle, PermissionRequest {
        request_id: "late-request".into(), tool: "Write".into(),
        input: json!({"file_path": handle.worktree.join("AFTER_STOP.md").to_string_lossy(), "content": "fixture"}),
        tool_use_id: "late-tool".into(), source: "test".into(), blocked_path: None, reason: None,
    });
    assert_eq!(pending(&f, &m.id).len(), 1, "a late request must not create another approval");
    let result = f.core.decide_approval_checked(&a.id, true, None, a.binding_sha256.as_deref());
    assert!(result.is_err(), "a stopping session must not grant new approvals");
    assert_eq!(f.core.store.get_approval(&a.id).unwrap().state, "pending");
    f.core.stop_mission(&m.id).unwrap();
}

#[test]
fn turn_completion_is_serialized_with_approval_decisions() {
    let f = fixture();
    let m = new_mission(&f, "add a scripted note", PermissionProfile::Default);
    f.core.start_mission(&m.id, None).unwrap();
    let a = first_pending(&f, &m);
    let handle = f.core.reg().sessions.get(&a.session_id).cloned().unwrap();
    let guard = lock(&handle.life);
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let (core, other) = (f.core.clone(), handle.clone());
    std::thread::spawn(move || {
        core.on_engine_msg(&other, EngineMsg::TurnComplete { status: "interrupted".into(), message: None, provider_session_id: None, usage: None, permission_denials: None });
        let _ = done_tx.send(());
    });
    std::thread::sleep(Duration::from_millis(100));
    assert!(!handle.completed.load(Ordering::SeqCst), "completion must wait for the lifecycle lock");
    drop(guard);
    done_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(f.core.decide_approval_checked(&a.id, true, None, a.binding_sha256.as_deref()).is_err());
    f.core.stop_mission(&m.id).unwrap();
}

/// Time-of-check/time-of-use: the request was safe when asked; while the human was reading it, the target was replaced by a
/// link to a file outside the worktree. Approving must not let the write through.
#[cfg(unix)]
#[test]
fn policy_is_rechecked_at_decision_time_so_a_target_swapped_meanwhile_is_denied() {
    let f = fixture();
    let m = new_mission(&f, "add a scripted note", PermissionProfile::Default);
    f.core.start_mission(&m.id, None).unwrap();
    let a = first_pending(&f, &m);
    let wt = PathBuf::from(f.core.store.get_mission(&m.id).unwrap().worktree_path.unwrap());
    let outside = tempfile::tempdir().unwrap();
    let victim = outside.path().join("victim.txt");
    std::fs::write(&victim, "original\n").unwrap();
    std::os::unix::fs::symlink(&victim, wt.join("FORGE_TEST.md")).unwrap();
    let err = f.core.decide_approval_checked(&a.id, true, None, a.binding_sha256.as_deref()).unwrap_err().to_string();
    assert!(err.contains("policy at decision time"), "{err}");
    let after = f.core.store.get_approval(&a.id).unwrap();
    assert_eq!((after.state.as_str(), after.decided_by.as_deref()), ("denied", Some("policy")));
    wait_until("mission moved on", || state(&f, &m.id) != "awaiting_approval");
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "original\n", "the file outside the worktree was written");
    assert!(f.core.store.audit_count("approval.policy_override").unwrap() == 1);
}

/// Security review F1, end to end: the swapped-in link points at a file that does not exist yet, so nothing on disk "exists"
/// to canonicalise. Approving must still be refused and the outside file must never be created.
#[cfg(unix)]
#[test]
fn a_dangling_link_swapped_in_before_approval_cannot_create_a_file_outside() {
    let f = fixture();
    let m = new_mission(&f, "add a scripted note", PermissionProfile::Default);
    f.core.start_mission(&m.id, None).unwrap();
    let a = first_pending(&f, &m);
    let wt = PathBuf::from(f.core.store.get_mission(&m.id).unwrap().worktree_path.unwrap());
    let outside = tempfile::tempdir().unwrap();
    let victim = outside.path().join("autostart.desktop");
    std::os::unix::fs::symlink(&victim, wt.join("FORGE_TEST.md")).unwrap();
    let err = f.core.decide_approval_checked(&a.id, true, None, a.binding_sha256.as_deref()).unwrap_err().to_string();
    assert!(err.contains("policy at decision time"), "{err}");
    wait_until("mission moved on", || state(&f, &m.id) != "awaiting_approval");
    std::thread::sleep(Duration::from_millis(300));
    assert!(!victim.exists(), "a file was created outside the worktree through a dangling link");
}

/// The same dangling link present before the agent even asks, with auto-approved edits: policy denies, nobody is asked.
#[cfg(unix)]
#[test]
fn a_dangling_link_is_denied_by_policy_even_with_accept_edits() {
    let d = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path().join("x"), d.path().join("notes.md")).unwrap();
    let input = json!({"file_path": d.path().join("notes.md").to_string_lossy(), "content": "x"});
    assert!(matches!(crate::policy::evaluate("Write", &input, d.path(), PermissionProfile::AcceptEdits), crate::policy::Decision::Deny { .. }));
}

fn results(f: &Fixture, m: &Mission) -> Vec<(bool, String)> {
    f.core.store.events(&m.id, 0, 1000).unwrap().into_iter().filter(|e| e.kind == "tool.result").map(|e| (e.payload["ok"].as_bool().unwrap(), e.payload["output"].as_str().unwrap_or("").to_string())).collect()
}

/// Security review F2: redaction would show `Run: TOKEN=[REDACTED]` while the real command pipes a download into a shell.
#[test]
fn a_command_whose_text_would_be_hidden_by_redaction_is_refused_not_asked() {
    let f = fixture();
    let m = new_mission(&f, "FAKE:hidden", PermissionProfile::Default);
    f.core.start_mission(&m.id, None).unwrap();
    wait_until("result", || !results(&f, &m).is_empty());
    let r = results(&f, &m);
    assert!(!r[0].0 && r[0].1.contains("hide"), "{r:?}");
    assert!(pending(&f, &m.id).is_empty(), "the human is never asked about a censored command");
}

/// Security review F8 / protocol review P1: an approved call is reused once (provider prompt after hook) and re-checked;
/// a third identical request is refused instead of riding the old approval forever.
#[test]
fn an_approval_is_reused_at_most_once_for_the_identical_call() {
    let f = fixture();
    let m = new_mission(&f, "FAKE:same3", PermissionProfile::Default);
    f.core.start_mission(&m.id, None).unwrap();
    let a = first_pending(&f, &m);
    f.core.decide_approval_checked(&a.id, true, None, a.binding_sha256.as_deref()).unwrap();
    wait_until("three results", || results(&f, &m).len() == 3);
    let r = results(&f, &m);
    assert_eq!(r.iter().map(|x| x.0).collect::<Vec<_>>(), vec![true, true, false], "{r:?}");
    assert!(r[2].1.contains("already answered"), "{r:?}");
}

#[test]
fn an_approved_call_cannot_be_reused_after_its_deadline() {
    let f = fixture();
    f.core.set_approval_ttl_for_tests(5);
    let m = new_mission(&f, "FAKE:delay-reuse", PermissionProfile::Default);
    f.core.start_mission(&m.id, None).unwrap();
    let a = first_pending(&f, &m);
    f.core.decide_approval_checked(&a.id, true, None, a.binding_sha256.as_deref()).unwrap();
    wait_until("two results", || results(&f, &m).len() == 2);
    let r = results(&f, &m);
    assert_eq!(r.iter().map(|x| x.0).collect::<Vec<_>>(), vec![true, false], "{r:?}");
    assert!(f.core.store.audit_count("approval.reuse_denied").unwrap() >= 1);
}

/// Security review F7: approving requires naming the request that was shown; denying does not.
#[test]
fn approving_without_the_cards_binding_is_refused() {
    let f = fixture();
    let m = new_mission(&f, "add a scripted note", PermissionProfile::Default);
    f.core.start_mission(&m.id, None).unwrap();
    let a = first_pending(&f, &m);
    assert!(f.core.decide_approval_checked(&a.id, true, None, None).unwrap_err().to_string().contains("must name"));
    f.core.decide_approval_checked(&a.id, false, None, None).unwrap();
}

#[test]
fn altered_or_missing_approval_context_cannot_be_approved() {
    let f = fixture();
    let m = new_mission(&f, "add a scripted note", PermissionProfile::Default);
    f.core.start_mission(&m.id, None).unwrap();
    let a = first_pending(&f, &m);
    let set = |binding: Option<&str>, policy: Option<&str>, execution: Option<&str>, expiry: Option<&str>| {
        f.core.store.set_approval_metadata_for_test(&a.id, binding, policy, execution, expiry).unwrap();
    };
    let (binding, policy, execution, expiry) = (a.binding_sha256.as_deref(), a.policy_rev.as_deref(), a.executes_on.as_deref(), a.expires_at.as_deref());
    for (b, p, e, x) in [
        (None, policy, execution, expiry),
        (binding, Some("old-policy"), execution, expiry),
        (binding, policy, Some("other-backend"), expiry),
        (binding, policy, execution, None),
        (binding, policy, execution, Some("2099-01-01T00:00:00.000Z")),
        (Some("forged-binding"), policy, execution, expiry),
    ] {
        set(b, p, e, x);
        assert!(f.core.decide_approval_checked(&a.id, true, None, b).is_err());
        assert_eq!(f.core.store.get_approval(&a.id).unwrap().state, "pending");
    }
    set(binding, policy, execution, expiry);
    let handle = f.core.reg().sessions.get(&a.session_id).cloned().unwrap();
    {
        let mut requests = lock(&handle.requests);
        let (_, input) = requests.get_mut(&a.id).unwrap();
        input["content"] = json!("altered after the card was shown");
    }
    assert!(f.core.decide_approval_checked(&a.id, true, None, binding).unwrap_err().to_string().contains("original request"));
    assert_eq!(f.core.store.get_approval(&a.id).unwrap().state, "pending");
    f.core.decide_approval_checked(&a.id, false, None, None).unwrap();
}
