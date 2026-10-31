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

pub const CLAUDE_SDK_VERSION: &str = "0.3.309";

pub fn claude_sdk_install_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    #[cfg(not(windows))]
    let base = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/share")));
    base.filter(|p| p.is_absolute()).map(|p| p.join("FORGEProviderDeps").join("claude-sdk"))
}

fn valid_claude_sdk(path: &Path) -> bool {
    let p = std::fs::read_to_string(path.join("package.json")).ok().and_then(|s| serde_json::from_str::<Value>(&s).ok());
    p.is_some_and(|p| p["name"] == "@anthropic-ai/claude-agent-sdk" && p["version"] == CLAUDE_SDK_VERSION) && path.join("sdk.mjs").is_file()
}

/// Metadata only; never executes code or reads provider credentials. Explicit overrides never fall back.
pub fn locate_claude_sdk(engine: Option<&Path>) -> std::result::Result<PathBuf, String> {
    if let Some(p) = std::env::var_os("FORGE_CLAUDE_SDK_PATH") {
        let p = PathBuf::from(p);
        if p.is_absolute() && valid_claude_sdk(&p) { return dunce::canonicalize(p).map_err(|e| e.to_string()); }
        return Err(format!("FORGE_CLAUDE_SDK_PATH must name an installed @anthropic-ai/claude-agent-sdk {CLAUDE_SDK_VERSION} package directory"));
    }
    let mut candidates = vec![];
    if let Some(dir) = claude_sdk_install_dir() { candidates.push(dir.join("node_modules/@anthropic-ai/claude-agent-sdk")); }
    if let Some(engine) = engine {
        for parent in engine.ancestors().skip(1) { candidates.push(parent.join("node_modules/@anthropic-ai/claude-agent-sdk")); }
    }
    candidates.into_iter().find(|p| valid_claude_sdk(p)).and_then(|p| dunce::canonicalize(p).ok()).ok_or_else(|| {
        let dir = claude_sdk_install_dir().map(|p| p.display().to_string()).unwrap_or_else(|| "<provider-dependencies>".into());
        format!("Claude requires a separately installed SDK. Run npm install --prefix \"{dir}\" --save-exact @anthropic-ai/claude-agent-sdk@{CLAUDE_SDK_VERSION}, then refresh providers. FORGE does not install it automatically.")
    })
}

fn run_timeout(program: &Path, args: &[&str], secs: u64) -> Option<(bool, String)> {
    let mut cmd = Command::new(program);
    cmd.args(args);
    run_timeout_command(cmd, Duration::from_secs(secs))
}

fn run_timeout_command(mut cmd: Command, timeout: Duration) -> Option<(bool, String)> {
    use std::io::Read;
    use std::sync::mpsc;

    // Drain both pipes concurrently: waiting for the process first deadlocks when a pipe fills.
    // Reject oversized output rather than retaining unbounded data.
    const OUTPUT_LIMIT: u64 = 1024 * 1024;
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000 | crate::proc::CREATE_SUSPENDED);
    }
    let mut child = cmd.spawn().ok()?;
    #[cfg(windows)]
    let _containment = match crate::proc::contain(&child) {
        Ok(job) => job,
        Err(_) => { let _ = child.kill(); let _ = child.wait(); return None; }
    };
    #[cfg(windows)]
    if crate::proc::resume_suspended(&child).is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    }
    let t0 = Instant::now();
    let (tx, rx) = mpsc::channel();
    for (index, mut pipe) in [(0, Box::new(child.stdout.take()?) as Box<dyn Read + Send>), (1, Box::new(child.stderr.take()?) as Box<dyn Read + Send>)] {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let mut output = Vec::new();
            let result = (&mut pipe).take(OUTPUT_LIMIT + 1).read_to_end(&mut output);
            if result.is_ok() && output.len() as u64 <= OUTPUT_LIMIT {
                let _ = tx.send((index, output));
            }
        });
    }
    drop(tx);
    let mut status = None;
    let mut outputs = [None, None];
    loop {
        while let Ok((index, output)) = rx.try_recv() { outputs[index] = Some(output); }
        if status.is_none() {
            match child.try_wait() {
                Ok(st) => status = st,
                Err(_) => break,
            }
        }
        if let (Some(st), [Some(stdout), Some(stderr)]) = (status, &outputs) {
            let mut text = String::from_utf8_lossy(stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(stderr));
            return Some((st.success(), text));
        }
        if t0.elapsed() >= timeout { break; }
        std::thread::sleep(Duration::from_millis(10));
    }
    let _ = child.kill();
    let _ = child.wait();
    None
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
    let found = match std::env::var_os("FORGE_CODEX_PATH") {
        Some(p) => dunce::canonicalize(PathBuf::from(p)).ok()?,
        None => which::which("codex").ok()?,
    };
    // The same rule for an override as for PATH: a real executable, never a script shim.
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
        if let Some((ok, t)) = run_timeout(p, &["login", "status"], 15) {
            let lower = t.to_lowercase();
            info.signed_in = Some(codex_signed_in(ok, &lower));
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

/// `codex login status`: signed in only if it succeeded and says so. "Not logged in" also contains "logged in".
fn codex_signed_in(exit_ok: bool, lower: &str) -> bool {
    exit_ok && lower.contains("logged in") && !lower.contains("not logged in")
}

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
    const MAX_RESPONSE: usize = 1024 * 1024;
    if !is_loopback_http(url) {
        return None;
    }
    let deadline = Instant::now().checked_add(Duration::from_secs(secs))?;
    let (host, port) = split_host_port(url)?;
    // Even a configured `localhost` resolver must not redirect local-model requests off-box.
    let addr = (host.as_str(), port).to_socket_addrs().ok()?.find(|a| a.ip().is_loopback())?;
    let remaining = deadline.checked_duration_since(Instant::now()).filter(|d| !d.is_zero())?;
    let mut s = TcpStream::connect_timeout(&addr, remaining).ok()?;
    let remaining = deadline.checked_duration_since(Instant::now()).filter(|d| !d.is_zero())?;
    s.set_write_timeout(Some(remaining)).ok()?;
    let req = format!("GET {path} HTTP/1.0\r\nHost: {host}:{port}\r\nAccept: application/json\r\n\r\n");
    s.write_all(req.as_bytes()).ok()?;
    let mut bytes = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        let remaining = deadline.checked_duration_since(Instant::now())?;
        if remaining.is_zero() { return None; }
        s.set_read_timeout(Some(remaining)).ok()?;
        match s.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if bytes.len() + n > MAX_RESPONSE { return None; }
                bytes.extend_from_slice(&buf[..n]);
            }
            // Some peers close with a reset after their response, notably on Windows.
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionReset && !bytes.is_empty() => break,
            Err(_) => return None,
        }
    }
    let raw = String::from_utf8(bytes).ok()?;
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

    // The test binary acts as a deterministic local CLI; ordinary suite runs do nothing here.
    #[test]
    fn provider_probe_fixture() {
        use std::io::Write;
        match std::env::var("FORGE_PROBE_FIXTURE").as_deref() {
            Ok("large") => { std::io::stdout().write_all(&vec![b'x'; 200_000]).unwrap(); }
            Ok("hold") => {
                let mut cmd = Command::new(std::env::current_exe().unwrap());
                cmd.args(["--exact", "providers::tests::provider_probe_fixture", "--nocapture"]).env("FORGE_PROBE_FIXTURE", "sleep");
                #[cfg(windows)]
                { use std::os::windows::process::CommandExt; cmd.creation_flags(0x0800_0000); }
                cmd.spawn().unwrap();
            }
            Ok("sleep") => {
                if let Some(marker) = std::env::var_os("FORGE_PROBE_MARKER") {
                    std::thread::sleep(Duration::from_secs(1));
                    let _ = std::fs::write(marker, "survived");
                } else { std::thread::sleep(Duration::from_secs(3)); }
            }
            _ => {}
        }
    }

    fn fixture_command(mode: &str) -> Command {
        let mut cmd = Command::new(std::env::current_exe().unwrap());
        cmd.args(["--exact", "providers::tests::provider_probe_fixture", "--nocapture"]).env("FORGE_PROBE_FIXTURE", mode);
        cmd
    }

    #[test]
    fn provider_probe_drains_large_output_and_respects_inherited_pipe_timeout() {
        let (ok, output) = run_timeout_command(fixture_command("large"), Duration::from_secs(10)).unwrap();
        assert!(ok && output.contains(&"x".repeat(200_000)));
        let start = Instant::now();
        assert!(run_timeout_command(fixture_command("hold"), Duration::from_millis(400)).is_none());
        assert!(start.elapsed() < Duration::from_secs(2), "an inherited pipe must not defeat the deadline");
    }

    #[cfg(windows)]
    #[test]
    fn timed_out_provider_probe_terminates_its_descendant() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("survived");
        let mut cmd = fixture_command("hold");
        cmd.env("FORGE_PROBE_MARKER", &marker);
        assert!(run_timeout_command(cmd, Duration::from_millis(400)).is_none());
        std::thread::sleep(Duration::from_millis(1500));
        assert!(!marker.exists(), "the timed-out probe's descendant must not survive its job");
    }

    #[test]
    fn separately_installed_sdk_metadata_is_version_pinned() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("sdk.mjs"), "throw new Error('must never execute during detection')").unwrap();
        std::fs::write(d.path().join("package.json"), json!({"name": "@anthropic-ai/claude-agent-sdk", "version": CLAUDE_SDK_VERSION}).to_string()).unwrap();
        assert!(valid_claude_sdk(d.path()));
        std::fs::write(d.path().join("package.json"), json!({"name": "@anthropic-ai/claude-agent-sdk", "version": "0.0.1"}).to_string()).unwrap();
        assert!(!valid_claude_sdk(d.path()));
        let package: Value = serde_json::from_str(include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../packages/engine/package.json"))).unwrap();
        assert_eq!(package["devDependencies"]["@anthropic-ai/claude-agent-sdk"], CLAUDE_SDK_VERSION);
    }

    #[test]
    fn codex_not_logged_in_is_not_signed_in() {
        assert!(codex_signed_in(true, "logged in using chatgpt"));
        assert!(!codex_signed_in(false, "not logged in"));
        assert!(!codex_signed_in(true, "not logged in"));
        assert!(!codex_signed_in(false, "logged in using chatgpt"), "a failing status command is not proof");
    }

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
    fn loopback_discovery_limits_response_size_and_total_time() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 512];
            let _ = stream.read(&mut request);
            let _ = stream.write_all(&vec![b'x'; 1024 * 1024 + 1]);
        });
        assert!(http_get_loopback(&format!("http://127.0.0.1:{port}"), "/", 2).is_none());
        server.join().unwrap();

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 512];
            let _ = stream.read(&mut request);
            for _ in 0..8 {
                if stream.write_all(b"x").is_err() { break; }
                std::thread::sleep(Duration::from_millis(200));
            }
        });
        let start = Instant::now();
        assert!(http_get_loopback(&format!("http://127.0.0.1:{port}"), "/", 1).is_none());
        assert!(start.elapsed() < Duration::from_millis(1500), "a trickle must not extend the deadline");
        server.join().unwrap();
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
