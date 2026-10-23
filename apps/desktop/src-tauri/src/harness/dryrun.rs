//! Dry-run of one hook command with a synthetic event, so the user can see what it would do *before* saving it.
//!
//! This executes a shell command with the user's privileges, in the project directory — exactly what the real hook would do —
//! so it only runs on an explicit user action (the UI shows the full command and asks first). It is deliberately not a sandbox:
//! it scrubs secret-looking environment variables, bounds time and output, and kills the whole process tree on timeout.

use super::hooks::{self, DryRun};
#[cfg(test)]
use serde_json::json;
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

/// The shell a hook command runs in. There is deliberately no fallback to another shell: `cmd.exe` and PowerShell parse quotes,
/// `&` and `$` differently, so a "dry run" in the wrong shell would report something the real hook does not do.
fn shell(command: &str) -> Result<(Command, String)> {
    let Some(bash) = find_bash() else {
        return Err(ForgeError::Other(
            "no bash was found to run the hook in. Claude Code runs hooks through bash (Git for Windows on Windows); install it or set CLAUDE_CODE_GIT_BASH_PATH. FORGE will not substitute another shell because the result would not match what the hook really does.".into(),
        ));
    };
    let mut c = Command::new(&bash);
    c.args(["-c", command]);
    Ok((c, format!("bash ({})", bash.display())))
}

type Capture = (Arc<Mutex<(Vec<u8>, bool)>>, std::thread::JoinHandle<()>);

fn read_capped(stream: impl Read + Send + 'static) -> Capture {
    let buf = Arc::new(Mutex::new((Vec::new(), false)));
    let b = buf.clone();
    let handle = std::thread::spawn(move || {
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
    (buf, handle)
}

/// Wait (bounded) for the reader to see end-of-stream, then return what it captured.
fn finish(cap: Capture) -> String {
    let (buf, handle) = cap;
    let t = Instant::now();
    while !handle.is_finished() && t.elapsed() < Duration::from_secs(2) {
        std::thread::sleep(Duration::from_millis(10));
    }
    if handle.is_finished() {
        let _ = handle.join();
    }
    let g = buf.lock().unwrap_or_else(|p| p.into_inner());
    let mut s = redact::redact_str(&String::from_utf8_lossy(&g.0));
    if g.1 {
        s.push_str("\n… (output truncated)");
    }
    s
}

pub fn run(root: &Path, event: &str, command: &str, timeout: Duration) -> Result<DryRun> {
    run_with_input(root, event, command, timeout, hooks::sample_event(event, root))
}

/// Like [`run`] with a caller-supplied event payload (used by tests to exercise a hook's decision paths).
pub fn run_with_input(root: &Path, event: &str, command: &str, timeout: Duration, input: serde_json::Value) -> Result<DryRun> {
    if !CLAUDE_HOOK_EVENTS.contains(&event) {
        return Err(ForgeError::Other(format!("`{event}` is not a Claude hook event")));
    }
    hooks::validate_command(command)?;
    let (mut cmd, shell_name) = shell(command)?;

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
        // CREATE_NO_WINDOW, and suspended until it is in the kill-on-close Job (see below).
        cmd.creation_flags(0x0800_0000 | proc::CREATE_SUSPENDED);
    }

    let started = Instant::now();
    let mut child = cmd.spawn().map_err(|e| ForgeError::Other(format!("could not start the hook command: {e}")))?;
    let pid = child.id();
    // Fail closed: a hook that cannot be put in a kill-on-close job is not run (a descendant could outlive the dry run).
    let containment = match proc::contain(&child) {
        Ok(c) => c,
        Err(e) => {
            proc::kill_tree(pid);
            let _ = child.kill();
            let _ = child.wait();
            return Err(ForgeError::Other(format!("could not contain the hook process, so the dry run was stopped: {e}")));
        }
    };
    // On Windows the hook was created suspended: it has run no code yet, so nothing it starts can predate the Job. Let it run
    // now; if it cannot be resumed it never ran and is ended with its Job.
    #[cfg(windows)]
    if let Err(e) = proc::resume_suspended(&child) {
        drop(containment);
        let _ = child.kill();
        let _ = child.wait();
        return Err(ForgeError::Other(format!("could not start the contained hook process, so the dry run was stopped: {e}")));
    }
    if let Some(mut stdin) = child.stdin.take() {
        let payload = input.to_string();
        std::thread::spawn(move || {
            let _ = stdin.write_all(payload.as_bytes());
        });
    }
    let out = child.stdout.take().map(read_capped);
    let err = child.stderr.take().map(read_capped);

    let mut containment_guard = Some(containment);
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
            // End the whole job first (it is what can reach descendants), then the direct child, then reap.
            drop(containment_guard.take());
            proc::kill_tree(pid);
            let _ = child.kill();
            let _ = child.wait();
            timed_out = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    // Anything the hook left running dies with the job, which also closes the pipes so the readers reach end-of-stream.
    drop(containment_guard.take());
    let stdout = out.map(finish).unwrap_or_default();
    let stderr = err.map(finish).unwrap_or_default();
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

    /// The shipped templates must actually *work* in the shell Claude Code uses, not merely validate. (A quoting bug in the
    /// "block protected files" template was found by running it in the real app.)
    #[test]
    fn every_shipped_template_runs_and_does_what_it_says() {
        let d = root();
        let none = std::collections::HashMap::new();

        // Remind: valid hook JSON carrying the reminder.
        let (t, cmd) = hooks::render_template("remind-checks", &none).unwrap();
        let r = run(d.path(), &t.event, &cmd, Duration::from_secs(30)).unwrap();
        assert_eq!(r.exit_code, Some(0), "{r:?}");
        let v: serde_json::Value = serde_json::from_str(r.stdout.trim()).unwrap();
        assert!(v["hookSpecificOutput"]["additionalContext"].as_str().unwrap().contains("npm test"));

        // Block protected files: exit 2 with a reason for protected paths (either slash style), exit 0 otherwise.
        let (t, cmd) = hooks::render_template("block-protected-files", &none).unwrap();
        let ev = |path: &str| json!({"hook_event_name": "PreToolUse", "tool_name": "Write", "tool_input": {"file_path": path}});
        for protected in ["C:\\proj\\.env", "C:/proj/.env.local", "C:\\proj\\secrets\\key.txt", "/home/u/proj/.git/config", "/p/package-lock.json"] {
            let r = run_with_input(d.path(), &t.event, &cmd, Duration::from_secs(30), ev(protected)).unwrap();
            assert_eq!(r.exit_code, Some(2), "{protected}: {r:?}");
            assert!(r.stderr.contains("Blocked by project policy"), "{protected}: {}", r.stderr);
        }
        for fine in ["C:\\proj\\src\\main.rs", "/home/u/proj/src/app.ts"] {
            let r = run_with_input(d.path(), &t.event, &cmd, Duration::from_secs(30), ev(fine)).unwrap();
            assert_eq!(r.exit_code, Some(0), "{fine}: {r:?}");
        }
        let mut custom = std::collections::HashMap::new();
        custom.insert("fragments".to_string(), "dist/,generated".to_string());
        let (_, cmd2) = hooks::render_template("block-protected-files", &custom).unwrap();
        assert_eq!(run_with_input(d.path(), &t.event, &cmd2, Duration::from_secs(30), ev("/p/dist/x.js")).unwrap().exit_code, Some(2));
        assert_eq!(run_with_input(d.path(), &t.event, &cmd2, Duration::from_secs(30), ev("/p/.env")).unwrap().exit_code, Some(0));

        // Format with no Prettier installed: never fails the agent.
        let (t, cmd) = hooks::render_template("format-after-edit", &none).unwrap();
        let r = run_with_input(d.path(), &t.event, &cmd, Duration::from_secs(60), json!({"hook_event_name": "PostToolUse", "tool_name": "Write", "tool_input": {"file_path": d.path().join("a.ts").to_string_lossy()}})).unwrap();
        assert_eq!(r.exit_code, Some(0), "{r:?}");
    }

    /// Security review finding: the formatter once passed an agent-chosen file name through `cmd.exe` (`shell: true`), so a name
    /// such as `a & calc & b.ts` was executed. The file name must reach Prettier as exactly one argument, with no shell.
    #[test]
    fn the_formatter_passes_hostile_file_names_as_one_argument_and_runs_nothing_else() {
        let d = root();
        let src = d.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        let name = "evil & echo pwned > pwned.txt & x.ts";
        let file = src.join(name);
        if std::fs::write(&file, "const a=1\n").is_err() {
            return; // a platform that cannot create the name cannot be attacked with it
        }
        let bin_dir = d.path().join("node_modules").join("prettier").join("bin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        // A fake Prettier that records exactly what it was given.
        std::fs::write(bin_dir.join("prettier.cjs"), "require('fs').writeFileSync('argv.json', JSON.stringify(process.argv.slice(2)))").unwrap();
        let (t, cmd) = hooks::render_template("format-after-edit", &Default::default()).unwrap();
        let input = json!({"hook_event_name": "PostToolUse", "tool_name": "Write", "tool_input": {"file_path": file.to_string_lossy()}});
        let r = run_with_input(d.path(), &t.event, &cmd, Duration::from_secs(60), input).unwrap();
        assert_eq!(r.exit_code, Some(0), "{r:?}");
        let argv: Vec<String> = serde_json::from_str(&std::fs::read_to_string(d.path().join("argv.json")).expect("the formatter did not run")).unwrap();
        assert_eq!(argv.len(), 2, "{argv:?}");
        assert_eq!(argv[0], "--write");
        assert_eq!(dunce::canonicalize(&argv[1]).unwrap(), dunce::canonicalize(&file).unwrap());
        assert!(!d.path().join("pwned.txt").exists() && !src.join("pwned.txt").exists(), "a shell interpreted the file name");

        // A path outside the project is never formatted.
        std::fs::remove_file(d.path().join("argv.json")).unwrap();
        let outside = tempfile::tempdir().unwrap();
        let other = outside.path().join("x.ts");
        std::fs::write(&other, "x").unwrap();
        let input = json!({"hook_event_name": "PostToolUse", "tool_name": "Write", "tool_input": {"file_path": other.to_string_lossy()}});
        run_with_input(d.path(), &t.event, &cmd, Duration::from_secs(60), input).unwrap();
        assert!(!d.path().join("argv.json").exists(), "formatted a file outside the project");
    }

    #[test]
    fn a_descendant_left_running_does_not_hang_the_dry_run_and_is_killed() {
        let d = root();
        let t = Instant::now();
        // The hook exits at once but leaves a grandchild holding the output pipes open.
        let cmd = "node -e \"require('child_process').spawn(process.execPath,['-e','setTimeout(()=>{},30000)'],{stdio:'inherit'}).unref()\"";
        let r = run(d.path(), "Stop", cmd, Duration::from_secs(30)).unwrap();
        assert_eq!(r.exit_code, Some(0), "{r:?}");
        assert!(t.elapsed() < Duration::from_secs(10), "took {:?}", t.elapsed());
    }
}
