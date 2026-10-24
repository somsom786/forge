//! Adversarial tests of the Linux-namespace backend. Each one runs a real command through the check runner and checks what it
//! could actually do (read, write, connect, outlive the run), not what a policy function returns. Linux only.
//!
//! When bubblewrap cannot run here the tests skip, unless FORGE_REQUIRE_SANDBOX_TESTS=1 (set in CI), where they fail instead:
//! a sandbox that silently is not there must not look green.

use crate::contract::RunState;
use crate::execution::{self, Backend};
use crate::verify::{RunOutcome, run_check_in};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

fn sandbox_available() -> bool {
    match execution::availability(Backend::LinuxNamespaces) {
        Ok(()) => true,
        Err(why) => {
            assert!(std::env::var("FORGE_REQUIRE_SANDBOX_TESTS").as_deref() != Ok("1"), "the Linux-namespace backend is required here but unavailable: {why}");
            eprintln!("skipping: {why}");
            false
        }
    }
}

fn run(ws: &Path, cmd: &str, secs: u64) -> RunOutcome {
    run_check_in(Backend::LinuxNamespaces, cmd, ws, Duration::from_secs(secs), None, Arc::new(AtomicBool::new(false)))
}

/// A workspace and, beside it, "the rest of the user's disk" holding a secret.
fn setup() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let ws = d.path().join("worktree");
    let outside = d.path().join("home-of-user");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::create_dir_all(outside.join(".ssh")).unwrap();
    std::fs::write(outside.join(".ssh").join("id_ed25519"), "SECRET-KEY-MATERIAL-123").unwrap();
    (d, ws, outside)
}

#[test]
fn a_command_cannot_read_files_outside_its_workspace() {
    if !sandbox_available() {
        return;
    }
    let (_d, ws, outside) = setup();
    let key = outside.join(".ssh").join("id_ed25519");
    // Control: the same command on the trusted backend can read it. Proves the test would notice a leak.
    let leak = run_check_in(Backend::TrustedLocal, &format!("cat '{}'", key.display()), &ws, Duration::from_secs(20), None, Arc::new(AtomicBool::new(false)));
    assert!(leak.output_tail.contains("SECRET-KEY-MATERIAL-123"));
    let r = run(&ws, &format!("cat '{}'; ls '{}'; ls /root /home", key.display(), outside.display()), 20);
    assert!(!r.output_tail.contains("SECRET-KEY-MATERIAL-123"), "{}", r.output_tail);
    // The directory holding it does not even exist in the sandbox (the error names the path, never its contents).
    assert!(r.output_tail.contains(&format!("cannot access '{}': No such file", outside.display())), "{}", r.output_tail);
    // The real home directory of the user running FORGE is not there either.
    if let Some(home) = std::env::var_os("HOME") {
        let r = run(&ws, &format!("ls -a '{}' 2>&1 | head -5; echo HOME_IS=$HOME", Path::new(&home).display()), 20);
        assert!(r.output_tail.contains("HOME_IS=/home/forge"), "{}", r.output_tail);
        assert!(r.output_tail.contains("No such file") || r.output_tail.lines().count() <= 2, "{}", r.output_tail);
    }
}

#[test]
fn a_command_cannot_write_outside_its_workspace_but_can_write_inside_it() {
    if !sandbox_available() {
        return;
    }
    let (_d, ws, outside) = setup();
    let r = run(&ws, &format!("echo pwned > '{}/planted' ; echo ok > made-inside.txt; echo x > /tmp/scratch && echo tmp-ok", outside.display()), 20);
    assert!(!outside.join("planted").exists(), "wrote outside the workspace: {}", r.output_tail);
    assert_eq!(std::fs::read_to_string(ws.join("made-inside.txt")).unwrap().trim(), "ok");
    assert!(r.output_tail.contains("tmp-ok"), "a private /tmp is usable: {}", r.output_tail);
    // ...and that private /tmp is not the host's.
    assert!(!Path::new("/tmp/scratch").exists() || std::fs::read_to_string("/tmp/scratch").map(|s| s.trim() != "x").unwrap_or(true));
}

#[test]
fn secrets_in_forges_environment_do_not_reach_the_command() {
    if !sandbox_available() {
        return;
    }
    let (_d, ws, _) = setup();
    // A value redaction would NOT catch, so a pass proves the sandbox kept it out, not that the redactor hid it.
    // SAFETY: a name unique to this test; nothing else reads it.
    unsafe { std::env::set_var("FORGE_SANDBOX_TEST_PLAIN", "plainvalue-unredactable-7731") };
    let ctl = run_check_in(Backend::TrustedLocal, "echo $FORGE_SANDBOX_TEST_PLAIN", &ws, Duration::from_secs(20), None, Arc::new(AtomicBool::new(false)));
    assert!(ctl.output_tail.contains("plainvalue-unredactable-7731"), "control: {}", ctl.output_tail);
    let r = run(&ws, "env; echo SANDBOX=$FORGE_SANDBOX", 20);
    assert!(!r.output_tail.contains("plainvalue-unredactable-7731"), "{}", r.output_tail);
    assert!(r.output_tail.contains("SANDBOX=linux_namespaces"));
    assert!(!r.output_tail.contains("SSH_AUTH_SOCK"), "{}", r.output_tail);
}

#[test]
fn a_command_cannot_reach_the_network_or_host_loopback_services() {
    if !sandbox_available() {
        return;
    }
    let (_d, ws, _) = setup();
    // A service on the host's loopback, e.g. a local database or an agent's API endpoint.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    listener.set_nonblocking(true).unwrap();
    let probe = format!("node -e \"const s=require('net').connect({port},'127.0.0.1');s.on('connect',()=>{{console.log('CONNECTED');process.exit(0)}});s.on('error',e=>{{console.log('BLOCKED '+e.code);process.exit(0)}})\"");
    // Control: from the host the connection succeeds.
    let ctl = run_check_in(Backend::TrustedLocal, &probe, &ws, Duration::from_secs(20), None, Arc::new(AtomicBool::new(false)));
    assert!(ctl.output_tail.contains("CONNECTED"), "control failed: {}", ctl.output_tail);
    let _ = listener.accept();
    let r = run(&ws, &probe, 20);
    assert!(r.output_tail.contains("BLOCKED"), "{}", r.output_tail);
    assert!(listener.accept().is_err(), "the sandbox reached the host's listener");
    let dns = run(&ws, "node -e \"require('dns').lookup('example.com',e=>{console.log(e?'NO-DNS':'RESOLVED');})\"", 20);
    assert!(dns.output_tail.contains("NO-DNS"), "{}", dns.output_tail);
}

#[test]
fn nothing_started_by_a_command_outlives_it_and_host_processes_are_invisible() {
    if !sandbox_available() {
        return;
    }
    let (_d, ws, _) = setup();
    // Leaves a background writer behind and exits at once.
    let r = run(&ws, "(while true; do date +%s%N >> heartbeat.txt; sleep 0.1; done) & echo started; ls /proc | grep -c '^[0-9]'", 20);
    assert_eq!(r.state, RunState::Passed, "{}", r.output_tail);
    let visible: usize = r.output_tail.lines().last().unwrap_or("999").trim().parse().unwrap_or(999);
    assert!(visible < 10, "only the sandbox's own processes are visible, saw {visible}");
    std::thread::sleep(Duration::from_millis(600));
    let a = std::fs::read_to_string(ws.join("heartbeat.txt")).unwrap_or_default().lines().count();
    std::thread::sleep(Duration::from_millis(800));
    let b = std::fs::read_to_string(ws.join("heartbeat.txt")).unwrap_or_default().lines().count();
    assert_eq!(a, b, "a background process kept running after the run ended");
}

#[test]
fn the_timeout_still_ends_the_whole_sandbox() {
    if !sandbox_available() {
        return;
    }
    let (_d, ws, _) = setup();
    let t = Instant::now();
    let r = run(&ws, "(sleep 60 &) ; sleep 60", 1);
    assert_eq!(r.state, RunState::Timeout);
    assert!(t.elapsed() < Duration::from_secs(10), "{:?}", t.elapsed());
}

#[test]
fn ordinary_toolchains_still_work_inside() {
    if !sandbox_available() {
        return;
    }
    let (_d, ws, _) = setup();
    std::fs::write(ws.join("package.json"), r#"{"name":"t","version":"1.0.0","scripts":{"test":"node -e \"console.log('tests ran')\""}}"#).unwrap();
    let r = run(&ws, "node --version && npm test --silent && git --version", 60);
    assert_eq!(r.state, RunState::Passed, "{}", r.output_tail);
    assert!(r.output_tail.contains("tests ran"), "{}", r.output_tail);
}

#[test]
fn git_inside_a_mission_worktree_can_read_history_but_not_change_the_repository() {
    if !sandbox_available() {
        return;
    }
    let repo = crate::git::testutil::init_repo();
    let parent = tempfile::tempdir().unwrap();
    let wt = parent.path().join("wt");
    let base = crate::git::head_commit(repo.path()).unwrap();
    crate::git::worktree_add(repo.path(), &wt, "forge/sbx", &base).unwrap();
    let r = run(&wt, "git log --oneline -1 && git -c user.name=x -c user.email=x@x commit --allow-empty -m evil; git branch -f main HEAD~0 2>&1; echo done", 30);
    assert!(r.output_tail.contains("init"), "history readable: {}", r.output_tail);
    assert_eq!(crate::git::head_commit(repo.path()).unwrap(), base, "the main repository was modified from inside the sandbox");
    assert_eq!(crate::git::run(repo.path(), &["rev-parse", "forge/sbx"]).unwrap().trim(), base, "the mission branch moved from inside the sandbox");
}

#[test]
fn an_unavailable_backend_is_an_error_never_a_silent_fallback() {
    let d = tempfile::tempdir().unwrap();
    let r = run_check_in(Backend::WindowsSandbox, "echo ran > ran.txt", d.path(), Duration::from_secs(5), None, Arc::new(AtomicBool::new(false)));
    assert_eq!(r.state, RunState::Error);
    assert!(r.output_tail.contains("will not fall back"), "{}", r.output_tail);
    assert!(!d.path().join("ran.txt").exists());
}
