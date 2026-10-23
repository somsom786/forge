//! SQLite connection and forward-only migrations (tracked with PRAGMA user_version).

use crate::error::Result;
use rusqlite::Connection;
use std::path::Path;

const MIGRATIONS: &[&str] = &[
    // v1 — core orchestration
    r#"
    CREATE TABLE project (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        root_path TEXT NOT NULL UNIQUE,
        default_branch TEXT NOT NULL,
        created_at TEXT NOT NULL,
        archived_at TEXT
    );
    CREATE TABLE mission (
        id TEXT PRIMARY KEY,
        project_id TEXT NOT NULL REFERENCES project(id),
        title TEXT NOT NULL,
        objective TEXT NOT NULL,
        state TEXT NOT NULL,
        provider TEXT NOT NULL,
        model TEXT,
        permission_profile TEXT NOT NULL,
        base_ref TEXT,
        base_commit TEXT,
        branch TEXT,
        worktree_path TEXT,
        contract_json TEXT,
        provider_session_id TEXT,
        last_error TEXT,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL
    );
    CREATE INDEX mission_project ON mission(project_id, updated_at DESC);
    CREATE INDEX mission_state ON mission(state);
    CREATE TABLE mission_transition (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        mission_id TEXT NOT NULL REFERENCES mission(id),
        from_state TEXT NOT NULL,
        to_state TEXT NOT NULL,
        reason TEXT,
        at TEXT NOT NULL
    );
    CREATE INDEX transition_mission ON mission_transition(mission_id, id);
    CREATE TABLE agent_session (
        id TEXT PRIMARY KEY,
        mission_id TEXT NOT NULL REFERENCES mission(id),
        provider TEXT NOT NULL,
        provider_session_id TEXT,
        state TEXT NOT NULL,
        pid INTEGER,
        started_at TEXT NOT NULL,
        ended_at TEXT,
        end_reason TEXT,
        capabilities_json TEXT,
        usage_json TEXT
    );
    CREATE INDEX session_mission ON agent_session(mission_id, started_at);
    CREATE TABLE activity_event (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        mission_id TEXT NOT NULL REFERENCES mission(id),
        session_id TEXT NOT NULL REFERENCES agent_session(id),
        seq INTEGER NOT NULL,
        kind TEXT NOT NULL,
        payload_json TEXT NOT NULL,
        at TEXT NOT NULL,
        UNIQUE(session_id, seq)
    );
    CREATE INDEX activity_mission ON activity_event(mission_id, id);
    CREATE TABLE approval_request (
        id TEXT PRIMARY KEY,
        mission_id TEXT NOT NULL REFERENCES mission(id),
        session_id TEXT NOT NULL REFERENCES agent_session(id),
        provider_request_id TEXT NOT NULL,
        tool TEXT NOT NULL,
        summary TEXT NOT NULL,
        detail_json TEXT NOT NULL,
        risk TEXT NOT NULL,
        warnings_json TEXT NOT NULL DEFAULT '[]',
        state TEXT NOT NULL,
        decided_by TEXT,
        decision_note TEXT,
        created_at TEXT NOT NULL,
        decided_at TEXT,
        UNIQUE(session_id, provider_request_id)
    );
    CREATE INDEX approval_state ON approval_request(state, created_at);
    CREATE TABLE audit_event (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        at TEXT NOT NULL,
        actor TEXT NOT NULL,
        action TEXT NOT NULL,
        subject TEXT,
        detail_json TEXT
    );
    "#,
    // v2 — verification, review, proof
    r#"
    CREATE TABLE verification_run (
        id TEXT PRIMARY KEY,
        mission_id TEXT NOT NULL REFERENCES mission(id),
        check_id TEXT NOT NULL,
        name TEXT NOT NULL,
        command TEXT NOT NULL,
        kind TEXT NOT NULL,
        state TEXT NOT NULL,
        exit_code INTEGER,
        started_at TEXT NOT NULL,
        ended_at TEXT,
        duration_ms INTEGER,
        fingerprint TEXT NOT NULL,
        head_commit TEXT,
        mutated INTEGER NOT NULL DEFAULT 0,
        output_tail TEXT NOT NULL DEFAULT '',
        output_truncated INTEGER NOT NULL DEFAULT 0,
        log_path TEXT
    );
    CREATE INDEX verification_mission ON verification_run(mission_id, started_at);
    CREATE TABLE manual_verdict (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        mission_id TEXT NOT NULL REFERENCES mission(id),
        criterion_id TEXT NOT NULL,
        pass INTEGER NOT NULL,
        note TEXT,
        fingerprint TEXT NOT NULL,
        decided_by TEXT NOT NULL,
        at TEXT NOT NULL
    );
    CREATE INDEX verdict_mission ON manual_verdict(mission_id, criterion_id, id);
    CREATE TABLE review_decision (
        id TEXT PRIMARY KEY,
        mission_id TEXT NOT NULL REFERENCES mission(id),
        decision TEXT NOT NULL,
        note TEXT,
        override_reason TEXT,
        fingerprint TEXT NOT NULL,
        head_commit TEXT,
        decided_by TEXT NOT NULL,
        at TEXT NOT NULL
    );
    CREATE INDEX review_mission ON review_decision(mission_id, at);
    CREATE TABLE proof_pack (
        id TEXT PRIMARY KEY,
        mission_id TEXT NOT NULL REFERENCES mission(id),
        created_at TEXT NOT NULL,
        content_json TEXT NOT NULL,
        content_sha256 TEXT NOT NULL,
        markdown TEXT NOT NULL,
        diff_path TEXT
    );
    CREATE INDEX proof_mission ON proof_pack(mission_id, created_at);
    "#,
    // v3 — knowledge (memory): authority lives here, never in page frontmatter
    r#"
    ALTER TABLE project ADD COLUMN vault_path TEXT;
    ALTER TABLE project ADD COLUMN vault_write INTEGER NOT NULL DEFAULT 0;
    ALTER TABLE mission ADD COLUMN use_memory INTEGER NOT NULL DEFAULT 1;
    ALTER TABLE mission ADD COLUMN memory_pins TEXT;

    CREATE TABLE knowledge_page (
        id TEXT PRIMARY KEY,
        project_id TEXT NOT NULL REFERENCES project(id),
        root_kind TEXT NOT NULL,              -- managed | vault
        rel_path TEXT NOT NULL,               -- '/'-separated, relative to that root
        slug TEXT NOT NULL,
        title TEXT NOT NULL,
        kind TEXT NOT NULL,
        scope TEXT NOT NULL DEFAULT 'project',
        author TEXT NOT NULL,                 -- user | agent | system
        origin TEXT NOT NULL,                 -- forge | imported
        status TEXT NOT NULL,                 -- proposed | verified | disputed | superseded | archived  (AUTHORITATIVE)
        verified_sha256 TEXT,                 -- the exact revision a human verified
        verified_at TEXT,
        verified_note TEXT,
        current_sha256 TEXT NOT NULL,         -- normalised hash of the file as last indexed
        file_size INTEGER NOT NULL DEFAULT 0,
        file_mtime INTEGER NOT NULL DEFAULT 0,
        topic TEXT,
        supersedes_id TEXT,
        superseded_by_id TEXT,
        files_json TEXT NOT NULL DEFAULT '[]',
        tags_json TEXT NOT NULL DEFAULT '[]',
        warnings_json TEXT NOT NULL DEFAULT '[]',
        body_text TEXT NOT NULL DEFAULT '',
        missing INTEGER NOT NULL DEFAULT 0,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        indexed_at TEXT NOT NULL,
        UNIQUE(project_id, root_kind, rel_path)
    );
    CREATE INDEX knowledge_page_project ON knowledge_page(project_id, status);
    CREATE TABLE knowledge_review (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        page_id TEXT NOT NULL,
        project_id TEXT NOT NULL,
        action TEXT NOT NULL,                 -- created | content_changed | verify | dispute | archive | restore | supersede
        revision_sha256 TEXT,
        reviewer TEXT NOT NULL,
        note TEXT,
        at TEXT NOT NULL
    );
    CREATE INDEX knowledge_review_page ON knowledge_review(page_id, id);
    CREATE TABLE knowledge_source (
        id TEXT PRIMARY KEY,
        project_id TEXT NOT NULL REFERENCES project(id),
        kind TEXT NOT NULL,                   -- repo_file | commit | mission | check_run | user_note
        locator TEXT NOT NULL,                -- path | sha | mission id | run id | free text
        git_blob TEXT,                        -- blob id at capture for repo files
        raw_sha256 TEXT,
        norm_sha256 TEXT,
        snapshot TEXT,                        -- redacted, capped
        extractor TEXT NOT NULL DEFAULT 'forge-1',
        state TEXT NOT NULL DEFAULT 'current',-- current | changed | missing | unknown
        captured_at TEXT NOT NULL,
        checked_at TEXT
    );
    CREATE INDEX knowledge_source_project ON knowledge_source(project_id, kind, locator);
    CREATE TABLE knowledge_page_source (
        page_id TEXT NOT NULL,
        source_id TEXT NOT NULL,
        PRIMARY KEY (page_id, source_id)
    );
    CREATE TABLE knowledge_link (
        from_page TEXT NOT NULL,
        kind TEXT NOT NULL,                   -- wikilink | file
        target TEXT NOT NULL,
        to_page TEXT,
        ambiguous INTEGER NOT NULL DEFAULT 0,
        PRIMARY KEY (from_page, kind, target)
    );
    CREATE TABLE knowledge_revision (
        id TEXT PRIMARY KEY,
        page_id TEXT NOT NULL,
        base_sha256 TEXT NOT NULL,
        text TEXT NOT NULL,
        author TEXT NOT NULL,
        note TEXT,
        state TEXT NOT NULL,                  -- proposed | applied | rejected | conflict
        created_at TEXT NOT NULL,
        decided_at TEXT
    );
    CREATE TABLE knowledge_op (
        id TEXT PRIMARY KEY,
        page_id TEXT NOT NULL,
        kind TEXT NOT NULL,
        old_sha256 TEXT,
        new_sha256 TEXT,
        state TEXT NOT NULL,                  -- started | done | failed
        started_at TEXT NOT NULL,
        finished_at TEXT
    );
    CREATE TABLE context_package (
        id TEXT PRIMARY KEY,
        mission_id TEXT NOT NULL REFERENCES mission(id),
        created_at TEXT NOT NULL,
        query TEXT NOT NULL,
        params_json TEXT NOT NULL,
        items_json TEXT NOT NULL,
        omitted_json TEXT NOT NULL,
        injected_text TEXT NOT NULL,
        injected_sha256 TEXT NOT NULL,
        char_budget INTEGER NOT NULL
    );
    CREATE INDEX context_package_mission ON context_package(mission_id, created_at);
    CREATE VIRTUAL TABLE knowledge_fts USING fts5(
        page_id UNINDEXED, project_id UNINDEXED, title, body, files, tags,
        tokenize = 'unicode61 remove_diacritics 2'
    );
    "#,
    // v4 - evidence is bound to the definition it was gathered for
    r#"
    ALTER TABLE verification_run ADD COLUMN spec_rev TEXT NOT NULL DEFAULT '';
    ALTER TABLE manual_verdict ADD COLUMN criterion_rev TEXT NOT NULL DEFAULT '';
    "#,
    // v5 - Harness Studio: journal of edits to agent configuration files, and what the user last reviewed
    r#"
    CREATE TABLE harness_change (
        id TEXT PRIMARY KEY,
        project_id TEXT NOT NULL REFERENCES project(id),
        rel TEXT NOT NULL,
        kind TEXT NOT NULL,                   -- edit | create | rollback
        before_sha256 TEXT,                   -- NULL: the file did not exist
        after_sha256 TEXT,                    -- NULL: the file was removed
        backup_path TEXT,                     -- relative to the FORGE home; content before this change
        rolls_back_id TEXT REFERENCES harness_change(id),
        note TEXT,
        state TEXT NOT NULL,                  -- started | applied | failed
        created_at TEXT NOT NULL,
        finished_at TEXT
    );
    CREATE INDEX harness_change_project ON harness_change(project_id, rel, created_at);
    CREATE TABLE harness_review (
        project_id TEXT NOT NULL REFERENCES project(id),
        rel TEXT NOT NULL,
        sha256 TEXT NOT NULL,
        reviewed_at TEXT NOT NULL,
        PRIMARY KEY (project_id, rel)
    );
    "#,
    // v6 - a change can be rolled back at most once (defence in depth behind the lock and the content check)
    r#"
    CREATE UNIQUE INDEX harness_rollback_once ON harness_change(rolls_back_id)
        WHERE rolls_back_id IS NOT NULL AND state IN ('started', 'applied');
    "#,
    // v7 - experiments (Phase 6): a baseline and variants at exact commits, one command measured N times per revision
    r#"
    CREATE TABLE experiment (
        id TEXT PRIMARY KEY,
        project_id TEXT NOT NULL REFERENCES project(id),
        name TEXT NOT NULL,
        objective TEXT,
        command TEXT NOT NULL,
        correctness_command TEXT,
        metric_json TEXT NOT NULL,
        repetitions INTEGER NOT NULL,
        warmup INTEGER NOT NULL,
        timeout_secs INTEGER NOT NULL,
        state TEXT NOT NULL,
        attempt INTEGER NOT NULL DEFAULT 0,
        env_json TEXT,
        report_json TEXT,
        last_error TEXT,
        created_at TEXT NOT NULL,
        started_at TEXT,
        finished_at TEXT
    );
    CREATE INDEX experiment_project ON experiment(project_id, created_at DESC);
    CREATE TABLE experiment_variant (
        id TEXT PRIMARY KEY,
        experiment_id TEXT NOT NULL REFERENCES experiment(id),
        position INTEGER NOT NULL,
        label TEXT NOT NULL,
        is_baseline INTEGER NOT NULL,
        git_ref TEXT NOT NULL,
        commit_sha TEXT NOT NULL,
        UNIQUE (experiment_id, position),
        UNIQUE (experiment_id, label)
    );
    CREATE TABLE experiment_result (
        id TEXT PRIMARY KEY,
        experiment_id TEXT NOT NULL REFERENCES experiment(id),
        variant_id TEXT NOT NULL REFERENCES experiment_variant(id),
        attempt INTEGER NOT NULL,
        phase TEXT NOT NULL,
        iteration INTEGER NOT NULL,
        state TEXT NOT NULL,
        exit_code INTEGER,
        duration_ms INTEGER NOT NULL,
        metric_value REAL,
        worktree_changed INTEGER NOT NULL,
        output_tail TEXT NOT NULL,
        log_path TEXT,
        started_at TEXT NOT NULL
    );
    CREATE INDEX experiment_result_attempt ON experiment_result(experiment_id, attempt, variant_id);
    "#,
    // v8 - execution backends: where a project's checks and experiments run, and where each run actually ran.
    // Existing projects keep running exactly as before ('trusted_local'); nothing is silently re-routed.
    r#"
    ALTER TABLE project ADD COLUMN execution_backend TEXT NOT NULL DEFAULT 'trusted_local';
    ALTER TABLE verification_run ADD COLUMN backend TEXT NOT NULL DEFAULT 'trusted_local';
    ALTER TABLE experiment_result ADD COLUMN backend TEXT NOT NULL DEFAULT 'trusted_local';
    "#,
    // v9 - approval binding: what exactly a human approves (digest of mission, session, provider, tool, canonical input,
    // worktree, permission profile, policy revision, where it executes) and until when. Older rows keep NULLs: they belong
    // to sessions that have ended and can no longer be decided.
    r#"
    ALTER TABLE approval_request ADD COLUMN binding_sha256 TEXT;
    ALTER TABLE approval_request ADD COLUMN policy_rev TEXT;
    ALTER TABLE approval_request ADD COLUMN executes_on TEXT;
    ALTER TABLE approval_request ADD COLUMN expires_at TEXT;
    "#,
    // v10 - MCP authorization: the exact server definitions a user allowed for a project's missions.
    r#"
    CREATE TABLE mcp_authorization (
        id TEXT PRIMARY KEY,
        project_id TEXT NOT NULL REFERENCES project(id),
        name TEXT NOT NULL,
        definition_json TEXT NOT NULL,
        definition_sha256 TEXT NOT NULL,
        tool_allowlist_json TEXT NOT NULL DEFAULT '[]',
        authorized_at TEXT NOT NULL,
        revoked_at TEXT
    );
    CREATE UNIQUE INDEX mcp_authorization_active ON mcp_authorization(project_id, name) WHERE revoked_at IS NULL;
    "#,
    // v11 - ACP agents the user registered (command pinned to an absolute path).
    r#"
    CREATE TABLE acp_agent (
        id TEXT PRIMARY KEY,
        name TEXT NOT NULL,
        command TEXT NOT NULL,
        args_json TEXT NOT NULL DEFAULT '[]',
        added_at TEXT NOT NULL,
        removed_at TEXT
    );
    CREATE UNIQUE INDEX acp_agent_active ON acp_agent(name) WHERE removed_at IS NULL;
    "#,
    // v12 - structured evidence from verification adapters (crate::evidence), JSON; NULL for plain checks.
    r#"
    ALTER TABLE verification_run ADD COLUMN evidence_json TEXT;
    "#,
    // v13 - a durable record of each merge FORGE starts, written before git runs, so a crash between git and the database
    // can be reconciled from the real repository (and squash merges know which branch commit they contained).
    r#"
    CREATE TABLE mission_merge (
        mission_id TEXT PRIMARY KEY REFERENCES mission(id),
        target_oid TEXT NOT NULL,
        branch_oid TEXT NOT NULL,
        predicted_tree TEXT NOT NULL,
        squash INTEGER NOT NULL,
        message TEXT NOT NULL,
        started_at TEXT NOT NULL,
        commit_oid TEXT,
        finished_at TEXT
    );
    "#,
];

pub fn open(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(path)?;
    configure(&conn)?;
    migrate(&conn)?;
    Ok(conn)
}

pub fn open_in_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    configure(&conn)?;
    migrate(&conn)?;
    Ok(conn)
}

fn configure(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.busy_timeout(std::time::Duration::from_millis(5000))?;
    Ok(())
}

pub fn migrate(conn: &Connection) -> Result<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", (i + 1) as i64)?;
        tx.commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_apply_and_are_idempotent() {
        let c = open_in_memory().unwrap();
        migrate(&c).unwrap();
        let v: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v as usize, MIGRATIONS.len());
    }

    #[test]
    fn migrations_apply_from_scratch_and_forward_from_every_earlier_version() {
        let tables = |c: &Connection| -> Vec<String> {
            let mut s = c.prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name").unwrap();
            s.query_map([], |r| r.get(0)).unwrap().collect::<rusqlite::Result<Vec<String>>>().unwrap()
        };
        let fresh = open_in_memory().unwrap();
        let want = tables(&fresh);
        for t in ["experiment", "experiment_variant", "experiment_result"] {
            assert!(want.iter().any(|x| x == t), "{t} missing from a fresh database: {want:?}");
        }
        // A database created by any earlier release (user_version k) upgrades to exactly the same schema, keeping its data.
        for k in 0..MIGRATIONS.len() {
            let c = Connection::open_in_memory().unwrap();
            configure(&c).unwrap();
            for (i, sql) in MIGRATIONS.iter().enumerate().take(k) {
                c.execute_batch(sql).unwrap();
                c.pragma_update(None, "user_version", (i + 1) as i64).unwrap();
            }
            if k >= 1 {
                c.execute("INSERT INTO project(id, name, root_path, default_branch, created_at) VALUES ('p','p','/p','main','t')", []).unwrap();
            }
            migrate(&c).unwrap();
            assert_eq!(tables(&c), want, "upgrading from v{k}");
            let v: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
            assert_eq!(v as usize, MIGRATIONS.len());
            if k >= 1 {
                let n: i64 = c.query_row("SELECT count(*) FROM project", [], |r| r.get(0)).unwrap();
                assert_eq!(n, 1, "data survives the upgrade from v{k}");
            }
        }
        // Columns match between a fresh and an upgraded database.
        let cols = |c: &Connection, t: &str| -> Vec<String> {
            let mut s = c.prepare(&format!("PRAGMA table_info({t})")).unwrap();
            s.query_map([], |r| r.get::<_, String>(1)).unwrap().collect::<rusqlite::Result<Vec<_>>>().unwrap()
        };
        let up = Connection::open_in_memory().unwrap();
        configure(&up).unwrap();
        for (i, sql) in MIGRATIONS.iter().enumerate().take(6) {
            up.execute_batch(sql).unwrap();
            up.pragma_update(None, "user_version", (i + 1) as i64).unwrap();
        }
        migrate(&up).unwrap();
        for t in ["experiment", "experiment_variant", "experiment_result"] {
            assert_eq!(cols(&up, t), cols(&fresh, t), "{t}");
        }
    }

    #[test]
    fn fts5_is_available_for_the_memory_engine() {
        let c = open_in_memory().unwrap();
        c.execute_batch(
            "CREATE VIRTUAL TABLE t USING fts5(title, body, tokenize='porter unicode61');
             INSERT INTO t VALUES ('Auth', 'password reset tokens expire after one hour');",
        )
        .unwrap();
        let n: i64 = c
            .query_row("SELECT count(*) FROM t WHERE t MATCH 'expiring OR expire*'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
    }
}
