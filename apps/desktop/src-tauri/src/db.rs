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
