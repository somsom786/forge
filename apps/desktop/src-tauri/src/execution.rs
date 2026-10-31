//! Execution backends: *where* FORGE runs a command it was asked to run (a verification check, an experiment run).
//!
//! FORGE's other defences (worktrees, policy, approvals, Job Objects / process groups) decide *whether* something runs. They do
//! not stop a command from reading the user's files, credentials or network once it runs. An execution backend is the layer
//! that can. Each backend states what it actually enforces ([`Capabilities`]); nothing here claims more than its tests show.
//!
//! * `trusted_local` — the user's own shell, privileges, home directory and network. **Not isolated.** Kept for compatibility.
//! * `linux_namespaces` — bubblewrap (an external program, LGPL-2.0-or-later, invoked not linked): new user/pid/net/ipc/uts
//!   namespaces, an empty filesystem with only system and toolchain directories mounted read-only, the workspace mounted
//!   read-write, no home directory, a cleared environment and isolated IP networking. Mounted Unix sockets can still reach
//!   host services. Linux only. On Windows this is the planned inner
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
    pub agent_capabilities: Capabilities,
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
            filesystem_isolation: prop(Partial, "Empty root filesystem with read-only system, selected /etc and external PATH toolchain mounts, plus the workspace and linked Git metadata. Custom FORGE data, home or other sensitive files placed within those mounts may be exposed; mount selection is not a complete secret boundary."),
            process_isolation: prop(Enforced, "New user, PID, IPC and UTS namespaces; no capabilities; every process inside ends when the run ends."),
            network_control: prop(Partial, "A private network namespace blocks outbound IP and host-loopback connections. Unix sockets exposed by mounted directories can still connect to host services."),
            resource_limits: prop(Partial, "Time limit, output caps, no core dumps and a per-file size cap. No memory or CPU limit (needs cgroups; not implemented)."),
            workspace_mapping: prop(Enforced, "The mission worktree is mounted read-write at its own path; the repository's git directory is mounted read-only so git can read history."),
            credential_exposure: prop(Partial, "Environment cleared to a fixed minimal set; home and SSH agent are not deliberately mounted. Credentials within the workspace, Git metadata, system directories or selected toolchain mounts may still be reachable. Toolchains under your home are normally unavailable."),
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
        agent_capabilities: agent_capabilities(b),
    }
}

/// Agent sessions need provider connectivity and persistent login state; checks do not.
pub fn agent_capabilities(b: Backend) -> Capabilities {
    let mut c = capabilities(b);
    if b == Backend::LinuxNamespaces {
        c.network_control = prop(Level::None, "Network shared with the host so the provider can connect. Anything the agent can read can be sent over the network.");
        c.credential_exposure = prop(Level::Partial, "Environment restricted to runtime, provider-state and proxy settings. Provider login and other-project transcripts remain accessible; credentials in the workspace, Git metadata, system or toolchain mounts may also be exposed. SSH agents, cloud tokens and unrelated environment secrets are not inherited.");
        c.persistent_state = prop(Level::Partial, "Workspace and selected provider state persist. Other home files are not deliberately mounted, but may be reachable through overlapping mounts; temporary files are discarded.");
    }
    c
}

/// Explicit allow-list for contained sessions. Never forward loader injection, shell startup configuration, SSH agents,
/// cloud credentials or arbitrary FORGE environment variables. Provider authentication remains in its mounted state.
pub fn contained_agent_env<I>(base: I) -> Vec<(std::ffi::OsString, std::ffi::OsString)>
where I: IntoIterator<Item = (std::ffi::OsString, std::ffi::OsString)> {
    const ALLOWED: &[&str] = &[
        "PATH", "HOME", "USER", "LOGNAME", "LANG", "LC_ALL", "LC_CTYPE", "TZ", "TERM",
        "CLAUDE_CONFIG_DIR", "CODEX_HOME", "HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "NO_PROXY",
        "http_proxy", "https_proxy", "all_proxy", "no_proxy", "SSL_CERT_FILE", "SSL_CERT_DIR", "NODE_EXTRA_CA_CERTS",
    ];
    base.into_iter().filter(|(k, _)| k.to_str().is_some_and(|s| ALLOWED.contains(&s))).collect()
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
    let forge_home = crate::forge_home();
    // A not-yet-created default under HOME is already covered by the home exclusion. Explicit overrides and
    // existing or externally located defaults must resolve before any optional mount is admitted.
    let covered_missing_default = std::env::var_os("FORGE_HOME").is_none() && !forge_home.exists() && home.is_some_and(|h| forge_home.starts_with(h));
    let protected = if covered_missing_default { vec![] } else { vec![forge_home.as_path()] };
    toolchain_roots_excluding_protected(path_var, home, &system, &protected)
}

#[cfg(test)]
fn toolchain_roots_excluding(path_var: &str, home: Option<&Path>, system: &[&Path]) -> Vec<PathBuf> {
    toolchain_roots_excluding_protected(path_var, home, system, &[])
}

fn toolchain_roots_excluding_protected(path_var: &str, home: Option<&Path>, system: &[&Path], protected: &[&Path]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = vec![];
    // Without a verifiable home, we cannot prove that a path alias does not expose it.
    let Some(resolved_home) = home.and_then(|h| std::fs::canonicalize(h).ok()) else { return out };
    // A custom FORGE_HOME can live outside the user's home, including beneath an otherwise valid PATH root.
    // If it cannot be resolved, do not guess which optional toolchain roots are safe.
    let Ok(protected): std::result::Result<Vec<_>, _> = protected.iter().map(std::fs::canonicalize).collect() else { return out };
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
        // PATH entries and installation roots may be symlinks. A lexical check alone could bind a user's entire home
        // through an alias outside it, or bind an ancestor that contains the home. Fail closed if resolution fails.
        let Ok(resolved_root) = std::fs::canonicalize(&root) else { continue };
        if resolved_root.components().count() < 3 || resolved_root.starts_with(&resolved_home) || resolved_home.starts_with(&resolved_root)
            || protected.iter().any(|p| resolved_root.starts_with(p) || p.starts_with(&resolved_root))
            || system.iter().any(|p| resolved_root.starts_with(p)) {
            continue;
        }
        if !out.iter().any(|o| root.starts_with(o)) {
            out.retain(|o| !o.starts_with(&root));
            out.push(root);
        }
    }
    out
}

fn read_git_metadata(path: &Path, label: &str) -> Result<String> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| ForgeError::PathRejected(format!("invalid linked-worktree Git metadata: {label} is unavailable")))?;
    if !metadata.is_file() || metadata.len() > 16_384 {
        return Err(ForgeError::PathRejected(format!("invalid linked-worktree Git metadata: {label} is not a small regular file")));
    }
    std::fs::read_to_string(path).map_err(|_| ForgeError::PathRejected(format!("invalid linked-worktree Git metadata: {label} is unreadable")))
}

/// Validate both sides of Git's linked-worktree metadata before mounting its shared directory. Never infer a mount
/// from an arbitrary path in an agent-controlled `.git` file. Ordinary directories without linked metadata need no mount.
fn git_common_dir(workspace: &Path) -> Result<Option<PathBuf>> {
    let gitfile = workspace.join(".git");
    let metadata = match std::fs::symlink_metadata(&gitfile) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let invalid = |message: &str| ForgeError::PathRejected(format!("invalid linked-worktree Git metadata: {message}"));
    if metadata.file_type().is_symlink() { return Err(invalid(".git is a link")); }
    if metadata.is_dir() { return Ok(None) }
    let contents = read_git_metadata(&gitfile, ".git")?;
    let pointer = contents.trim().strip_prefix("gitdir:").map(str::trim).filter(|s| !s.is_empty()).ok_or_else(|| invalid("missing gitdir pointer"))?;
    let pointer = PathBuf::from(pointer);
    let gitdir = dunce::canonicalize(if pointer.is_absolute() { pointer } else { workspace.join(pointer) })
        .map_err(|_| invalid("gitdir does not exist"))?;
    let worktrees = gitdir.parent().filter(|p| p.file_name().is_some_and(|n| n == "worktrees"))
        .ok_or_else(|| invalid("gitdir is not a linked-worktree directory"))?;
    let common = worktrees.parent().ok_or_else(|| invalid("missing common Git directory"))?.to_path_buf();
    let back = read_git_metadata(&gitdir.join("gitdir"), "worktree backlink")?;
    let back = PathBuf::from(back.trim());
    let back = dunce::canonicalize(if back.is_absolute() { back } else { gitdir.join(back) })
        .map_err(|_| invalid("worktree backlink does not exist"))?;
    let expected = dunce::canonicalize(&gitfile).map_err(|_| invalid("workspace .git file is unavailable"))?;
    if back != expected { return Err(invalid("worktree backlink names a different workspace")); }
    let common_link = read_git_metadata(&gitdir.join("commondir"), "common-directory pointer")?;
    let common_link = PathBuf::from(common_link.trim());
    let resolved_common = dunce::canonicalize(if common_link.is_absolute() { common_link } else { gitdir.join(common_link) })
        .map_err(|_| invalid("common Git directory is unavailable"))?;
    if resolved_common != common { return Err(invalid("common-directory pointer is inconsistent")); }
    Ok(Some(common))
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
            if let Some(common) = git_common_dir(&ws)? {
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
/// model). The supervisor restricts the environment before spawning this process. Everything else is the same
/// empty root with selected provider state, read-only system and toolchain mounts, read-only Git metadata and a private /tmp.
/// Overlapping mounts can expose other data; the mounts are not a complete credential boundary.
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
            if let Some(common) = git_common_dir(&ws)? {
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
    fn contained_environment_drops_secrets_and_loader_injection_but_keeps_provider_runtime() {
        let base = ["PATH", "HOME", "CLAUDE_CONFIG_DIR", "CODEX_HOME", "HTTPS_PROXY", "NODE_OPTIONS", "NODE_PATH", "LD_PRELOAD", "BASH_ENV", "SSH_AUTH_SOCK", "GH_TOKEN", "AWS_SESSION_TOKEN", "unusual_private_value", "FORGE_ALLOW_FAKE_PROVIDER"];
        let kept: Vec<_> = contained_agent_env(base.map(|k| (k.into(), "fixture".into()))).into_iter().map(|(k, _)| k.to_string_lossy().into_owned()).collect();
        assert_eq!(kept, vec!["PATH", "HOME", "CLAUDE_CONFIG_DIR", "CODEX_HOME", "HTTPS_PROXY"]);
        assert_eq!(capabilities(Backend::LinuxNamespaces).network_control.level, Level::Partial);
        assert_eq!(capabilities(Backend::LinuxNamespaces).filesystem_isolation.level, Level::Partial);
        assert_eq!(capabilities(Backend::LinuxNamespaces).credential_exposure.level, Level::Partial);
        assert_eq!(agent_capabilities(Backend::LinuxNamespaces).network_control.level, Level::None);
        assert_eq!(agent_capabilities(Backend::LinuxNamespaces).credential_exposure.level, Level::Partial);
    }

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
        assert!(toolchain_roots_excluding(&path.to_string_lossy(), None, &[]).is_empty(), "unknown home cannot be protected from aliases");
    }

    #[test]
    fn toolchain_roots_do_not_expose_custom_forge_data() {
        let d = tempfile::tempdir().unwrap();
        let home = d.path().join("home");
        let bin = d.path().join("opt/tool/bin");
        let private = d.path().join("opt/tool/private-data");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&private).unwrap();
        let path = std::env::join_paths([&bin]).unwrap();
        assert!(toolchain_roots_excluding_protected(&path.to_string_lossy(), Some(&home), &[], &[&private]).is_empty());
        assert!(toolchain_roots_excluding_protected(&path.to_string_lossy(), Some(&home), &[], &[&d.path().join("opt")]).is_empty());
        assert!(toolchain_roots_excluding_protected(&path.to_string_lossy(), Some(&home), &[], &[&d.path().join("missing")]).is_empty());
        assert!(!toolchain_roots_excluding_protected(&path.to_string_lossy(), Some(&home), &[], &[]).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn toolchain_aliases_cannot_mount_home_or_its_ancestor() {
        use std::os::unix::fs::symlink;
        let d = tempfile::tempdir().unwrap();
        let home = d.path().join("home/user");
        std::fs::create_dir_all(home.join("bin")).unwrap();
        std::fs::create_dir_all(d.path().join("aliases")).unwrap();
        symlink(&home, d.path().join("aliases/home-alias")).unwrap();
        symlink(home.parent().unwrap(), d.path().join("aliases/parent-alias")).unwrap();
        let system = d.path().join("system");
        std::fs::create_dir(&system).unwrap();
        symlink(&system, d.path().join("aliases/system-alias")).unwrap();
        let path = std::env::join_paths([d.path().join("aliases/home-alias/bin"), d.path().join("aliases/parent-alias"), d.path().join("aliases/system-alias")]).unwrap();
        assert!(toolchain_roots_excluding(&path.to_string_lossy(), Some(&home), &[system.as_path()]).is_empty());
    }

    #[test]
    fn quoting_survives_hostile_commands() {
        assert_eq!(sh_quote("echo 'a'; rm -rf x"), r"'echo '\''a'\''; rm -rf x'");
    }

    #[test]
    fn forged_git_pointer_cannot_select_an_unrelated_mount() {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        let private = root.path().join("private-fixture");
        std::fs::create_dir(&workspace).unwrap();
        std::fs::create_dir(&private).unwrap();
        std::fs::write(private.join("fixture.txt"), "harmless").unwrap();
        std::fs::write(workspace.join(".git"), format!("gitdir: {}\n", private.join("worktrees/nonexistent").display())).unwrap();
        assert!(git_common_dir(&workspace).is_err(), "an unrelated directory must not become a sandbox mount");
        let fake_gitdir = private.join("worktrees/fake");
        std::fs::create_dir_all(&fake_gitdir).unwrap();
        std::fs::write(fake_gitdir.join("gitdir"), private.join("fixture.txt").to_string_lossy().as_bytes()).unwrap();
        std::fs::write(fake_gitdir.join("commondir"), "../..\n").unwrap();
        std::fs::write(workspace.join(".git"), format!("gitdir: {}\n", fake_gitdir.display())).unwrap();
        assert!(git_common_dir(&workspace).is_err(), "a backlink to another file must be refused");
    }

    #[test]
    fn legitimate_linked_worktree_resolves_its_git_directory() {
        let repo = crate::git::testutil::init_repo();
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("linked");
        let base = crate::git::head_commit(repo.path()).unwrap();
        crate::git::worktree_add(repo.path(), &workspace, "forge/sandbox-fixture", &base).unwrap();
        let common = git_common_dir(&workspace).unwrap().unwrap();
        assert_eq!(common, dunce::canonicalize(repo.path().join(".git")).unwrap());
        crate::git::worktree_remove(repo.path(), &workspace, true).unwrap();
    }

    #[test]
    fn ordinary_or_missing_git_directories_need_no_separate_mount() {
        let root = tempfile::tempdir().unwrap();
        assert!(git_common_dir(root.path()).unwrap().is_none());
        std::fs::create_dir(root.path().join(".git")).unwrap();
        assert!(git_common_dir(root.path()).unwrap().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn linked_git_metadata_cannot_be_read_through_a_symlink() {
        let root = tempfile::tempdir().unwrap();
        let private = root.path().join("private-fixture");
        let link = root.path().join("gitdir");
        std::fs::write(&private, "harmless fixture").unwrap();
        std::os::unix::fs::symlink(&private, &link).unwrap();
        assert!(read_git_metadata(&link, "worktree backlink").is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn sandbox_refuses_forged_git_metadata_before_spawning() {
        if availability(Backend::LinuxNamespaces).is_err() {
            assert!(std::env::var("FORGE_REQUIRE_SANDBOX_TESTS").as_deref() != Ok("1"), "sandbox required");
            return;
        }
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        let private = root.path().join("outside");
        std::fs::create_dir(&workspace).unwrap();
        std::fs::create_dir(&private).unwrap();
        std::fs::write(private.join("fixture.txt"), "harmless host fixture").unwrap();
        assert_eq!(std::fs::read_to_string(private.join("fixture.txt")).unwrap(), "harmless host fixture");
        std::fs::write(workspace.join(".git"), format!("gitdir: {}\n", private.join("worktrees/absent").display())).unwrap();
        let spec = RunSpec { command: "true", workspace: &workspace };
        let error = command(Backend::LinuxNamespaces, &spec).unwrap_err().to_string();
        assert!(error.contains("linked-worktree Git metadata"), "{error}");
    }
}
