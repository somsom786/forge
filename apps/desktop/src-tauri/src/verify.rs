//! Check runner. Executes a user-authorised command in a mission worktree with a hard timeout,
//! whole-tree termination, bounded in-memory output and a capped on-disk log.
//!
//! Note: running a check executes code from the mission worktree (including agent-written code) with the
//! user's privileges. FORGE therefore only runs checks on an explicit user action.

use crate::contract::RunState;
use crate::execution::{self, Backend, RunSpec};
use crate::proc;
use crate::redact;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const TAIL_BYTES: usize = 64 * 1024;
const LOG_CAP_BYTES: u64 = 5 * 1024 * 1024;
/// Separate stdout capture for verification adapters (a tool's JSON report).
pub const STDOUT_CAP_BYTES: u64 = 16 * 1024 * 1024;

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
    /// Log size cap; 0 means `LOG_CAP_BYTES`.
    cap: u64,
    truncated: bool,
    closed: bool,
    write_failed: bool,
    in_private_key: bool,
}

impl Buf {
    fn write_log(&mut self, bytes: &[u8]) {
        if let Some(f) = self.log.as_mut() {
            let cap = if self.cap == 0 { LOG_CAP_BYTES } else { self.cap };
            if self.logged < cap {
                let room = (cap - self.logged) as usize;
                let n = bytes.len().min(room);
                if f.write_all(&bytes[..n]).is_err() { self.write_failed = true; }
                self.logged += n as u64;
                self.truncated |= n < bytes.len();
            } else if !bytes.is_empty() {
                self.truncated = true;
            }
        }
    }

    fn redact_log_line(&mut self, line: &[u8]) {
        let text = String::from_utf8_lossy(line);
        let begin = text.contains("-----BEGIN ") && text.contains("PRIVATE KEY-----");
        let end = text.contains("-----END ") && text.contains("PRIVATE KEY-----");
        if begin || self.in_private_key {
            if begin { self.write_log(b"[REDACTED PRIVATE KEY]\n"); }
            self.in_private_key = !end;
        } else {
            self.write_log(redact::redact_str(&text).as_bytes());
        }
    }

    /// Write the log redacted, line by line.
    fn log_chunk(&mut self, chunk: &[u8]) {
        if self.closed { return; }
        self.pending.extend_from_slice(chunk);
        while let Some(pos) = self.pending.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=pos).collect();
            self.redact_log_line(&line);
        }
        if self.pending.len() > 64 * 1024 {
            let line: Vec<u8> = std::mem::take(&mut self.pending);
            self.redact_log_line(&line);
        }
    }

    fn finish_log(&mut self) {
        if !self.pending.is_empty() {
            let line: Vec<u8> = std::mem::take(&mut self.pending);
            self.redact_log_line(&line);
        }
        self.closed = true;
    }

    fn push(&mut self, chunk: &[u8]) {
        if self.closed { return; }
        self.total += chunk.len() as u64;
        self.log_chunk(chunk);
        self.tail.extend_from_slice(chunk);
        if self.tail.len() > TAIL_BYTES * 2 {
            let cut = self.tail.len() - TAIL_BYTES;
            self.tail.drain(..cut);
        }
    }
}

/// Run `command` in `cwd` with the user's own shell and privileges (the `trusted_local` backend).
pub fn run_check(command: &str, cwd: &Path, timeout: Duration, log_path: Option<&Path>, cancel: Arc<AtomicBool>) -> RunOutcome {
    run_check_in(Backend::TrustedLocal, command, cwd, timeout, log_path, cancel)
}

/// Run `command` in `cwd` on execution backend `backend`. `cancel` can be flipped from another thread to abort the run. A
/// backend that is not available makes the run an `Error` that says why; it never falls back to another backend.
pub fn run_check_in(backend: Backend, command: &str, cwd: &Path, timeout: Duration, log_path: Option<&Path>, cancel: Arc<AtomicBool>) -> RunOutcome {
    run_check_capture(backend, command, cwd, timeout, log_path, None, cancel)
}

/// As `run_check_in`, and additionally writes standard output alone (redacted line by line, capped at `STDOUT_CAP_BYTES`)
/// to `stdout_path`, for an adapter to parse.
#[allow(clippy::too_many_arguments)]
pub fn run_check_capture(backend: Backend, command: &str, cwd: &Path, timeout: Duration, log_path: Option<&Path>, stdout_path: Option<&Path>, cancel: Arc<AtomicBool>) -> RunOutcome {
    let started = Instant::now();
    let fail = |msg: String| RunOutcome {
        state: RunState::Error,
        exit_code: None,
        duration_ms: started.elapsed().as_millis() as u64,
        output_tail: msg,
        output_truncated: false,
        log_path: None,
    };

    let mut cmd = match execution::command(backend, &RunSpec { command, workspace: cwd }) {
        Ok(c) => c,
        Err(e) => return fail(e.to_string()),
    };
    let log = match log_path {
        Some(p) => {
            let open = || -> std::io::Result<std::fs::File> {
                if let Some(parent) = p.parent() { std::fs::create_dir_all(parent)?; }
                std::fs::File::create(p)
            };
            match open() {
                Ok(f) => Some(f),
                Err(e) => return fail(format!("could not create the check log: {e}")),
            }
        }
        None => None,
    };
    // An adapter cannot produce trustworthy evidence if its report cannot be captured.
    let capture = match stdout_path {
        Some(p) => {
            let open = || -> std::io::Result<std::fs::File> {
                if let Some(parent) = p.parent() { std::fs::create_dir_all(parent)?; }
                std::fs::File::create(p)
            };
            match open() {
                Ok(f) => Some(Arc::new(Mutex::new(Buf { log: Some(f), cap: STDOUT_CAP_BYTES, ..Default::default() }))),
                Err(e) => return fail(format!("could not capture the check report: {e}")),
            }
        }
        None => None,
    };
    cmd.current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("CI", "true")
        .env("NO_COLOR", "1")
        .env("FORCE_COLOR", "0")
        .env("GIT_TERMINAL_PROMPT", "0");
    proc::isolate(&mut cmd);
    // Assign the check to its Job Object before it can create a child process.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000 | proc::CREATE_SUSPENDED);
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return fail(format!("could not start the command: {e}")),
    };
    let pid = child.id();
    #[cfg(windows)]
    let _containment = match proc::contain(&child) {
        Ok(job) => job,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            return fail(format!("could not contain the check: {e}"));
        }
    };
    #[cfg(windows)]
    if let Err(e) = proc::resume_suspended(&child) {
        let _ = child.kill();
        let _ = child.wait();
        return fail(format!("could not start the contained check: {e}"));
    }
    // Unix process-group containment is best effort for an already-exited short-lived command.
    #[cfg(not(windows))]
    let _containment = proc::contain(&child).ok();

    let buf = Arc::new(Mutex::new(Buf { log, ..Default::default() }));
    let mut readers = vec![];
    for (i, stream) in [child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>), child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>)].into_iter().enumerate() {
        let Some(stream) = stream else { continue };
        let b = buf.clone();
        let cap = if i == 0 { capture.clone() } else { None };
        readers.push(std::thread::spawn(move || {
            let mut s = stream;
            let mut chunk = [0u8; 8192];
            loop {
                let n = match s.read(&mut chunk) {
                    Ok(0) => return true,
                    Ok(n) => n,
                    Err(_) => return false,
                };
                b.lock().unwrap_or_else(|p| p.into_inner()).push(&chunk[..n]);
                if let Some(c) = &cap {
                    c.lock().unwrap_or_else(|p| p.into_inner()).log_chunk(&chunk[..n]);
                }
            }
        }));
    }

    let mut state: RunState;
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
    let mut incomplete_output = false;
    for r in readers {
        while !r.is_finished() && grace.elapsed() < Duration::from_secs(2) {
            std::thread::sleep(Duration::from_millis(10));
        }
        if r.is_finished() {
            if !matches!(r.join(), Ok(true)) { incomplete_output = true; }
        } else {
            incomplete_output = true;
        }
    }

    let mut capture_error = incomplete_output.then_some("check output was incomplete or did not close");
    if let Some(c) = &capture {
        let mut c = c.lock().unwrap_or_else(|p| p.into_inner());
        c.finish_log();
        if c.write_failed { capture_error = Some("check report could not be written"); }
        else if c.truncated { capture_error = Some("check report exceeded the capture limit"); }
    }
    let mut b = buf.lock().unwrap_or_else(|p| p.into_inner());
    b.finish_log();
    if b.write_failed { capture_error = Some("check log could not be written"); }
    if capture_error.is_some() && matches!(state, RunState::Passed | RunState::Failed) { state = RunState::Error; }
    let truncated = b.total as usize > b.tail.len().min(TAIL_BYTES);
    let start = b.tail.len().saturating_sub(TAIL_BYTES);
    let mut text = String::from_utf8_lossy(&b.tail[start..]).into_owned();
    if let Some(error) = capture_error { text.push_str(&format!("\n{error}")); }
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

    // Node's Windows child fixture does not retain these inherited pipe handles after its parent exits.
    #[cfg(unix)]
    #[test]
    fn completed_parent_with_open_output_is_not_a_passing_check() {
        let r = run("node -e \"let c=require('child_process').spawn(process.execPath,['-e','setTimeout(()=>{},5000)'],{stdio:'inherit'});c.unref()\"", 15);
        assert_eq!(r.state, RunState::Error);
        assert!(r.output_tail.contains("output was incomplete"));
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
    fn stdout_can_be_captured_alone_for_an_adapter() {
        let d = tempfile::tempdir().unwrap();
        let out = d.path().join("cap").join("r.stdout");
        let r = run_check_capture(
            Backend::TrustedLocal,
            "node -e \"console.error('progress on stderr'); console.log(JSON.stringify([{a:1,token:'ghp_abcdefghijklmnopqrstuvwxyz0123'}]))\"",
            d.path(),
            Duration::from_secs(20),
            None,
            Some(&out),
            Arc::new(AtomicBool::new(false)),
        );
        assert_eq!(r.state, RunState::Passed);
        let text = std::fs::read_to_string(&out).unwrap();
        assert!(!text.contains("progress on stderr"), "stderr is not mixed into the report");
        assert!(!text.contains("ghp_abcdefghijklmnopqrstuvwxyz0123"), "redacted");
        assert!(serde_json::from_str::<serde_json::Value>(text.trim()).is_ok(), "still valid JSON after redaction: {text}");
        assert!(r.output_tail.contains("progress on stderr"), "the combined tail still has both");
    }

    #[test]
    fn missing_report_capture_stops_the_check() {
        let d = tempfile::tempdir().unwrap();
        let r = run_check_capture(Backend::TrustedLocal, "node -e \"require('fs').writeFileSync('ran','yes')\"", d.path(), Duration::from_secs(10), None, Some(d.path()), Arc::new(AtomicBool::new(false)));
        assert_eq!(r.state, RunState::Error);
        assert!(!d.path().join("ran").exists(), "the check must not run without report capture");
    }

    #[test]
    fn missing_log_stops_the_check() {
        let d = tempfile::tempdir().unwrap();
        let r = run_check("node -e \"require('fs').writeFileSync('ran','yes')\"", d.path(), Duration::from_secs(10), Some(d.path()), Arc::new(AtomicBool::new(false)));
        assert_eq!(r.state, RunState::Error);
        assert!(r.log_path.is_none());
        assert!(!d.path().join("ran").exists(), "the check must not run without its audit log");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn failed_log_or_report_write_is_not_a_successful_check() {
        let d = tempfile::tempdir().unwrap();
        let full = Path::new("/dev/full");
        let run = |log, report| run_check_capture(Backend::TrustedLocal, "printf '[]\\n'", d.path(), Duration::from_secs(10), log, report, Arc::new(AtomicBool::new(false)));
        let log = run(Some(full), None);
        assert_eq!(log.state, RunState::Error);
        assert!(log.output_tail.contains("log could not be written"));
        let report = run(None, Some(full));
        assert_eq!(report.state, RunState::Error);
        assert!(report.output_tail.contains("report could not be written"));
    }

    #[test]
    fn report_capture_rejects_truncation_and_freezes_after_finish() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("report");
        let mut capture = Buf { log: Some(std::fs::File::create(&path).unwrap()), cap: 4, ..Default::default() };
        capture.log_chunk(b"[]   \n");
        capture.finish_log();
        assert!(capture.truncated, "even a valid JSON prefix must not be treated as a complete report");
        let before = std::fs::read(&path).unwrap();
        capture.log_chunk(b"late output\n");
        assert_eq!(std::fs::read(&path).unwrap(), before, "late readers must not alter completed evidence");
    }

    #[test]
    fn multiline_private_keys_are_not_written_to_verification_logs() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("log");
        let mut log = Buf { log: Some(std::fs::File::create(&path).unwrap()), ..Default::default() };
        log.log_chunk(b"before\n-----BEGIN PRIVATE ");
        log.log_chunk(b"KEY-----\nprivate-fixture-part-one\nprivate-fixture-part-two\n-----END PRIVATE KEY-----\nafter\n");
        log.finish_log();
        let contents = std::fs::read_to_string(path).unwrap();
        assert!(contents.contains("before") && contents.contains("after") && contents.contains("[REDACTED PRIVATE KEY]"));
        assert!(!contents.contains("private-fixture-part"), "{contents}");
    }

    #[test]
    fn verification_command_redacts_a_multiline_private_key_in_log_and_tail() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("run.log");
        let command = "node -e \"console.log('-----BEGIN '+'PRIVATE KEY-----\\nprivate-fixture-value\\n-----END '+'PRIVATE KEY-----')\"";
        let result = run_check(command, d.path(), Duration::from_secs(20), Some(&path), Arc::new(AtomicBool::new(false)));
        assert_eq!(result.state, RunState::Passed);
        assert!(!result.output_tail.contains("private-fixture-value"));
        assert!(!std::fs::read_to_string(path).unwrap().contains("private-fixture-value"));
    }

    #[test]
    fn oversized_scanner_output_is_not_successful_evidence() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("report");
        let r = run_check_capture(Backend::TrustedLocal, "node -e \"process.stdout.write('[]'+' '.repeat(17*1024*1024))\"", d.path(), Duration::from_secs(30), None, Some(&path), Arc::new(AtomicBool::new(false)));
        assert_eq!(r.state, RunState::Error, "a truncated valid JSON prefix is incomplete evidence");
        assert!(r.output_tail.contains("capture limit"));
        assert!(std::fs::metadata(path).unwrap().len() <= STDOUT_CAP_BYTES);
    }

    #[test]
    fn runs_in_the_given_directory() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("marker.txt"), "x").unwrap();
        let r = run_check("node -e \"console.log(require('fs').existsSync('marker.txt'))\"", d.path(), Duration::from_secs(20), None, Arc::new(AtomicBool::new(false)));
        assert!(r.output_tail.contains("true"), "{}", r.output_tail);
    }
}
