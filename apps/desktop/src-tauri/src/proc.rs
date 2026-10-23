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

/// `CREATE_SUSPENDED`: the process is created with its first thread suspended, so it runs no code until it is resumed. Spawning
/// with it and resuming only after [`contain`] succeeded closes the window in which a process could start a descendant before it
/// is in the Job (a descendant created before assignment is not in the Job and would survive the kill).
#[cfg(windows)]
pub const CREATE_SUSPENDED: u32 = 0x0000_0004;

/// Resume a process created with [`CREATE_SUSPENDED`]. A new process has exactly one thread; it is found by owner pid and
/// resumed. Fails if no thread of the process could be resumed (the caller must then kill the process).
#[cfg(windows)]
pub fn resume_suspended(child: &Child) -> Result<(), String> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next};
    use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};
    let pid = child.id();
    // SAFETY: plain Win32 calls on handles this function opens and closes itself; THREADENTRY32 is initialised with its size.
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        if snap == INVALID_HANDLE_VALUE {
            return Err(format!("could not list the threads of pid {pid}"));
        }
        let mut entry: THREADENTRY32 = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
        let mut resumed = 0;
        let mut ok = Thread32First(snap, &mut entry);
        while ok != 0 {
            if entry.th32OwnerProcessID == pid {
                let t = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                if !t.is_null() {
                    if ResumeThread(t) != u32::MAX {
                        resumed += 1;
                    }
                    CloseHandle(t);
                }
            }
            ok = Thread32Next(snap, &mut entry);
        }
        CloseHandle(snap);
        if resumed == 0 {
            return Err(format!("could not resume pid {pid}"));
        }
    }
    Ok(())
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

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::os::windows::process::CommandExt;
    use std::time::{Duration, Instant};

    /// A suspended process runs nothing until it is contained and resumed, then runs normally.
    #[test]
    fn a_suspended_process_runs_only_after_it_is_contained_and_resumed() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("ran.txt");
        let mut child = std::process::Command::new(std::env::var("ComSpec").unwrap_or_else(|_| "cmd.exe".into()))
            .args(["/d", "/c", "echo ran>ran.txt"])
            .current_dir(dir.path())
            .creation_flags(0x0800_0000 | CREATE_SUSPENDED)
            .spawn()
            .unwrap();
        std::thread::sleep(Duration::from_millis(500));
        assert!(!marker.exists(), "a suspended process must not have run");
        assert!(child.try_wait().unwrap().is_none());
        let job = contain(&child).unwrap();
        resume_suspended(&child).unwrap();
        let t = Instant::now();
        while child.try_wait().unwrap().is_none() && t.elapsed() < Duration::from_secs(20) {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(marker.exists(), "the resumed process did not run");
        drop(job);
    }
}
