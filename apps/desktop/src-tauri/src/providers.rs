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
    /// Selectable models, when the provider has a discoverable list (e.g. local Ollama models).
    pub models: Vec<String>,
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
    json!({
        "sessionStart": {"supported": true},
        "resume": {"supported": true, "note": "Resumes the Codex thread transcript, not process state."},
        "streaming": {"supported": true},
        "interrupt": {"supported": true},
        "toolEvents": {"supported": true},
        "fileChanges": {"supported": true, "note": "Derived from Codex file-change items; the git diff is authoritative."},
        "approvals": {"supported": true, "note": "Every command and file change is decided by FORGE; Codex's own sandbox stays on."},
        "usageReporting": {"supported": true, "note": "Token counts only; no cost is reported for a ChatGPT login."},
        "modelSelection": {"supported": true},
        "worktreeIntegration": {"supported": true}
    })
}

/// The Codex executable FORGE can spawn directly. `which` may find an npm `.cmd`/`.ps1` shim, which modern Node/Rust cannot
/// start without a shell (and FORGE never uses a shell for agents), so only a real `.exe` counts.
pub fn locate_codex() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("FORGE_CODEX_PATH") {
        return dunce::canonicalize(PathBuf::from(p)).ok();
    }
    let found = which::which("codex").ok()?;
    let is_exe = found.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe")) || !cfg!(windows);
    is_exe.then(|| dunce::simplified(&found).to_path_buf())
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
        models: vec![],
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
        adapter_available: path.is_some(),
        notes: vec!["Missions run `codex app-server` with approvals decided by FORGE, Codex's sandbox on, and MCP servers, plugins, apps and hooks disabled. Your ChatGPT login is used; no API key is.".into()],
        capabilities: codex_capabilities(),
        models: vec![],
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

// ---- local models (Ollama) ---------------------------------------------------------------

/// Plain `http://` on a loopback host only. A "local model" setting must never be able to send prompts off-box.
pub fn is_loopback_http(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("http://") else { return false };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.contains('@') {
        return false;
    }
    let host = if let Some(v6) = authority.strip_prefix('[') {
        v6.split(']').next().unwrap_or("")
    } else {
        authority.rsplit_once(':').map(|(h, _)| h).unwrap_or(authority)
    };
    matches!(host, "127.0.0.1" | "localhost" | "::1")
}

fn split_host_port(url: &str) -> Option<(String, u16)> {
    let rest = url.strip_prefix("http://")?;
    let authority = rest.split('/').next()?;
    if let Some(v6) = authority.strip_prefix('[') {
        let (h, tail) = v6.split_once(']')?;
        let port = tail.strip_prefix(':').and_then(|p| p.parse().ok()).unwrap_or(80);
        return Some((h.to_string(), port));
    }
    match authority.rsplit_once(':') {
        Some((h, p)) => Some((h.to_string(), p.parse().ok()?)),
        None => Some((authority.to_string(), 80)),
    }
}

/// Minimal HTTP/1.0 GET to a loopback server (HTTP/1.0 avoids chunked bodies). Returns the body on a 200.
fn http_get_loopback(url: &str, path: &str, secs: u64) -> Option<String> {
    use std::io::{Read, Write};
    use std::net::{TcpStream, ToSocketAddrs};
    if !is_loopback_http(url) {
        return None;
    }
    let (host, port) = split_host_port(url)?;
    let addr = (host.as_str(), port).to_socket_addrs().ok()?.next()?;
    let mut s = TcpStream::connect_timeout(&addr, Duration::from_secs(secs)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(secs))).ok()?;
    s.set_write_timeout(Some(Duration::from_secs(secs))).ok()?;
    let req = format!("GET {path} HTTP/1.0\r\nHost: {host}:{port}\r\nAccept: application/json\r\n\r\n");
    s.write_all(req.as_bytes()).ok()?;
    let mut raw = String::new();
    // Some peers close with a reset (notably on Windows) after sending a complete response; keep what arrived.
    if s.read_to_string(&mut raw).is_err() && raw.is_empty() {
        return None;
    }
    let (head, body) = raw.split_once("\r\n\r\n")?;
    head.starts_with("HTTP/1.0 200").then(|| body.to_string()).or_else(|| head.starts_with("HTTP/1.1 200").then(|| body.to_string()))
}

pub fn list_ollama_models(url: &str) -> Option<Vec<String>> {
    let body = http_get_loopback(url, "/api/tags", 3)?;
    let v: Value = serde_json::from_str(&body).ok()?;
    let mut names: Vec<String> = v
        .get("models")?
        .as_array()?
        .iter()
        .filter_map(|m| m.get("name").and_then(Value::as_str))
        .filter(|n| !n.contains("embed"))
        .map(str::to_string)
        .collect();
    // FORGE-prepared tags (long-context variants) first, then alphabetical.
    names.sort_by_key(|n| (!n.starts_with("forge-"), n.clone()));
    Some(names)
}

pub fn detect_claude_local(claude: Option<&PathBuf>, ollama_url: &str) -> ProviderInfo {
    let models = list_ollama_models(ollama_url);
    let mut notes = vec![
        "Runs the Claude Code harness against a model on this machine. No Anthropic usage or billing.".to_string(),
        "Quality and speed depend on the model; Claude Code's prompt needs a context window of at least ~16k tokens (use a `forge-*` tag or raise num_ctx).".to_string(),
    ];
    if claude.is_none() {
        notes.push("Requires the Claude Code CLI (not found on PATH).".into());
    }
    if models.is_none() {
        notes.push(format!("No local model server answered at {ollama_url}. Start Ollama to use this."));
    }
    ProviderInfo {
        id: "claude-local".into(),
        name: "Claude Code · local model (Ollama)".into(),
        installed: models.is_some() && claude.is_some(),
        path: Some(ollama_url.to_string()),
        version: None,
        signed_in: None,
        auth_method: Some("local".into()),
        plan: None,
        adapter_available: true,
        notes,
        capabilities: claude_capabilities(),
        models: models.unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_http_loopback_is_accepted() {
        for ok in ["http://127.0.0.1:11434", "http://localhost:11434", "http://[::1]:11434", "http://localhost", "http://127.0.0.1:1/x"] {
            assert!(is_loopback_http(ok), "{ok}");
        }
        for bad in [
            "https://127.0.0.1:11434",
            "http://example.com",
            "http://192.168.0.2:11434",
            "http://127.0.0.1.evil.com:11434",
            "http://evil.com/127.0.0.1",
            "http://user@evil.com:80@127.0.0.1",
            "http://127.0.0.1@evil.com",
            "http://0.0.0.0:11434",
            "ftp://localhost",
            "",
        ] {
            assert!(!is_loopback_http(bad), "{bad}");
        }
    }

    #[test]
    fn non_loopback_urls_are_never_contacted() {
        assert!(http_get_loopback("http://example.com", "/", 1).is_none());
        assert!(list_ollama_models("http://10.0.0.5:11434").is_none());
    }

    #[test]
    fn model_discovery_parses_a_server_response() {
        use std::io::{Read, Write};
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            // Read the whole request before answering, then close gracefully (an unread request makes Windows send RST).
            let mut seen = Vec::new();
            let mut buf = [0u8; 256];
            while !seen.windows(4).any(|w| w == b"\r\n\r\n") {
                match s.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => seen.extend_from_slice(&buf[..n]),
                }
            }
            let body = r#"{"models":[{"name":"qwen3:8b"},{"name":"embeddinggemma:300m"},{"name":"forge-qwen3.5-9b-32k:latest"},{"name":"gemma3:12b"}]}"#;
            let resp = format!("HTTP/1.0 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len());
            s.write_all(resp.as_bytes()).unwrap();
            let _ = s.shutdown(std::net::Shutdown::Write);
        });
        let names = list_ollama_models(&format!("http://127.0.0.1:{port}")).unwrap();
        assert_eq!(names, vec!["forge-qwen3.5-9b-32k:latest", "gemma3:12b", "qwen3:8b"], "embedding model hidden, forge tags first");
    }
}
