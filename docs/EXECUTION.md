# Execution backends

Where FORGE runs commands it was asked to run. Implemented in `apps/desktop/src-tauri/src/execution.rs`; chosen per project
(**Projects → project → "Checks run: …"**); recorded on every verification run, experiment run and Proof Pack.

## What is covered today

| Runs through a backend | Not yet |
|---|---|
| Verification checks (`start_check`), experiment runs (correctness, warm-up, measured) | The coding agents themselves (Claude Code, Codex) and the shell commands they run after approval; hook dry-runs (Harness Studio) |

The agents still run on the host: their tool calls are gated by FORGE policy and human approval, but an *approved* command runs
with the user's privileges (SECURITY.md limit 1). Moving the agent into a backend needs provider credentials inside the sandbox,
which conflicts with "never copy provider tokens"; it needs a design decision (see "Open questions").

## Backends

### `trusted_local` — implemented, **not isolated**
The user's shell (`cmd.exe` / `sh`), privileges, home directory, credentials and network. Contained only for clean-up (Job
Object / process group) and bounded by a timeout. Default for every project, so existing projects behave exactly as before
migration 8. The UI labels it "Not isolated" wherever a run shows where it ran.

### `linux_namespaces` — implemented (Linux), tested
bubblewrap (`bwrap`, invoked as an external program) with:

* `--unshare-all` (user, PID, IPC, UTS, network, cgroup namespaces), `--cap-drop ALL`, `--new-session`, `--die-with-parent`;
* an empty root: `/usr` and `/bin`, `/lib*` (or their merged-usr symlinks) read-only, a fixed list of `/etc` files (users, DNS
  config, TLS roots, dynamic linker), toolchain directories from `PATH` that are **outside** system directories and the home
  directory (e.g. `/opt/node22`) read-only;
* the workspace (mission or experiment worktree) read-write at its own path; the repository's git directory read-only;
* private `/tmp` and `/home/forge`; `--clearenv` then a fixed environment (`PATH`, `HOME`, `TMPDIR`, `LANG`, `CI`, ...);
* no network at all (a private loopback only);
* `ulimit -c 0` (no core dumps) and a 4 GiB per-file cap; the existing timeout and output caps.

**Enforced and tested** (`execution_tests.rs`, `supervisor::tests::phase2::checks_run_on_the_projects_execution_backend…`; every
test has a trusted-local control proving it would notice a leak, and mutation checks showed they fail when `--unshare-all` or
`--clearenv` is removed):

| Attack | Result |
|---|---|
| Read `~/.ssh/id_*` / another directory outside the workspace | not present in the sandbox |
| Write outside the workspace | impossible; workspace writes work |
| Read FORGE's environment (tokens, `SSH_AUTH_SOCK`) | cleared |
| Connect to a service on the host's loopback; resolve DNS | refused / no DNS |
| Leave a background process running | killed with the sandbox's PID namespace |
| See host processes | only the sandbox's own |
| Commit or move branches in the repository | git directory is read-only |
| Timeout | the whole sandbox ends |

**Not enforced:** memory/CPU limits (needs cgroups); toolchains installed under the home directory (rustup, nvm) are invisible
by design, so such projects need a system-wide toolchain; anything readable under `/usr` or the listed `/etc` files is readable;
the kernel is shared (a kernel exploit escapes, unlike a VM); requires unprivileged user namespaces (some distributions restrict
them; FORGE then reports the backend unavailable).

### `windows_sandbox` — designed, **not implemented**
Windows Sandbox (Windows 10/11 Pro, Enterprise, Education; not Home) is a disposable lightweight VM configured by a `.wsb` file.
Design: `<Networking>Disable</Networking>`, `<VGpu>Disable</VGpu>`, `<ClipboardRedirection>Disable</ClipboardRedirection>`,
`<PrinterRedirection>Disable</PrinterRedirection>`, `<AudioInput>Disable</AudioInput>`, `<VideoInput>Disable</VideoInput>`, one
read-only `<MappedFolder>` holding a *copy* of the workspace and one writable exchange folder for results (never the user
profile), a `<LogonCommand>` that copies in, runs the command, writes results and shuts down. Constraint: **only one sandbox can
run at a time**, so concurrent missions would queue. Toolchains must be installed inside each run (slow) or mapped read-only.

### `isolated_wsl` — designed, **not implemented**
A FORGE-owned WSL 2 distribution imported from a known image (`wsl --import`), with `/etc/wsl.conf`:
`[automount] enabled=false`, `[interop] enabled=false appendWindowsPath=false`, `[user] default=forge`, so no Windows drive and no
Windows executables are reachable; the workspace copied in (or one explicit mount) and results copied out; the
`linux_namespaces` backend running **inside** the distribution per command (this is why that backend exists). Requires explicit
user consent to install WSL/the distribution; networking control relies on the inner network namespace because WSL's own
networking is shared with the host. Not a VM per command: one utility VM is shared by all WSL distributions.

## Selection rules

* A backend that is not implemented or not available cannot be selected (`project_set_execution_backend` refuses with the reason).
* If a selected backend later becomes unavailable, every check/experiment fails with the reason. **There is no fall-back.**
* Every change of backend is audited (`project.execution_backend`).

## Open questions (for the owner)

1. Agents inside a backend: provider CLIs need their login. Options: keep agents on the host (today); run only the agent's
   *tool execution* inside the backend (requires provider support for a remote/sandboxed shell; Codex has its own sandbox,
   Claude Code has a `sandbox` setting); or mount provider credentials read-only into the sandbox (violates the current rule).
2. Network allow-lists for checks that must download dependencies (e.g. a package registry only). Today: none or everything.
