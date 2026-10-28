//! What FORGE keeps on disk, and a user-confirmed clean-up of what has stopped being useful.
//!
//! Nothing is removed automatically. A clean-up is previewed first; the preview carries a token (a digest of exactly what
//! it lists) and applying requires that token, so nothing the user did not see is deleted. Only finished missions
//! (merged, rejected, cancelled) last changed longer ago than the chosen age are eligible, and only their bulky raw data:
//! check logs and captured output, and the agent transcript. Kept: the mission, its state history, approvals, review
//! decisions, check results (with their 64 KiB output tail), Proof Packs, the audit log, and memory. Worktrees are never
//! deleted here: ones FORGE no longer tracks are listed for the user to inspect.

use super::{Core, lock};
use crate::error::{ForgeError, Result};
use crate::mission::MissionState;
use crate::proof::sha256_hex;
use rusqlite::params;
use serde::Serialize;
use serde_json::json;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageUsage {
    pub database_bytes: u64,
    pub events: i64,
    pub run_files: u64,
    pub run_bytes: u64,
    pub proof_files: u64,
    pub proof_bytes: u64,
    pub experiment_bytes: u64,
    pub worktrees: usize,
    /// Worktree folders under FORGE's home that no mission refers to (left by a crash or an older version).
    pub untracked_worktrees: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CleanupMission {
    pub id: String,
    pub title: String,
    pub state: String,
    pub updated_at: String,
    pub events: i64,
    pub files: Vec<String>,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CleanupPlan {
    pub older_than_days: u32,
    pub missions: Vec<CleanupMission>,
    pub experiments: Vec<CleanupExperiment>,
    pub skipped_experiments: Vec<String>,
    /// Files in `runs/` that no check run refers to (left by a crash).
    pub orphan_files: Vec<String>,
    pub events: i64,
    pub files: usize,
    pub bytes: u64,
    /// Digest of everything listed; applying requires it.
    pub token: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CleanupArtifact {
    pub path: String,
    pub bytes: u64,
    pub modified: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CleanupExperiment {
    pub id: String,
    pub name: String,
    pub state: String,
    pub attempt: i64,
    pub finished_at: Option<String>,
    pub files: Vec<CleanupArtifact>,
    /// Empty folders only, removed deepest first after the listed files. Unexpected entries prevent removal.
    pub directories: Vec<String>,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupResult {
    pub events_deleted: i64,
    pub files_deleted: usize,
    pub bytes_freed: u64,
    pub failures: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactResult {
    pub pages_before: i64,
    pub pages_after: i64,
    pub free_pages_before: i64,
}

const MAX_AGE_DAYS: u32 = 36_500;

/// Files and bytes under `dir`, not following links. A folder holding a `.git` entry (an experiment's checkout) is not
/// walked: it can be a whole repository with its dependencies, and Settings must open quickly.
fn dir_stats(dir: &Path) -> (u64, u64) {
    let (mut files, mut bytes) = (0, 0);
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        if std::fs::symlink_metadata(&d).is_ok_and(|m| linked(&m)) { continue; }
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let Ok(m) = std::fs::symlink_metadata(e.path()) else { continue };
            if m.is_dir() {
                if !e.path().join(".git").exists() {
                    stack.push(e.path());
                }
            } else if m.is_file() {
                files += 1;
                bytes += m.len();
            }
        }
    }
    (files, bytes)
}

fn file_len(p: &Path) -> u64 {
    std::fs::symlink_metadata(p).map(|m| if m.is_file() { m.len() } else { 0 }).unwrap_or(0)
}

fn linked(m: &std::fs::Metadata) -> bool {
    #[cfg(windows)] { use std::os::windows::fs::MetadataExt; m.file_attributes() & 0x400 != 0 }
    #[cfg(not(windows))] { m.file_type().is_symlink() }
}

/// Check every ancestor below the owned data root. Never traverse links/junctions, even ones pointing back inside it.
fn owned_artifact(home: &Path, path: &Path) -> Result<()> {
    let rel = path.strip_prefix(home).map_err(|_| ForgeError::PathRejected("artifact outside FORGE data".into()))?;
    let mut p = home.to_path_buf();
    for component in rel.components() {
        if !matches!(component, std::path::Component::Normal(_)) { return Err(ForgeError::PathRejected("invalid artifact path".into())); }
        p.push(component);
        let m = std::fs::symlink_metadata(&p)?;
        if linked(&m) { return Err(ForgeError::PathRejected(format!("linked artifact refused: {}", p.display()))); }
    }
    Ok(())
}

fn experiment_artifacts(home: &Path, e: &crate::store_experiment::Experiment) -> Result<CleanupExperiment> {
    if e.id.is_empty() || !e.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') { return Err(ForgeError::PathRejected("invalid experiment id".into())); }
    let root = home.join("experiments").join(&e.id);
    owned_artifact(home, &root)?;
    let mut files = vec![];
    let mut directories = vec![String::new()];
    for attempt in std::fs::read_dir(&root)? {
        let attempt = attempt?;
        owned_artifact(home, &attempt.path())?;
        let name = attempt.file_name().to_string_lossy().into_owned();
        if !attempt.file_type()?.is_dir() || !name.parse::<i64>().is_ok_and(|n| n > 0 && n <= e.attempt) { return Err(ForgeError::InvalidState("unexpected experiment entry; inspect it manually".into())); }
        directories.push(name.clone());
        if directories.len() > 10_000 { return Err(ForgeError::InvalidState("excessive experiment folders".into())); }
        for entry in std::fs::read_dir(attempt.path())? {
            let entry = entry?;
            owned_artifact(home, &entry.path())?;
            if entry.file_name() != "logs" || !entry.file_type()?.is_dir() { return Err(ForgeError::InvalidState("leftover worktree or unexpected files; inspect them manually".into())); }
            directories.push(format!("{name}/logs"));
            for log in std::fs::read_dir(entry.path())? {
                let log = log?;
                owned_artifact(home, &log.path())?;
                let filename = log.file_name().to_string_lossy().into_owned();
                if !log.file_type()?.is_file() || !filename.ends_with(".log") || files.len() >= 10_000 { return Err(ForgeError::InvalidState("unexpected or excessive log entries; inspect them manually".into())); }
                let m = log.metadata()?;
                let modified = m.modified()?.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos().to_string()).unwrap_or_default();
                files.push(CleanupArtifact { path: format!("{name}/logs/{filename}"), bytes: m.len(), modified });
            }
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    directories.sort_by(|a, b| b.len().cmp(&a.len()).then(a.cmp(b)));
    let bytes = files.iter().map(|f| f.bytes).sum();
    Ok(CleanupExperiment { id: e.id.clone(), name: e.name.clone(), state: e.state.clone(), attempt: e.attempt, finished_at: e.finished_at.clone(), files, directories, bytes })
}

impl Core {
    /// Explicit maintenance only. Hold startup reservations and runner registries while SQLite rewrites the file.
    pub fn storage_compact(&self) -> Result<CompactResult> {
        let reg = self.reg();
        let checks = lock(&self.checks);
        let experiments = lock(&self.experiments);
        let merging = lock(&self.merging);
        if !reg.reserved.is_empty() || !reg.sessions.is_empty() || !checks.is_empty() || !experiments.is_empty() || !merging.is_empty() {
            return Err(ForgeError::InvalidState("database compaction requires FORGE to be idle; stop active missions, checks and experiments first".into()));
        }
        let operation = crate::store::new_id();
        self.store.audit_required("user", "storage.compact_started", Some(&operation), &json!({}))?;
        let c = self.store.c();
        let pages_before = c.pragma_query_value(None, "page_count", |r| r.get(0))?;
        let free_pages_before = c.pragma_query_value(None, "freelist_count", |r| r.get(0))?;
        let busy: i64 = c.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| r.get(0))?;
        if busy != 0 { return Err(ForgeError::InvalidState("the database is busy; compaction was not started".into())); }
        c.execute_batch("VACUUM;")?;
        let pages_after = c.pragma_query_value(None, "page_count", |r| r.get(0))?;
        c.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
        drop(c);
        let result = CompactResult { pages_before, pages_after, free_pages_before };
        self.store.audit_required("system", "storage.compact_finished", Some(&operation), &json!(result))?;
        Ok(result)
    }

    fn runs_dir(&self) -> PathBuf {
        self.cfg.home.join("runs")
    }

    pub fn storage_usage(&self) -> Result<StorageUsage> {
        let db = self.cfg.home.join("forge.db");
        let database_bytes = ["", "-wal", "-shm"].iter().map(|s| file_len(&PathBuf::from(format!("{}{s}", db.display())))).sum();
        let events = self.store.c().query_row("SELECT COUNT(*) FROM activity_event", [], |r| r.get(0))?;
        let (run_files, run_bytes) = dir_stats(&self.runs_dir());
        let (proof_files, proof_bytes) = dir_stats(&self.cfg.home.join("proofs"));
        let (_, experiment_bytes) = dir_stats(&self.cfg.home.join("experiments"));
        let tracked: HashSet<PathBuf> = {
            let c = self.store.c();
            let mut s = c.prepare("SELECT worktree_path FROM mission WHERE worktree_path IS NOT NULL")?;
            let rows = s.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
            rows.into_iter().map(|p| dunce::canonicalize(&p).unwrap_or_else(|_| PathBuf::from(p))).collect()
        };
        let mut worktrees = 0;
        let mut untracked = vec![];
        // Layout: worktrees/<project>/<mission-slug>.
        for proj in std::fs::read_dir(self.cfg.home.join("worktrees")).into_iter().flatten().flatten() {
            if !proj.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            for wt in std::fs::read_dir(proj.path()).into_iter().flatten().flatten() {
                if !wt.file_type().is_ok_and(|t| t.is_dir()) {
                    continue;
                }
                worktrees += 1;
                let p = dunce::canonicalize(wt.path()).unwrap_or_else(|_| wt.path());
                if !tracked.contains(&p) {
                    untracked.push(wt.path().to_string_lossy().into_owned());
                }
            }
        }
        untracked.sort();
        Ok(StorageUsage { database_bytes, events, run_files, run_bytes, proof_files, proof_bytes, experiment_bytes, worktrees, untracked_worktrees: untracked })
    }

    /// What a clean-up of finished missions older than `older_than_days` would remove. Read-only.
    pub fn storage_cleanup_preview(&self, older_than_days: u32) -> Result<CleanupPlan> {
        let reg = self.reg();
        let mut active_missions: HashSet<String> = reg.reserved.iter().cloned().collect();
        active_missions.extend(reg.sessions.values().map(|s| s.mission_id.clone()));
        active_missions.extend(lock(&self.checks).keys().cloned());
        let active = lock(&self.experiments).keys().cloned().collect();
        self.storage_cleanup_plan(older_than_days, &active, &active_missions)
    }

    fn storage_cleanup_plan(&self, older_than_days: u32, active: &HashSet<String>, active_missions: &HashSet<String>) -> Result<CleanupPlan> {
        // A century is "everything"; beyond that the date arithmetic would overflow.
        let older_than_days = older_than_days.min(MAX_AGE_DAYS);
        let cutoff = (chrono::Utc::now() - chrono::Duration::days(older_than_days as i64)).to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let finished = self.store.missions_in_states(&[MissionState::Merged, MissionState::Rejected, MissionState::Cancelled])?;
        let runs = self.runs_dir();
        let mut missions = vec![];
        for m in finished.into_iter().filter(|m| m.updated_at < cutoff) {
            if active_missions.contains(&m.id) { continue; }
            let events: i64 = self.store.c().query_row(
                "SELECT COUNT(*) FROM activity_event WHERE mission_id=?1 AND NOT (kind='status' AND json_extract(payload_json, '$.retention') IS NOT NULL)",
                [&m.id],
                |r| r.get(0),
            )?;
            let mut files = vec![];
            for r in self.store.runs_for_mission(&m.id)? {
                for name in [format!("{}.log", r.id), format!("{}.stdout", r.id)] {
                    if runs.join(&name).is_file() {
                        files.push(name);
                    }
                }
            }
            files.sort();
            let bytes = files.iter().map(|f| file_len(&runs.join(f))).sum();
            if events > 0 || !files.is_empty() {
                missions.push(CleanupMission { id: m.id, title: m.title, state: m.state, updated_at: m.updated_at, events, files, bytes });
            }
        }
        missions.sort_by(|a, b| a.updated_at.cmp(&b.updated_at).then(a.id.cmp(&b.id)));
        // Files no run refers to, older than the cutoff (a run in progress writes its files before its row is final).
        let known: HashSet<String> = {
            let c = self.store.c();
            let mut s = c.prepare("SELECT id FROM verification_run")?;
            let ids = s.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
            ids.into_iter().flat_map(|id| [format!("{id}.log"), format!("{id}.stdout")]).collect()
        };
        let horizon = std::time::SystemTime::now() - std::time::Duration::from_secs(older_than_days as u64 * 86_400);
        let mut orphan_files: Vec<String> = std::fs::read_dir(&runs)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .filter(|e| e.metadata().and_then(|m| m.modified()).is_ok_and(|t| t < horizon))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| !known.contains(n))
            .collect();
        orphan_files.sort();
        let mut experiments = vec![];
        let mut skipped_experiments = vec![];
        for e in self.store.exp_finished_before(&cutoff)? {
            if active.contains(&e.id) { continue; }
            if !self.cfg.home.join("experiments").join(&e.id).try_exists()? { continue; }
            match experiment_artifacts(&self.cfg.home, &e) {
                Ok(entry) => experiments.push(entry),
                Err(err) => skipped_experiments.push(format!("{}: {err}", e.name)),
            }
        }
        let events = missions.iter().map(|m| m.events).sum();
        let files = missions.iter().map(|m| m.files.len()).sum::<usize>() + orphan_files.len() + experiments.iter().map(|e| e.files.len()).sum::<usize>();
        let bytes = missions.iter().map(|m| m.bytes).sum::<u64>() + orphan_files.iter().map(|f| file_len(&runs.join(f))).sum::<u64>() + experiments.iter().map(|e| e.bytes).sum::<u64>();
        let token = sha256_hex(serde_json::to_string(&json!([older_than_days, missions, orphan_files, experiments])).unwrap_or_default().as_bytes());
        Ok(CleanupPlan { older_than_days, missions, experiments, skipped_experiments, orphan_files, events, files, bytes, token })
    }

    /// Apply a previewed clean-up. Refused if anything changed since the preview (the token no longer matches).
    pub fn storage_cleanup_apply(&self, older_than_days: u32, token: &str) -> Result<CleanupResult> {
        let older_than_days = older_than_days.min(MAX_AGE_DAYS);
        // Lock in the same order as compaction and check startup: registry, checks, then experiments.
        // Hold the experiment reservation until cleanup finishes; no experiment restart can race deletion.
        let reg = self.reg();
        let mut active_missions: HashSet<String> = reg.reserved.iter().cloned().collect();
        active_missions.extend(reg.sessions.values().map(|s| s.mission_id.clone()));
        active_missions.extend(lock(&self.checks).keys().cloned());
        let running = lock(&self.experiments);
        let active = running.keys().cloned().collect();
        let plan = self.storage_cleanup_plan(older_than_days, &active, &active_missions)?;
        // The selected missions are terminal and cannot start again. Release the registry before deleting artifacts so
        // live sessions do not wait for filesystem cleanup, and cleanup never waits for the registry while holding experiments.
        drop(reg);
        if plan.token != token {
            return Err(ForgeError::InvalidState("what would be removed changed since the preview; preview again and confirm what it lists".into()));
        }
        let runs = self.runs_dir();
        let operation = crate::store::new_id();
        self.store.audit_required("user", "storage.cleanup_started", Some(&operation), &json!(plan))?;
        let mut res = CleanupResult { events_deleted: 0, files_deleted: 0, bytes_freed: 0, failures: vec![] };
        let remove = |name: &str, res: &mut CleanupResult| {
            // Plain names only: never a path out of runs/.
            if name.contains(['/', '\\']) || name.starts_with('.') {
                res.failures.push(format!("{name}: not a plain file name"));
                return;
            }
            let p = runs.join(name);
            if let Err(err) = owned_artifact(&self.cfg.home, &p) {
                res.failures.push(format!("{name}: {err}"));
                return;
            }
            let len = file_len(&p);
            match std::fs::remove_file(&p) {
                Ok(()) => {
                    res.files_deleted += 1;
                    res.bytes_freed += len;
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => res.failures.push(format!("{name}: {e}")),
            }
        };
        // One mission failing (a full disk is exactly when people clean up) does not stop the rest, and whatever happened
        // is always audited below.
        let mut done: Vec<&str> = vec![];
        for m in &plan.missions {
            let failures_before = res.failures.len();
            for f in &m.files {
                remove(f, &mut res);
            }
            if res.failures.len() != failures_before {
                res.failures.push(format!("mission {}: transcript and log references retained because a log could not be removed; preview and retry", m.id));
                continue;
            }
            match self.forget_transcript(m) {
                Ok(n) => {
                    res.events_deleted += n;
                    done.push(&m.id);
                }
                Err(e) => res.failures.push(format!("mission {}: {e}", m.id)),
            }
        }
        for f in &plan.orphan_files {
            remove(f, &mut res);
        }
        for e in &plan.experiments {
            let root = self.cfg.home.join("experiments").join(&e.id);
            for file in &e.files {
                let p = root.join(&file.path);
                if let Err(err) = owned_artifact(&self.cfg.home, &p).and_then(|_| std::fs::remove_file(&p).map_err(Into::into)) {
                    res.failures.push(format!("experiment {} / {}: {err}", e.name, file.path));
                } else {
                    res.files_deleted += 1;
                    res.bytes_freed += file.bytes;
                }
            }
            for dir in &e.directories {
                let p = root.join(dir);
                // Empty-directory removal cannot delete new/unlisted entries or a leftover checkout.
                if let Err(err) = owned_artifact(&self.cfg.home, &p).and_then(|_| std::fs::remove_dir(&p).map_err(Into::into)) {
                    res.failures.push(format!("experiment {} folder {}: {err}", e.name, dir));
                }
            }
            if !root.exists() {
                if let Err(err) = self.store.c().execute("UPDATE experiment_result SET log_path=NULL WHERE experiment_id=?1", [&e.id]) { res.failures.push(format!("experiment {} log records: {err}", e.name)); }
            }
        }
        self.store.audit(
            "user",
            "storage.cleanup",
            None,
            &json!({"olderThanDays": older_than_days, "missions": done, "eventsDeleted": res.events_deleted, "filesDeleted": res.files_deleted, "bytesFreed": res.bytes_freed, "failures": res.failures}),
        );
        self.store.audit_required("system", "storage.cleanup_finished", Some(&operation), &json!(res))?;
        Ok(res)
    }

    /// Remove one finished mission's transcript (keeping retention markers), clear its dead log paths and leave a marker,
    /// in one transaction.
    fn forget_transcript(&self, m: &CleanupMission) -> Result<i64> {
        {
            let mut c = self.store.c();
            let tx = c.transaction()?;
            let n = tx.execute(
                "DELETE FROM activity_event WHERE mission_id=?1 AND NOT (kind='status' AND json_extract(payload_json, '$.retention') IS NOT NULL)",
                [&m.id],
            )? as i64;
            tx.execute("UPDATE verification_run SET log_path=NULL WHERE mission_id=?1", [&m.id])?;
            // A marker in the timeline, so an empty transcript is explained rather than mysterious.
            let session: Option<String> = tx.query_row("SELECT id FROM agent_session WHERE mission_id=?1 ORDER BY started_at DESC LIMIT 1", [&m.id], |r| r.get(0)).ok();
            if let (Some(sid), true) = (session, n > 0) {
                let at = crate::store::now();
                let payload = json!({"message": format!("Transcript removed by a storage clean-up on {} ({n} events). Approvals, check results, Proof Packs and the audit log are kept.", &at[..10]), "retention": true});
                tx.execute(
                    "INSERT OR IGNORE INTO activity_event(mission_id, session_id, seq, kind, payload_json, at) VALUES (?1,?2,?3,'status',?4,?5)",
                    params![m.id, sid, i64::MIN + 1 + chrono::Utc::now().timestamp_millis(), payload.to_string(), at],
                )?;
            }
            tx.commit()?;
            Ok(n)
        }
    }

    pub(super) fn recover_storage_cleanup(&self) {
        let pending = (|| -> Result<Vec<String>> {
            let c = self.store.c();
            let mut s = c.prepare("SELECT subject FROM audit_event a WHERE action='storage.cleanup_started' AND NOT EXISTS (SELECT 1 FROM audit_event b WHERE b.subject=a.subject AND b.action IN ('storage.cleanup_finished','storage.cleanup_interrupted'))")?;
            Ok(s.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<Vec<_>>>()?)
        })();
        match pending {
            Ok(ids) => for id in ids {
                self.store.audit("system", "storage.cleanup_interrupted", Some(&id), &json!({"message": "Cleanup stopped before completion was recorded. Some listed files may already be removed; preview remaining artifacts before another cleanup."}));
            },
            Err(e) => self.notice("warn", format!("could not inspect interrupted storage cleanup: {e}")),
        }
    }
}
