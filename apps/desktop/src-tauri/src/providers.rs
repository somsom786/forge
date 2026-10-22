//! Provider detection and the honest capability matrix shown in the UI.
//! "Installed" and "signed in" are verified by asking the CLI itself, with a hard timeout.

use serde::Serialize;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInfo {
    pub id: String,
    pub name: String,
    pub installed: bool,
    pub path: Option<String>,
    pub version: Option<String>,
    /// Some(true/false) when the CLI can tell us; None when unknown.
    pub signed_in: Option<bool>,
    /// How the provider bills, as reported by the CLI (never guessed).
    pub auth_method: Option<String>,
    pub plan: Option<String>,
    /// Whether FORGE has a working adapter for this provider in this build.
    pub adapter_available: bool,
    pub notes: Vec<String>,
    pub capabilities: Value,
}

fn run_timeout(program: &Path, args: &[&str], secs: u64) -> Option<(bool, String)> {
    let mut cmd = Command::new(program);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd.spawn().ok()?;
    let t0 = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(st)) => {
                let out = child.wait_with_output().ok()?;
                let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
                text.push_str(&String::from_utf8_lossy(&out.stderr));
                return Some((st.success(), text));
            }
            Ok(None) if t0.elapsed() > Duration::from_secs(secs) => {
                let _ = child.kill();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(30)),
            Err(_) => return None,
        }
    }
}

fn unsupported(note: &str) -> Value {
    json!({"supported": false, "note": note})
}

pub fn claude_capabilities() -> Value {
    json!({
        "sessionStart": {"supported": true},
        "resume": {"supported": true, "note": "Resumes the provider transcript, not process state."},
        "streaming": {"supported": true},
        "interrupt": {"supported": true},
        "toolEvents": {"supported": true},
        "fileChanges": {"supported": true, "note": "Derived from tool results; the git diff is authoritative."},
        "approvals": {"supported": true},
        "usageReporting": {"supported": true, "note": "Token and cost figures are provider estimates."},
        "modelSelection": {"supported": true},
        "worktreeIntegration": {"supported": true}
    })
}

pub fn codex_capabilities() -> Value {
    let n = "Adapter not implemented yet (planned: Codex app-server).";
    json!({
        "sessionStart": unsupported(n), "resume": unsupported(n), "streaming": unsupported(n),
        "interrupt": unsupported(n), "toolEvents": unsupported(n), "fileChanges": unsupported(n),
        "approvals": unsupported(n), "usageReporting": unsupported(n), "modelSelection": unsupported(n),
        "worktreeIntegration": unsupported(n)
    })
}

pub fn detect_claude(path: Option<&PathBuf>) -> ProviderInfo {
    let mut info = ProviderInfo {
        id: "claude".into(),
        name: "Claude Code".into(),
        installed: path.is_some(),
        path: path.map(|p| p.to_string_lossy().into_owned()),
        version: None,
        signed_in: None,
        auth_method: None,
        plan: None,
        adapter_available: true,
        notes: vec![],
        capabilities: claude_capabilities(),
    };
    let Some(p) = path else {
        info.notes.push("Claude Code was not found on PATH.".into());
        return info;
    };
    info.version = run_timeout(p, &["--version"], 10).map(|(_, t)| t.trim().to_string());
    if let Some((_, text)) = run_timeout(p, &["auth", "status"], 15) {
        if let Ok(v) = serde_json::from_str::<Value>(text.trim()) {
            info.signed_in = v.get("loggedIn").and_then(Value::as_bool);
            info.auth_method = v.get("authMethod").and_then(Value::as_str).map(str::to_string);
            info.plan = v.get("subscriptionType").and_then(Value::as_str).map(str::to_string);
        }
    }
    if info.signed_in == Some(false) {
        info.notes.push("Not signed in. Run `claude auth login` in a terminal.".into());
    }
    if std::env::var_os("ANTHROPIC_API_KEY").is_some() {
        info.notes.push("ANTHROPIC_API_KEY is set in your environment. FORGE strips it from missions so they never bill the API silently.".into());
    }
    info
}

pub fn detect_codex(path: Option<PathBuf>) -> ProviderInfo {
    let mut info = ProviderInfo {
        id: "codex".into(),
        name: "OpenAI Codex".into(),
        installed: path.is_some(),
        path: path.as_ref().map(|p| p.to_string_lossy().into_owned()),
        version: None,
        signed_in: None,
        auth_method: None,
        plan: None,
        adapter_available: false,
        notes: vec!["Detected for visibility only; missions cannot use Codex until its adapter ships.".into()],
        capabilities: codex_capabilities(),
    };
    if let Some(p) = &path {
        info.version = run_timeout(p, &["--version"], 10).map(|(_, t)| t.trim().to_string());
        if let Some((_, t)) = run_timeout(p, &["login", "status"], 15) {
            let lower = t.to_lowercase();
            info.signed_in = Some(lower.contains("logged in"));
            if lower.contains("chatgpt") {
                info.auth_method = Some("chatgpt".into());
            } else if lower.contains("api key") {
                info.auth_method = Some("api_key".into());
            }
        }
    }
    info
}
