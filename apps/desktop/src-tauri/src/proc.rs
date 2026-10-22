//! Process containment. On Windows every engine process is placed in a Job Object with
//! KILL_ON_JOB_CLOSE, so if FORGE crashes or the handle is dropped, the engine and every
//! descendant (the agent CLI, its shells, test runners) is terminated by the OS.

use std::process::Child;

pub struct Containment {
    #[cfg(windows)]
    _job: Option<win32job::Job>,
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

#[cfg(not(windows))]
pub fn contain(_child: &Child) -> Result<Containment, String> {
    // No kill-on-close primitive is wired up on this platform yet; sessions rely on the engine exiting when its
    // stdin closes plus an explicit tree kill.
    Ok(Containment {})
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
