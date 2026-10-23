//! Agent containment on Linux: the engine and the provider it starts run inside the `linux_namespaces` backend, so what an
//! agent runs after approval (or without asking) cannot read or write outside the worktree. Each test has a host control
//! showing the same action succeeds without containment, so a containment that silently is not there cannot pass.
//!
//! Requires bubblewrap; skipped without it unless FORGE_REQUIRE_SANDBOX_TESTS=1 (Linux CI), which makes absence a failure.

use super::*;

fn sandbox_available() -> bool {
    if crate::execution::availability(crate::execution::Backend::LinuxNamespaces).is_ok() {
        return true;
    }
    assert!(std::env::var("FORGE_REQUIRE_SANDBOX_TESTS").as_deref() != Ok("1"), "the Linux-namespace backend is required here but unavailable");
    eprintln!("SKIPPED: linux_namespaces backend unavailable");
    false
}

fn snoop(f: &Fixture, secret: &Path) -> Value {
    let m = new_mission(f, &format!("FAKE:snoop SNOOP={}", secret.display()), PermissionProfile::Default);
    f.core.start_mission(&m.id, None).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while !f.core.store.events(&m.id, 0, 1000).unwrap().iter().any(|e| e.payload["message"] == "snoop") {
        assert!(std::time::Instant::now() < deadline, "no snoop report; events: {:#?}", f.core.store.events(&m.id, 0, 1000).unwrap().iter().map(|e| (&e.kind, &e.payload)).collect::<Vec<_>>());
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    wait_until("ended", || f.core.live_session_count() == 0);
    f.core.store.events(&m.id, 0, 1000).unwrap().into_iter().find(|e| e.payload["message"] == "snoop").unwrap().payload
}

#[test]
fn a_contained_agent_cannot_read_or_write_outside_its_worktree() {
    if !sandbox_available() {
        return;
    }
    let f = fixture();
    let outside = tempfile::tempdir().unwrap();
    let secret = outside.path().join("id_rsa");
    std::fs::write(&secret, "PRIVATE KEY MATERIAL").unwrap();

    // Control: on the host the same agent reads it and writes next to it.
    let host = snoop(&f, &secret);
    assert_eq!(host["read"], "PRIVATE KEY MATERIAL", "{host}");
    assert_eq!(host["wrote"], true);
    std::fs::remove_file(secret.with_extension("pwned")).ok();
    std::fs::remove_file(outside.path().join("id_rsa.pwned")).ok();

    f.core.project_set_agent_backend(&f.project.id, "linux_namespaces").unwrap();
    let boxed = snoop(&f, &secret);
    assert_eq!(boxed["sandbox"], "linux_namespaces", "{boxed}");
    assert!(boxed["read"].as_str().unwrap().starts_with("ERROR"), "a contained agent read a file outside its worktree: {boxed}");
    assert_eq!(boxed["wrote"], false, "{boxed}");
    assert!(!outside.path().join("id_rsa.pwned").exists());
}

#[test]
fn a_contained_agent_still_works_in_its_worktree_and_approvals_record_where_it_ran() {
    if !sandbox_available() {
        return;
    }
    let f = fixture();
    f.core.project_set_agent_backend(&f.project.id, "linux_namespaces").unwrap();
    let m = new_mission(&f, "add a scripted note", PermissionProfile::Default);
    f.core.start_mission(&m.id, None).unwrap();
    wait_until("approval", || !pending(&f, &m.id).is_empty());
    let a = pending(&f, &m.id).remove(0);
    assert_eq!(a.executes_on.as_deref(), Some("linux_namespaces"), "the approval names where the action runs");
    f.core.decide_approval_checked(&a.id, true, None, a.binding_sha256.as_deref()).unwrap();
    wait_until("awaiting_review", || state(&f, &m.id) == "awaiting_review");
    let wt = PathBuf::from(f.core.store.get_mission(&m.id).unwrap().worktree_path.unwrap());
    assert!(wt.join("FORGE_TEST.md").exists(), "an approved write inside the worktree works when contained");
    assert!(f.core.store.events(&m.id, 0, 1000).unwrap().iter().any(|e| e.payload["agentBackend"] == "linux_namespaces"), "the timeline says where the agent ran");
    assert!(f.core.store.sessions_for_mission(&m.id).unwrap().iter().all(|s| s.executes_on == "linux_namespaces"), "the session records it");
    let pack = f.core.generate_proof_pack(&m.id).unwrap();
    assert!(pack.markdown.contains("Agent ran: Isolated: Linux namespaces"), "and so does the Proof Pack");
}

#[test]
fn an_unavailable_agent_backend_cannot_be_selected_and_never_falls_back() {
    let f = fixture();
    for b in ["windows_sandbox", "isolated_wsl"] {
        assert!(f.core.project_set_agent_backend(&f.project.id, b).is_err(), "{b}");
    }
    // A backend recorded as selected that is unavailable at start time refuses to start the agent.
    f.core.store.set_project_agent_backend(&f.project.id, "windows_sandbox").unwrap();
    let m = new_mission(&f, "add a scripted note", PermissionProfile::Default);
    assert!(f.core.start_mission(&m.id, None).is_err() || { wait_until("failed", || state(&f, &m.id) == "failed"); true });
    assert!(f.core.store.events(&m.id, 0, 1000).unwrap().iter().all(|e| e.kind != "session.started"), "no agent ran on the host instead");
}
