//! Execution backends: *where* FORGE runs a command it was asked to run (a verification check, an experiment run).
//!
//! FORGE's other defences (worktrees, policy, approvals, Job Objects / process groups) decide *whether* something runs. They do
//! not stop a command from reading the user's files, credentials or network once it runs. An execution backend is the layer
//! that can. Each backend states what it actually enforces ([`Capabilities`]); nothing here claims more than its tests show.
//!
//! * `trusted_local` — the user's own shell, privileges, home directory and network. **Not isolated.** Kept for compatibility.
//! * `linux_namespaces` — bubblewrap (an external program, LGPL-2.0-or-later, invoked not linked): new user/pid/net/ipc/uts
//!   namespaces, an empty filesystem with only system and toolchain directories mounted read-only, the workspace mounted
//!   read-write, no home directory, a cleared environment and no network. Linux only. On Windows this is the planned inner
//!   layer of the isolated-WSL backend.
//! * `windows_sandbox`, `isolated_wsl` — designed (docs/EXECUTION.md), **not implemented**; always reported unavailable.
//!
//! A selected backend that is unavailable is an error, never a silent fall-back to `trusted_local`.

use crate::error::{ForgeError, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    TrustedLocal,
    LinuxNamespaces,
    WindowsSandbox,
    IsolatedWsl,
}

impl Backend {
    pub const ALL: [Backend; 4] = [Backend::TrustedLocal, Backend::LinuxNamespaces, Backend::WindowsSandbox, Backend::IsolatedWsl];

    pub fn as_str(self) -> &'static str {
        match self {
            Backend::TrustedLocal => "trusted_local",
            Backend::LinuxNamespaces => "linux_namespaces",
            Backend::WindowsSandbox => "windows_sandbox",
            Backend::IsolatedWsl => "isolated_wsl",
        }
    }

    pub fn parse(s: &str) -> Result<Backend> {
        Backend::ALL.into_iter().find(|b| b.as_str() == s).ok_or_else(|| ForgeError::Other(format!("unknown execution backend \"{s}\"")))
    }

    pub fn label(self) -> &'static str {
        match self {
            Backend::TrustedLocal => "Trusted local (not isolated)",
            Backend::LinuxNamespaces => "Isolated: Linux namespaces (bubblewrap)",
            Backend::WindowsSandbox => "Isolated: Windows Sandbox",
            Backend::IsolatedWsl => "Isolated: dedicated WSL distribution",
        }
    }
}

/// How strongly a property is enforced. `None` means "not at all", stated plainly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    None,
    Partial,
    Enforced,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Property {
    pub level: Level,
    pub detail: String,
}

fn prop(level: Level, detail: &str) -> Property {
    Property { level, detail: detail.into() }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub operating_systems: Vec<String>,
    pub filesystem_isolation: Property,
    pub process_isolation: Property,
    pub network_control: Property,
    pub resource_limits: Property,
    pub workspace_mapping: Property,
    pub credential_exposure: Property,
    pub persistent_state: Property,
    pub interactive_terminal: bool,
    pub concurrent_instances: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BackendInfo {
    pub id: Backend,
    pub label: String,
    /// Code for this backend exists in this build.
    pub implemented: bool,
    /// It can run on this machine right now.
    pub available: bool,
    pub unavailable_reason: Option<String>,
    pub capabilities: Capabilities,
}

pub fn capabilities(b: Backend) -> Capabilities {
    use Level::*;
    match b {
        Backend::TrustedLocal => Capabilities {
            operating_systems: vec!["windows".into(), "linux".into(), "macos".into()],
            filesystem_isolation: prop(None, "Runs as you: can read and write everything your account can, including your home directory, other repositories and FORGE's own data."),
            process_isolation: prop(Partial, "Contained for clean-up only (Windows Job Object / Unix process group); not separated from your other processes."),
            network_control: prop(None, "Full network access."),
            resource_limits: prop(Partial, "Time limit and output caps only."),
            workspace_mapping: prop(Enforced, "Runs directly in the mission worktree."),
            credential_exposure: prop(None, "Your environment variables, SSH keys, git credentials and provider logins are all reachable."),
            persistent_state: prop(None, "Anything it writes anywhere stays."),
            interactive_terminal: false,
            concurrent_instances: true,
        },
        Backend::LinuxNamespaces => Capabilities {
            operating_systems: vec!["linux".into()],
            filesystem_isolation: prop(Enforced, "Empty root filesystem: only /usr, the system library and binary directories, a short list of /etc files and toolchain directories from PATH outside your home are mounted, read-only. Your home directory, other repositories and FORGE's data are not mounted."),
            process_isolation: prop(Enforced, "New user, PID, IPC and UTS namespaces; no capabilities; every process inside ends when the run ends."),
            network_control: prop(Enforced, "New network namespace with only a private loopback: no outbound or host-loopback connections."),
            resource_limits: prop(Partial, "Time limit, output caps, no core dumps and a per-file size cap. No memory or CPU limit (needs cgroups; not implemented)."),
            workspace_mapping: prop(Enforced, "The mission worktree is mounted read-write at its own path; the repository's git directory is mounted read-only so git can read history."),
            credential_exposure: prop(Enforced, "Environment cleared to a fixed minimal set; no home directory, SSH agent, git credentials or provider logins are reachable. Toolchains installed under your home (rustup, nvm) are therefore not available."),
            persistent_state: prop(Partial, "Only the workspace persists; /tmp and the home directory are discarded after each run."),
            interactive_terminal: false,
            concurrent_instances: true,
        },
        Backend::WindowsSandbox => Capabilities {
            operating_systems: vec!["windows (Pro, Enterprise, Education)".into()],
            filesystem_isolation: prop(None, "Not implemented. Design: a disposable VM with no host profile mount and only an explicit exchange folder."),
            process_isolation: prop(None, "Not implemented."),
            network_control: prop(None, "Not implemented. Design: networking disabled by default."),
            resource_limits: prop(None, "Not implemented."),
            workspace_mapping: prop(None, "Not implemented. Design: copy in, copy results out; no writable host mount by default."),
            credential_exposure: prop(None, "Not implemented."),
            persistent_state: prop(None, "Not implemented. Design: discarded when the sandbox closes."),
            interactive_terminal: false,
            concurrent_instances: false,
        },
        Backend::IsolatedWsl => Capabilities {
            operating_systems: vec!["windows (WSL 2)".into()],
            filesystem_isolation: prop(None, "Not implemented. Design: a FORGE-owned distribution with automount and interop disabled, running the Linux-namespace backend inside."),
            process_isolation: prop(None, "Not implemented."),
            network_control: prop(None, "Not implemented."),
            resource_limits: prop(None, "Not implemented."),
            workspace_mapping: prop(None, "Not implemented."),
            credential_exposure: prop(None, "Not implemented."),
            persistent_state: prop(None, "Not implemented."),
            interactive_terminal: false,
            concurrent_instances: true,
        },
    }
}

fn bwrap_path() -> Option<PathBuf> {
    which::which("bwrap").ok()
}

/// Can bubblewrap create the namespaces it needs here? (User namespaces can be disabled by the kernel or by AppArmor.)
fn probe_linux_namespaces() -> std::result::Result<(), String> {
    if !cfg!(target_os = "linux") {
        return Err("Linux only. On Windows this backend is the planned inner layer of the isolated-WSL backend.".into());
    }
    let Some(bwrap) = bwrap_path() else {
        return Err("bubblewrap (`bwrap`) is not installed. Install it with your package manager (e.g. `apt install bubblewrap`).".into());
    };
    let out = Command::new(bwrap)
        .args(["--unshare-all", "--die-with-parent", "--ro-bind", "/usr", "/usr", "--proc", "/proc", "--dev", "/dev"])
        .args(symlink_or_bind_args())
        .args(["/bin/sh", "-c", "exit 0"])
        .output()
        .map_err(|e| format!("could not run bwrap: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("bubblewrap cannot create a sandbox here (user namespaces may be disabled): {}", String::from_utf8_lossy(&out.stderr).trim()))
    }
}

pub fn availability(b: Backend) -> std::result::Result<(), String> {
    match b {
        Backend::TrustedLocal => Ok(()),
        Backend::LinuxNamespaces => {
            static PROBE: OnceLock<std::result::Result<(), String>> = OnceLock::new();
            PROBE.get_or_init(probe_linux_namespaces).clone()
        }
        Backend::WindowsSandbox | Backend::IsolatedWsl => Err("Not implemented in this build. See docs/EXECUTION.md for the design.".into()),
    }
}

pub fn info(b: Backend) -> BackendInfo {
    let a = availability(b);
    BackendInfo {
        id: b,
        label: b.label().into(),
        implemented: matches!(b, Backend::TrustedLocal | Backend::LinuxNamespaces),
        available: a.is_ok(),
        unavailable_reason: a.err(),
        capabilities: capabilities(b),
    }
}

pub fn all() -> Vec<BackendInfo> {
    Backend::ALL.into_iter().map(info).collect()
}

/// Refuse rather than downgrade: the caller asked for `b`, so `b` must be usable.
pub fn require(b: Backend) -> Result<()> {
    availability(b).map_err(|why| {
        ForgeError::InvalidState(format!(
            "this project runs commands in \"{}\", which is not available: {why} FORGE will not fall back to a less isolated backend; choose one explicitly in the project's settings.",
            b.label()
        ))
    })
}

/// What a command needs from the backend.
pub struct RunSpec<'a> {
    pub command: &'a str,
    /// The directory the command runs in (a mission worktree or an experiment worktree).
    pub workspace: &'a Path,
}

/// On merged-/usr systems /bin, /lib, ... are symlinks into /usr; mirror them, otherwise bind them read-only if present.
fn symlink_or_bind_args() -> Vec<String> {
    let mut a = vec![];
    for d in ["/bin", "/sbin", "/lib", "/lib64", "/lib32"] {
        match std::fs::symlink_metadata(d) {
            Ok(m) if m.file_type().is_symlink() => {
                if let Ok(t) = std::fs::read_link(d) {
                    a.extend(["--symlink".into(), t.to_string_lossy().into_owned(), d.into()]);
                }
            }
            Ok(_) => a.extend(["--ro-bind".into(), d.into(), d.into()]),
            Err(_) => {}
        }
    }
    a
}

/// Files from /etc that tools commonly need (users, DNS config, TLS roots, dynamic linker). Not the whole of /etc.
const ETC_FILES: &[&str] = &[
    "/etc/passwd", "/etc/group", "/etc/hosts", "/etc/nsswitch.conf", "/etc/resolv.conf", "/etc/ld.so.cache", "/etc/ld.so.conf",
    "/etc/ld.so.conf.d", "/etc/localtime", "/etc/ssl", "/etc/ca-certificates", "/etc/pki", "/etc/alternatives", "/etc/crypto-policies",
];

const SYSTEM_PREFIXES: &[&str] = &["/usr", "/bin", "/sbin", "/lib", "/lib64", "/lib32", "/etc", "/proc", "/dev", "/tmp", "/home", "/root", "/run", "/var"];

/// Toolchain directories from PATH that live outside system directories and outside the user's home (e.g. /opt/node22/bin →
/// /opt/node22). Directories under the home are left out on purpose: mounting them would expose the home directory.
pub fn toolchain_roots(path_var: &str, home: Option<&Path>) -> Vec<PathBuf> {
    let system: Vec<&Path> = SYSTEM_PREFIXES.iter().map(Path::new).collect();
    toolchain_roots_excluding(path_var, home, &system)
}

fn toolchain_roots_excluding(path_var: &str, home: Option<&Path>, system: &[&Path]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = vec![];
    for d in std::env::split_paths(path_var) {
        if !d.is_absolute() || !d.is_dir() {
            continue;
        }
        if home.is_some_and(|h| d.starts_with(h)) || system.iter().any(|p| d.starts_with(p)) || d == Path::new("/") {
            continue;
        }
        // A `bin` directory usually needs its installation root (node's lib/node_modules, a JDK's lib, ...).
        let root = if d.file_name().is_some_and(|n| n == "bin") { d.parent().map(Path::to_path_buf).unwrap_or(d.clone()) } else { d.clone() };
        if root.components().count() < 3 {
            continue; // never a top-level directory such as /opt or /nix itself
        }
        if !out.iter().any(|o| root.starts_with(o)) {
            out.retain(|o| !o.starts_with(&root));
            out.push(root);
        }
    }
    out
}

/// The git directory shared by a worktree (read from its `.git` file), so it can be mounted read-only.
fn git_common_dir(workspace: &Path) -> Option<PathBuf> {
    let gitfile = std::fs::read_to_string(workspace.join(".git")).ok()?;
    let gitdir = PathBuf::from(gitfile.trim().strip_prefix("gitdir:")?.trim());
    let gitdir = if gitdir.is_absolute() { gitdir } else { workspace.join(gitdir) };
    // <repo>/.git/worktrees/<name> -> <repo>/.git
    let common = gitdir.parent()?.parent()?.to_path_buf();
    common.is_dir().then_some(common)
}

const SANDBOX_HOME: &str = "/home/forge";

/// The process to spawn for `spec` on backend `b`. The caller adds stdio, containment and timeouts as before.
pub fn command(b: Backend, spec: &RunSpec) -> Result<Command> {
    require(b)?;
    match b {
        Backend::TrustedLocal => Ok(host_shell(spec)),
        Backend::LinuxNamespaces => {
            let bwrap = bwrap_path().ok_or_else(|| ForgeError::Other("bwrap disappeared".into()))?;
            let ws = dunce::canonicalize(spec.workspace).map_err(|e| ForgeError::PathRejected(format!("workspace not accessible: {e}")))?;
            let path_var = std::env::var("PATH").unwrap_or_default();
            let home = std::env::var_os("HOME").map(PathBuf::from);
            let roots = toolchain_roots(&path_var, home.as_deref());
            let mut c = Command::new(bwrap);
            c.args(["--unshare-all", "--die-with-parent", "--new-session", "--cap-drop", "ALL", "--clearenv"]);
            c.args(["--ro-bind", "/usr", "/usr"]).args(symlink_or_bind_args());
            for f in ETC_FILES {
                c.args(["--ro-bind-try", f, f]);
            }
            for r in &roots {
                c.arg("--ro-bind-try").arg(r).arg(r);
            }
            c.args(["--proc", "/proc", "--dev", "/dev", "--tmpfs", "/tmp", "--tmpfs", "/home", "--dir", SANDBOX_HOME]);
            if let Some(common) = git_common_dir(&ws) {
                c.arg("--ro-bind").arg(&common).arg(&common);
            }
            c.arg("--bind").arg(&ws).arg(&ws).arg("--chdir").arg(&ws);
            // Only what tools need; nothing inherited from FORGE's environment.
            let inner_path: Vec<String> = roots.iter().map(|r| if r.join("bin").is_dir() { r.join("bin") } else { r.clone() }).map(|p| p.to_string_lossy().into_owned()).chain(["/usr/local/sbin", "/usr/local/bin", "/usr/sbin", "/usr/bin", "/sbin", "/bin"].map(String::from)).collect();
            for (k, v) in [("PATH", inner_path.join(":")), ("HOME", SANDBOX_HOME.into()), ("TMPDIR", "/tmp".into()), ("LANG", "C.UTF-8".into()), ("CI", "true".into()), ("NO_COLOR", "1".into()), ("FORCE_COLOR", "0".into()), ("GIT_TERMINAL_PROMPT", "0".into()), ("FORGE_SANDBOX", "linux_namespaces".into())] {
                c.arg("--setenv").arg(k).arg(v);
            }
            // No core dumps (they can contain secrets from memory); no single file over 4 GiB.
            let inner = format!("ulimit -c 0; ulimit -f 8388608; exec /bin/sh -c {}", sh_quote(spec.command));
            c.args(["--", "/bin/sh", "-c", &inner]);
            Ok(c)
        }
        Backend::WindowsSandbox | Backend::IsolatedWsl => Err(ForgeError::Other("not implemented".into())),
    }
}

/// An agent session (the engine and the provider process it starts) to run inside a backend.
pub struct AgentSpec<'a> {
    pub program: &'a Path,
    pub args: &'a [std::ffi::OsString],
    /// The mission worktree: the only directory the agent may write besides `read_write`.
    pub workspace: &'a Path,
    /// Directories the agent needs to read (the engine and its modules, the provider binary).
    pub read_only: Vec<PathBuf>,
    /// The provider's own state (its login and session transcripts), writable because the provider updates it.
    pub read_write: Vec<PathBuf>,
}

/// The process to spawn for an agent session on backend `b`. `trusted_local` is the plain process (today's behaviour).
///
/// `linux_namespaces` for agents differs from checks in two deliberate ways: the network is shared (the agent must reach its
/// model), and the environment is inherited (the engine already removed billing credentials). Everything else is the same
/// empty root: no home directory except the provider's own state, no other repositories, read-only system and toolchains,
/// the git directory read-only, a private /tmp, no new privileges, and the whole tree dies with FORGE.
pub fn agent_command(b: Backend, spec: &AgentSpec) -> Result<Command> {
    require(b)?;
    match b {
        Backend::TrustedLocal => {
            let mut c = Command::new(spec.program);
            c.args(spec.args).current_dir(spec.workspace);
            Ok(c)
        }
        Backend::LinuxNamespaces => {
            let bwrap = bwrap_path().ok_or_else(|| ForgeError::Other("bwrap disappeared".into()))?;
            let ws = dunce::canonicalize(spec.workspace).map_err(|e| ForgeError::PathRejected(format!("workspace not accessible: {e}")))?;
            let path_var = std::env::var("PATH").unwrap_or_default();
            let home = std::env::var_os("HOME").map(PathBuf::from);
            let roots = toolchain_roots(&path_var, home.as_deref());
            let mut c = Command::new(bwrap);
            c.args(["--unshare-all", "--share-net", "--die-with-parent", "--new-session", "--cap-drop", "ALL"]);
            c.args(["--ro-bind", "/usr", "/usr"]).args(symlink_or_bind_args());
            for f in ETC_FILES {
                c.args(["--ro-bind-try", f, f]);
            }
            // Empty, private versions of the places users keep things; what the agent needs is bound on top, below.
            c.args(["--proc", "/proc", "--dev", "/dev", "--tmpfs", "/tmp", "--tmpfs", "/home", "--tmpfs", "/root", "--tmpfs", "/run"]);
            // The home directory exists (programs expect it) but is empty, except for the provider's own state below.
            if let Some(h) = &home {
                c.arg("--dir").arg(h);
            }
            for r in roots.iter().chain(spec.read_only.iter()) {
                c.arg("--ro-bind-try").arg(r).arg(r);
            }
            for rw in &spec.read_write {
                c.arg("--bind-try").arg(rw).arg(rw);
            }
            if let Some(common) = git_common_dir(&ws) {
                c.arg("--ro-bind").arg(&common).arg(&common);
            }
            c.arg("--bind").arg(&ws).arg(&ws).arg("--chdir").arg(&ws);
            c.args(["--setenv", "FORGE_SANDBOX", "linux_namespaces", "--"]).arg(spec.program).args(spec.args);
            Ok(c)
        }
        Backend::WindowsSandbox | Backend::IsolatedWsl => Err(ForgeError::Other("not implemented".into())),
    }
}

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// The user's own shell (what checks always used): cmd.exe on Windows, sh elsewhere.
fn host_shell(spec: &RunSpec) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut c = Command::new("cmd.exe");
        c.args(["/d", "/s", "/c"]).raw_arg(format!("\"{}\"", spec.command));
        c.creation_flags(0x0800_0000);
        c.current_dir(spec.workspace);
        c
    }
    #[cfg(not(windows))]
    {
        let mut c = Command::new("sh");
        c.args(["-c", spec.command]);
        c.current_dir(spec.workspace);
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unimplemented_backends_are_never_available_and_say_so() {
        for b in [Backend::WindowsSandbox, Backend::IsolatedWsl] {
            let i = info(b);
            assert!(!i.implemented && !i.available);
            assert!(i.unavailable_reason.unwrap().contains("Not implemented"));
            assert!(require(b).is_err());
            assert_eq!(i.capabilities.filesystem_isolation.level, Level::None);
        }
        assert!(info(Backend::TrustedLocal).available);
        assert_eq!(capabilities(Backend::TrustedLocal).filesystem_isolation.level, Level::None, "trusted local must never claim isolation");
        for b in Backend::ALL {
            assert_eq!(Backend::parse(b.as_str()).unwrap(), b);
        }
        assert!(Backend::parse("docker").is_err());
    }

    #[test]
    fn toolchains_under_the_home_or_system_directories_are_not_mounted() {
        let d = tempfile::tempdir().unwrap();
        let home = d.path().join("home");
        for p in ["opt/node22/bin", "opt/tools", "home/.cargo/bin"] {
            std::fs::create_dir_all(d.path().join(p)).unwrap();
        }
        let path = std::env::join_paths([d.path().join("opt/node22/bin"), d.path().join("opt/tools"), home.join(".cargo/bin"), PathBuf::from("/usr/bin"), PathBuf::from("relative")]).unwrap();
        let roots = toolchain_roots_excluding(&path.to_string_lossy(), Some(&home), &[Path::new("/usr")]);
        assert_eq!(roots, vec![d.path().join("opt/node22"), d.path().join("opt/tools")]);
    }

    #[test]
    fn quoting_survives_hostile_commands() {
        assert_eq!(sh_quote("echo 'a'; rm -rf x"), r"'echo '\''a'\''; rm -rf x'");
    }
}
