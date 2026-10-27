# Execution backends

Where FORGE runs commands it was asked to run. Implemented in `apps/desktop/src-tauri/src/execution.rs`; chosen per project
(**Projects → project → "Checks run: …"** and **"Agents run: …"**); recorded on every verification run, experiment run,
Proof Pack, approval and agent session.

## What is covered today

| Runs through a backend | Not yet |
|---|---|
| Verification checks (`start_check`), experiment runs (correctness, warm-up, measured) | Hook dry-runs (Harness Studio) |
| The agent process (engine + Claude Code / Codex / ACP agent / authorized MCP servers) and every command it runs, approved or not, when the project's **agent backend** is `linux_namespaces` | Agents on Windows (no Windows backend yet: they run on the host, labelled so) |

### Agents in `linux_namespaces` (`execution::agent_command`)

The project's agent backend (migration 14, default `trusted_local`) decides where the engine starts; everything the agent runs is
its descendant and inherits the sandbox. What the agent sees:

| | Contained agent | Checks in the same backend |
|---|---|---|
| Worktree | read-write | read-write |
| Git common dir | read-only | read-only |
| Other host files | **none**: `/home`, `/root`, `/tmp`, `/run` are empty tmpfs; `/usr` and `/etc` basics read-only | same |
| Toolchain / runtime | node, the engine package and every `node_modules` it resolves from, the provider binaries; for an ACP agent or authorized MCP server: the folder of its program and of any script it is given, and any other *file* argument as that file only. **A directory argument is never exposed** (a filesystem-style MCP server pointed at a data folder will not see it). All read-only | toolchain only |
| Provider state | Claude Code's config dir (`CLAUDE_CONFIG_DIR`, else `~/.claude` + `~/.claude.json`) or `CODEX_HOME` (else `~/.codex`) bound **read-write in place**: the agent must use its own login. Nothing is copied. These folders also hold the provider's transcripts of the user's *other* projects, which the agent can therefore read | none |
| Network | **shared** (the agent must reach its provider) | none |
| Environment | allow-listed runtime, provider-state, proxy and TLS settings; loader injection, SSH agents, cloud tokens and other variables removed; plus `FORGE_SANDBOX=linux_namespaces` | cleared |
| FORGE's database, other worktrees, `~/.ssh`, cloud credentials | not visible (unless FORGE's home was configured under a bound path) | not visible |

Before a contained agent starts, every path above is checked: the root or a top-level folder, the home folder or one
above it, FORGE's own data folder (or anything inside or above it), a folder above the worktree, or a relative provider
state path stops the mission with the reason instead of being exposed (`agent_sandbox::what_a_contained_agent_may_see…`,
`…provider_state_folder_that_is_too_broad…`).

So an approved `cat ~/.ssh/id_rsa`, `rm -rf ~`, or a write to another project fails inside the sandbox, while the agent still
works in its worktree. It does **not** stop exfiltration over the network of what the agent can see (the worktree, its own
provider login), and the agent can still `git push` only if push credentials are reachable — they are not, unless they live in
an explicitly forwarded proxy setting or the provider's state. Tests (Linux, required in CI): `agent_sandbox::a_contained_agent_cannot_read_or_write_outside_its_worktree`
(scripted agent, with a host control), `…still_works_in_its_worktree…`, `an_unavailable_agent_backend…never_falls_back`, and
`claude_real::an_approved_command_from_real_claude_code_cannot_read_outside_the_worktree_when_contained` (the real Claude Code
binary, an approved `Bash cat` of a host file; host control prints it). Mutation-checked: building the agent command with
`trusted_local` regardless of the setting fails all three containment tests.

ACP agents that keep their login somewhere other than the paths above will not find it when contained; they fail visibly.

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
them; FORGE then reports the backend unavailable). Ubuntu 23.10+ (including 24.04) restricts them through AppArmor by default
(`kernel.apparmor_restrict_unprivileged_userns=1`): bubblewrap fails with "loopback: Failed RTM_NEWADDR". Enabling the backend
there needs an AppArmor profile that allows `bwrap` user namespaces, or that sysctl set to 0 — a host security decision FORGE
leaves to the user. CI sets the sysctl on its disposable runner so the isolation tests run.

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
