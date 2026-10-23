//! Tauri command surface. Thin: every command delegates to `Core`, runs off the UI thread,
//! and returns serialisable data or a human-readable error string.

use crate::error::{ForgeError, Result};
use crate::git;
use crate::policy::PermissionProfile;
use crate::providers::{self, ProviderInfo};
use crate::store::{ActivityEvent, AgentSession, Approval, Mission, Project, Transition};
use crate::supervisor::{Core, RecoveryReport};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::State;

pub type CoreState<'a> = State<'a, Arc<Core>>;

async fn blocking<T, F>(core: &Arc<Core>, f: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce(Arc<Core>) -> Result<T> + Send + 'static,
{
    let c = core.clone();
    tauri::async_runtime::spawn_blocking(move || f(c))
        .await
        .map_err(|e| ForgeError::Other(format!("background task failed: {e}")))?
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub home: String,
    pub git_version: Option<String>,
    pub node_found: bool,
    pub engine_found: bool,
    pub claude_found: bool,
    pub scripted_agent_enabled: bool,
    pub max_concurrent: usize,
    pub live_sessions: usize,
}

#[tauri::command]
pub async fn app_info(core: CoreState<'_>) -> Result<AppInfo> {
    blocking(&core, |c| {
        Ok(AppInfo {
            version: env!("CARGO_PKG_VERSION").into(),
            home: c.cfg.home.to_string_lossy().into_owned(),
            git_version: git::version().ok(),
            node_found: c.cfg.node.is_some() || which::which("node").is_ok(),
            engine_found: c.cfg.engine_script.as_ref().is_some_and(|p| p.exists()),
            claude_found: c.cfg.claude.is_some(),
            scripted_agent_enabled: c.cfg.allow_fake,
            max_concurrent: c.cfg.max_concurrent,
            live_sessions: c.live_session_count(),
        })
    })
    .await
}

#[tauri::command]
pub async fn recovery_report(core: CoreState<'_>) -> Result<RecoveryReport> {
    blocking(&core, |c| c.recover()).await
}

#[tauri::command]
pub async fn providers_detect(core: CoreState<'_>) -> Result<Vec<ProviderInfo>> {
    blocking(&core, |c| {
        let mut v = vec![
            providers::detect_claude(c.cfg.claude.as_ref()),
            providers::detect_claude_local(c.cfg.claude.as_ref(), &c.cfg.ollama_url),
            providers::detect_codex(c.cfg.codex.clone()),
            crate::acp::provider_info(&c.acp_agents()?),
        ];
        if c.cfg.allow_fake {
            v.push(ProviderInfo {
                id: "fake".into(),
                name: "Scripted test agent (development only)".into(),
                installed: true,
                path: None,
                version: None,
                signed_in: None,
                auth_method: None,
                plan: None,
                adapter_available: true,
                notes: vec!["Deterministic stand-in used to exercise FORGE itself. It is not a real coding agent.".into()],
                capabilities: serde_json::json!({
                    "sessionStart": {"supported": true}, "resume": {"supported": false}, "streaming": {"supported": true},
                    "interrupt": {"supported": true}, "toolEvents": {"supported": true}, "fileChanges": {"supported": true},
                    "approvals": {"supported": true}, "usageReporting": {"supported": false}, "modelSelection": {"supported": false},
                    "worktreeIntegration": {"supported": true}
                }),
                models: vec![],
            });
        }
        Ok(v)
    })
    .await
}

// ---- projects -------------------------------------------------------------------------

#[tauri::command]
pub async fn project_add(core: CoreState<'_>, path: String) -> Result<Project> {
    blocking(&core, move |c| c.add_project(Path::new(&path))).await
}

#[tauri::command]
pub async fn project_list(core: CoreState<'_>) -> Result<Vec<Project>> {
    blocking(&core, |c| c.store.list_projects()).await
}

#[tauri::command]
pub async fn project_remove(core: CoreState<'_>, project_id: String) -> Result<()> {
    blocking(&core, move |c| {
        let live = c.store.list_missions(Some(&project_id))?.into_iter().any(|m| {
            crate::mission::MissionState::parse(&m.state).is_some_and(|s| s.expects_live_session())
        });
        if live {
            return Err(ForgeError::InvalidState("this project has running missions; stop them first".into()));
        }
        c.store.archive_project(&project_id)?;
        c.store.audit("user", "project.remove", Some(&project_id), &serde_json::json!({}));
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn project_status(core: CoreState<'_>, project_id: String) -> Result<git::RepoStatus> {
    blocking(&core, move |c| {
        let p = c.store.get_project(&project_id)?;
        git::status(Path::new(&p.root_path))
    })
    .await
}

// ---- missions -------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewMissionArgs {
    pub project_id: String,
    pub objective: String,
    pub title: Option<String>,
    pub provider: String,
    pub model: Option<String>,
    pub permission_profile: Option<String>,
}

#[tauri::command]
pub async fn mission_create(core: CoreState<'_>, args: NewMissionArgs) -> Result<Mission> {
    blocking(&core, move |c| {
        c.create_mission(
            &args.project_id,
            &args.objective,
            args.title.as_deref(),
            &args.provider,
            args.model.as_deref(),
            PermissionProfile::parse(args.permission_profile.as_deref().unwrap_or("default")),
        )
    })
    .await
}

#[tauri::command]
pub async fn mission_start(core: CoreState<'_>, mission_id: String) -> Result<Mission> {
    blocking(&core, move |c| c.start_mission(&mission_id, None)).await
}

#[tauri::command]
pub async fn mission_followup(core: CoreState<'_>, mission_id: String, text: String) -> Result<Mission> {
    blocking(&core, move |c| {
        if text.trim().is_empty() {
            return Err(ForgeError::Other("a follow-up needs some text".into()));
        }
        c.start_mission(&mission_id, Some(text.trim()))
    })
    .await
}

#[tauri::command]
pub async fn mission_stop(core: CoreState<'_>, mission_id: String) -> Result<()> {
    blocking(&core, move |c| c.stop_mission(&mission_id)).await
}

#[tauri::command]
pub async fn mission_discard(core: CoreState<'_>, mission_id: String, confirm_loss: bool) -> Result<Mission> {
    blocking(&core, move |c| c.discard_mission(&mission_id, confirm_loss)).await
}

#[tauri::command]
pub async fn mission_list(core: CoreState<'_>, project_id: Option<String>) -> Result<Vec<Mission>> {
    blocking(&core, move |c| c.store.list_missions(project_id.as_deref())).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MissionDetail {
    pub mission: Mission,
    pub project: Project,
    pub sessions: Vec<AgentSession>,
    pub transitions: Vec<Transition>,
    pub approvals: Vec<Approval>,
    pub running: bool,
}

#[tauri::command]
pub async fn mission_get(core: CoreState<'_>, mission_id: String) -> Result<MissionDetail> {
    blocking(&core, move |c| {
        let mission = c.store.get_mission(&mission_id)?;
        Ok(MissionDetail {
            project: c.store.get_project(&mission.project_id)?,
            sessions: c.store.sessions_for_mission(&mission_id)?,
            transitions: c.store.transitions(&mission_id)?,
            approvals: c.store.approvals_for_mission(&mission_id)?,
            running: c.is_running(&mission_id),
            mission,
        })
    })
    .await
}

#[tauri::command]
pub async fn mission_events(core: CoreState<'_>, mission_id: String, after_id: Option<i64>, limit: Option<i64>) -> Result<Vec<ActivityEvent>> {
    blocking(&core, move |c| c.store.events(&mission_id, after_id.unwrap_or(0), limit.unwrap_or(2000).clamp(1, 5000))).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MissionChanges {
    pub files: Vec<git::ChangedFile>,
    pub safety: git::WorktreeSafety,
}

#[tauri::command]
pub async fn mission_changes(core: CoreState<'_>, mission_id: String) -> Result<MissionChanges> {
    blocking(&core, move |c| {
        let m = c.store.get_mission(&mission_id)?;
        let safety = c.worktree_safety(&mission_id)?;
        let (Some(wt), Some(base)) = (m.worktree_path.as_deref(), m.base_commit.as_deref()) else {
            return Ok(MissionChanges { files: vec![], safety });
        };
        if !Path::new(wt).exists() {
            return Ok(MissionChanges { files: vec![], safety });
        }
        Ok(MissionChanges { files: git::changed_files(Path::new(wt), base)?, safety })
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MissionDiff {
    pub diff: String,
    pub truncated: bool,
}

#[tauri::command]
pub async fn mission_diff(core: CoreState<'_>, mission_id: String) -> Result<MissionDiff> {
    blocking(&core, move |c| {
        let m = c.store.get_mission(&mission_id)?;
        let (Some(wt), Some(base)) = (m.worktree_path.as_deref(), m.base_commit.as_deref()) else {
            return Ok(MissionDiff { diff: String::new(), truncated: false });
        };
        if !Path::new(wt).exists() {
            return Ok(MissionDiff { diff: String::new(), truncated: false });
        }
        let d = git::diff(Path::new(wt), base)?;
        let (diff, truncated) = crate::redact::truncate(&d, 4 * 1024 * 1024);
        Ok(MissionDiff { diff, truncated })
    })
    .await
}

#[tauri::command]
pub async fn open_worktree_in_editor(core: CoreState<'_>, mission_id: String) -> Result<()> {
    blocking(&core, move |c| {
        let m = c.store.get_mission(&mission_id)?;
        let wt = m.worktree_path.ok_or_else(|| ForgeError::NotFound("this mission has no worktree yet".into()))?;
        let code: PathBuf = which::which("code").map_err(|_| ForgeError::Other("VS Code (`code`) was not found on PATH".into()))?;
        let mut cmd = std::process::Command::new(code);
        cmd.arg(&wt);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        cmd.spawn()?;
        Ok(())
    })
    .await
}

// ---- approvals -------------------------------------------------------------------------

#[tauri::command]
pub async fn approvals_pending(core: CoreState<'_>, mission_id: Option<String>) -> Result<Vec<Approval>> {
    blocking(&core, move |c| c.store.pending_approvals(mission_id.as_deref())).await
}

#[tauri::command]
pub async fn approval_decide(core: CoreState<'_>, approval_id: String, approved: bool, note: Option<String>, binding: Option<String>) -> Result<Approval> {
    // The card the user decided on carries the binding it showed; a request that is not that one is refused.
    blocking(&core, move |c| c.decide_approval_checked(&approval_id, approved, note.as_deref(), binding.as_deref())).await
}

// ---- Phase 2: contracts, verification, review, merge, proof ---------------------------------

use crate::contract::{CheckSpec, Contract};
use crate::proof::VerificationReport;
use crate::store_review::{ProofPackRow, VerificationRun};
use crate::supervisor::{MergeOutcome, ReviewDecision};

#[tauri::command]
pub async fn contract_get(core: CoreState<'_>, mission_id: String) -> Result<Contract> {
    blocking(&core, move |c| c.get_contract(&mission_id)).await
}

#[tauri::command]
pub async fn contract_set(core: CoreState<'_>, mission_id: String, contract: Contract) -> Result<Mission> {
    blocking(&core, move |c| c.set_contract(&mission_id, contract)).await
}

#[tauri::command]
pub async fn checks_suggest(core: CoreState<'_>, project_id: String) -> Result<Vec<CheckSpec>> {
    blocking(&core, move |c| c.suggest_checks(&project_id)).await
}

#[tauri::command]
pub async fn verification_report(core: CoreState<'_>, mission_id: String) -> Result<VerificationReport> {
    blocking(&core, move |c| c.verification_report(&mission_id)).await
}

#[tauri::command]
pub async fn verification_runs(core: CoreState<'_>, mission_id: String) -> Result<Vec<VerificationRun>> {
    blocking(&core, move |c| c.store.runs_for_mission(&mission_id)).await
}

#[tauri::command]
pub async fn check_start(core: CoreState<'_>, mission_id: String, check_id: String) -> Result<VerificationRun> {
    blocking(&core, move |c| c.start_check(&mission_id, &check_id)).await
}

#[tauri::command]
pub async fn check_cancel(core: CoreState<'_>, mission_id: String) -> Result<()> {
    blocking(&core, move |c| c.cancel_check(&mission_id)).await
}

#[tauri::command]
pub async fn verdict_set(core: CoreState<'_>, mission_id: String, criterion_id: String, pass: bool, note: Option<String>, expected_fingerprint: String) -> Result<()> {
    blocking(&core, move |c| c.set_manual_verdict(&mission_id, &criterion_id, pass, note.as_deref(), &expected_fingerprint)).await
}

#[tauri::command]
pub async fn review_submit(core: CoreState<'_>, mission_id: String, decision: String, note: Option<String>, override_reason: Option<String>, seen: Option<crate::supervisor::Seen>) -> Result<Mission> {
    blocking(&core, move |c| {
        let d = ReviewDecision::parse(&decision).ok_or_else(|| ForgeError::Other(format!("unknown review decision \"{decision}\"")))?;
        c.review(&mission_id, d, note.as_deref(), override_reason.as_deref(), seen.as_ref())
    })
    .await
}

#[tauri::command]
pub async fn mission_merge(core: CoreState<'_>, mission_id: String, squash: bool) -> Result<MergeOutcome> {
    blocking(&core, move |c| c.merge_mission(&mission_id, squash)).await
}

#[tauri::command]
pub async fn mission_cleanup(core: CoreState<'_>, mission_id: String, confirm_loss: bool) -> Result<git::WorktreeSafety> {
    blocking(&core, move |c| c.cleanup_workspace(&mission_id, confirm_loss)).await
}

#[tauri::command]
pub async fn proofpack_generate(core: CoreState<'_>, mission_id: String) -> Result<ProofPackRow> {
    blocking(&core, move |c| c.generate_proof_pack(&mission_id)).await
}

#[tauri::command]
pub async fn proofpack_list(core: CoreState<'_>, mission_id: String) -> Result<Vec<ProofPackRow>> {
    blocking(&core, move |c| c.store.proof_packs(&mission_id)).await
}

// ---- Phase 3: knowledge (memory) --------------------------------------------------------------

use crate::knowledge::lint::Issue;
use crate::knowledge::retrieve::ContextPackage;
use crate::knowledge::service::{FreshnessReport, ScanReport};
use crate::knowledge::store::{ContextPackageRow, KPage, KRevision};
use crate::supervisor::{KnowledgeStatus, NewKnowledgeArgs, PageDetail};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub scan: ScanReport,
    pub freshness: FreshnessReport,
}

#[tauri::command]
pub async fn knowledge_status(core: CoreState<'_>, project_id: String) -> Result<KnowledgeStatus> {
    blocking(&core, move |c| c.knowledge_status(&project_id)).await
}

#[tauri::command]
pub async fn knowledge_scan(core: CoreState<'_>, project_id: String) -> Result<ScanResult> {
    blocking(&core, move |c| c.knowledge_scan(&project_id).map(|(scan, freshness)| ScanResult { scan, freshness })).await
}

#[tauri::command]
pub async fn knowledge_rebuild_index(core: CoreState<'_>, project_id: String) -> Result<usize> {
    blocking(&core, move |c| c.knowledge_rebuild_index(&project_id)).await
}

#[tauri::command]
pub async fn knowledge_lint(core: CoreState<'_>, project_id: String) -> Result<Vec<Issue>> {
    blocking(&core, move |c| c.knowledge_lint(&project_id)).await
}

#[tauri::command]
pub async fn knowledge_vault_set(core: CoreState<'_>, project_id: String, path: Option<String>, write: bool) -> Result<KnowledgeStatus> {
    blocking(&core, move |c| c.knowledge_set_vault(&project_id, path.as_deref().map(Path::new), write)).await
}

#[tauri::command]
pub async fn knowledge_list(core: CoreState<'_>, project_id: String, query: Option<String>) -> Result<Vec<KPage>> {
    blocking(&core, move |c| c.knowledge_pages(&project_id, query.as_deref())).await
}

#[tauri::command]
pub async fn knowledge_get(core: CoreState<'_>, page_id: String) -> Result<PageDetail> {
    blocking(&core, move |c| c.knowledge_page_detail(&page_id)).await
}

#[tauri::command]
pub async fn knowledge_create(core: CoreState<'_>, project_id: String, args: NewKnowledgeArgs) -> Result<KPage> {
    blocking(&core, move |c| c.knowledge_create(&project_id, args)).await
}

#[tauri::command]
pub async fn knowledge_save_from_event(core: CoreState<'_>, mission_id: String, event_id: i64, title: String, kind: String) -> Result<KPage> {
    blocking(&core, move |c| c.knowledge_save_from_event(&mission_id, event_id, &title, &kind)).await
}

#[tauri::command]
pub async fn knowledge_propose_revision(core: CoreState<'_>, page_id: String, text: String, note: Option<String>) -> Result<KRevision> {
    blocking(&core, move |c| c.knowledge_propose_revision(&page_id, &text, note.as_deref())).await
}

#[tauri::command]
pub async fn knowledge_apply_revision(core: CoreState<'_>, revision_id: String) -> Result<KPage> {
    blocking(&core, move |c| c.knowledge_apply_revision(&revision_id)).await
}

#[tauri::command]
pub async fn knowledge_reject_revision(core: CoreState<'_>, revision_id: String) -> Result<()> {
    blocking(&core, move |c| c.knowledge_reject_revision(&revision_id)).await
}

#[tauri::command]
pub async fn knowledge_verify(core: CoreState<'_>, page_id: String, expected_sha256: String, note: Option<String>) -> Result<KPage> {
    blocking(&core, move |c| c.knowledge_verify(&page_id, &expected_sha256, note.as_deref())).await
}

#[tauri::command]
pub async fn knowledge_set_status(core: CoreState<'_>, page_id: String, action: String, note: Option<String>) -> Result<KPage> {
    blocking(&core, move |c| c.knowledge_set_status(&page_id, &action, note.as_deref())).await
}

#[tauri::command]
pub async fn knowledge_supersede(core: CoreState<'_>, old_id: String, new_id: String, note: Option<String>) -> Result<KPage> {
    blocking(&core, move |c| c.knowledge_supersede(&old_id, &new_id, note.as_deref())).await
}

#[tauri::command]
pub async fn knowledge_forget(core: CoreState<'_>, page_id: String) -> Result<()> {
    blocking(&core, move |c| c.knowledge_forget(&page_id)).await
}

#[tauri::command]
pub async fn knowledge_open_in_obsidian(core: CoreState<'_>, page_id: String) -> Result<()> {
    blocking(&core, move |c| {
        let uri = c.knowledge_open_in_obsidian(&page_id)?;
        // rundll32 hands the URI to the registered handler without any shell parsing of `%` or `&`.
        let mut cmd = std::process::Command::new("rundll32");
        cmd.args(["url.dll,FileProtocolHandler", &uri]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000);
        }
        cmd.spawn()?;
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn context_preview(core: CoreState<'_>, project_id: String, objective: String, hints: Vec<String>, pins: Vec<String>) -> Result<ContextPackage> {
    blocking(&core, move |c| c.context_preview(&project_id, &objective, &hints, &pins)).await
}

#[tauri::command]
pub async fn context_for_mission(core: CoreState<'_>, mission_id: String) -> Result<Vec<ContextPackageRow>> {
    blocking(&core, move |c| c.store.k_contexts_for(&mission_id)).await
}

#[tauri::command]
pub async fn mission_memory_set(core: CoreState<'_>, mission_id: String, use_memory: bool, pins: Vec<String>) -> Result<Mission> {
    blocking(&core, move |c| c.set_mission_memory(&mission_id, use_memory, &pins)).await
}

// ---- Phase 4: Harness Studio ----------------------------------------------------------------------------------

use crate::harness::{doctor::Finding, edit::{FileRead, Preview}, hooks::{DryRun, HookTemplate}, inventory::Inventory, store::HarnessChange};
use crate::supervisor::{BackupUsage, HookSpec, PruneReport, RollbackPreview};

#[tauri::command]
pub async fn harness_inventory(core: CoreState<'_>, project_id: String) -> Result<Inventory> {
    blocking(&core, move |c| c.harness_inventory(&project_id)).await
}

#[tauri::command]
pub async fn harness_doctor(core: CoreState<'_>, project_id: String) -> Result<Vec<Finding>> {
    blocking(&core, move |c| c.harness_doctor(&project_id)).await
}

#[tauri::command]
pub async fn harness_read(core: CoreState<'_>, project_id: String, rel: String) -> Result<FileRead> {
    blocking(&core, move |c| c.harness_read(&project_id, &rel)).await
}

#[tauri::command]
pub async fn harness_preview(core: CoreState<'_>, project_id: String, rel: String, text: String) -> Result<Preview> {
    blocking(&core, move |c| c.harness_preview(&project_id, &rel, &text)).await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessApplyArgs {
    pub project_id: String,
    pub rel: String,
    pub text: String,
    /// Hash of the file the user was looking at; absent when they were creating it.
    pub expected_sha256: Option<String>,
    /// Hash of the preview they approved.
    pub expected_after_sha256: String,
    #[serde(default)]
    pub acknowledge_risks: bool,
    pub note: Option<String>,
}

#[tauri::command]
pub async fn harness_apply(core: CoreState<'_>, args: HarnessApplyArgs) -> Result<HarnessChange> {
    blocking(&core, move |c| {
        c.harness_apply(&args.project_id, &args.rel, &args.text, args.expected_sha256.as_deref(), &args.expected_after_sha256, args.acknowledge_risks, args.note.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn harness_history(core: CoreState<'_>, project_id: String, rel: Option<String>) -> Result<Vec<HarnessChange>> {
    blocking(&core, move |c| c.harness_history(&project_id, rel.as_deref())).await
}

#[tauri::command]
pub async fn harness_rollback_preview(core: CoreState<'_>, change_id: String) -> Result<RollbackPreview> {
    blocking(&core, move |c| c.harness_rollback_preview(&change_id)).await
}

#[tauri::command]
pub async fn harness_rollback(core: CoreState<'_>, change_id: String) -> Result<HarnessChange> {
    blocking(&core, move |c| c.harness_rollback(&change_id)).await
}

/// Disk used by this project's Harness Studio backups and what pruning with `keep_per_file` undo steps would free. Read-only.
#[tauri::command]
pub async fn harness_backup_usage(core: CoreState<'_>, project_id: String, keep_per_file: Option<usize>) -> Result<BackupUsage> {
    blocking(&core, move |c| c.harness_backup_usage(&project_id, keep_per_file)).await
}

/// Remove backups that are no longer needed for crash recovery, the newest undo steps, or a rollback possible right now.
#[tauri::command]
pub async fn harness_prune_backups(core: CoreState<'_>, project_id: String, keep_per_file: usize) -> Result<PruneReport> {
    blocking(&core, move |c| c.harness_prune_backups(&project_id, keep_per_file)).await
}

#[tauri::command]
pub async fn harness_mark_reviewed(core: CoreState<'_>, project_id: String, rel: String) -> Result<()> {
    blocking(&core, move |c| c.harness_mark_reviewed(&project_id, &rel)).await
}

#[tauri::command]
pub async fn harness_hook_templates(core: CoreState<'_>) -> Result<Vec<HookTemplate>> {
    blocking(&core, move |c| Ok(c.harness_hook_templates())).await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookSpecArgs {
    #[serde(default)]
    pub event: String,
    pub matcher: Option<String>,
    pub command: Option<String>,
    pub timeout_secs: Option<u64>,
    pub template_id: Option<String>,
    #[serde(default)]
    pub params: std::collections::HashMap<String, String>,
}

#[tauri::command]
pub async fn harness_hook_preview_add(core: CoreState<'_>, project_id: String, spec: HookSpecArgs) -> Result<Preview> {
    blocking(&core, move |c| {
        c.harness_hook_preview_add(&project_id, HookSpec { event: spec.event, matcher: spec.matcher, command: spec.command, timeout_secs: spec.timeout_secs, template_id: spec.template_id, params: spec.params })
    })
    .await
}

#[tauri::command]
pub async fn harness_hook_preview_remove(core: CoreState<'_>, project_id: String, hook_id: String) -> Result<Preview> {
    blocking(&core, move |c| c.harness_hook_preview_remove(&project_id, &hook_id)).await
}

#[tauri::command]
pub async fn harness_hook_dry_run(core: CoreState<'_>, project_id: String, event: String, command: String) -> Result<DryRun> {
    blocking(&core, move |c| c.harness_hook_dry_run(&project_id, &event, &command)).await
}

// ---- Phase 6: Experiments ---------------------------------------------------------------------------------------

use crate::experiment::NewExperiment;
use crate::store_experiment::Experiment;
use crate::supervisor::ExperimentDetail;

/// Define an experiment: every revision is resolved to an exact commit now. Nothing runs.
#[tauri::command]
pub async fn experiment_create(core: CoreState<'_>, project_id: String, spec: NewExperiment) -> Result<Experiment> {
    blocking(&core, move |c| c.experiment_create(&project_id, spec)).await
}

#[tauri::command]
pub async fn experiment_list(core: CoreState<'_>, project_id: String) -> Result<Vec<Experiment>> {
    blocking(&core, move |c| c.experiment_list(&project_id)).await
}

#[tauri::command]
pub async fn experiment_get(core: CoreState<'_>, experiment_id: String) -> Result<ExperimentDetail> {
    blocking(&core, move |c| c.experiment_get(&experiment_id)).await
}

/// Runs the user's command against every revision (explicit user action; executes repository code). Returns when the attempt ends.
#[tauri::command]
pub async fn experiment_run(core: CoreState<'_>, experiment_id: String) -> Result<ExperimentDetail> {
    blocking(&core, move |c| c.experiment_run(&experiment_id)).await
}

#[tauri::command]
pub async fn experiment_cancel(core: CoreState<'_>, experiment_id: String) -> Result<()> {
    blocking(&core, move |c| c.experiment_cancel(&experiment_id)).await
}

// ---- Phase 5: conflicts between missions ---------------------------------------------------------------------------

use crate::supervisor::ConflictReport;

/// File overlap, predicted textual conflicts and base drift across the project's active missions. Read-only.
#[tauri::command]
pub async fn mission_conflicts(core: CoreState<'_>, project_id: String) -> Result<ConflictReport> {
    blocking(&core, move |c| c.mission_conflicts(&project_id)).await
}

// ---- Execution backends ------------------------------------------------------------------------------------------

/// Every backend with what it enforces and whether it can run on this machine (probed, not assumed).
#[tauri::command]
pub async fn execution_backends(core: CoreState<'_>) -> Result<Vec<crate::execution::BackendInfo>> {
    blocking(&core, move |c| Ok(c.execution_backends())).await
}

#[tauri::command]
pub async fn project_set_execution_backend(core: CoreState<'_>, project_id: String, backend: String) -> Result<Project> {
    blocking(&core, move |c| c.project_set_execution_backend(&project_id, &backend)).await
}

#[tauri::command]
pub async fn project_set_agent_backend(core: CoreState<'_>, project_id: String, backend: String) -> Result<Project> {
    blocking(&core, move |c| c.project_set_agent_backend(&project_id, &backend)).await
}

// ---- Agents overview -------------------------------------------------------------------------------------------------

use crate::supervisor::AgentsOverview;

/// Recent agent sessions (and anything running), with provider-reported usage per provider. Read-only.
#[tauri::command]
pub async fn agents_overview(core: CoreState<'_>, days: u32) -> Result<AgentsOverview> {
    blocking(&core, move |c| c.agents_overview(days)).await
}

// ---- MCP authorization -----------------------------------------------------------------------------------------------

use crate::mcp::McpAuthorization;
use crate::supervisor::{McpAuthorizationStatus, McpPreview};

#[tauri::command]
pub async fn mcp_preview(core: CoreState<'_>, project_id: String, name: String) -> Result<McpPreview> {
    blocking(&core, move |c| c.mcp_preview(&project_id, &name)).await
}

#[tauri::command]
pub async fn mcp_authorize(core: CoreState<'_>, project_id: String, name: String, definition_sha256: String, tool_allowlist: Vec<String>) -> Result<McpAuthorization> {
    blocking(&core, move |c| c.mcp_authorize(&project_id, &name, &definition_sha256, &tool_allowlist)).await
}

#[tauri::command]
pub async fn mcp_revoke(core: CoreState<'_>, authorization_id: String) -> Result<McpAuthorization> {
    blocking(&core, move |c| c.mcp_revoke(&authorization_id)).await
}

#[tauri::command]
pub async fn mcp_authorizations(core: CoreState<'_>, project_id: String) -> Result<Vec<McpAuthorizationStatus>> {
    blocking(&core, move |c| c.mcp_authorizations(&project_id)).await
}

// ---- ACP agents ------------------------------------------------------------------------------------------------------

use crate::acp::AcpAgent;

#[tauri::command]
pub async fn acp_agents(core: CoreState<'_>) -> Result<Vec<AcpAgent>> {
    blocking(&core, |c| c.acp_agents()).await
}

/// Registers a program FORGE will start for ACP missions. Explicit user action only.
#[tauri::command]
pub async fn acp_agent_register(core: CoreState<'_>, name: String, command: String, args: Vec<String>) -> Result<AcpAgent> {
    blocking(&core, move |c| c.acp_agent_register(&name, &command, &args)).await
}

#[tauri::command]
pub async fn acp_agent_remove(core: CoreState<'_>, id: String) -> Result<AcpAgent> {
    blocking(&core, move |c| c.acp_agent_remove(&id)).await
}

/// The verification adapters this build understands (id, label, suggested command, kind).
#[tauri::command]
pub fn verification_adapters() -> serde_json::Value {
    crate::evidence::adapters_json()
}

// ---- Code intelligence -----------------------------------------------------------------------------------------------

use crate::codeintel::{CodeMap, SymbolHit};

/// Per-directory summary of the project's symbol index (built on demand, cached in memory per HEAD + working tree).
#[tauri::command]
pub async fn code_map(core: CoreState<'_>, project_id: String) -> Result<CodeMap> {
    blocking(&core, move |c| c.code_map(&project_id)).await
}

/// Symbols by name, or every symbol of `file` when `query` is empty.
#[tauri::command]
pub async fn code_symbols(core: CoreState<'_>, project_id: String, query: String, file: Option<String>, limit: usize) -> Result<Vec<SymbolHit>> {
    blocking(&core, move |c| c.code_symbols(&project_id, &query, file.as_deref(), limit)).await
}
