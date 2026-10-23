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
    let b1 = approval_binding(&m.id, &a.session_id, "fake", "Write", &input, &wt, "default", AGENT_EXECUTES_ON);
    for other in [
        approval_binding(&m.id, &a.session_id, "fake", "Edit", &input, &wt, "default", AGENT_EXECUTES_ON),
        approval_binding(&m.id, &a.session_id, "codex", "Write", &input, &wt, "default", AGENT_EXECUTES_ON),
        approval_binding(&m.id, &a.session_id, "fake", "Write", &input, &wt, "accept_edits", AGENT_EXECUTES_ON),
        approval_binding(&m.id, &a.session_id, "fake", "Write", &input, &wt, "default", "linux_namespaces"),
        approval_binding(&m.id, "other-session", "fake", "Write", &input, &wt, "default", AGENT_EXECUTES_ON),
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
