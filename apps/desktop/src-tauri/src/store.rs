//! Persistence operations. All state transitions go through here so the transition
//! table in `mission.rs` is enforced and every change is recorded before anything
//! is reported to the UI.

use crate::error::{ForgeError, Result};
use crate::mission::{self, MissionState};
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::Serialize;
use std::path::Path;
use std::sync::Mutex;

pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

pub fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

pub struct Store {
    conn: Mutex<Connection>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub root_path: String,
    pub default_branch: String,
    pub created_at: String,
    /// Where this project's checks and experiments run (see `execution.rs`).
    pub execution_backend: String,
    /// Where this project's agent sessions run (the engine and the provider it starts).
    pub agent_backend: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Mission {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub objective: String,
    pub state: String,
    pub provider: String,
    pub model: Option<String>,
    pub permission_profile: String,
    pub base_ref: Option<String>,
    pub base_commit: Option<String>,
    pub branch: Option<String>,
    pub worktree_path: Option<String>,
    pub provider_session_id: Option<String>,
    pub last_error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    /// Retrieve verified project knowledge into the first prompt (default on).
    pub use_memory: bool,
    /// Pages the user explicitly chose to include even if unverified.
    pub memory_pins: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentSession {
    pub id: String,
    pub mission_id: String,
    pub provider: String,
    pub provider_session_id: Option<String>,
    pub state: String,
    pub pid: Option<i64>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub end_reason: Option<String>,
    pub usage: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ActivityEvent {
    pub id: i64,
    pub mission_id: String,
    pub session_id: String,
    pub seq: i64,
    pub kind: String,
    pub payload: serde_json::Value,
    pub at: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Approval {
    pub id: String,
    pub mission_id: String,
    pub session_id: String,
    pub provider_request_id: String,
    pub tool: String,
    pub summary: String,
    pub detail: serde_json::Value,
    pub risk: String,
    pub warnings: Vec<String>,
    pub state: String,
    pub decided_by: Option<String>,
    pub decision_note: Option<String>,
    pub created_at: String,
    pub decided_at: Option<String>,
    /// Digest of everything the decision is about (see `supervisor::approval_binding`). The UI echoes it back when deciding.
    pub binding_sha256: Option<String>,
    pub policy_rev: Option<String>,
    /// Where the approved action would execute (agents' tool calls run on the host today: `agent_host`).
    pub executes_on: Option<String>,
    /// After this, the request can no longer be approved.
    pub expires_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Transition {
    pub from_state: String,
    pub to_state: String,
    pub reason: Option<String>,
    pub at: String,
}

pub struct NewMission<'a> {
    pub project_id: &'a str,
    pub title: &'a str,
    pub objective: &'a str,
    pub provider: &'a str,
    pub model: Option<&'a str>,
    pub permission_profile: &'a str,
}

const MISSION_COLS: &str = "id, project_id, title, objective, state, provider, model, permission_profile, base_ref, base_commit, branch, worktree_path, provider_session_id, last_error, created_at, updated_at, use_memory, memory_pins";

fn mission_from(r: &Row) -> rusqlite::Result<Mission> {
    Ok(Mission {
        id: r.get(0)?,
        project_id: r.get(1)?,
        title: r.get(2)?,
        objective: r.get(3)?,
        state: r.get(4)?,
        provider: r.get(5)?,
        model: r.get(6)?,
        permission_profile: r.get(7)?,
        base_ref: r.get(8)?,
        base_commit: r.get(9)?,
        branch: r.get(10)?,
        worktree_path: r.get(11)?,
        provider_session_id: r.get(12)?,
        last_error: r.get(13)?,
        created_at: r.get(14)?,
        updated_at: r.get(15)?,
        use_memory: r.get::<_, i64>(16)? != 0,
        memory_pins: r.get::<_, Option<String>>(17)?.and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default(),
    })
}

const APPROVAL_COLS: &str = "id, mission_id, session_id, provider_request_id, tool, summary, detail_json, risk, warnings_json, state, decided_by, decision_note, created_at, decided_at, binding_sha256, policy_rev, executes_on, expires_at";

fn approval_from(r: &Row) -> rusqlite::Result<Approval> {
    let detail: String = r.get(6)?;
    let warnings: String = r.get(8)?;
    Ok(Approval {
        id: r.get(0)?,
        mission_id: r.get(1)?,
        session_id: r.get(2)?,
        provider_request_id: r.get(3)?,
        tool: r.get(4)?,
        summary: r.get(5)?,
        detail: serde_json::from_str(&detail).unwrap_or(serde_json::Value::Null),
        risk: r.get(7)?,
        warnings: serde_json::from_str(&warnings).unwrap_or_default(),
        state: r.get(9)?,
        decided_by: r.get(10)?,
        decision_note: r.get(11)?,
        created_at: r.get(12)?,
        decided_at: r.get(13)?,
        binding_sha256: r.get(14)?,
        policy_rev: r.get(15)?,
        executes_on: r.get(16)?,
        expires_at: r.get(17)?,
    })
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        Ok(Self { conn: Mutex::new(crate::db::open(path)?) })
    }

    #[cfg(test)]
    pub fn in_memory() -> Self {
        Self { conn: Mutex::new(crate::db::open_in_memory().unwrap()) }
    }

    pub(crate) fn c(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|p| p.into_inner())
    }

    // ---- audit -------------------------------------------------------------------

    pub fn audit(&self, actor: &str, action: &str, subject: Option<&str>, detail: &serde_json::Value) {
        // Audit failures must never take down the operation being audited, but they are loud.
        if let Err(e) = self.c().execute(
            "INSERT INTO audit_event(at, actor, action, subject, detail_json) VALUES (?1,?2,?3,?4,?5)",
            params![now(), actor, action, subject, detail.to_string()],
        ) {
            eprintln!("[forge] AUDIT WRITE FAILED ({action}): {e}");
        }
    }

    pub fn audit_count(&self, action: &str) -> Result<i64> {
        Ok(self.c().query_row("SELECT count(*) FROM audit_event WHERE action=?1", [action], |r| r.get(0))?)
    }

    // ---- projects ----------------------------------------------------------------

    pub fn add_project(&self, name: &str, root: &str, default_branch: &str) -> Result<Project> {
        let c = self.c();
        if let Some(p) = c
            .query_row("SELECT id, name, root_path, default_branch, created_at, execution_backend, agent_backend FROM project WHERE root_path=?1", [root], project_from)
            .optional()?
        {
            // Re-adding an archived or existing project returns it rather than duplicating.
            c.execute("UPDATE project SET archived_at=NULL WHERE id=?1", [&p.id])?;
            return Ok(p);
        }
        let p = Project { id: new_id(), name: name.into(), root_path: root.into(), default_branch: default_branch.into(), created_at: now(), execution_backend: "trusted_local".into(), agent_backend: "trusted_local".into() };
        c.execute(
            "INSERT INTO project(id,name,root_path,default_branch,created_at) VALUES (?1,?2,?3,?4,?5)",
            params![p.id, p.name, p.root_path, p.default_branch, p.created_at],
        )?;
        Ok(p)
    }

    pub fn list_projects(&self) -> Result<Vec<Project>> {
        let c = self.c();
        let mut s = c.prepare("SELECT id, name, root_path, default_branch, created_at, execution_backend, agent_backend FROM project WHERE archived_at IS NULL ORDER BY name COLLATE NOCASE")?;
        let rows = s.query_map([], project_from)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn set_project_agent_backend(&self, id: &str, backend: &str) -> Result<()> {
        let n = self.c().execute("UPDATE project SET agent_backend=?2 WHERE id=?1", params![id, backend])?;
        if n == 0 {
            return Err(ForgeError::NotFound(format!("project {id}")));
        }
        Ok(())
    }

    pub fn set_project_backend(&self, id: &str, backend: &str) -> Result<()> {
        let n = self.c().execute("UPDATE project SET execution_backend=?2 WHERE id=?1", params![id, backend])?;
        if n == 0 {
            return Err(ForgeError::NotFound(format!("project {id}")));
        }
        Ok(())
    }

    pub fn get_project(&self, id: &str) -> Result<Project> {
        self.c()
            .query_row("SELECT id, name, root_path, default_branch, created_at, execution_backend, agent_backend FROM project WHERE id=?1", [id], project_from)
            .optional()?
            .ok_or_else(|| ForgeError::NotFound(format!("project {id}")))
    }

    pub fn archive_project(&self, id: &str) -> Result<()> {
        let n = self.c().execute("UPDATE project SET archived_at=?2 WHERE id=?1", params![id, now()])?;
        if n == 0 {
            return Err(ForgeError::NotFound(format!("project {id}")));
        }
        Ok(())
    }

    // ---- missions ----------------------------------------------------------------

    pub fn create_mission(&self, m: &NewMission) -> Result<Mission> {
        let id = new_id();
        let t = now();
        let c = self.c();
        c.execute(
            "INSERT INTO mission(id,project_id,title,objective,state,provider,model,permission_profile,created_at,updated_at)
             VALUES (?1,?2,?3,?4,'ready',?5,?6,?7,?8,?8)",
            params![id, m.project_id, m.title, m.objective, m.provider, m.model, m.permission_profile, t],
        )?;
        c.execute(
            "INSERT INTO mission_transition(mission_id, from_state, to_state, reason, at) VALUES (?1,'draft','ready','created',?2)",
            params![id, t],
        )?;
        drop(c);
        self.get_mission(&id)
    }

    pub fn get_mission(&self, id: &str) -> Result<Mission> {
        self.c()
            .query_row(&format!("SELECT {MISSION_COLS} FROM mission WHERE id=?1"), [id], mission_from)
            .optional()?
            .ok_or_else(|| ForgeError::NotFound(format!("mission {id}")))
    }

    pub fn list_missions(&self, project_id: Option<&str>) -> Result<Vec<Mission>> {
        let c = self.c();
        let sql = format!(
            "SELECT {MISSION_COLS} FROM mission WHERE (?1 IS NULL OR project_id=?1) ORDER BY updated_at DESC LIMIT 500"
        );
        let mut s = c.prepare(&sql)?;
        Ok(s.query_map([project_id], mission_from)?.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Every mission in one of `states`, however old (recovery must not miss a mission beyond any listing limit).
    pub fn missions_in_states(&self, states: &[MissionState]) -> Result<Vec<Mission>> {
        let names: Vec<&str> = states.iter().map(|s| s.as_str()).collect();
        let placeholders = vec!["?"; names.len()].join(",");
        let c = self.c();
        let mut s = c.prepare(&format!("SELECT {MISSION_COLS} FROM mission WHERE state IN ({placeholders}) ORDER BY updated_at DESC"))?;
        Ok(s.query_map(rusqlite::params_from_iter(names), mission_from)?.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Enforce the transition table and record the change atomically.
    pub fn transition(&self, id: &str, to: MissionState, reason: &str) -> Result<Mission> {
        {
            let mut c = self.c();
            let tx = c.transaction()?;
            let cur: String = tx
                .query_row("SELECT state FROM mission WHERE id=?1", [id], |r| r.get(0))
                .optional()?
                .ok_or_else(|| ForgeError::NotFound(format!("mission {id}")))?;
            let from = MissionState::parse(&cur).ok_or_else(|| ForgeError::InvalidState(format!("unknown state {cur}")))?;
            if from == to {
                return {
                    drop(tx);
                    drop(c);
                    self.get_mission(id)
                };
            }
            if !mission::can_transition(from, to) {
                return Err(ForgeError::InvalidState(format!("mission {id}: {} -> {} is not allowed", from.as_str(), to.as_str())));
            }
            let t = now();
            tx.execute("UPDATE mission SET state=?2, updated_at=?3 WHERE id=?1", params![id, to.as_str(), t])?;
            tx.execute(
                "INSERT INTO mission_transition(mission_id, from_state, to_state, reason, at) VALUES (?1,?2,?3,?4,?5)",
                params![id, from.as_str(), to.as_str(), reason, t],
            )?;
            tx.commit()?;
        }
        self.get_mission(id)
    }

    pub fn set_mission_workspace(&self, id: &str, base_ref: &str, base_commit: &str, branch: &str, worktree: &str) -> Result<()> {
        self.c().execute(
            "UPDATE mission SET base_ref=?2, base_commit=?3, branch=?4, worktree_path=?5, updated_at=?6 WHERE id=?1",
            params![id, base_ref, base_commit, branch, worktree, now()],
        )?;
        Ok(())
    }

    pub fn set_mission_memory(&self, id: &str, use_memory: bool, pins: &[String]) -> Result<()> {
        let n = self.c().execute("UPDATE mission SET use_memory=?2, memory_pins=?3, updated_at=?4 WHERE id=?1", params![id, use_memory as i64, serde_json::to_string(pins)?, now()])?;
        if n == 0 {
            return Err(ForgeError::NotFound(format!("mission {id}")));
        }
        Ok(())
    }

    pub fn set_mission_error(&self, id: &str, err: Option<&str>) -> Result<()> {
        self.c().execute("UPDATE mission SET last_error=?2, updated_at=?3 WHERE id=?1", params![id, err, now()])?;
        Ok(())
    }

    pub fn set_mission_provider_session(&self, id: &str, provider_session_id: &str) -> Result<()> {
        self.c().execute(
            "UPDATE mission SET provider_session_id=?2 WHERE id=?1",
            params![id, provider_session_id],
        )?;
        Ok(())
    }

    pub fn transitions(&self, mission_id: &str) -> Result<Vec<Transition>> {
        let c = self.c();
        let mut s = c.prepare("SELECT from_state, to_state, reason, at FROM mission_transition WHERE mission_id=?1 ORDER BY id")?;
        Ok(s.query_map([mission_id], |r| {
            Ok(Transition { from_state: r.get(0)?, to_state: r.get(1)?, reason: r.get(2)?, at: r.get(3)? })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    // ---- sessions ----------------------------------------------------------------

    pub fn create_session(&self, mission_id: &str, provider: &str, resume_from: Option<&str>) -> Result<AgentSession> {
        let id = new_id();
        self.c().execute(
            "INSERT INTO agent_session(id, mission_id, provider, provider_session_id, state, started_at) VALUES (?1,?2,?3,?4,'starting',?5)",
            params![id, mission_id, provider, resume_from, now()],
        )?;
        self.get_session(&id)
    }

    pub fn get_session(&self, id: &str) -> Result<AgentSession> {
        self.c()
            .query_row(SESSION_SQL_ONE, [id], session_from)
            .optional()?
            .ok_or_else(|| ForgeError::NotFound(format!("session {id}")))
    }

    pub fn sessions_for_mission(&self, mission_id: &str) -> Result<Vec<AgentSession>> {
        let c = self.c();
        let mut s = c.prepare(&format!("{SESSION_SELECT} WHERE mission_id=?1 ORDER BY started_at"))?;
        Ok(s.query_map([mission_id], session_from)?.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Sessions started at or after `since` (RFC 3339), newest first, with their mission's title and project.
    pub fn sessions_since(&self, since: &str, limit: i64) -> Result<Vec<(AgentSession, String, String)>> {
        let c = self.c();
        let mut s = c.prepare(
            "SELECT s.id, s.mission_id, s.provider, s.provider_session_id, s.state, s.pid, s.started_at, s.ended_at, s.end_reason, s.usage_json, m.title, m.project_id \
             FROM agent_session s JOIN mission m ON m.id = s.mission_id WHERE s.started_at >= ?1 OR s.state IN ('starting','live') ORDER BY s.started_at DESC LIMIT ?2",
        )?;
        let rows = s.query_map(params![since, limit], |r| Ok((session_from(r)?, r.get::<_, String>(10)?, r.get::<_, String>(11)?)))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn live_sessions(&self) -> Result<Vec<AgentSession>> {
        let c = self.c();
        let mut s = c.prepare(&format!("{SESSION_SELECT} WHERE state IN ('starting','live')"))?;
        Ok(s.query_map([], session_from)?.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn session_live(&self, id: &str, pid: Option<u32>, provider_session_id: Option<&str>, capabilities: &serde_json::Value) -> Result<()> {
        self.c().execute(
            "UPDATE agent_session SET state='live', pid=?2, provider_session_id=COALESCE(?3, provider_session_id), capabilities_json=?4 WHERE id=?1",
            params![id, pid, provider_session_id, capabilities.to_string()],
        )?;
        Ok(())
    }

    pub fn session_provider_id(&self, id: &str, provider_session_id: &str) -> Result<()> {
        self.c().execute(
            "UPDATE agent_session SET provider_session_id=?2 WHERE id=?1",
            params![id, provider_session_id],
        )?;
        Ok(())
    }

    pub fn session_usage(&self, id: &str, usage: &serde_json::Value) -> Result<()> {
        self.c().execute("UPDATE agent_session SET usage_json=?2 WHERE id=?1", params![id, usage.to_string()])?;
        Ok(())
    }

    /// Mark a session ended. `state` is `ended` for an orderly end, `lost` when the process vanished.
    pub fn end_session(&self, id: &str, state: &str, reason: &str) -> Result<()> {
        self.c().execute(
            "UPDATE agent_session SET state=?2, ended_at=?3, end_reason=?4 WHERE id=?1 AND state IN ('starting','live')",
            params![id, state, now(), reason],
        )?;
        Ok(())
    }

    // ---- events ------------------------------------------------------------------

    /// Insert idempotently on (session_id, seq). Returns the stored event, or None for a duplicate.
    pub fn insert_event(&self, mission_id: &str, session_id: &str, seq: i64, kind: &str, payload: &serde_json::Value) -> Result<Option<ActivityEvent>> {
        let t = now();
        let c = self.c();
        let n = c.execute(
            "INSERT OR IGNORE INTO activity_event(mission_id, session_id, seq, kind, payload_json, at) VALUES (?1,?2,?3,?4,?5,?6)",
            params![mission_id, session_id, seq, kind, payload.to_string(), t],
        )?;
        if n == 0 {
            return Ok(None);
        }
        let id = c.last_insert_rowid();
        Ok(Some(ActivityEvent { id, mission_id: mission_id.into(), session_id: session_id.into(), seq, kind: kind.into(), payload: payload.clone(), at: t }))
    }

    /// Events for a mission in insertion order after `after_id` (exclusive).
    pub fn events(&self, mission_id: &str, after_id: i64, limit: i64) -> Result<Vec<ActivityEvent>> {
        let c = self.c();
        let mut s = c.prepare(
            "SELECT id, mission_id, session_id, seq, kind, payload_json, at FROM activity_event WHERE mission_id=?1 AND id>?2 ORDER BY id LIMIT ?3",
        )?;
        Ok(s.query_map(params![mission_id, after_id, limit], |r| {
            let p: String = r.get(5)?;
            Ok(ActivityEvent {
                id: r.get(0)?,
                mission_id: r.get(1)?,
                session_id: r.get(2)?,
                seq: r.get(3)?,
                kind: r.get(4)?,
                payload: serde_json::from_str(&p).unwrap_or(serde_json::Value::Null),
                at: r.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn max_seq(&self, session_id: &str) -> Result<i64> {
        Ok(self.c().query_row("SELECT COALESCE(MAX(seq),0) FROM activity_event WHERE session_id=?1", [session_id], |r| r.get(0))?)
    }

    // ---- approvals ---------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    pub fn create_approval(&self, mission_id: &str, session_id: &str, provider_request_id: &str, tool: &str, summary: &str, detail: &serde_json::Value, risk: &str, warnings: &[String], state: &str, decided_by: Option<&str>, note: Option<&str>) -> Result<Approval> {
        let id = new_id();
        let t = now();
        let decided_at = if state == "pending" { None } else { Some(t.clone()) };
        self.c().execute(
            "INSERT INTO approval_request(id,mission_id,session_id,provider_request_id,tool,summary,detail_json,risk,warnings_json,state,decided_by,decision_note,created_at,decided_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
            params![id, mission_id, session_id, provider_request_id, tool, summary, detail.to_string(), risk, serde_json::to_string(warnings)?, state, decided_by, note, t, decided_at],
        )?;
        self.get_approval(&id)
    }

    /// Record what a still-pending request is bound to. Written once, under the session's lifecycle lock, before anyone can see it.
    pub fn bind_approval(&self, id: &str, binding: &str, policy_rev: &str, executes_on: &str, expires_at: &str) -> Result<Approval> {
        let n = self.c().execute(
            "UPDATE approval_request SET binding_sha256=?2, policy_rev=?3, executes_on=?4, expires_at=?5 WHERE id=?1 AND binding_sha256 IS NULL",
            params![id, binding, policy_rev, executes_on, expires_at],
        )?;
        if n != 1 {
            return Err(ForgeError::InvalidState(format!("approval {id} is already bound")));
        }
        self.get_approval(id)
    }

    pub fn get_approval(&self, id: &str) -> Result<Approval> {
        self.c()
            .query_row(&format!("SELECT {APPROVAL_COLS} FROM approval_request WHERE id=?1"), [id], approval_from)
            .optional()?
            .ok_or_else(|| ForgeError::NotFound(format!("approval {id}")))
    }

    pub fn approval_by_provider_request(&self, session_id: &str, provider_request_id: &str) -> Result<Option<Approval>> {
        Ok(self
            .c()
            .query_row(
                &format!("SELECT {APPROVAL_COLS} FROM approval_request WHERE session_id=?1 AND provider_request_id=?2"),
                params![session_id, provider_request_id],
                approval_from,
            )
            .optional()?)
    }

    pub fn pending_approvals(&self, mission_id: Option<&str>) -> Result<Vec<Approval>> {
        let c = self.c();
        let mut s = c.prepare(&format!(
            "SELECT {APPROVAL_COLS} FROM approval_request WHERE state='pending' AND (?1 IS NULL OR mission_id=?1) ORDER BY created_at"
        ))?;
        Ok(s.query_map([mission_id], approval_from)?.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn approvals_for_mission(&self, mission_id: &str) -> Result<Vec<Approval>> {
        let c = self.c();
        let mut s = c.prepare(&format!("SELECT {APPROVAL_COLS} FROM approval_request WHERE mission_id=?1 ORDER BY created_at"))?;
        Ok(s.query_map([mission_id], approval_from)?.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Single-shot decision: only a still-pending request can be decided, exactly once.
    pub fn decide_approval(&self, id: &str, approved: bool, decided_by: &str, note: Option<&str>) -> Result<Approval> {
        let n = self.c().execute(
            "UPDATE approval_request SET state=?2, decided_by=?3, decision_note=?4, decided_at=?5 WHERE id=?1 AND state='pending'",
            params![id, if approved { "approved" } else { "denied" }, decided_by, note, now()],
        )?;
        if n == 0 {
            let cur = self.get_approval(id)?;
            return Err(ForgeError::InvalidState(format!("approval {id} is already {}", cur.state)));
        }
        self.get_approval(id)
    }

    /// Expire every still-pending approval of a session (it can no longer be answered).
    pub fn expire_pending_for_session(&self, session_id: &str, reason: &str) -> Result<usize> {
        Ok(self.c().execute(
            "UPDATE approval_request SET state='expired', decision_note=?2, decided_at=?3 WHERE session_id=?1 AND state='pending'",
            params![session_id, reason, now()],
        )?)
    }

    /// Withdraw a single still-pending request (the provider cancelled the tool call it was for).
    pub fn expire_approval(&self, id: &str, reason: &str) -> Result<Option<Approval>> {
        let n = self.c().execute(
            "UPDATE approval_request SET state='expired', decision_note=?2, decided_at=?3 WHERE id=?1 AND state='pending'",
            params![id, reason, now()],
        )?;
        if n == 0 { Ok(None) } else { Ok(Some(self.get_approval(id)?)) }
    }

    pub fn pending_count_for_mission(&self, mission_id: &str) -> Result<i64> {
        Ok(self.c().query_row("SELECT count(*) FROM approval_request WHERE mission_id=?1 AND state='pending'", [mission_id], |r| r.get(0))?)
    }
}

fn project_from(r: &Row) -> rusqlite::Result<Project> {
    Ok(Project { id: r.get(0)?, name: r.get(1)?, root_path: r.get(2)?, default_branch: r.get(3)?, created_at: r.get(4)?, execution_backend: r.get(5)?, agent_backend: r.get(6)? })
}

const SESSION_SELECT: &str = "SELECT id, mission_id, provider, provider_session_id, state, pid, started_at, ended_at, end_reason, usage_json FROM agent_session";
const SESSION_SQL_ONE: &str = "SELECT id, mission_id, provider, provider_session_id, state, pid, started_at, ended_at, end_reason, usage_json FROM agent_session WHERE id=?1";

fn session_from(r: &Row) -> rusqlite::Result<AgentSession> {
    let usage: Option<String> = r.get(9)?;
    Ok(AgentSession {
        id: r.get(0)?,
        mission_id: r.get(1)?,
        provider: r.get(2)?,
        provider_session_id: r.get(3)?,
        state: r.get(4)?,
        pid: r.get(5)?,
        started_at: r.get(6)?,
        ended_at: r.get(7)?,
        end_reason: r.get(8)?,
        usage: usage.and_then(|u| serde_json::from_str(&u).ok()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn seeded() -> (Store, Project, Mission) {
        let s = Store::in_memory();
        let p = s.add_project("demo", "C:/demo", "main").unwrap();
        let m = s
            .create_mission(&NewMission { project_id: &p.id, title: "t", objective: "do it", provider: "claude", model: None, permission_profile: "default" })
            .unwrap();
        (s, p, m)
    }

    #[test]
    fn mission_starts_ready_and_records_history() {
        let (s, _, m) = seeded();
        assert_eq!(m.state, "ready");
        let t = s.transitions(&m.id).unwrap();
        assert_eq!(t.len(), 1);
        assert_eq!((t[0].from_state.as_str(), t[0].to_state.as_str()), ("draft", "ready"));
    }

    #[test]
    fn illegal_transitions_are_rejected_and_not_recorded() {
        let (s, _, m) = seeded();
        let err = s.transition(&m.id, MissionState::Merged, "x").unwrap_err().to_string();
        assert!(err.contains("not allowed"), "{err}");
        assert_eq!(s.get_mission(&m.id).unwrap().state, "ready");
        assert_eq!(s.transitions(&m.id).unwrap().len(), 1);
        // same-state is an idempotent no-op
        assert_eq!(s.transition(&m.id, MissionState::Ready, "again").unwrap().state, "ready");
        assert_eq!(s.transitions(&m.id).unwrap().len(), 1);
    }

    #[test]
    fn duplicate_and_out_of_order_events_are_safe() {
        let (s, _, m) = seeded();
        let sess = s.create_session(&m.id, "claude", None).unwrap();
        assert!(s.insert_event(&m.id, &sess.id, 2, "agent.text", &json!({"t":"b"})).unwrap().is_some());
        assert!(s.insert_event(&m.id, &sess.id, 1, "agent.text", &json!({"t":"a"})).unwrap().is_some());
        assert!(s.insert_event(&m.id, &sess.id, 2, "agent.text", &json!({"t":"dup"})).unwrap().is_none(), "duplicate seq ignored");
        let ev = s.events(&m.id, 0, 100).unwrap();
        assert_eq!(ev.len(), 2);
        let mut by_seq = ev.clone();
        by_seq.sort_by_key(|e| e.seq);
        assert_eq!(by_seq[0].payload["t"], "a");
        assert_eq!(s.max_seq(&sess.id).unwrap(), 2);
    }

    #[test]
    fn approvals_can_be_decided_exactly_once() {
        let (s, _, m) = seeded();
        let sess = s.create_session(&m.id, "claude", None).unwrap();
        let a = s.create_approval(&m.id, &sess.id, "req1", "Bash", "Run: npm test", &json!({"command":"npm test"}), "medium", &[], "pending", None, None).unwrap();
        assert_eq!(s.pending_approvals(None).unwrap().len(), 1);
        let d = s.decide_approval(&a.id, true, "user", None).unwrap();
        assert_eq!(d.state, "approved");
        assert!(s.decide_approval(&a.id, false, "user", None).is_err(), "second decision must fail");
        assert_eq!(s.get_approval(&a.id).unwrap().state, "approved");
        assert!(s.pending_approvals(None).unwrap().is_empty());
    }

    #[test]
    fn same_provider_request_cannot_create_two_approvals() {
        let (s, _, m) = seeded();
        let sess = s.create_session(&m.id, "claude", None).unwrap();
        s.create_approval(&m.id, &sess.id, "r", "Bash", "x", &json!({}), "low", &[], "pending", None, None).unwrap();
        assert!(s.create_approval(&m.id, &sess.id, "r", "Bash", "x", &json!({}), "low", &[], "pending", None, None).is_err());
    }

    #[test]
    fn expiring_session_approvals_leaves_decided_ones() {
        let (s, _, m) = seeded();
        let sess = s.create_session(&m.id, "claude", None).unwrap();
        let a = s.create_approval(&m.id, &sess.id, "a", "Bash", "x", &json!({}), "low", &[], "pending", None, None).unwrap();
        let b = s.create_approval(&m.id, &sess.id, "b", "Bash", "x", &json!({}), "low", &[], "pending", None, None).unwrap();
        s.decide_approval(&a.id, true, "user", None).unwrap();
        assert_eq!(s.expire_pending_for_session(&sess.id, "session lost").unwrap(), 1);
        assert_eq!(s.get_approval(&a.id).unwrap().state, "approved");
        assert_eq!(s.get_approval(&b.id).unwrap().state, "expired");
    }

    #[test]
    fn readding_a_project_does_not_duplicate() {
        let s = Store::in_memory();
        let a = s.add_project("x", "C:/x", "main").unwrap();
        let b = s.add_project("x", "C:/x", "main").unwrap();
        assert_eq!(a.id, b.id);
        s.archive_project(&a.id).unwrap();
        assert!(s.list_projects().unwrap().is_empty());
        s.add_project("x", "C:/x", "main").unwrap();
        assert_eq!(s.list_projects().unwrap().len(), 1);
    }

    #[test]
    fn end_session_does_not_overwrite_an_already_ended_state() {
        let (s, _, m) = seeded();
        let sess = s.create_session(&m.id, "claude", None).unwrap();
        s.end_session(&sess.id, "ended", "completed").unwrap();
        s.end_session(&sess.id, "lost", "late").unwrap();
        let got = s.get_session(&sess.id).unwrap();
        assert_eq!((got.state.as_str(), got.end_reason.as_deref()), ("ended", Some("completed")));
    }
}
