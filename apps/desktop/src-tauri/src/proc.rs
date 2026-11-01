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

/// The command was configured with `isolate` immediately before spawning. Unlike `contain`, this
/// also handles a fast-exiting leader while descendants still retain its process group and pipes.
#[cfg(unix)]
pub fn contain_isolated(child: &Child) -> Containment {
    Containment { pgid: child.id() }
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

/// Resolve a Windows utility without searching the current directory or PATH.
#[cfg(windows)]
fn system_utility(name: &str) -> Option<std::path::PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
    let mut buf = vec![0u16; 32768];
    // SAFETY: the buffer is valid for the supplied number of UTF-16 code units.
    let len = unsafe { GetSystemDirectoryW(buf.as_mut_ptr(), buf.len() as u32) } as usize;
    if len == 0 || len >= buf.len() { return None; }
    let dir = std::path::PathBuf::from(std::ffi::OsString::from_wide(&buf[..len]));
    if !dir.is_absolute() { return None; }
    Some(dir.join(name))
}

/// Forcefully terminate a process and its whole tree.
pub fn kill_tree(pid: u32) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let Some(exe) = system_utility("taskkill.exe") else { return };
        let _ = std::process::Command::new(&exe)
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .current_dir(exe.parent().unwrap())
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
        let Some(exe) = system_utility("tasklist.exe") else { return false };
        let out = std::process::Command::new(&exe)
            .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
            .current_dir(exe.parent().unwrap())
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

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::os::windows::process::CommandExt;
    use std::time::{Duration, Instant};

    #[test]
    fn process_utilities_use_the_windows_system_directory() {
        for name in ["taskkill.exe", "tasklist.exe"] {
            let path = system_utility(name).unwrap();
            assert!(path.is_absolute() && path.is_file(), "{}", path.display());
            assert_eq!(path.file_name().unwrap(), name);
        }
    }

    #[test]
    fn system_utility_can_terminate_a_disposable_child() {
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "providers::tests::provider_probe_fixture", "--nocapture"])
            .env("FORGE_PROBE_FIXTURE", "sleep")
            .creation_flags(0x0800_0000)
            .spawn().unwrap();
        assert!(is_alive(child.id()));
        kill_tree(child.id());
        let start = Instant::now();
        while child.try_wait().unwrap().is_none() && start.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(child.try_wait().unwrap().is_some(), "disposable child was not terminated");
    }

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
