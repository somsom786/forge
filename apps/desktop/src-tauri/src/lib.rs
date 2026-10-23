pub mod commands;
pub mod contract;
pub mod db;
pub mod detect;
pub mod error;
pub mod git;
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
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let home = forge_home();
            std::fs::create_dir_all(&home)?;
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
            commands::harness_hook_templates,
            commands::harness_hook_preview_add,
            commands::harness_hook_preview_remove,
            commands::harness_hook_dry_run,
            commands::knowledge_open_in_obsidian,
            commands::context_preview,
            commands::context_for_mission,
            commands::mission_memory_set,
        ])
        .run(tauri::generate_context!())
        .expect("error while running FORGE");
}
