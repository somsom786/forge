//! Memory integration for missions: retrieval into the first prompt, factual mission records, and the
//! user-facing maintenance operations. A child module of the supervisor so it shares the store and notice machinery.
//!
//! Memory is an aid, never a dependency: any failure here is reported and the mission proceeds without it.

use super::*;
use crate::knowledge::lint::{self, Issue};
use crate::knowledge::page::{self, Author};
use crate::knowledge::retrieve::{self, ContextPackage};
use crate::knowledge::service::{self, Ctx, FreshnessReport, NewPage, RecoveryNotes, ScanReport, SourceSpec};
use crate::knowledge::store::{KLink, KPage, KReview, KRevision, KSource};
use crate::knowledge::vault;
use crate::store_review::VerificationRun;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultInfo {
    pub path: String,
    pub is_obsidian_vault: bool,
    pub writable: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeStatus {
    pub managed_root: String,
    pub vault: Option<VaultInfo>,
    pub obsidian_installed: bool,
    /// Counts by *effective* state (a page edited after verification counts as proposed).
    pub counts: std::collections::BTreeMap<String, usize>,
    pub total: usize,
    pub missing: usize,
    pub unfinished_operations: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageDetail {
    pub page: KPage,
    /// The file exactly as it is on disk (what an editor would show).
    pub text: String,
    /// Hash of exactly `text`. Verify against THIS, so what a person reads is what they verify.
    pub text_sha256: String,
    pub links: Vec<KLink>,
    pub backlinks: Vec<KPage>,
    pub sources: Vec<KSource>,
    pub reviews: Vec<KReview>,
    pub revisions: Vec<KRevision>,
    pub writable: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceArg {
    pub kind: String,
    pub locator: String,
    pub event_seq: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewKnowledgeArgs {
    pub title: String,
    pub kind: String,
    pub body: String,
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub topic: Option<String>,
    #[serde(default)]
    pub sources: Vec<SourceArg>,
}

fn source_spec(a: &SourceArg) -> Result<SourceSpec> {
    Ok(match a.kind.as_str() {
        "repo_file" => SourceSpec::RepoFile { rel: a.locator.clone() },
        "commit" => SourceSpec::Commit { sha: a.locator.clone() },
        "mission" => SourceSpec::Mission { id: a.locator.clone(), event_seq: a.event_seq },
        "check_run" => SourceSpec::CheckRun { id: a.locator.clone() },
        "user_note" => SourceSpec::UserNote { text: a.locator.clone() },
        other => return Err(ForgeError::Other(format!("unknown source kind \"{other}\""))),
    })
}

impl Core {
    pub(crate) fn kctx(&self) -> Ctx<'_> {
        Ctx { store: &self.store, home: &self.cfg.home }
    }

    fn project_root(&self, project_id: &str) -> Result<PathBuf> {
        Ok(PathBuf::from(self.store.get_project(project_id)?.root_path))
    }

    /// First-turn prompt = objective + (when anything qualifies) the verified-knowledge block. Never blocks a mission.
    pub(super) fn memory_prompt(&self, m: &Mission, project: &Project, objective: &str) -> String {
        if !m.use_memory {
            return objective.to_string();
        }
        let hints = self.store.get_contract(&m.id).map(|c| c.in_scope).unwrap_or_default();
        match retrieve::build(self.kctx(), &project.id, Path::new(&project.root_path), objective, &hints, &m.memory_pins, retrieve::DEFAULT_BUDGET_CHARS) {
            Ok(pkg) => {
                let (params, items, omitted) = retrieve::package_json(&pkg);
                if let Err(e) = self.store.k_save_context(&m.id, &pkg.query, &params, &items, &omitted, &pkg.text, &pkg.sha256, pkg.char_budget as i64) {
                    self.notice("warn", format!("could not save the context package: {e}"));
                }
                retrieve::compose_prompt(objective, &pkg.text)
            }
            Err(e) => {
                self.notice("warn", format!("project memory was skipped for this mission: {e}"));
                objective.to_string()
            }
        }
    }

    pub fn set_mission_memory(&self, mission_id: &str, use_memory: bool, pins: &[String]) -> Result<Mission> {
        let m = self.store.get_mission(mission_id)?;
        if m.state != MissionState::Ready.as_str() && m.state != MissionState::Draft.as_str() {
            return Err(ForgeError::InvalidState("memory choices are fixed once a mission has started (the context was pinned to its first turn)".into()));
        }
        for id in pins {
            let p = self.store.k_get(id)?;
            if p.project_id != m.project_id {
                return Err(ForgeError::InvalidState("a pinned page belongs to another project".into()));
            }
        }
        self.store.set_mission_memory(mission_id, use_memory, pins)?;
        let m = self.store.get_mission(mission_id)?;
        self.emit(UiEvent::Mission { mission: m.clone() });
        Ok(m)
    }

    // ---- status and maintenance ------------------------------------------------------------------

    pub fn knowledge_status(&self, project_id: &str) -> Result<KnowledgeStatus> {
        let roots = service::roots(self.kctx(), project_id)?;
        let pages = self.store.k_list(project_id)?;
        let mut counts = std::collections::BTreeMap::new();
        for p in pages.iter().filter(|p| !p.missing) {
            *counts.entry(p.effective_state.clone()).or_insert(0usize) += 1;
        }
        Ok(KnowledgeStatus {
            managed_root: roots.iter().find(|r| r.kind == "managed").map(|r| r.path.to_string_lossy().into_owned()).unwrap_or_default(),
            vault: roots.iter().find(|r| r.kind == "vault").map(|r| VaultInfo { path: r.path.to_string_lossy().into_owned(), is_obsidian_vault: vault::is_vault(&r.path), writable: r.writable }),
            obsidian_installed: vault::obsidian_installed(),
            total: pages.iter().filter(|p| !p.missing).count(),
            missing: pages.iter().filter(|p| p.missing).count(),
            counts,
            unfinished_operations: self.store.k_ops_unfinished()?.len(),
        })
    }

    pub fn knowledge_scan(&self, project_id: &str) -> Result<(ScanReport, FreshnessReport)> {
        let root = self.project_root(project_id)?;
        let scan = service::scan(self.kctx(), project_id)?;
        let fresh = service::refresh_sources(self.kctx(), project_id, &root)?;
        if scan.indexed > 0 || scan.removed > 0 {
            self.emit(UiEvent::Knowledge { project_id: project_id.to_string() });
        }
        Ok((scan, fresh))
    }

    pub fn knowledge_rebuild_index(&self, project_id: &str) -> Result<usize> {
        self.store.k_fts_rebuild(project_id)
    }

    pub fn knowledge_lint(&self, project_id: &str) -> Result<Vec<Issue>> {
        let root = self.project_root(project_id)?;
        service::scan(self.kctx(), project_id)?;
        lint::run(self.kctx(), project_id, &root)
    }

    pub fn knowledge_set_vault(&self, project_id: &str, path: Option<&Path>, write: bool) -> Result<KnowledgeStatus> {
        service::set_vault(self.kctx(), project_id, path, write)?;
        service::scan(self.kctx(), project_id)?;
        self.emit(UiEvent::Knowledge { project_id: project_id.to_string() });
        self.knowledge_status(project_id)
    }

    pub fn knowledge_pages(&self, project_id: &str, query: Option<&str>) -> Result<Vec<KPage>> {
        service::scan(self.kctx(), project_id)?;
        match query.map(str::trim).filter(|q| !q.is_empty()) {
            None => self.store.k_list(project_id),
            Some(q) => match crate::knowledge::query::fts_match(q) {
                None => Ok(vec![]),
                Some(fts) => Ok(self.store.k_search(project_id, &fts, 100)?.into_iter().map(|(p, _)| p).collect()),
            },
        }
    }

    pub fn knowledge_page_detail(&self, page_id: &str) -> Result<PageDetail> {
        let page = self.store.k_get(page_id)?;
        let page = if page.missing { page } else { service::reindex_page(self.kctx(), &page)? };
        let root = service::root_for(self.kctx(), &page.project_id, &page.root_kind)?;
        let text = if page.missing { String::new() } else { service::read_page_text(self.kctx(), &page)? };
        Ok(PageDetail {
            text_sha256: page::content_hash(&text),
            text,
            links: self.store.k_links(page_id)?,
            backlinks: self.store.k_backlinks(page_id)?,
            sources: self.store.k_sources_for(page_id)?,
            reviews: self.store.k_reviews(page_id)?,
            revisions: self.store.k_revisions(page_id)?,
            writable: root.writable,
            page,
        })
    }

    // ---- creating and changing knowledge ---------------------------------------------------------------

    /// A page the user wrote or asked FORGE to write. Starts `proposed` like everything else.
    pub fn knowledge_create(&self, project_id: &str, a: NewKnowledgeArgs) -> Result<KPage> {
        let root = self.project_root(project_id)?;
        let sources = a.sources.iter().map(source_spec).collect::<Result<Vec<_>>>()?;
        let p = service::create_page(self.kctx(), project_id, &root, NewPage { title: a.title, kind: a.kind, body: a.body, files: a.files, tags: a.tags, topic: a.topic, author: Author::User, sources })?;
        self.emit(UiEvent::Knowledge { project_id: project_id.to_string() });
        Ok(p)
    }

    /// Save something an agent said during a mission as a *proposal*, sourced to that exact event.
    pub fn knowledge_save_from_event(&self, mission_id: &str, event_id: i64, title: &str, kind: &str) -> Result<KPage> {
        let m = self.store.get_mission(mission_id)?;
        let ev = self.store.events(mission_id, event_id - 1, 1)?.into_iter().find(|e| e.id == event_id).ok_or_else(|| ForgeError::NotFound(format!("event {event_id}")))?;
        let text = ev.payload.get("text").and_then(|t| t.as_str()).filter(|_| matches!(ev.kind.as_str(), "agent.text" | "agent.thinking")).ok_or_else(|| ForgeError::Other("only an agent message can be saved as knowledge".into()))?;
        let root = self.project_root(&m.project_id)?;
        let p = service::create_page(
            self.kctx(), &m.project_id, &root,
            NewPage {
                title: title.to_string(), kind: kind.to_string(), body: text.to_string(), files: vec![], tags: vec![], topic: None, author: Author::Agent,
                sources: vec![SourceSpec::Mission { id: mission_id.to_string(), event_seq: Some(ev.seq) }],
            },
        )?;
        self.emit(UiEvent::Knowledge { project_id: m.project_id.clone() });
        Ok(p)
    }

    pub fn knowledge_propose_revision(&self, page_id: &str, text: &str, note: Option<&str>) -> Result<KRevision> {
        service::propose_revision(self.kctx(), page_id, text, Author::User, note)
    }

    pub fn knowledge_apply_revision(&self, revision_id: &str) -> Result<KPage> {
        let p = service::apply_revision(self.kctx(), revision_id)?;
        self.emit(UiEvent::Knowledge { project_id: p.project_id.clone() });
        Ok(p)
    }

    pub fn knowledge_reject_revision(&self, revision_id: &str) -> Result<()> {
        service::reject_revision(self.kctx(), revision_id)
    }

    /// The only road to `verified`. `expected_sha256` is the revision the reviewer was looking at.
    pub fn knowledge_verify(&self, page_id: &str, expected_sha256: &str, note: Option<&str>) -> Result<KPage> {
        let page = self.store.k_get(page_id)?;
        // Make sure the index reflects the file *now*; verifying a revision that no longer exists would be meaningless.
        let fresh = service::reindex_page(self.kctx(), &page)?;
        let out = self.store.k_verify(page_id, expected_sha256, note)?;
        self.store.audit("user", "knowledge.verify", Some(page_id), &json!({"sha256": expected_sha256, "title": fresh.title, "author": fresh.author}));
        service::regenerate_catalogue(self.kctx(), &out.project_id)?;
        self.emit(UiEvent::Knowledge { project_id: out.project_id.clone() });
        Ok(out)
    }

    pub fn knowledge_set_status(&self, page_id: &str, action: &str, note: Option<&str>) -> Result<KPage> {
        let out = self.store.k_set_status(page_id, action, note)?;
        self.store.audit("user", &format!("knowledge.{action}"), Some(page_id), &json!({"title": out.title}));
        service::regenerate_catalogue(self.kctx(), &out.project_id)?;
        self.emit(UiEvent::Knowledge { project_id: out.project_id.clone() });
        Ok(out)
    }

    pub fn knowledge_supersede(&self, old_id: &str, new_id: &str, note: Option<&str>) -> Result<KPage> {
        let out = self.store.k_supersede(old_id, new_id, note)?;
        self.store.audit("user", "knowledge.supersede", Some(old_id), &json!({"by": new_id}));
        service::regenerate_catalogue(self.kctx(), &out.project_id)?;
        self.emit(UiEvent::Knowledge { project_id: out.project_id.clone() });
        Ok(out)
    }

    pub fn knowledge_forget(&self, page_id: &str) -> Result<()> {
        let project_id = self.store.k_get(page_id)?.project_id;
        service::forget_page(self.kctx(), page_id)?;
        self.emit(UiEvent::Knowledge { project_id });
        Ok(())
    }

    pub fn knowledge_open_in_obsidian(&self, page_id: &str) -> Result<String> {
        let page = self.store.k_get(page_id)?;
        let root = service::root_for(self.kctx(), &page.project_id, &page.root_kind)?;
        let real = vault::safe_file(&root.path, &page.rel_path)?;
        if page.root_kind == "vault" && !vault::obsidian_installed() {
            return Err(ForgeError::Other("Obsidian does not appear to be installed (no obsidian:// handler registered)".into()));
        }
        Ok(vault::obsidian_uri(&real))
    }

    // ---- context ----------------------------------------------------------------------------------------------

    pub fn context_preview(&self, project_id: &str, objective: &str, hints: &[String], pins: &[String]) -> Result<ContextPackage> {
        let root = self.project_root(project_id)?;
        retrieve::build(self.kctx(), project_id, &root, objective, hints, pins, retrieve::DEFAULT_BUDGET_CHARS)
    }

    // ---- capture ------------------------------------------------------------------------------------------------

    /// A factual record of a finished mission, derived only from recorded evidence (no agent prose). Proposed, like all
    /// new knowledge: a person decides whether it is worth trusting.
    pub fn capture_mission_record(&self, mission_id: &str, merge_commit: Option<&str>) -> Result<KPage> {
        let m = self.store.get_mission(mission_id)?;
        let project = self.store.get_project(&m.project_id)?;
        let runs: Vec<VerificationRun> = self.store.latest_runs(mission_id)?.into_values().collect();
        let decisions = self.store.review_decisions(mission_id)?;
        let pack = self.store.proof_packs(mission_id)?.into_iter().next();

        let mut body = format!("# Mission record: {}\n\n- Outcome: **{}**\n- Provider: {}{}\n", m.title, m.state, m.provider, m.model.as_deref().map(|x| format!(" ({x})")).unwrap_or_default());
        if let Some(c) = merge_commit {
            body.push_str(&format!("- Merge commit: `{c}`\n"));
        }
        body.push_str(&format!("\n## Objective\n\n{}\n", m.objective));
        let mut files: Vec<String> = vec![];
        if let Some(pk) = &pack {
            body.push_str("\n## Files changed\n\n");
            for f in pk.content["changes"]["files"].as_array().cloned().unwrap_or_default() {
                let path = f["path"].as_str().unwrap_or("");
                body.push_str(&format!("- `{path}` (+{} −{})\n", f["added"].as_u64().unwrap_or(0), f["removed"].as_u64().unwrap_or(0)));
                if !path.is_empty() && !service::sensitive_path(path) {
                    files.push(path.to_string());
                }
            }
        }
        if !runs.is_empty() {
            body.push_str("\n## Checks\n\n");
            for r in &runs {
                body.push_str(&format!("- {}: {} (exit {}){}\n", r.name, crate::store_review::run_state_str(r.state), r.exit_code.map(|c| c.to_string()).unwrap_or_else(|| "?".into()), if r.mutated { ", modified the worktree" } else { "" }));
            }
        }
        if !decisions.is_empty() {
            body.push_str("\n## Review\n\n");
            for d in &decisions {
                body.push_str(&format!("- {}{}{}\n", d.decision, d.note.as_deref().map(|n| format!(": {n}")).unwrap_or_default(), d.override_reason.as_deref().map(|n| format!(" (approved with unresolved items; reason: {n})")).unwrap_or_default()));
            }
        }
        body.push_str("\n_Generated by FORGE from recorded evidence. It states what happened, not whether the change was correct._\n");

        let mut sources = vec![SourceSpec::Mission { id: mission_id.to_string(), event_seq: None }];
        sources.extend(runs.iter().map(|r| SourceSpec::CheckRun { id: r.id.clone() }));
        if let Some(c) = merge_commit {
            sources.push(SourceSpec::Commit { sha: c.to_string() });
        }
        files.truncate(40);
        let p = service::create_page(
            self.kctx(), &project.id, Path::new(&project.root_path),
            NewPage { title: format!("Mission: {}", m.title), kind: "mission_record".into(), body, files, tags: vec!["mission".into()], topic: None, author: Author::System, sources },
        )?;
        self.emit(UiEvent::Knowledge { project_id: project.id.clone() });
        Ok(p)
    }

    /// Resolve interrupted knowledge writes after a restart.
    pub(super) fn recover_knowledge(&self) -> Option<RecoveryNotes> {
        match service::recover(self.kctx()) {
            Ok(n) => Some(n),
            Err(e) => {
                eprintln!("[forge] knowledge recovery failed: {e}");
                None
            }
        }
    }
}

#[allow(dead_code)]
fn _page_uses() {
    let _ = page::MAX_PAGE_BYTES;
}
