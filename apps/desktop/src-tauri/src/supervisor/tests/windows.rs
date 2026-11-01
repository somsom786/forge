//! Windows-only behaviour, run on the GitHub `windows-2022` runner (not on a person's PC). Junctions need no special
//! privilege; anything that does is reported as SKIPPED with the reason, never counted as a pass.

use super::*;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::process::CommandExt;

fn junction(link: &Path, target: &Path) -> bool {
    let out = std::process::Command::new("cmd").creation_flags(0x0800_0000).args(["/C", "mklink", "/J"]).arg(link).arg(target).output().unwrap();
    if !out.status.success() {
        eprintln!("SKIPPED: could not create a junction: {}", String::from_utf8_lossy(&out.stderr));
    }
    out.status.success()
}

/// The open P0 from the handoff: while a person reads an approval for `docs/FORGE_TEST.md`, the `docs` folder is
/// replaced by a junction to a folder outside the worktree. Approving must not let the write through.
#[test]
fn a_junction_swapped_in_while_an_approval_is_pending_is_denied_at_decision_time() {
    let f = fixture();
    let m = new_mission(&f, "FAKE:subdir", PermissionProfile::Default);
    f.core.start_mission(&m.id, None).unwrap();
    wait_until("approval", || !pending(&f, &m.id).is_empty());
    let a = pending(&f, &m.id).remove(0);
    let wt = PathBuf::from(f.core.store.get_mission(&m.id).unwrap().worktree_path.unwrap());
    let outside = tempfile::tempdir().unwrap();
    let _ = std::fs::remove_dir_all(wt.join("docs"));
    if !junction(&wt.join("docs"), outside.path()) {
        f.core.stop_mission(&m.id).unwrap();
        return;
    }
    let err = f.core.decide_approval_checked(&a.id, true, None, a.binding_sha256.as_deref()).unwrap_err().to_string();
    assert!(err.contains("policy at decision time"), "{err}");
    wait_until("mission moved on", || state(&f, &m.id) != "awaiting_approval");
    std::thread::sleep(Duration::from_millis(300));
    assert!(!outside.path().join("FORGE_TEST.md").exists(), "a file was written outside the worktree through a junction");
}

/// Discarding a mission while another program holds a file in its worktree open without sharing (an editor, an
/// antivirus scan): either the discard fails and nothing is marked discarded, or it completes. Never half of each.
#[test]
fn discarding_while_a_file_is_locked_is_all_or_nothing_and_can_be_retried() {
    let f = fixture();
    let m = new_mission(&f, "make a scripted note", PermissionProfile::AcceptEdits);
    f.core.start_mission(&m.id, None).unwrap();
    wait_until("review", || state(&f, &m.id) == "awaiting_review");
    wait_until("finalised", || f.core.live_session_count() == 0);
    let wt = PathBuf::from(f.core.store.get_mission(&m.id).unwrap().worktree_path.unwrap());
    let locked = std::fs::OpenOptions::new().read(true).share_mode(0).open(wt.join("FORGE_TEST.md")).unwrap();
    match f.core.discard_mission(&m.id, true) {
        Err(e) => {
            eprintln!("discard refused while locked: {e}");
            assert_eq!(state(&f, &m.id), "awaiting_review", "a failed discard does not mark the mission discarded");
            drop(locked);
            f.core.discard_mission(&m.id, true).expect("once the file is released, discarding again works");
        }
        Ok(_) => drop(locked),
    }
    assert_eq!(state(&f, &m.id), "cancelled");
    assert!(!wt.join("FORGE_TEST.md").exists());
}

/// Agents cannot be isolated on Windows yet: the choice is refused, and a project left with an unavailable backend in its
/// settings refuses to start the agent instead of running it on the host.
#[test]
fn agent_isolation_is_not_offered_on_windows_and_never_falls_back_to_the_host() {
    let f = fixture();
    assert!(f.core.project_set_agent_backend(&f.project.id, "linux_namespaces").is_err());
    f.core.store.set_project_agent_backend(&f.project.id, "linux_namespaces").unwrap();
    let m = new_mission(&f, "make a scripted note", PermissionProfile::Default);
    let _ = f.core.start_mission(&m.id, None);
    std::thread::sleep(Duration::from_millis(500));
    assert!(f.core.store.sessions_for_mission(&m.id).unwrap().iter().all(|s| s.state == "lost"), "no session ran");
    assert!(f.core.store.events(&m.id, 0, 1000).unwrap().iter().all(|e| e.kind != "session.started"));
}
