//! Process containment. On Windows every engine process is placed in a Job Object with
//! KILL_ON_JOB_CLOSE, so if FORGE crashes or the handle is dropped, the engine and every
//! descendant (the agent CLI, its shells, test runners) is terminated by the OS.

use std::process::Child;

pub struct Containment {
    #[cfg(windows)]
    _job: Option<win32job::Job>,
}

#[cfg(windows)]
pub fn contain(child: &Child) -> Containment {
    use std::os::windows::io::AsRawHandle;
    let job = (|| -> Result<win32job::Job, win32job::JobError> {
        let job = win32job::Job::create()?;
        let mut info = job.query_extended_limit_info()?;
        info.limit_kill_on_job_close();
        job.set_extended_limit_info(&info)?;
        job.assign_process(child.as_raw_handle() as isize)?;
        Ok(job)
    })();
    if let Err(e) = &job {
        eprintln!("[forge] could not place process {} in a job object: {e}", child.id());
    }
    Containment { _job: job.ok() }
}

#[cfg(not(windows))]
pub fn contain(_child: &Child) -> Containment {
    Containment {}
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
