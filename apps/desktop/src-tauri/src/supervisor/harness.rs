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

    fn harness_lock(&self, project_id: &str) -> Result<vault::WriterLock> {
        vault::writer_lock(&self.cfg.home.join("harness-locks").join(project_id))
    }

    fn user_claude_home(&self) -> Option<PathBuf> {
        std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from).filter(|p| p.is_dir())
    }

    pub fn harness_inventory(&self, project_id: &str) -> Result<inventory::Inventory> {
        let root = self.harness_root(project_id)?;
        Ok(inventory::scan(&root, self.user_claude_home().as_deref()))
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
        let change = self.store.h_change_begin(NewChange {
            project_id,
            rel,
            kind,
            before: before_sha.as_deref(),
            after: after_sha.as_deref(),
            backup: backup_rel.as_deref(),
            rolls_back,
            note,
        })?;
        let result = (|| -> Result<()> {
            match after {
                Some(text) => {
                    page::atomic_write(path, text.as_bytes())?;
                    let back = std::fs::read(path)?;
                    if Some(edit::sha256_hex(&back)) != after_sha {
                        return Err(ForgeError::Other("the file on disk does not match what was written".into()));
                    }
                }
                None => match std::fs::remove_file(path) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.into()),
                },
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
        let cur_bytes = match std::fs::read(&path) {
            Ok(b) => Some(b),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
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
            (Some(_), None) => Err(ForgeError::Other("this change has no backup".into())),
        }
    }

    fn rollback_plan(&self, c: &HarnessChange) -> Result<(PathBuf, Option<Vec<u8>>, Option<Vec<u8>>, Option<String>)> {
        if c.state != "applied" {
            return Err(ForgeError::InvalidState("only a completed change can be rolled back".into()));
        }
        let root = self.harness_root(&c.project_id)?;
        let path = edit::target(&root, &c.rel)?;
        let cur = match std::fs::read(&path) {
            Ok(b) => Some(b),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
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
        let c = self.store.h_change_get(change_id)?;
        let _lock = self.harness_lock(&c.project_id)?;
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

    /// Resolve edits interrupted by a crash by inspecting the file: it either has the new content (the write finished) or it
    /// does not (writes are atomic, so the old file is intact).
    pub(super) fn recover_harness(&self) {
        let Ok(open) = self.store.h_changes_unfinished() else { return };
        for c in open {
            let finished = (|| -> Result<bool> {
                let root = self.harness_root(&c.project_id)?;
                let _lock = self.harness_lock(&c.project_id)?;
                let cur = edit::read(&root, &c.rel).ok().and_then(|f| f.sha256);
                Ok(cur == c.after_sha256)
            })()
            .unwrap_or(false);
            let _ = self.store.h_change_finish(&c.id, finished);
            self.store.audit("system", "harness.recovery", Some(&c.project_id), &json!({"rel": c.rel, "change": c.id, "completed": finished}));
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
        self.store.audit("user", "harness.hook_dry_run", Some(project_id), &json!({"event": event, "command": hooks::display_command(command), "exit": r.exit_code, "timedOut": r.timed_out}));
        Ok(r)
    }
}
