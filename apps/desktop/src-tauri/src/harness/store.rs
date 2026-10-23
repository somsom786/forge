//! Journal of edits FORGE made to agent configuration files. Every write is recorded *before* the file is touched
//! (state `started`) and finished afterwards, so a crash mid-write is resolved by looking at what is actually on disk.

use crate::error::{ForgeError, Result};
use crate::store::{Store, new_id, now};
use rusqlite::{OptionalExtension, Row, params};
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HarnessChange {
    pub id: String,
    pub project_id: String,
    pub rel: String,
    /// edit | create | rollback
    pub kind: String,
    /// Hash of the file before this change (None = the file did not exist).
    pub before_sha256: Option<String>,
    /// Hash after this change (None = the file was removed).
    pub after_sha256: Option<String>,
    /// Backup of the file as it was before this change, relative to the FORGE home. None when there was nothing to back up.
    pub backup_path: Option<String>,
    pub rolls_back_id: Option<String>,
    pub note: Option<String>,
    /// started | applied | failed
    pub state: String,
    pub created_at: String,
    pub finished_at: Option<String>,
    /// Set when a later rollback undid this change.
    pub rolled_back_by: Option<String>,
}

const COLS: &str = "c.id, c.project_id, c.rel, c.kind, c.before_sha256, c.after_sha256, c.backup_path, c.rolls_back_id, c.note, c.state, c.created_at, c.finished_at, \
    (SELECT r.id FROM harness_change r WHERE r.rolls_back_id = c.id AND r.state='applied' ORDER BY r.created_at DESC LIMIT 1)";

fn row(r: &Row) -> rusqlite::Result<HarnessChange> {
    Ok(HarnessChange {
        id: r.get(0)?,
        project_id: r.get(1)?,
        rel: r.get(2)?,
        kind: r.get(3)?,
        before_sha256: r.get(4)?,
        after_sha256: r.get(5)?,
        backup_path: r.get(6)?,
        rolls_back_id: r.get(7)?,
        note: r.get(8)?,
        state: r.get(9)?,
        created_at: r.get(10)?,
        finished_at: r.get(11)?,
        rolled_back_by: r.get(12)?,
    })
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HarnessReview {
    pub rel: String,
    pub sha256: String,
    pub reviewed_at: String,
}

pub struct NewChange<'a> {
    pub project_id: &'a str,
    pub rel: &'a str,
    pub kind: &'a str,
    pub before: Option<&'a str>,
    pub after: Option<&'a str>,
    pub backup: Option<&'a str>,
    pub rolls_back: Option<&'a str>,
    pub note: Option<&'a str>,
}

impl Store {
    pub fn h_change_begin(&self, n: NewChange) -> Result<HarnessChange> {
        let id = new_id();
        self.c().execute(
            "INSERT INTO harness_change(id, project_id, rel, kind, before_sha256, after_sha256, backup_path, rolls_back_id, note, state, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,'started',?10)",
            params![id, n.project_id, n.rel, n.kind, n.before, n.after, n.backup, n.rolls_back, n.note, now()],
        )?;
        self.h_change_get(&id)
    }

    pub fn h_change_finish(&self, id: &str, ok: bool) -> Result<HarnessChange> {
        let n = self.c().execute("UPDATE harness_change SET state=?2, finished_at=?3 WHERE id=?1 AND state='started'", params![id, if ok { "applied" } else { "failed" }, now()])?;
        if n != 1 {
            return Err(ForgeError::InvalidState(format!("harness change {id} was already resolved by recovery; check the file and the change history")));
        }
        self.h_change_get(id)
    }

    pub fn h_change_get(&self, id: &str) -> Result<HarnessChange> {
        self.c()
            .query_row(&format!("SELECT {COLS} FROM harness_change c WHERE c.id=?1"), [id], row)
            .optional()?
            .ok_or_else(|| ForgeError::NotFound(format!("harness change {id}")))
    }

    /// Newest first. `rel` narrows to one file.
    pub fn h_changes(&self, project_id: &str, rel: Option<&str>, limit: i64) -> Result<Vec<HarnessChange>> {
        let c = self.c();
        let mut s = c.prepare(&format!("SELECT {COLS} FROM harness_change c WHERE c.project_id=?1 AND (?2 IS NULL OR c.rel=?2) AND c.state<>'failed' ORDER BY c.created_at DESC, c.id DESC LIMIT ?3"))?;
        Ok(s.query_map(params![project_id, rel, limit], row)?.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn h_changes_unfinished(&self) -> Result<Vec<HarnessChange>> {
        let c = self.c();
        let mut s = c.prepare(&format!("SELECT {COLS} FROM harness_change c WHERE c.state='started'"))?;
        Ok(s.query_map([], row)?.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Record that the user has seen (or just wrote) this exact content of `rel`.
    pub fn h_review_set(&self, project_id: &str, rel: &str, sha256: &str) -> Result<()> {
        self.c().execute(
            "INSERT INTO harness_review(project_id, rel, sha256, reviewed_at) VALUES (?1,?2,?3,?4) ON CONFLICT(project_id, rel) DO UPDATE SET sha256=excluded.sha256, reviewed_at=excluded.reviewed_at",
            params![project_id, rel, sha256, now()],
        )?;
        Ok(())
    }

    pub fn h_review_clear(&self, project_id: &str, rel: &str) -> Result<()> {
        self.c().execute("DELETE FROM harness_review WHERE project_id=?1 AND rel=?2", params![project_id, rel])?;
        Ok(())
    }

    pub fn h_reviews(&self, project_id: &str) -> Result<Vec<HarnessReview>> {
        let c = self.c();
        let mut s = c.prepare("SELECT rel, sha256, reviewed_at FROM harness_review WHERE project_id=?1 ORDER BY rel")?;
        Ok(s.query_map([project_id], |r| Ok(HarnessReview { rel: r.get(0)?, sha256: r.get(1)?, reviewed_at: r.get(2)? }))?.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}
