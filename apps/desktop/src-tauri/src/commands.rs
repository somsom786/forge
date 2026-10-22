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
            providers::detect_codex(which::which("codex").ok()),
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
pub async fn approval_decide(core: CoreState<'_>, approval_id: String, approved: bool, note: Option<String>) -> Result<Approval> {
    blocking(&core, move |c| c.decide_approval(&approval_id, approved, note.as_deref())).await
}
