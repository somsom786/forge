//! Process containment. On Windows every engine process is placed in a Job Object with
//! KILL_ON_JOB_CLOSE, so if FORGE crashes or the handle is dropped, the engine and every
//! descendant (the agent CLI, its shells, test runners) is terminated by the OS.
//!
//! On Unix every supervised process is started as the leader of its own process group ([`isolate`]); dropping the
//! containment kills the whole group. Unlike a Job Object this does not survive a crash of FORGE itself (the engine then
//! exits on its own when its stdin closes), and a descendant that calls `setsid` leaves the group.

use std::process::Child;

pub struct Containment {
    #[cfg(windows)]
    _job: Option<win32job::Job>,
    #[cfg(not(windows))]
    pgid: u32,
}

#[cfg(not(windows))]
impl Drop for Containment {
    fn drop(&mut self) {
        kill_group(self.pgid);
    }
}

/// Prepare a command so that it can be contained: on Unix it becomes the leader of a new process group, so it and every
/// descendant can be ended together. Must be called before `spawn`. (On Windows the Job Object is assigned after spawn.)
pub fn isolate(cmd: &mut std::process::Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    #[cfg(not(unix))]
    let _ = cmd;
}

#[cfg(not(windows))]
fn kill_group(pgid: u32) {
    if pgid > 1 {
        let _ = std::process::Command::new("kill").args(["-KILL", "--", &format!("-{pgid}")]).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
    }
}

/// Place a freshly spawned process in a kill-on-close containment. Callers must treat failure as fatal for the
/// session: an uncontained agent could outlive FORGE. (Dropping the returned value terminates the process tree.)
#[cfg(windows)]
pub fn contain(child: &Child) -> Result<Containment, String> {
    use std::os::windows::io::AsRawHandle;
    let job = (|| -> Result<win32job::Job, win32job::JobError> {
        let job = win32job::Job::create()?;
        let mut info = job.query_extended_limit_info()?;
        info.limit_kill_on_job_close();
        job.set_extended_limit_info(&info)?;
        job.assign_process(child.as_raw_handle() as isize)?;
        Ok(job)
    })()
    .map_err(|e| format!("job object setup failed for pid {}: {e}", child.id()))?;
    Ok(Containment { _job: Some(job) })
}

/// The child must have been spawned with [`isolate`] (it leads its own process group); otherwise containment is refused,
/// because killing "its group" would mean killing FORGE's own.
#[cfg(not(windows))]
pub fn contain(child: &Child) -> Result<Containment, String> {
    let pid = child.id();
    let out = std::process::Command::new("ps").args(["-o", "pgid=", "-p", &pid.to_string()]).output().map_err(|e| format!("could not inspect pid {pid}: {e}"))?;
    let pgid: u32 = String::from_utf8_lossy(&out.stdout).trim().parse().map_err(|_| format!("pid {pid} has no readable process group (it may have exited already)"))?;
    if pgid != pid {
        return Err(format!("pid {pid} is not the leader of its own process group; it was not started with proc::isolate"));
    }
    Ok(Containment { pgid })
}

/// Forcefully terminate a process and its whole tree.
pub fn kill_tree(pid: u32) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .creation_flags(0x0800_0000)
            .output();
    }
    #[cfg(not(windows))]
    {
        // The group first (it reaches descendants of an isolated process), then the process itself.
        kill_group(pid);
        let _ = std::process::Command::new("kill").args(["-9", &pid.to_string()]).output();
    }
}

/// Is a process with this pid currently alive?
pub fn is_alive(pid: u32) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let out = std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
            .creation_flags(0x0800_0000)
            .output();
        match out {
            Ok(o) => String::from_utf8_lossy(&o.stdout).contains(&format!("\"{pid}\"")),
            Err(_) => false,
        }
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("kill").args(["-0", &pid.to_string()]).output().map(|o| o.status.success()).unwrap_or(false)
    }
}

#[cfg(all(test, unix))]
mod unix_tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// Dropping the containment of an isolated process kills its descendants too (they share its process group), the Unix
    /// counterpart of a Job Object's kill-on-close.
    #[test]
    fn dropping_containment_kills_the_whole_process_group() {
        let dir = tempfile::tempdir().unwrap();
        let pidfile = dir.path().join("grandchild.pid");
        let mut cmd = std::process::Command::new("sh");
        cmd.args(["-c", &format!("sleep 60 & echo $! > '{}'; wait", pidfile.display())]);
        isolate(&mut cmd);
        let mut child = cmd.spawn().unwrap();
        let c = contain(&child).unwrap();
        let t = Instant::now();
        while !pidfile.exists() && t.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(20));
        }
        std::thread::sleep(Duration::from_millis(50));
        let grandchild: u32 = std::fs::read_to_string(&pidfile).unwrap().trim().parse().unwrap();
        assert!(is_alive(grandchild));
        drop(c);
        let _ = child.wait();
        let t = Instant::now();
        while is_alive(grandchild) && t.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!is_alive(grandchild), "the grandchild survived");
    }

    #[test]
    fn a_process_that_is_not_isolated_is_never_contained() {
        // Containing it would mean killing the group it shares with FORGE itself.
        let mut child = std::process::Command::new("sleep").arg("5").spawn().unwrap();
        assert!(contain(&child).is_err());
        let _ = child.kill();
        let _ = child.wait();
    }
}
