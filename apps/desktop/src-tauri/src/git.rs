//! Git integration via the installed Git CLI. No libgit2: worktree and merge behaviour
//! on NTFS matches exactly what the developer's own `git` does.

use crate::error::{ForgeError, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn git_binary() -> Result<PathBuf> {
    which::which("git").map_err(|_| ForgeError::Git("Git is not installed or not on PATH".into()))
}

fn base_command(dir: &Path) -> Result<Command> {
    let mut c = Command::new(git_binary()?);
    c.current_dir(dir)
        .args(["-c", "core.quotepath=false", "-c", "core.longpaths=true", "--no-optional-locks"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("LC_ALL", "C");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    Ok(c)
}

pub fn run_raw(dir: &Path, args: &[&str]) -> Result<Output> {
    let out = base_command(dir)?.args(args).output()?;
    Ok(out)
}

/// Run git and return stdout on success; stderr becomes the error otherwise.
pub fn run(dir: &Path, args: &[&str]) -> Result<String> {
    let out = run_raw(dir, args)?;
    if !out.status.success() {
        return Err(ForgeError::Git(format!(
            "git {} failed: {}",
            args.first().copied().unwrap_or(""),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn version() -> Result<String> {
    let out = Command::new(git_binary()?).arg("--version").output()?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub fn toplevel(dir: &Path) -> Result<PathBuf> {
    let s = run(dir, &["rev-parse", "--show-toplevel"])?;
    crate::paths::canonical_dir(Path::new(s.trim()))
}

pub fn head_commit(dir: &Path) -> Result<String> {
    Ok(run(dir, &["rev-parse", "HEAD"])?.trim().to_string())
}

pub fn has_commits(dir: &Path) -> bool {
    run(dir, &["rev-parse", "--verify", "HEAD"]).is_ok()
}

pub fn current_branch(dir: &Path) -> Result<String> {
    let b = run(dir, &["rev-parse", "--abbrev-ref", "HEAD"])?.trim().to_string();
    Ok(b)
}

pub fn default_branch(dir: &Path) -> String {
    if let Ok(s) = run(dir, &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"]) {
        if let Some(b) = s.trim().strip_prefix("origin/") {
            return b.to_string();
        }
    }
    current_branch(dir).unwrap_or_else(|_| "main".into())
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FileStatus {
    pub path: String,
    pub orig_path: Option<String>,
    /// Index (staged) status letter, '.' when unchanged.
    pub index: char,
    /// Working-tree status letter, '.' when unchanged.
    pub worktree: char,
    pub untracked: bool,
    pub conflicted: bool,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct RepoStatus {
    pub branch: Option<String>,
    pub head: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub files: Vec<FileStatus>,
    pub staged: u32,
    pub unstaged: u32,
    pub untracked: u32,
    pub conflicted: u32,
    pub dirty: bool,
}

/// Parse `git status --porcelain=v2 --branch -z` output.
pub fn parse_status(raw: &str) -> RepoStatus {
    let mut st = RepoStatus::default();
    let mut fields = raw.split('\0').peekable();
    while let Some(rec) = fields.next() {
        if rec.is_empty() {
            continue;
        }
        if let Some(h) = rec.strip_prefix("# ") {
            if let Some(v) = h.strip_prefix("branch.oid ") {
                st.head = if v == "(initial)" { None } else { Some(v.to_string()) };
            } else if let Some(v) = h.strip_prefix("branch.head ") {
                st.branch = if v == "(detached)" { None } else { Some(v.to_string()) };
            } else if let Some(v) = h.strip_prefix("branch.upstream ") {
                st.upstream = Some(v.to_string());
            } else if let Some(v) = h.strip_prefix("branch.ab ") {
                for tok in v.split_whitespace() {
                    if let Some(n) = tok.strip_prefix('+') {
                        st.ahead = n.parse().unwrap_or(0);
                    } else if let Some(n) = tok.strip_prefix('-') {
                        st.behind = n.parse().unwrap_or(0);
                    }
                }
            }
            continue;
        }
        let kind = rec.as_bytes()[0];
        match kind {
            b'1' => {
                let parts: Vec<&str> = rec.splitn(9, ' ').collect();
                if parts.len() == 9 {
                    st.files.push(file(parts[8], None, parts[1], false, false));
                }
            }
            b'2' => {
                let parts: Vec<&str> = rec.splitn(10, ' ').collect();
                if parts.len() == 10 {
                    let orig = fields.next().map(str::to_string);
                    st.files.push(file(parts[9], orig, parts[1], false, false));
                }
            }
            b'u' => {
                let parts: Vec<&str> = rec.splitn(11, ' ').collect();
                if parts.len() == 11 {
                    st.files.push(file(parts[10], None, parts[1], false, true));
                }
            }
            b'?' => st.files.push(FileStatus {
                path: rec[2..].to_string(),
                orig_path: None,
                index: '?',
                worktree: '?',
                untracked: true,
                conflicted: false,
            }),
            _ => {}
        }
    }
    for f in &st.files {
        if f.conflicted {
            st.conflicted += 1;
        } else if f.untracked {
            st.untracked += 1;
        } else {
            if f.index != '.' {
                st.staged += 1;
            }
            if f.worktree != '.' {
                st.unstaged += 1;
            }
        }
    }
    st.dirty = !st.files.is_empty();
    st
}

fn file(path: &str, orig: Option<String>, xy: &str, untracked: bool, conflicted: bool) -> FileStatus {
    let mut ch = xy.chars();
    FileStatus {
        path: path.to_string(),
        orig_path: orig,
        index: ch.next().unwrap_or('.'),
        worktree: ch.next().unwrap_or('.'),
        untracked,
        conflicted,
    }
}

pub fn status(dir: &Path) -> Result<RepoStatus> {
    let raw = run(dir, &["status", "--porcelain=v2", "--branch", "-z", "--untracked-files=all"])?;
    Ok(parse_status(&raw))
}

/// Slug safe for branch names and Windows directory names.
pub fn slugify(s: &str, max: usize) -> String {
    let mut out = String::new();
    let mut last_dash = true;
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
        if out.len() >= max {
            break;
        }
    }
    let t = out.trim_matches('-').to_string();
    if t.is_empty() { "mission".into() } else { t }
}

/// Create an isolated worktree on a fresh branch from `base` (a commit or ref).
pub fn worktree_add(repo: &Path, path: &Path, branch: &str, base: &str) -> Result<()> {
    if path.exists() {
        return Err(ForgeError::Git(format!("worktree path already exists: {}", path.display())));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let p = path.to_string_lossy().into_owned();
    run(repo, &["worktree", "add", "-b", branch, &p, base])?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WorktreeSafety {
    pub exists: bool,
    pub dirty_files: u32,
    pub unmerged_commits: u32,
}

/// What would be lost by removing this worktree and branch?
pub fn worktree_safety(worktree: &Path, base_commit: &str) -> Result<WorktreeSafety> {
    if !worktree.exists() {
        return Ok(WorktreeSafety { exists: false, dirty_files: 0, unmerged_commits: 0 });
    }
    let st = status(worktree)?;
    let range = format!("{base_commit}..HEAD");
    let n: u32 = run(worktree, &["rev-list", "--count", &range])?.trim().parse().unwrap_or(0);
    Ok(WorktreeSafety { exists: true, dirty_files: st.files.len() as u32, unmerged_commits: n })
}

/// Remove a worktree. Refuses when there is uncommitted work unless `force`.
pub fn worktree_remove(repo: &Path, path: &Path, force: bool) -> Result<()> {
    if !path.exists() {
        let _ = run(repo, &["worktree", "prune"]);
        return Ok(());
    }
    if !force {
        let st = status(path)?;
        if st.dirty {
            return Err(ForgeError::Git(format!(
                "worktree has {} uncommitted change(s); refusing to remove without explicit confirmation",
                st.files.len()
            )));
        }
    }
    let p = path.to_string_lossy().into_owned();
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.push(&p);
    run(repo, &args)?;
    Ok(())
}

pub fn delete_branch(repo: &Path, branch: &str, force: bool) -> Result<()> {
    run(repo, &["branch", if force { "-D" } else { "-d" }, branch])?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ChangedFile {
    pub path: String,
    pub added: Option<u32>,
    pub removed: Option<u32>,
    pub binary: bool,
}

/// Mark untracked files intent-to-add so they appear in diffs. Only ever called on a
/// mission worktree, which FORGE owns.
fn intent_to_add(worktree: &Path) -> Result<()> {
    run(worktree, &["add", "-A", "-N", "--", "."])?;
    Ok(())
}

pub fn changed_files(worktree: &Path, base: &str) -> Result<Vec<ChangedFile>> {
    intent_to_add(worktree)?;
    let raw = run(worktree, &["diff", "--numstat", "-z", base])?;
    let mut out = vec![];
    let mut it = raw.split('\0').peekable();
    while let Some(rec) = it.next() {
        if rec.is_empty() {
            continue;
        }
        let mut p = rec.splitn(3, '\t');
        let (a, r, path) = (p.next().unwrap_or(""), p.next().unwrap_or(""), p.next().unwrap_or(""));
        // Rename records: "a\tr\t" followed by two NUL-separated paths.
        let path = if path.is_empty() {
            let _old = it.next();
            it.next().unwrap_or("").to_string()
        } else {
            path.to_string()
        };
        out.push(ChangedFile {
            path,
            added: a.parse().ok(),
            removed: r.parse().ok(),
            binary: a == "-",
        });
    }
    Ok(out)
}

/// Unified diff of everything the mission changed relative to its base commit.
pub fn diff(worktree: &Path, base: &str) -> Result<String> {
    intent_to_add(worktree)?;
    run(worktree, &["diff", "--no-color", "--find-renames", "-U4", base])
}

#[cfg(test)]
pub(crate) mod testutil {
    use super::*;

    pub fn init_repo() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        let p = d.path();
        run(p, &["init", "-b", "main"]).unwrap();
        run(p, &["config", "user.email", "t@example.com"]).unwrap();
        run(p, &["config", "user.name", "Test"]).unwrap();
        run(p, &["config", "commit.gpgsign", "false"]).unwrap();
        std::fs::write(p.join("README.md"), "# t\n").unwrap();
        std::fs::create_dir_all(p.join("src")).unwrap();
        std::fs::write(p.join("src/lib.txt"), "one\ntwo\n").unwrap();
        run(p, &["add", "-A"]).unwrap();
        run(p, &["commit", "-m", "init"]).unwrap();
        d
    }
}

#[cfg(test)]
mod tests {
    use super::testutil::init_repo;
    use super::*;

    #[test]
    fn parses_porcelain_v2_including_renames_and_untracked() {
        let raw = "# branch.oid abc123\0# branch.head main\0# branch.upstream origin/main\0# branch.ab +2 -1\0\
1 .M N... 100644 100644 100644 aaa bbb src/a.rs\0\
2 R. N... 100644 100644 100644 aaa bbb R100 new name.rs\0old name.rs\0\
u UU N... 100644 100644 100644 100644 a b c conflicted.rs\0\
? new file.txt\0";
        let st = parse_status(raw);
        assert_eq!(st.branch.as_deref(), Some("main"));
        assert_eq!((st.ahead, st.behind), (2, 1));
        assert_eq!(st.files.len(), 4);
        assert_eq!(st.files[1].orig_path.as_deref(), Some("old name.rs"));
        assert_eq!(st.files[1].path, "new name.rs");
        assert_eq!((st.staged, st.unstaged, st.untracked, st.conflicted), (1, 1, 1, 1));
        assert!(st.dirty);
    }

    #[test]
    fn clean_repo_status_is_not_dirty() {
        let d = init_repo();
        let st = status(d.path()).unwrap();
        assert!(!st.dirty);
        assert_eq!(st.branch.as_deref(), Some("main"));
        assert!(st.head.is_some());
    }

    #[test]
    fn slugify_is_branch_and_path_safe() {
        assert_eq!(slugify("Implement user auth: reset/password!", 24), "implement-user-auth-rese");
        assert_eq!(slugify("***", 10), "mission");
        assert_eq!(slugify("ünïcode título", 30), "n-code-t-tulo");
    }

    #[test]
    fn worktree_lifecycle_protects_uncommitted_work() {
        let repo = init_repo();
        let wt_parent = tempfile::tempdir().unwrap();
        let wt = wt_parent.path().join("m1");
        let base = head_commit(repo.path()).unwrap();
        worktree_add(repo.path(), &wt, "forge/m1", &base).unwrap();
        assert!(wt.join("README.md").exists());

        // Main checkout is untouched by work in the worktree.
        std::fs::write(wt.join("new.txt"), "hello\n").unwrap();
        std::fs::write(wt.join("src/lib.txt"), "one\nTWO\nthree\n").unwrap();
        assert!(!status(repo.path()).unwrap().dirty);

        let files = changed_files(&wt, &base).unwrap();
        let names: Vec<_> = files.iter().map(|f| f.path.as_str()).collect();
        assert!(names.contains(&"new.txt") && names.contains(&"src/lib.txt"), "{names:?}");
        let d = diff(&wt, &base).unwrap();
        assert!(d.contains("+hello") && d.contains("+three"), "{d}");

        let safety = worktree_safety(&wt, &base).unwrap();
        assert_eq!(safety.dirty_files, 2);

        // Dirty worktree must not be removed without force.
        let err = worktree_remove(repo.path(), &wt, false).unwrap_err().to_string();
        assert!(err.contains("uncommitted"), "{err}");
        assert!(wt.exists());

        worktree_remove(repo.path(), &wt, true).unwrap();
        assert!(!wt.exists());
        delete_branch(repo.path(), "forge/m1", true).unwrap();
    }

    #[test]
    fn clean_worktree_removes_without_force_and_duplicate_path_rejected() {
        let repo = init_repo();
        let parent = tempfile::tempdir().unwrap();
        let wt = parent.path().join("w");
        let base = head_commit(repo.path()).unwrap();
        worktree_add(repo.path(), &wt, "forge/w", &base).unwrap();
        assert!(worktree_add(repo.path(), &wt, "forge/w2", &base).is_err());
        worktree_remove(repo.path(), &wt, false).unwrap();
        assert!(!wt.exists());
    }

    #[test]
    fn unicode_paths_roundtrip() {
        let repo = init_repo();
        let parent = tempfile::tempdir().unwrap();
        let wt = parent.path().join("wörk trée");
        let base = head_commit(repo.path()).unwrap();
        worktree_add(repo.path(), &wt, "forge/uni", &base).unwrap();
        std::fs::write(wt.join("fïle ünï.txt"), "x\n").unwrap();
        let files = changed_files(&wt, &base).unwrap();
        assert_eq!(files[0].path, "fïle ünï.txt");
        worktree_remove(repo.path(), &wt, true).unwrap();
    }
}
