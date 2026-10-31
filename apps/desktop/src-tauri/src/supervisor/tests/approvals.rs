//! Approval binding (Milestone 2): what a human approves is bound to the exact request and context, expires, and is re-checked
//! against policy at the moment of the decision.

use super::*;

fn first_pending(f: &Fixture, m: &Mission) -> crate::store::Approval {
    wait_until("approval request", || !pending(f, &m.id).is_empty());
    pending(f, &m.id).remove(0)
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
    let b1 = approval_binding(&m.id, &a.session_id, "fake", "Write", &input, &wt, "default", AGENT_EXECUTES_ON);
    for other in [
        approval_binding(&m.id, &a.session_id, "fake", "Edit", &input, &wt, "default", AGENT_EXECUTES_ON),
        approval_binding(&m.id, &a.session_id, "codex", "Write", &input, &wt, "default", AGENT_EXECUTES_ON),
        approval_binding(&m.id, &a.session_id, "fake", "Write", &input, &wt, "accept_edits", AGENT_EXECUTES_ON),
        approval_binding(&m.id, &a.session_id, "fake", "Write", &input, &wt, "default", "linux_namespaces"),
        approval_binding(&m.id, "other-session", "fake", "Write", &input, &wt, "default", AGENT_EXECUTES_ON),
        approval_binding("other-mission", &a.session_id, "fake", "Write", &input, &wt, "default", AGENT_EXECUTES_ON),
        approval_binding(&m.id, &a.session_id, "fake", "Write", &changed_content, &wt, "default", AGENT_EXECUTES_ON),
        approval_binding(&m.id, &a.session_id, "fake", "Write", &changed_path, &wt, "default", AGENT_EXECUTES_ON),
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
