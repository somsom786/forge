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

#[cfg(unix)]
fn core_with_env(env: Vec<(String, String)>) -> (Arc<Core>, tempfile::TempDir) {
    let home = tempfile::tempdir().unwrap();
    let core = Core::new(
        Store::in_memory(),
        Config { home: home.path().to_path_buf(), node: None, engine_script: Some(engine_script()), claude: None, codex: None, allow_fake: true, max_concurrent: 1, ollama_url: "http://127.0.0.1:11434".into(), engine_env: env },
        Arc::new(|_| {}),
    );
    (core, home)
}

#[cfg(target_os = "linux")]
#[test]
fn a_contained_engine_does_not_inherit_unrelated_environment_secrets() {
    if !sandbox_available() { return; }
    let state = tempfile::tempdir().unwrap();
    let (core, home) = core_with_env(vec![("FORGE_TEST_ENV_SECRET".into(), "fixture-only-secret".into()), ("CLAUDE_CONFIG_DIR".into(), state.path().to_string_lossy().into_owned())]);
    let repo = git::testutil::init_repo();
    let project = core.add_project(repo.path()).unwrap();
    let f = Fixture { core, repo, _home: home, project, events: Arc::new(StdMutex::new(vec![])) };
    let target = state.path().join("harmless.txt");
    let host = snoop(&f, &target);
    assert_eq!(host["fixtureSecretPresent"], true, "the host control sees the fixture secret");
    f.core.project_set_agent_backend(&f.project.id, "linux_namespaces").unwrap();
    let contained = snoop(&f, &target);
    assert_eq!(contained["fixtureSecretPresent"], false, "the contained engine must not inherit it");
    assert_eq!(contained["fixtureProviderStatePresent"], true, "provider state configuration survives");
}

/// Security review (agent containment): paths named by ACP/MCP definitions used to expose their parent folder, so an
/// argument like `$HOME/projects` exposed the whole home. Now: directories are never exposed, scripts by their folder,
/// other files only themselves, and anything too broad stops the agent from starting.
#[cfg(unix)]
#[test]
fn what_a_contained_agent_may_see_is_never_broader_than_its_programs() {
    let (core, _h) = core_with_env(vec![]);
    let wt = tempfile::tempdir().unwrap();
    let home = PathBuf::from(std::env::var("HOME").unwrap());
    let tools = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tools.path().join("srv")).unwrap();
    std::fs::write(tools.path().join("srv/server.mjs"), "").unwrap();
    std::fs::write(tools.path().join("config.json"), "{}").unwrap();
    let node = which::which("node").unwrap_or_else(|_| PathBuf::from("/usr/bin/node"));
    let mcp: serde_json::Map<String, Value> = serde_json::from_value(json!({
        "docs": {"command": node.to_string_lossy(), "args": [tools.path().join("srv/server.mjs"), tools.path().join("config.json"), home.join("projects"), home, "/home", "/"]}
    }))
    .unwrap();
    let paths = core.agent_read_only_paths(&node, &engine_script(), wt.path(), None, &mcp).unwrap();
    let canon = |p: PathBuf| dunce::canonicalize(&p).unwrap();
    assert!(paths.contains(&canon(tools.path().join("srv"))), "a script is exposed with its folder: {paths:?}");
    assert!(paths.contains(&canon(tools.path().join("config.json"))), "a data file only as itself: {paths:?}");
    let home_c = canon(PathBuf::from(std::env::var("HOME").unwrap()));
    for p in &paths {
        assert!(!home_c.starts_with(p), "{} exposes the home folder", p.display());
        assert!(p.components().count() > 2, "{} is too broad", p.display());
    }
    // A script whose folder is a top-level one is refused outright.
    let broad = tempfile::Builder::new().suffix(".js").tempfile_in("/tmp").unwrap();
    let mcp: serde_json::Map<String, Value> = serde_json::from_value(json!({"x": {"command": "node", "args": [broad.path()]}})).unwrap();
    let err = core.agent_read_only_paths(&node, &engine_script(), wt.path(), None, &mcp).unwrap_err().to_string();
    assert!(err.contains("top-level"), "{err}");
}

#[cfg(unix)]
#[test]
fn a_provider_state_folder_that_is_too_broad_or_relative_stops_a_contained_agent() {
    let wt = tempfile::tempdir().unwrap();
    for (dir, why) in [(std::env::var("HOME").unwrap(), "home"), ("relative/dir".to_string(), "absolute"), ("/".to_string(), "top-level")] {
        let (core, _h) = core_with_env(vec![("CLAUDE_CONFIG_DIR".into(), dir.clone())]);
        let err = core.agent_state_paths("claude", wt.path()).unwrap_err().to_string();
        // A home directly under the root (`/root`) is caught as a top-level folder first.
        assert!(err.contains(why) || (why == "home" && err.contains("top-level")), "{dir}: {err}");
    }
    let ok = tempfile::tempdir().unwrap();
    let (core, _h) = core_with_env(vec![("CLAUDE_CONFIG_DIR".into(), ok.path().to_string_lossy().into_owned())]);
    assert_eq!(core.agent_state_paths("claude", wt.path()).unwrap(), vec![dunce::canonicalize(ok.path()).unwrap()]);
}
