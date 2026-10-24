//! Upgrade simulation: a database written by the released schema (v6, what `main` ships) with realistic user data is
//! opened by this build. Migrations run, nothing is lost, the new features work on the old rows, and the app still runs
//! missions in the upgraded project.

use super::*;

/// SHA-256 of the first six migrations, exactly as `main` ships them. A migration that users already ran must never
/// change: an edit here would leave existing databases with a different schema at the same version number.
const SHIPPED_V6_SHA256: &str = "14d8fbe3b6a585371cb20f742035845bdbef5e4eb3523e72db08f32d1f244ab7";

#[test]
fn shipped_migrations_are_never_edited() {
    let joined: String = crate::db::MIGRATIONS[..6].concat();
    assert_eq!(crate::proof::sha256_hex(joined.as_bytes()), SHIPPED_V6_SHA256, "a migration already shipped in main was edited; add a new migration instead");
}

#[test]
fn a_database_from_the_released_schema_upgrades_keeps_its_data_and_keeps_working() {
    let repo = git::testutil::init_repo();
    let home = tempfile::tempdir().unwrap();
    let db_path = home.path().join("forge.db");
    let root = repo.path().to_string_lossy().into_owned();
    {
        // What a user of the released version has on disk.
        let c = rusqlite::Connection::open(&db_path).unwrap();
        c.pragma_update(None, "journal_mode", "WAL").unwrap();
        for (i, sql) in crate::db::MIGRATIONS[..6].iter().enumerate() {
            c.execute_batch(sql).unwrap();
            c.pragma_update(None, "user_version", (i + 1) as i64).unwrap();
        }
        c.execute_batch(&format!(
            "INSERT INTO project(id, name, root_path, default_branch, created_at) VALUES ('p1', 'shop', '{root}', 'main', '2025-11-01T10:00:00.000Z');
             INSERT INTO mission(id, project_id, title, objective, state, provider, permission_profile, created_at, updated_at)
               VALUES ('m-run', 'p1', 'Running when FORGE closed', 'x', 'running', 'claude', 'default', '2025-11-02T10:00:00.000Z', '2025-11-02T10:05:00.000Z'),
                      ('m-ask', 'p1', 'Waiting on a person', 'x', 'awaiting_approval', 'claude', 'default', '2025-11-02T11:00:00.000Z', '2025-11-02T11:01:00.000Z'),
                      ('m-old', 'p1', 'Merged long ago', 'x', 'merged', 'claude', 'accept_edits', '2025-01-02T11:00:00.000Z', '2025-01-03T11:00:00.000Z');
             INSERT INTO agent_session(id, mission_id, provider, state, started_at) VALUES
               ('s-run', 'm-run', 'claude', 'live', '2025-11-02T10:00:01.000Z'),
               -- FORGE crashed after ending this session but before expiring its pending request.
               ('s-ask', 'm-ask', 'claude', 'lost', '2025-11-02T11:00:01.000Z'),
               ('s-old', 'm-old', 'claude', 'ended', '2025-01-02T11:00:01.000Z');
             INSERT INTO activity_event(mission_id, session_id, seq, kind, payload_json, at) VALUES
               ('m-run', 's-run', 1, 'agent.text', '{{\"text\":\"working\"}}', '2025-11-02T10:01:00.000Z'),
               ('m-old', 's-old', 1, 'agent.text', '{{\"text\":\"old transcript\"}}', '2025-01-02T11:01:00.000Z'),
               ('m-old', 's-old', 2, 'turn.completed', '{{\"status\":\"success\"}}', '2025-01-02T11:02:00.000Z');
             INSERT INTO approval_request(id, mission_id, session_id, provider_request_id, tool, summary, detail_json, risk, state, created_at)
               VALUES ('a-ask', 'm-ask', 's-ask', 'tu1', 'Write', 'Write README.md', '{{}}', 'low', 'pending', '2025-11-02T11:00:30.000Z');"
        ))
        .unwrap();
    }

    let core = Core::new(
        Store::open(&db_path).unwrap(),
        Config { home: home.path().to_path_buf(), node: None, engine_script: Some(engine_script()), claude: None, codex: None, allow_fake: true, max_concurrent: 2, ollama_url: "http://127.0.0.1:11434".into(), engine_env: vec![] },
        Arc::new(|_| {}),
    );
    let v: i64 = core.store.c().query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(v as usize, crate::db::MIGRATIONS.len());

    // Old rows read through the new code, with the new columns at their documented defaults.
    let p = core.store.get_project("p1").unwrap();
    assert_eq!((p.execution_backend.as_str(), p.agent_backend.as_str()), ("trusted_local", "trusted_local"));
    assert!(core.store.sessions_for_mission("m-run").unwrap().iter().all(|s| s.executes_on == "agent_host"), "sessions before this version ran on the host");
    assert_eq!(core.store.events("m-old", 0, 100).unwrap().len(), 2);

    // Restart recovery on the old rows.
    core.recover().unwrap();
    assert_eq!(core.store.get_mission("m-run").unwrap().state, "interrupted");
    assert_eq!(core.store.get_approval("a-ask").unwrap().state, "expired", "a prompt from before the restart can never be answered, even if its session row was already closed");
    assert_eq!(core.store.get_mission("m-ask").unwrap().state, "interrupted");

    // New features on old data: storage clean-up of the old merged mission.
    let plan = core.storage_cleanup_preview(30).unwrap();
    assert_eq!(plan.missions.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), vec!["m-old"]);
    core.storage_cleanup_apply(30, &plan.token).unwrap();
    assert!(core.store.events("m-old", 0, 100).unwrap().iter().all(|e| e.payload["retention"] == true));

    // And the upgraded project still runs a mission end to end.
    let m = core.create_mission("p1", "add a scripted note", None, "fake", None, PermissionProfile::AcceptEdits).unwrap();
    core.start_mission(&m.id, None).unwrap();
    let t0 = Instant::now();
    while core.store.get_mission(&m.id).unwrap().state != "awaiting_review" {
        assert!(t0.elapsed() < Duration::from_secs(25), "stuck in {}", core.store.get_mission(&m.id).unwrap().state);
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(core.store.sessions_for_mission(&m.id).unwrap().iter().all(|s| s.executes_on == "agent_host"));
}
