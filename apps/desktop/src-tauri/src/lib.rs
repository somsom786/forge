pub mod codeintel;
pub mod commands;
pub mod conflicts;
pub mod contract;
pub mod db;
pub mod detect;
pub mod error;
pub mod execution;
#[cfg(all(test, target_os = "linux"))]
mod execution_tests;
pub mod experiment;
pub mod git;
pub mod acp;
pub mod evidence;
pub mod mcp;
pub mod merge;
pub mod harness;
pub mod knowledge;
pub mod mission;
pub mod paths;
pub mod proof;
pub mod policy;
pub mod proc;
pub mod providers;
pub mod redact;
pub mod store;
pub mod store_review;
pub mod store_experiment;
pub mod verify;
pub mod supervisor;

use std::path::PathBuf;
use std::sync::Arc;
use supervisor::{Config, Core};
use tauri::{Emitter, Manager};

fn forge_home() -> PathBuf {
    if let Some(h) = std::env::var_os("FORGE_HOME") {
        return PathBuf::from(h);
    }
    dirs_home().join(".forge")
}

fn dirs_home() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Where the provider engine lives. Every candidate is normalised with `dunce`: Tauri hands out verbatim `\?\C:\...`
/// paths, which Node's module loader cannot resolve (`EISDIR: lstat 'C:'`).
fn locate_engine(app: &tauri::App) -> Option<PathBuf> {
    let norm = |p: PathBuf| dunce::canonicalize(&p).ok();
    if let Some(p) = std::env::var_os("FORGE_ENGINE_PATH") {
        return norm(PathBuf::from(p));
    }
    // Development checkout first: it has node_modules next to it, which a copied resource folder does not.
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../packages/engine/dist/main.js");
    if cfg!(debug_assertions) {
        if let Some(p) = norm(dev.clone()) {
            return Some(p);
        }
    }
    // Packaged: bundled next to the app resources.
    if let Ok(res) = app.path().resource_dir() {
        if let Some(p) = norm(res.join("engine").join("main.js")) {
            return Some(p);
        }
    }
    norm(dev)
}

/// Node runtime for the engine: an explicit override, then (installed app) the copy shipped next to the engine, then PATH.
/// A development build prefers PATH so `npm run dev` uses whatever Node the developer has.
fn locate_node(app: &tauri::App) -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("FORGE_NODE_PATH") {
        return dunce::canonicalize(PathBuf::from(p)).ok();
    }
    if cfg!(debug_assertions) {
        if let Ok(p) = which::which("node") {
            return Some(p);
        }
    }
    let exe = if cfg!(windows) { "node.exe" } else { "node" };
    if let Ok(res) = app.path().resource_dir() {
        if let Ok(p) = dunce::canonicalize(res.join("node").join(exe)) {
            return Some(p);
        }
    }
    which::which("node").ok()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// One FORGE per home directory. A second copy would run startup recovery against the first one's live state: mark its
/// running missions interrupted, expire the approvals its agents are waiting on, and race it to merge. The OS drops the lock
/// when the process ends (also on a crash), so a stale lock file never blocks a restart.
pub fn acquire_instance_lock(home: &std::path::Path) -> std::result::Result<std::fs::File, String> {
    let path = home.join("forge.lock");
    let f = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    match f.try_lock() {
        Ok(()) => Ok(f),
        Err(std::fs::TryLockError::WouldBlock) => Err(format!("FORGE is already running with the data folder {}", home.display())),
        Err(std::fs::TryLockError::Error(e)) => Err(format!("{}: {e}", path.display())),
    }
}

struct InstanceLock(#[allow(dead_code)] std::fs::File);

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let home = forge_home();
            std::fs::create_dir_all(&home)?;
            // Held (managed by Tauri) for the life of the app.
            let lock = acquire_instance_lock(&home).map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
            app.manage(InstanceLock(lock));
            let store = store::Store::open(&home.join("forge.db"))?;
            let handle = app.handle().clone();
            let cfg = Config {
                home,
                node: locate_node(app),
                engine_script: locate_engine(app),
                claude: which::which("claude").ok(),
                codex: providers::locate_codex(),
                allow_fake: cfg!(debug_assertions) || std::env::var("FORGE_ALLOW_FAKE_PROVIDER").as_deref() == Ok("1"),
                max_concurrent: 3,
                ollama_url: std::env::var("FORGE_OLLAMA_URL").unwrap_or_else(|_| "http://127.0.0.1:11434".into()),
                engine_env: vec![],
            };
            let core = Core::new(store, cfg, Arc::new(move |ev| {
                let _ = handle.emit("forge://event", &ev);
            }));
            match core.recover() {
                Ok(r) if !r.interrupted_missions.is_empty() => {
                    eprintln!("[forge] recovered: {} mission(s) marked interrupted", r.interrupted_missions.len());
                }
                Ok(_) => {}
                Err(e) => eprintln!("[forge] recovery failed: {e}"),
            }
            app.manage(core);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::recovery_report,
            commands::providers_detect,
            commands::project_add,
            commands::project_list,
            commands::project_remove,
            commands::project_status,
            commands::mission_create,
            commands::mission_start,
            commands::mission_followup,
            commands::mission_stop,
            commands::mission_discard,
            commands::mission_list,
            commands::mission_get,
            commands::mission_events,
            commands::mission_changes,
            commands::mission_diff,
            commands::open_worktree_in_editor,
            commands::approvals_pending,
            commands::approval_decide,
            commands::contract_get,
            commands::contract_set,
            commands::checks_suggest,
            commands::verification_report,
            commands::verification_runs,
            commands::check_start,
            commands::check_cancel,
            commands::verdict_set,
            commands::review_submit,
            commands::mission_merge,
            commands::mission_cleanup,
            commands::proofpack_generate,
            commands::proofpack_list,
            commands::knowledge_status,
            commands::knowledge_scan,
            commands::knowledge_rebuild_index,
            commands::knowledge_lint,
            commands::knowledge_vault_set,
            commands::knowledge_list,
            commands::knowledge_get,
            commands::knowledge_create,
            commands::knowledge_save_from_event,
            commands::knowledge_propose_revision,
            commands::knowledge_apply_revision,
            commands::knowledge_reject_revision,
            commands::knowledge_verify,
            commands::knowledge_set_status,
            commands::knowledge_supersede,
            commands::knowledge_forget,
            commands::harness_inventory,
            commands::harness_doctor,
            commands::harness_read,
            commands::harness_preview,
            commands::harness_apply,
            commands::harness_history,
            commands::harness_rollback_preview,
            commands::harness_rollback,
            commands::harness_mark_reviewed,
            commands::harness_backup_usage,
            commands::harness_prune_backups,
            commands::harness_hook_templates,
            commands::harness_hook_preview_add,
            commands::harness_hook_preview_remove,
            commands::harness_hook_dry_run,
            commands::experiment_create,
            commands::experiment_list,
            commands::experiment_get,
            commands::experiment_run,
            commands::experiment_cancel,
            commands::mission_conflicts,
            commands::execution_backends,
            commands::project_set_execution_backend,
            commands::project_set_agent_backend,
            commands::storage_usage,
            commands::storage_cleanup_preview,
            commands::storage_cleanup_apply,
            commands::code_map,
            commands::code_symbols,
            commands::mcp_preview,
            commands::mcp_authorize,
            commands::mcp_revoke,
            commands::mcp_authorizations,
            commands::acp_agents,
            commands::acp_agent_register,
            commands::acp_agent_remove,
            commands::verification_adapters,
            commands::agents_overview,
            commands::knowledge_open_in_obsidian,
            commands::context_preview,
            commands::context_for_mission,
            commands::mission_memory_set,
        ])
        .run(tauri::generate_context!())
        .expect("error while running FORGE");
}

#[cfg(test)]
mod instance_lock_tests {
    #[test]
    fn a_second_instance_on_the_same_home_is_refused_and_the_lock_is_released_on_exit() {
        let home = tempfile::tempdir().unwrap();
        let first = super::acquire_instance_lock(home.path()).unwrap();
        let err = super::acquire_instance_lock(home.path()).unwrap_err();
        assert!(err.contains("already running"), "{err}");
        drop(first);
        assert!(super::acquire_instance_lock(home.path()).is_ok(), "the lock goes away with its holder");
    }
}
