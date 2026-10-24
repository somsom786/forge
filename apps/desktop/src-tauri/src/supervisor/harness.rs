//! Harness Studio service: inventory, Doctor, previewed + journaled + backed-up edits of agent configuration files,
//! rollback with conflict detection, and hook dry-runs. Every mutation is a user action (a Tauri command from the app's
//! own webview); nothing here is reachable from an agent.

use super::{Core, UiEvent};
use crate::error::{ForgeError, Result};
use crate::harness::dryrun;
use crate::harness::edit::{self, FileRead, Preview};
use crate::harness::store::{HarnessChange, NewChange};
use crate::harness::{doctor, hooks, inventory};
use crate::knowledge::{page, vault};
use serde::Serialize;
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RollbackPreview {
    pub rel: String,
    /// Why a rollback would be unsafe right now (the file changed after this edit).
    pub conflict: Option<String>,
    /// Rolling back removes the file (the edit created it).
    pub removes_file: bool,
    pub diff: String,
    pub added: usize,
    pub removed: usize,
}

/// Disk used by Harness Studio backups for one project, and what a prune with `keep_per_file` would remove.
#[derive(Debug, Clone, Serialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct BackupUsage {
    pub keep_per_file: usize,
    pub files: usize,
    pub bytes: u64,
    /// Backups a prune never removes (see `backup_plan`).
    pub protected_files: usize,
    pub protected_bytes: u64,
    pub prunable_files: usize,
    pub prunable_bytes: u64,
    /// Backup files no journal entry refers to (left by an interrupted save); always prunable.
    pub orphan_files: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PruneReport {
    pub removed_files: usize,
    pub removed_bytes: u64,
    pub after: BackupUsage,
}

/// Default number of rollback steps kept per file.
pub const DEFAULT_KEEP_BACKUPS_PER_FILE: usize = 10;

struct BackupPlan {
    usage: BackupUsage,
    /// (change id or None for an orphan, absolute path, size)
    prunable: Vec<(Option<String>, PathBuf, u64)>,
}

#[derive(Debug, Clone, Default)]
pub struct HookSpec {
    pub event: String,
    pub matcher: Option<String>,
    pub command: Option<String>,
    pub timeout_secs: Option<u64>,
    pub template_id: Option<String>,
    pub params: HashMap<String, String>,
}

impl Core {
    fn harness_root(&self, project_id: &str) -> Result<PathBuf> {
        Ok(PathBuf::from(self.store.get_project(project_id)?.root_path))
    }

    pub(super) fn harness_lock(&self, project_id: &str) -> Result<vault::WriterLock> {
        vault::writer_lock(&self.cfg.home.join("harness-locks").join(project_id))
    }

    fn user_claude_home(&self) -> Option<PathBuf> {
        std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from).filter(|p| p.is_dir())
    }

    pub fn harness_inventory(&self, project_id: &str) -> Result<inventory::Inventory> {
        let root = self.harness_root(project_id)?;
        let mut inv = inventory::scan(&root, self.user_claude_home().as_deref());
        // A project server runs in missions only once the user authorized it (and then from the stored definition).
        let authorized: Vec<String> = self.store.mcp_active(project_id)?.into_iter().map(|a| a.name).collect();
        for s in inv.mcp_servers.iter_mut().filter(|s| s.scope == "project") {
            s.runs_in_forge_missions = authorized.contains(&s.name);
        }
        Ok(inv)
    }

    /// Doctor findings plus "changed since you last reviewed it" for files FORGE wrote or the user marked as reviewed.
    pub fn harness_doctor(&self, project_id: &str) -> Result<Vec<doctor::Finding>> {
        let root = self.harness_root(project_id)?;
        let inv = inventory::scan(&root, self.user_claude_home().as_deref());
        let mut out = doctor::run(&inv, &root);
        for r in self.store.h_reviews(project_id)? {
            let now = edit::read(&root, &r.rel).ok().and_then(|f| f.sha256);
            match now {
                Some(sha) if sha != r.sha256 => out.push(doctor::Finding {
                    severity: "info".into(),
                    kind: "changed_since_review".into(),
                    subject: Some(r.rel.clone()),
                    message: format!("{} changed outside FORGE since you last reviewed it ({}).", r.rel, r.reviewed_at),
                    suggestion: Some("Open it, read the change, and mark it reviewed. Instruction and settings files steer the agent, so unexpected edits deserve a look (a merge, a teammate, or a tool).".into()),
                }),
                None => out.push(doctor::Finding {
                    severity: "info".into(),
                    kind: "removed_since_review".into(),
                    subject: Some(r.rel.clone()),
                    message: format!("{} existed when you last reviewed it but is gone now.", r.rel),
                    suggestion: None,
                }),
                _ => {}
            }
        }
        Ok(out)
    }

    pub fn harness_read(&self, project_id: &str, rel: &str) -> Result<FileRead> {
        edit::read(&self.harness_root(project_id)?, rel)
    }

    pub fn harness_mark_reviewed(&self, project_id: &str, rel: &str) -> Result<()> {
        let root = self.harness_root(project_id)?;
        let rel = edit::clean_rel(rel)?;
        let cur = edit::read(&root, &rel)?;
        match cur.sha256 {
            Some(sha) => self.store.h_review_set(project_id, &rel, &sha)?,
            None => self.store.h_review_clear(project_id, &rel)?,
        }
        self.store.audit("user", "harness.reviewed", Some(project_id), &json!({"rel": rel}));
        self.emit(UiEvent::Harness { project_id: project_id.to_string() });
        Ok(())
    }

    pub fn harness_preview(&self, project_id: &str, rel: &str, text: &str) -> Result<Preview> {
        edit::preview(&self.harness_root(project_id)?, rel, text)
    }

    fn backup_dir(&self, project_id: &str) -> PathBuf {
        self.cfg.home.join("harness-backups").join(project_id)
    }

    fn backup_abs(&self, rel_to_home: &str) -> Result<PathBuf> {
        let p = self.cfg.home.join(rel_to_home);
        // Only ever read back files that live under the backups folder.
        if !rel_to_home.starts_with("harness-backups/") || rel_to_home.contains("..") {
            return Err(ForgeError::PathRejected(format!("`{rel_to_home}` is not a FORGE backup path")));
        }
        Ok(p)
    }

    /// Write `text` to the (already validated) target, journaled, with the previous content backed up first.
    #[allow(clippy::too_many_arguments)]
    fn write_journaled(
        &self,
        project_id: &str,
        rel: &str,
        path: &Path,
        before: Option<&[u8]>,
        after: Option<&str>,
        kind: &str,
        rolls_back: Option<&str>,
        note: Option<&str>,
    ) -> Result<HarnessChange> {
        let before_sha = before.map(edit::sha256_hex);
        let after_sha = after.map(|t| edit::sha256_hex(t.as_bytes()));
        let change_id_hint = crate::store::new_id();
        let backup_rel = match before {
            Some(bytes) => {
                let dir = self.backup_dir(project_id);
                std::fs::create_dir_all(&dir)?;
                let file = dir.join(format!("{change_id_hint}.bak"));
                page::atomic_write(&file, bytes)?;
                if std::fs::read(&file).map(|b| edit::sha256_hex(&b)).ok() != before_sha {
                    return Err(ForgeError::Other("the backup could not be verified, so nothing was changed".into()));
                }
                Some(format!("harness-backups/{project_id}/{change_id_hint}.bak"))
            }
            None => None,
        };
        let change = match self.store.h_change_begin(NewChange {
            project_id,
            rel,
            kind,
            before: before_sha.as_deref(),
            after: after_sha.as_deref(),
            backup: backup_rel.as_deref(),
            rolls_back,
            note,
        }) {
            Ok(c) => c,
            Err(e) => {
                // Nothing was journaled and nothing was written: do not leave an unreachable backup behind.
                if let Some(b) = &backup_rel {
                    let _ = std::fs::remove_file(self.cfg.home.join(b));
                }
                return Err(e);
            }
        };
        let root = self.harness_root(project_id)?;
        let result = (|| -> Result<()> {
            match after {
                Some(text) => {
                    edit::write_checked(&root, rel, path, text.as_bytes(), before_sha.as_deref())?;
                    let back = std::fs::read(path)?;
                    if Some(edit::sha256_hex(&back)) != after_sha {
                        return Err(ForgeError::Other("the file on disk does not match what was written".into()));
                    }
                }
                None => edit::remove_checked(&root, rel, path, before_sha.as_deref())?,
            }
            Ok(())
        })();
        match result {
            Ok(()) => self.store.h_change_finish(&change.id, true),
            Err(e) => {
                let _ = self.store.h_change_finish(&change.id, false);
                Err(e)
            }
        }
    }

    /// Apply a previewed edit. `expected_sha256` is the hash of the file the user was looking at (None = they saw no file);
    /// `expected_after_sha256` is the hash of the preview they approved, so what is written is exactly what they saw.
    #[allow(clippy::too_many_arguments)]
    pub fn harness_apply(
        &self,
        project_id: &str,
        rel: &str,
        text: &str,
        expected_sha256: Option<&str>,
        expected_after_sha256: &str,
        acknowledge_risks: bool,
        note: Option<&str>,
    ) -> Result<HarnessChange> {
        let root = self.harness_root(project_id)?;
        let rel = edit::clean_rel(rel)?;
        let _lock = self.harness_lock(project_id)?;
        let path = edit::target(&root, &rel)?;
        // Re-read and re-preview under the lock: validation applies to the bytes that will actually be written.
        let cur_bytes = edit::read_bytes(&path)?;
        let cur_sha = cur_bytes.as_deref().map(edit::sha256_hex);
        if cur_sha.as_deref() != expected_sha256 {
            return Err(ForgeError::InvalidState(format!("{rel} changed on disk after you opened it. Reload it and redo the edit so nothing is overwritten.")));
        }
        let p = edit::preview(&root, &rel, text)?;
        if !p.problems.is_empty() {
            return Err(ForgeError::Other(format!("cannot save {rel}: {}", p.problems.join("; "))));
        }
        if p.after_sha256 != expected_after_sha256 {
            return Err(ForgeError::InvalidState("the text differs from the preview you approved; preview it again".into()));
        }
        if p.unchanged {
            return Err(ForgeError::InvalidState("there is nothing to save: the text is identical to the file".into()));
        }
        if !p.risks.is_empty() && !acknowledge_risks {
            return Err(ForgeError::InvalidState("this change adds hooks that run shell commands; acknowledge them to save".into()));
        }
        let kind = if p.exists { "edit" } else { "create" };
        let change = self.write_journaled(project_id, &rel, &path, cur_bytes.as_deref(), Some(&p.text), kind, None, note)?;
        // Writing through FORGE means the user reviewed this exact content.
        if let Some(sha) = &change.after_sha256 {
            self.store.h_review_set(project_id, &rel, sha)?;
        }
        self.store.audit("user", "harness.apply", Some(project_id), &json!({"rel": rel, "kind": kind, "before": change.before_sha256, "after": change.after_sha256, "risks": p.risks.len()}));
        self.emit(UiEvent::Harness { project_id: project_id.to_string() });
        Ok(change)
    }

    pub fn harness_history(&self, project_id: &str, rel: Option<&str>) -> Result<Vec<HarnessChange>> {
        let rel = rel.map(edit::clean_rel).transpose()?;
        self.store.h_changes(project_id, rel.as_deref(), 200)
    }

    fn backup_bytes(&self, c: &HarnessChange) -> Result<Option<Vec<u8>>> {
        match (&c.before_sha256, &c.backup_path) {
            (None, _) => Ok(None),
            (Some(sha), Some(p)) => {
                let bytes = std::fs::read(self.backup_abs(p)?).map_err(|e| ForgeError::Other(format!("the backup for this change is missing or unreadable: {e}")))?;
                if &edit::sha256_hex(&bytes) != sha {
                    return Err(ForgeError::Other("the backup for this change no longer matches its recorded hash; it will not be restored".into()));
                }
                Ok(Some(bytes))
            }
            (Some(_), None) => Err(ForgeError::Other("this change has no backup (old backups are removed when you prune them), so it cannot be rolled back".into())),
        }
    }

    fn rollback_plan(&self, c: &HarnessChange) -> Result<(PathBuf, Option<Vec<u8>>, Option<Vec<u8>>, Option<String>)> {
        if c.state != "applied" {
            return Err(ForgeError::InvalidState("only a completed change can be rolled back".into()));
        }
        let root = self.harness_root(&c.project_id)?;
        let path = edit::target(&root, &c.rel)?;
        let cur = edit::read_bytes(&path)?;
        let cur_sha = cur.as_deref().map(edit::sha256_hex);
        let conflict = (cur_sha != c.after_sha256).then(|| {
            "The file has changed since this edit (by you, a merge or another tool). Restoring the backup would discard those later changes, so FORGE will not do it automatically. Open the file and edit it by hand, or use the diff to see what differs.".to_string()
        });
        let restore = self.backup_bytes(c)?;
        Ok((path, cur, restore, conflict))
    }

    pub fn harness_rollback_preview(&self, change_id: &str) -> Result<RollbackPreview> {
        let c = self.store.h_change_get(change_id)?;
        let (_, cur, restore, conflict) = self.rollback_plan(&c)?;
        let cur_text = cur.as_deref().map(|b| String::from_utf8_lossy(b).into_owned()).unwrap_or_default();
        let restore_text = restore.as_deref().map(|b| String::from_utf8_lossy(b).into_owned()).unwrap_or_default();
        let d = similar::TextDiff::from_lines(&cur_text, &restore_text);
        let (mut added, mut removed) = (0, 0);
        for ch in d.iter_all_changes() {
            match ch.tag() {
                similar::ChangeTag::Insert => added += 1,
                similar::ChangeTag::Delete => removed += 1,
                similar::ChangeTag::Equal => {}
            }
        }
        let diff = d.unified_diff().context_radius(3).header(&format!("a/{}", c.rel), &format!("b/{}", c.rel)).to_string();
        Ok(RollbackPreview { rel: c.rel, conflict, removes_file: restore.is_none(), diff, added, removed })
    }

    pub fn harness_rollback(&self, change_id: &str) -> Result<HarnessChange> {
        let first = self.store.h_change_get(change_id)?;
        let _lock = self.harness_lock(&first.project_id)?;
        // Everything below is decided from state read *under the lock*: an earlier read can be stale (another rollback, or an
        // edit that restored the same bytes, may have run in between).
        let c = self.store.h_change_get(change_id)?;
        let (path, cur, restore, conflict) = self.rollback_plan(&c)?;
        if let Some(msg) = conflict {
            return Err(ForgeError::InvalidState(msg));
        }
        if c.rolled_back_by.is_some() {
            return Err(ForgeError::InvalidState("this change was already rolled back".into()));
        }
        let restore_text = match &restore {
            Some(b) => Some(String::from_utf8(b.clone()).map_err(|_| ForgeError::Other("the backup is not valid UTF-8".into()))?),
            None => None,
        };
        let ch = self.write_journaled(&c.project_id, &c.rel, &path, cur.as_deref(), restore_text.as_deref(), "rollback", Some(&c.id), None)?;
        match &ch.after_sha256 {
            Some(sha) => self.store.h_review_set(&c.project_id, &c.rel, sha)?,
            None => self.store.h_review_clear(&c.project_id, &c.rel)?,
        }
        self.store.audit("user", "harness.rollback", Some(&c.project_id), &json!({"rel": c.rel, "rolledBack": c.id}));
        self.emit(UiEvent::Harness { project_id: c.project_id.clone() });
        Ok(ch)
    }

    // ---- backup retention ----------------------------------------------------------------------------------------------

    /// Decide which backups a prune may remove. A backup is **protected** when any of these holds:
    ///   * its change is unresolved (`started`): crash recovery may still need it;
    ///   * it belongs to one of the newest `keep_per_file` applied, not-rolled-back changes of its file (undo history);
    ///   * a rollback of its change is possible right now (the file still has that change's result), whatever its age;
    ///   * the file's current content cannot be read (FORGE cannot tell whether a rollback is possible, so it keeps it).
    ///
    /// Everything else (failed saves, changes already rolled back, older history, backup files no journal entry refers to) is
    /// prunable. Only `*.bak` regular files inside this project's backup folder are ever considered.
    fn backup_plan(&self, project_id: &str, keep_per_file: usize) -> Result<BackupPlan> {
        let root = self.harness_root(project_id)?;
        let changes = self.store.h_changes_with_backups(project_id)?;
        let mut usage = BackupUsage { keep_per_file, ..Default::default() };
        let mut prunable = vec![];
        let mut referenced = std::collections::HashSet::new();
        let mut kept_per_rel: HashMap<String, usize> = HashMap::new();
        let mut current: HashMap<String, Option<Option<String>>> = HashMap::new();
        for c in &changes {
            let Some(rel_path) = &c.backup_path else { continue };
            let abs = self.backup_abs(rel_path)?;
            referenced.insert(abs.clone());
            let Ok(meta) = std::fs::symlink_metadata(&abs) else { continue }; // already gone
            if !meta.is_file() {
                continue;
            }
            let size = meta.len();
            let cur = current.entry(c.rel.clone()).or_insert_with(|| edit::target(&root, &c.rel).and_then(|p| edit::read_bytes(&p)).ok().map(|b| b.map(|b| edit::sha256_hex(&b))));
            let live = c.state == "applied" && c.rolled_back_by.is_none();
            let rollback_possible_now = live && matches!(cur, Some(sha) if *sha == c.after_sha256);
            let unknown_file_state = live && cur.is_none();
            let in_history = live && {
                let n = kept_per_rel.entry(c.rel.clone()).or_default();
                *n += 1;
                *n <= keep_per_file
            };
            usage.files += 1;
            usage.bytes += size;
            if c.state == "started" || in_history || rollback_possible_now || unknown_file_state {
                usage.protected_files += 1;
                usage.protected_bytes += size;
            } else {
                usage.prunable_files += 1;
                usage.prunable_bytes += size;
                prunable.push((Some(c.id.clone()), abs, size));
            }
        }
        if let Ok(dir) = std::fs::read_dir(self.backup_dir(project_id)) {
            for e in dir.flatten() {
                let p = e.path();
                let Ok(meta) = std::fs::symlink_metadata(&p) else { continue };
                if !meta.is_file() || p.extension().and_then(|x| x.to_str()) != Some("bak") || referenced.contains(&p) {
                    continue;
                }
                usage.files += 1;
                usage.bytes += meta.len();
                usage.orphan_files += 1;
                usage.prunable_files += 1;
                usage.prunable_bytes += meta.len();
                prunable.push((None, p, meta.len()));
            }
        }
        Ok(BackupPlan { usage, prunable })
    }

    /// How much space this project's backups use and what a prune keeping `keep_per_file` undo steps would free. Read-only.
    pub fn harness_backup_usage(&self, project_id: &str, keep_per_file: Option<usize>) -> Result<BackupUsage> {
        Ok(self.backup_plan(project_id, keep_per_file.unwrap_or(DEFAULT_KEEP_BACKUPS_PER_FILE))?.usage)
    }

    /// Remove prunable backups (see `backup_plan`). Runs under the project's harness lock, so no save or rollback is in flight
    /// and an unreferenced backup cannot belong to one. The journal entry forgets the backup before the file is deleted, so a
    /// crash in between leaves at most an orphan file (pruned next time), never an entry pointing at nothing.
    pub fn harness_prune_backups(&self, project_id: &str, keep_per_file: usize) -> Result<PruneReport> {
        let _lock = self.harness_lock(project_id)?;
        let plan = self.backup_plan(project_id, keep_per_file)?;
        let (mut removed_files, mut removed_bytes) = (0, 0);
        for (change, path, size) in plan.prunable {
            if let Some(id) = &change {
                if self.store.h_change_drop_backup(id).is_err() {
                    continue;
                }
            }
            match std::fs::remove_file(&path) {
                Ok(()) => {
                    removed_files += 1;
                    removed_bytes += size;
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => eprintln!("[forge] could not remove backup {}: {e}", path.display()),
            }
        }
        let after = self.backup_plan(project_id, keep_per_file)?.usage;
        self.store.audit("user", "harness.prune_backups", Some(project_id), &json!({"keepPerFile": keep_per_file, "removedFiles": removed_files, "removedBytes": removed_bytes}));
        self.emit(UiEvent::Harness { project_id: project_id.to_string() });
        Ok(PruneReport { removed_files, removed_bytes, after })
    }

    /// Resolve edits interrupted by a crash by inspecting the file: it either has the new content (the write finished) or it
    /// does not (writes are atomic, so the old file is intact).
    pub(super) fn recover_harness(&self) {
        let Ok(open) = self.store.h_changes_unfinished() else { return };
        for c in open {
            // Resolve only what can be inspected while holding the project's lock. If the lock is busy (another FORGE process
            // may be mid-write) or the file cannot be read, leave the row `started`: guessing would mislabel a live write.
            let resolved = (|| -> Result<bool> {
                let root = self.harness_root(&c.project_id)?;
                let _lock = self.harness_lock(&c.project_id)?;
                let still_open = self.store.h_change_get(&c.id)?;
                if still_open.state != "started" {
                    return Ok(false);
                }
                let path = edit::target(&root, &c.rel)?;
                let cur = edit::read_bytes(&path)?.map(|b| edit::sha256_hex(&b));
                let finished = cur == c.after_sha256;
                self.store.h_change_finish(&c.id, finished)?;
                self.store.audit("system", "harness.recovery", Some(&c.project_id), &json!({"rel": c.rel, "change": c.id, "completed": finished}));
                Ok(true)
            })();
            if let Err(e) = resolved {
                eprintln!("[forge] harness change {} left unresolved: {e}", c.id);
            }
        }
    }

    // ---- hooks -----------------------------------------------------------------------------------------------------

    pub fn harness_hook_templates(&self) -> Vec<hooks::HookTemplate> {
        hooks::templates()
    }

    pub fn harness_hook_preview_add(&self, project_id: &str, spec: HookSpec) -> Result<Preview> {
        let root = self.harness_root(project_id)?;
        let (event, matcher, command, timeout) = match &spec.template_id {
            Some(id) => {
                let (t, cmd) = hooks::render_template(id, &spec.params)?;
                (t.event.clone(), t.matcher.clone(), cmd, t.timeout_secs)
            }
            None => (
                spec.event.clone(),
                spec.matcher.clone().filter(|m| !m.trim().is_empty()),
                spec.command.clone().ok_or_else(|| ForgeError::Other("a hook needs a command".into()))?,
                spec.timeout_secs.unwrap_or(30),
            ),
        };
        let rel = ".claude/settings.json";
        let cur = edit::read(&root, rel)?;
        let new_text = hooks::add_hook(cur.exists.then_some(cur.text.as_str()), &event, matcher.as_deref(), &command, timeout)?;
        edit::preview(&root, rel, &new_text)
    }

    pub fn harness_hook_preview_remove(&self, project_id: &str, hook_id: &str) -> Result<Preview> {
        let root = self.harness_root(project_id)?;
        // `<settings item id>#<event>#<group>#<index>`
        let parts: Vec<&str> = hook_id.rsplitn(4, '#').collect();
        if parts.len() != 4 {
            return Err(ForgeError::Other("malformed hook id".into()));
        }
        let (index, group, event, item) = (parts[0], parts[1], parts[2], parts[3]);
        if !item.starts_with("settings:project:") {
            return Err(ForgeError::PathRejected("only project hooks in .claude/settings.json can be removed here".into()));
        }
        let rel = ".claude/settings.json";
        let cur = edit::read(&root, rel)?;
        if !cur.exists {
            return Err(ForgeError::NotFound(rel.into()));
        }
        let group: usize = group.parse().map_err(|_| ForgeError::Other("malformed hook id".into()))?;
        let index: usize = index.parse().map_err(|_| ForgeError::Other("malformed hook id".into()))?;
        let new_text = hooks::remove_hook(&cur.text, event, group, index)?;
        edit::preview(&root, rel, &new_text)
    }

    /// Run one hook command against a synthetic event. Explicit user action only (see `harness::dryrun`).
    pub fn harness_hook_dry_run(&self, project_id: &str, event: &str, command: &str) -> Result<hooks::DryRun> {
        let root = self.harness_root(project_id)?;
        let r = dryrun::run(&root, event, command, Duration::from_secs(20))?;
        // The command text can contain a token; the audit log keeps it redacted plus a hash for exact identity.
        let shown = crate::redact::redact_str(&hooks::display_command(command));
        self.store.audit("user", "harness.hook_dry_run", Some(project_id), &json!({"event": event, "command": shown, "commandSha256": edit::sha256_hex(command.as_bytes()), "exit": r.exit_code, "timedOut": r.timed_out}));
        Ok(r)
    }
}
