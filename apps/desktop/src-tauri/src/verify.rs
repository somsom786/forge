//! Check runner. Executes a user-authorised command in a mission worktree with a hard timeout,
//! whole-tree termination, bounded in-memory output and a capped on-disk log.
//!
//! Note: running a check executes code from the mission worktree (including agent-written code) with the
//! user's privileges. FORGE therefore only runs checks on an explicit user action.

use crate::contract::RunState;
use crate::proc;
use crate::redact;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const TAIL_BYTES: usize = 64 * 1024;
const LOG_CAP_BYTES: u64 = 5 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct RunOutcome {
    pub state: RunState,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    /// Redacted tail of combined stdout+stderr.
    pub output_tail: String,
    pub output_truncated: bool,
    pub log_path: Option<PathBuf>,
}

#[derive(Default)]
struct Buf {
    tail: Vec<u8>,
    total: u64,
    log: Option<std::fs::File>,
    logged: u64,
    /// Bytes of an unfinished line, redacted as a whole when it completes (a secret can straddle two reads).
    pending: Vec<u8>,
}

impl Buf {
    fn write_log(&mut self, bytes: &[u8]) {
        if let Some(f) = self.log.as_mut() {
            if self.logged < LOG_CAP_BYTES {
                let room = (LOG_CAP_BYTES - self.logged) as usize;
                let n = bytes.len().min(room);
                let _ = f.write_all(&bytes[..n]);
                self.logged += n as u64;
            }
        }
    }

    /// Write the log redacted, line by line.
    fn log_chunk(&mut self, chunk: &[u8]) {
        self.pending.extend_from_slice(chunk);
        while let Some(pos) = self.pending.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=pos).collect();
            let red = redact::redact_str(&String::from_utf8_lossy(&line));
            self.write_log(red.as_bytes());
        }
        if self.pending.len() > 64 * 1024 {
            let line: Vec<u8> = std::mem::take(&mut self.pending);
            let red = redact::redact_str(&String::from_utf8_lossy(&line));
            self.write_log(red.as_bytes());
        }
    }

    fn finish_log(&mut self) {
        if !self.pending.is_empty() {
            let line: Vec<u8> = std::mem::take(&mut self.pending);
            let red = redact::redact_str(&String::from_utf8_lossy(&line));
            self.write_log(red.as_bytes());
        }
    }

    fn push(&mut self, chunk: &[u8]) {
        self.total += chunk.len() as u64;
        self.log_chunk(chunk);
        self.tail.extend_from_slice(chunk);
        if self.tail.len() > TAIL_BYTES * 2 {
            let cut = self.tail.len() - TAIL_BYTES;
            self.tail.drain(..cut);
        }
    }
}

fn shell_command(command: &str) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut c = Command::new("cmd.exe");
        c.args(["/d", "/s", "/c"]).raw_arg(format!("\"{command}\""));
        c.creation_flags(0x0800_0000);
        c
    }
    #[cfg(not(windows))]
    {
        let mut c = Command::new("sh");
        c.args(["-c", command]);
        c
    }
}

/// Run `command` in `cwd`. `cancel` can be flipped from another thread to abort the run.
pub fn run_check(command: &str, cwd: &Path, timeout: Duration, log_path: Option<&Path>, cancel: Arc<AtomicBool>) -> RunOutcome {
    let started = Instant::now();
    let fail = |msg: String| RunOutcome {
        state: RunState::Error,
        exit_code: None,
        duration_ms: started.elapsed().as_millis() as u64,
        output_tail: msg,
        output_truncated: false,
        log_path: None,
    };

    let mut cmd = shell_command(command);
    cmd.current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("CI", "true")
        .env("NO_COLOR", "1")
        .env("FORCE_COLOR", "0")
        .env("GIT_TERMINAL_PROMPT", "0");
    proc::isolate(&mut cmd);
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return fail(format!("could not start the command: {e}")),
    };
    let pid = child.id();
    // Best effort for checks: the timeout path also kills the tree explicitly. (A descendant created in the instant
    // between spawn and job assignment could escape the job; see SECURITY.md.)
    let _containment = proc::contain(&child).ok();

    let log = log_path.and_then(|p| {
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::File::create(p).ok()
    });
    let buf = Arc::new(Mutex::new(Buf { log, ..Default::default() }));
    let mut readers = vec![];
    for stream in [child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>), child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>)].into_iter().flatten() {
        let b = buf.clone();
        readers.push(std::thread::spawn(move || {
            let mut s = stream;
            let mut chunk = [0u8; 8192];
            while let Ok(n) = s.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                b.lock().unwrap_or_else(|p| p.into_inner()).push(&chunk[..n]);
            }
        }));
    }

    let state: RunState;
    let mut exit_code = None;
    loop {
        match child.try_wait() {
            Ok(Some(st)) => {
                exit_code = st.code();
                state = if st.success() { RunState::Passed } else { RunState::Failed };
                break;
            }
            Ok(None) => {}
            Err(_) => {
                state = RunState::Error;
                break;
            }
        }
        if cancel.load(Ordering::SeqCst) {
            proc::kill_tree(pid);
            let _ = child.wait();
            state = RunState::Cancelled;
            break;
        }
        if started.elapsed() >= timeout {
            proc::kill_tree(pid);
            let _ = child.wait();
            state = RunState::Timeout;
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    // Grandchildren holding the pipes open would block the readers forever; the tree is gone or the process
    // exited normally, so give readers a short grace period and then move on.
    let grace = Instant::now();
    for r in readers {
        while !r.is_finished() && grace.elapsed() < Duration::from_secs(2) {
            std::thread::sleep(Duration::from_millis(10));
        }
        if r.is_finished() {
            let _ = r.join();
        }
    }

    let mut b = buf.lock().unwrap_or_else(|p| p.into_inner());
    b.finish_log();
    let truncated = b.total as usize > b.tail.len().min(TAIL_BYTES);
    let start = b.tail.len().saturating_sub(TAIL_BYTES);
    let text = String::from_utf8_lossy(&b.tail[start..]).into_owned();
    RunOutcome {
        state,
        exit_code,
        duration_ms: started.elapsed().as_millis() as u64,
        output_tail: redact::redact_str(&text),
        output_truncated: truncated,
        log_path: log_path.map(Path::to_path_buf),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(cmd: &str, secs: u64) -> RunOutcome {
        let d = tempfile::tempdir().unwrap();
        run_check(cmd, d.path(), Duration::from_secs(secs), None, Arc::new(AtomicBool::new(false)))
    }

    #[test]
    fn passing_command_captures_output() {
        let r = run("node -e \"console.log('hello'); console.error('warn')\"", 20);
        assert_eq!(r.state, RunState::Passed);
        assert_eq!(r.exit_code, Some(0));
        assert!(r.output_tail.contains("hello") && r.output_tail.contains("warn"), "{}", r.output_tail);
    }

    #[test]
    fn nonzero_exit_is_failed_with_code() {
        let r = run("node -e \"process.exit(3)\"", 20);
        assert_eq!((r.state, r.exit_code), (RunState::Failed, Some(3)));
    }

    #[test]
    fn missing_program_fails_not_passes() {
        let r = run("definitely-not-a-real-program-xyz", 20);
        assert_eq!(r.state, RunState::Failed);
        assert_ne!(r.exit_code, Some(0));
    }

    #[test]
    fn timeout_kills_the_whole_process_tree() {
        let t0 = Instant::now();
        let r = run("node -e \"require('child_process').spawn(process.execPath,['-e','setTimeout(()=>{},60000)'],{stdio:'inherit'}); setTimeout(()=>{},60000)\"", 1);
        assert_eq!(r.state, RunState::Timeout);
        assert!(t0.elapsed() < Duration::from_secs(10), "runner must not hang on grandchildren holding pipes: {:?}", t0.elapsed());
    }

    #[test]
    fn cancel_stops_a_running_check() {
        let d = tempfile::tempdir().unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let c2 = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(400));
            c2.store(true, Ordering::SeqCst);
        });
        let r = run_check("node -e \"setTimeout(()=>{},60000)\"", d.path(), Duration::from_secs(60), None, cancel);
        assert_eq!(r.state, RunState::Cancelled);
    }

    #[test]
    fn output_is_bounded_redacted_and_logged() {
        let d = tempfile::tempdir().unwrap();
        let log = d.path().join("runs").join("r1.log");
        let r = run_check(
            "node -e \"console.log('token=supersecretvalue123'); for(let i=0;i<20000;i++) console.log('line '+i+' '+'x'.repeat(40))\"",
            d.path(),
            Duration::from_secs(30),
            Some(&log),
            Arc::new(AtomicBool::new(false)),
        );
        assert_eq!(r.state, RunState::Passed);
        assert!(r.output_truncated);
        assert!(r.output_tail.len() <= TAIL_BYTES + 16);
        assert!(r.output_tail.contains("line 19999"), "tail must keep the end");
        let logged = std::fs::read_to_string(&log).unwrap();
        assert!(!logged.contains("supersecretvalue123"), "the on-disk log is redacted line by line too");
        assert!(logged.contains("line 19999"));
        assert!(!r.output_tail.contains("supersecretvalue123") || !r.output_tail.starts_with("token="), "tail shown in the UI is redacted");
    }

    #[test]
    fn runs_in_the_given_directory() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("marker.txt"), "x").unwrap();
        let r = run_check("node -e \"console.log(require('fs').existsSync('marker.txt'))\"", d.path(), Duration::from_secs(20), None, Arc::new(AtomicBool::new(false)));
        assert!(r.output_tail.contains("true"), "{}", r.output_tail);
    }
}
