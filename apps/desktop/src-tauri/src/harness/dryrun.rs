//! Dry-run of one hook command with a synthetic event, so the user can see what it would do *before* saving it.
//!
//! This executes a shell command with the user's privileges, in the project directory — exactly what the real hook would do —
//! so it only runs on an explicit user action (the UI shows the full command and asks first). It is deliberately not a sandbox:
//! it scrubs secret-looking environment variables, bounds time and output, and kills the whole process tree on timeout.

use super::hooks::{self, DryRun};
use super::inventory::CLAUDE_HOOK_EVENTS;
use crate::error::{ForgeError, Result};
use crate::{proc, redact};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const OUT_CAP: usize = 32 * 1024;

/// Claude Code runs hook commands through bash. On Windows that is Git for Windows' bash (never the WSL launcher in System32).
pub fn find_bash() -> Option<PathBuf> {
    if !cfg!(windows) {
        return which::which("bash").ok().or_else(|| which::which("sh").ok());
    }
    let mut cands: Vec<PathBuf> = vec![];
    if let Ok(p) = std::env::var("CLAUDE_CODE_GIT_BASH_PATH") {
        cands.push(PathBuf::from(p));
    }
    for var in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
        if let Ok(base) = std::env::var(var) {
            let base = PathBuf::from(base);
            cands.push(base.join("Git").join("bin").join("bash.exe"));
            cands.push(base.join("Programs").join("Git").join("bin").join("bash.exe"));
        }
    }
    cands.into_iter().find(|p| p.is_file())
}

fn shell(command: &str) -> (Command, String) {
    if let Some(bash) = find_bash() {
        let mut c = Command::new(&bash);
        c.args(["-c", command]);
        return (c, format!("bash ({})", bash.display()));
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut c = Command::new("cmd.exe");
        c.args(["/d", "/s", "/c"]).raw_arg(format!("\"{command}\""));
        (c, "cmd.exe (Git Bash was not found; Claude Code itself needs Git for Windows to run hooks)".into())
    }
    #[cfg(not(windows))]
    {
        let mut c = Command::new("sh");
        c.args(["-c", command]);
        (c, "sh".into())
    }
}

fn read_capped(stream: impl Read + Send + 'static) -> Arc<Mutex<(Vec<u8>, bool)>> {
    let buf = Arc::new(Mutex::new((Vec::new(), false)));
    let b = buf.clone();
    std::thread::spawn(move || {
        let mut s = stream;
        let mut chunk = [0u8; 4096];
        while let Ok(n) = s.read(&mut chunk) {
            if n == 0 {
                break;
            }
            let mut g = b.lock().unwrap_or_else(|p| p.into_inner());
            let room = OUT_CAP.saturating_sub(g.0.len());
            g.0.extend_from_slice(&chunk[..n.min(room)]);
            if n > room {
                g.1 = true;
            }
        }
    });
    buf
}

fn text(buf: &Arc<Mutex<(Vec<u8>, bool)>>) -> String {
    let g = buf.lock().unwrap_or_else(|p| p.into_inner());
    let mut s = redact::redact_str(&String::from_utf8_lossy(&g.0));
    if g.1 {
        s.push_str("\n… (output truncated)");
    }
    s
}

pub fn run(root: &Path, event: &str, command: &str, timeout: Duration) -> Result<DryRun> {
    if !CLAUDE_HOOK_EVENTS.contains(&event) {
        return Err(ForgeError::Other(format!("`{event}` is not a Claude hook event")));
    }
    hooks::validate_command(command)?;
    let input = hooks::sample_event(event, root);
    let (mut cmd, shell_name) = shell(command);

    let mut scrubbed = vec![];
    cmd.env_clear();
    for (k, v) in std::env::vars() {
        if hooks::looks_secret_env(&k) {
            scrubbed.push(k);
        } else {
            cmd.env(k, v);
        }
    }
    scrubbed.sort();
    cmd.env("CLAUDE_PROJECT_DIR", root).env("CI", "true").env("NO_COLOR", "1");
    cmd.current_dir(root).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }

    let started = Instant::now();
    let mut child = cmd.spawn().map_err(|e| ForgeError::Other(format!("could not start the hook command: {e}")))?;
    let pid = child.id();
    let _containment = proc::contain(&child).ok();
    if let Some(mut stdin) = child.stdin.take() {
        let payload = input.to_string();
        std::thread::spawn(move || {
            let _ = stdin.write_all(payload.as_bytes());
        });
    }
    let out = child.stdout.take().map(read_capped);
    let err = child.stderr.take().map(read_capped);

    let mut exit_code = None;
    let mut timed_out = false;
    loop {
        match child.try_wait() {
            Ok(Some(st)) => {
                exit_code = st.code();
                break;
            }
            Ok(None) => {}
            Err(_) => break,
        }
        if started.elapsed() >= timeout {
            proc::kill_tree(pid);
            let _ = child.wait();
            timed_out = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    // Let the readers drain what the process wrote before it ended.
    std::thread::sleep(Duration::from_millis(60));
    let stdout = out.as_ref().map(text).unwrap_or_default();
    let stderr = err.as_ref().map(text).unwrap_or_default();
    Ok(DryRun {
        shell: shell_name,
        exit_code,
        timed_out,
        interpretation: hooks::interpret(event, exit_code, timed_out, &stdout),
        stdout,
        stderr,
        input,
        duration_ms: started.elapsed().as_millis() as u64,
        scrubbed_env: scrubbed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    const READ_EVENT: &str = "node -e \"let d='';process.stdin.on('data',c=>d+=c).on('end',()=>console.log(JSON.parse(d).hook_event_name))\"";

    #[test]
    fn the_hook_receives_the_synthetic_event_on_stdin() {
        let d = root();
        let r = run(d.path(), "PostToolUse", READ_EVENT, Duration::from_secs(30)).unwrap();
        assert_eq!(r.exit_code, Some(0), "{r:?}");
        assert_eq!(r.stdout.trim(), "PostToolUse");
        assert!(r.interpretation.contains("Succeeded"), "{}", r.interpretation);
        assert_eq!(r.input["hook_event_name"], "PostToolUse");
    }

    #[test]
    fn exit_code_two_is_reported_as_blocking_with_the_reason() {
        let d = root();
        let r = run(d.path(), "PreToolUse", "node -e \"console.error('nope');process.exit(2)\"", Duration::from_secs(30)).unwrap();
        assert_eq!(r.exit_code, Some(2));
        assert!(r.stderr.contains("nope"));
        assert!(r.interpretation.contains("BLOCKS"), "{}", r.interpretation);
    }

    #[test]
    fn a_hung_hook_is_killed_at_the_timeout() {
        let d = root();
        let t = Instant::now();
        let r = run(d.path(), "Stop", "node -e \"setTimeout(()=>{},60000)\"", Duration::from_millis(800)).unwrap();
        assert!(r.timed_out);
        assert!(t.elapsed() < Duration::from_secs(20));
        assert!(r.interpretation.contains("timeout"));
    }

    #[test]
    fn secret_looking_environment_is_not_passed_to_the_hook() {
        let d = root();
        // SAFETY: a name unique to this test; no other code reads it.
        unsafe { std::env::set_var("FORGE_DRYRUN_TEST_SECRET_TOKEN", "hunter2-super-secret") };
        let r = run(d.path(), "Stop", "node -e \"console.log('seen=' + (process.env.FORGE_DRYRUN_TEST_SECRET_TOKEN || 'none'))\"", Duration::from_secs(30)).unwrap();
        assert!(r.stdout.contains("seen=none"), "{r:?}");
        assert!(r.scrubbed_env.iter().any(|n| n == "FORGE_DRYRUN_TEST_SECRET_TOKEN"));
        assert!(!r.stdout.contains("hunter2"));
    }

    #[test]
    fn invalid_commands_and_events_never_start_a_process() {
        let d = root();
        assert!(run(d.path(), "NotAnEvent", "echo hi", Duration::from_secs(5)).is_err());
        assert!(run(d.path(), "Stop", "echo a\necho b", Duration::from_secs(5)).is_err());
        assert!(run(d.path(), "Stop", "", Duration::from_secs(5)).is_err());
    }

    #[test]
    fn the_hook_runs_in_the_project_directory_with_claude_project_dir_set() {
        let d = root();
        let r = run(d.path(), "Stop", "node -e \"console.log(process.cwd() + '|' + process.env.CLAUDE_PROJECT_DIR)\"", Duration::from_secs(30)).unwrap();
        let canon = dunce::canonicalize(d.path()).unwrap();
        let first = r.stdout.trim().split('|').next().unwrap().to_string();
        assert_eq!(dunce::canonicalize(first).unwrap(), canon, "{r:?}");
    }
}
