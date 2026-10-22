//! Safe merge of a reviewed mission branch into the user's main checkout.
//!
//! Rules: the main checkout must be clean and on the branch the mission was based on; conflicts are detected
//! *before* anything is touched (`git merge-tree`); a failed merge is aborted so the checkout is left as it was;
//! FORGE never pushes.

use crate::error::{ForgeError, Result};
use crate::git;
use std::path::Path;

/// `-c user.name/email` fallbacks, only when the user has no identity configured.
fn identity_args(repo: &Path) -> Vec<String> {
    let has = |k: &str| git::run(repo, &["config", "--get", k]).map(|s| !s.trim().is_empty()).unwrap_or(false);
    if has("user.name") && has("user.email") {
        vec![]
    } else {
        vec!["-c".into(), "user.name=FORGE".into(), "-c".into(), "user.email=forge@localhost".into()]
    }
}

fn run_with(repo: &Path, prefix: &[String], args: &[&str]) -> Result<String> {
    let mut all: Vec<&str> = prefix.iter().map(String::as_str).collect();
    all.extend_from_slice(args);
    git::run(repo, &all)
}

/// Commit everything in the mission worktree. Returns the new commit, or None if there was nothing to commit.
/// Repository hooks are bypassed (`--no-verify`): FORGE's own checks are the verification step, and a hook must not
/// be able to silently alter or block a reviewed change.
pub fn commit_all(repo: &Path, worktree: &Path, message: &str) -> Result<Option<String>> {
    git::run(worktree, &["add", "-A", "--", "."])?;
    if git::run(worktree, &["status", "--porcelain"])?.trim().is_empty() {
        return Ok(None);
    }
    run_with(worktree, &identity_args(repo), &["commit", "--no-verify", "-q", "-m", message])?;
    Ok(Some(git::head_commit(worktree)?))
}

/// Paths that would conflict if `branch` were merged into `target`. Nothing in the working tree changes.
pub fn conflicts(repo: &Path, target: &str, branch: &str) -> Result<Vec<String>> {
    let out = git::run_raw(repo, &["merge-tree", "--write-tree", "--name-only", "--no-messages", target, branch])?;
    let text = String::from_utf8_lossy(&out.stdout);
    match out.status.code() {
        Some(0) => Ok(vec![]),
        Some(1) => Ok(text.lines().skip(1).filter(|l| !l.trim().is_empty()).map(str::to_string).collect()),
        _ => Err(ForgeError::Git(format!("could not check for conflicts: {}", String::from_utf8_lossy(&out.stderr).trim()))),
    }
}

/// The main checkout must be a safe place to merge into.
pub fn ensure_ready(repo: &Path, expected_branch: &str) -> Result<()> {
    let gitdir = git::run(repo, &["rev-parse", "--absolute-git-dir"])?;
    let gitdir = Path::new(gitdir.trim());
    for marker in ["MERGE_HEAD", "CHERRY_PICK_HEAD", "REVERT_HEAD", "rebase-merge", "rebase-apply"] {
        if gitdir.join(marker).exists() {
            return Err(ForgeError::Git(format!("the main checkout is in the middle of another operation ({marker}); finish or abort it first")));
        }
    }
    let cur = git::current_branch(repo)?;
    if cur != expected_branch {
        return Err(ForgeError::Git(format!(
            "the main checkout is on \"{cur}\" but this mission was based on \"{expected_branch}\"; switch it back before merging"
        )));
    }
    let st = git::status(repo)?;
    if st.dirty {
        return Err(ForgeError::Git(format!(
            "the main checkout has {} uncommitted change(s); commit or stash them first so nothing of yours is mixed into the merge",
            st.files.len()
        )));
    }
    Ok(())
}

/// Merge `branch` into the currently checked-out branch of `repo`. On any failure the checkout is restored.
pub fn merge(repo: &Path, branch: &str, message: &str, squash: bool) -> Result<String> {
    let id = identity_args(repo);
    let before = git::head_commit(repo)?;
    let attempt = if squash {
        run_with(repo, &id, &["merge", "--squash", "--no-verify", branch])
            .and_then(|_| run_with(repo, &id, &["commit", "--no-verify", "-q", "-m", message]))
    } else {
        run_with(repo, &id, &["merge", "--no-ff", "--no-verify", "-q", "-m", message, branch])
    };
    if let Err(e) = attempt {
        // Leave the checkout exactly as we found it.
        let _ = git::run(repo, &["merge", "--abort"]);
        let _ = git::run(repo, &["reset", "--merge"]);
        let _ = git::run(repo, &["reset", "--hard", &before]);
        return Err(e);
    }
    git::head_commit(repo)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::testutil::init_repo;

    fn mission_branch(repo: &Path, name: &str, file: &str, content: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let parent = tempfile::tempdir().unwrap();
        let wt = parent.path().join("wt");
        let base = git::head_commit(repo).unwrap();
        git::worktree_add(repo, &wt, name, &base).unwrap();
        std::fs::write(wt.join(file), content).unwrap();
        (parent, wt)
    }

    #[test]
    fn commit_all_commits_changes_and_reports_nothing_to_do() {
        let repo = init_repo();
        let (_p, wt) = mission_branch(repo.path(), "forge/a", "new.txt", "hello\n");
        let c = commit_all(repo.path(), &wt, "mission: add new.txt").unwrap();
        assert!(c.is_some());
        assert_eq!(commit_all(repo.path(), &wt, "again").unwrap(), None, "second call has nothing to commit");
        assert_eq!(git::run(&wt, &["log", "-1", "--format=%s"]).unwrap().trim(), "mission: add new.txt");
    }

    #[test]
    fn clean_merge_creates_a_merge_commit_and_brings_the_files() {
        let repo = init_repo();
        let (_p, wt) = mission_branch(repo.path(), "forge/m", "feature.txt", "f\n");
        commit_all(repo.path(), &wt, "work").unwrap();
        assert!(conflicts(repo.path(), "main", "forge/m").unwrap().is_empty());
        ensure_ready(repo.path(), "main").unwrap();
        let sha = merge(repo.path(), "forge/m", "Merge mission", false).unwrap();
        assert!(repo.path().join("feature.txt").exists());
        let parents = git::run(repo.path(), &["rev-list", "--parents", "-n", "1", &sha]).unwrap();
        assert_eq!(parents.split_whitespace().count(), 3, "commit + two parents = a real merge commit");
    }

    #[test]
    fn squash_merge_makes_a_single_parent_commit() {
        let repo = init_repo();
        let (_p, wt) = mission_branch(repo.path(), "forge/s", "s.txt", "s\n");
        commit_all(repo.path(), &wt, "work 1").unwrap();
        std::fs::write(wt.join("s2.txt"), "s2\n").unwrap();
        commit_all(repo.path(), &wt, "work 2").unwrap();
        let sha = merge(repo.path(), "forge/s", "Squashed mission", true).unwrap();
        let parents = git::run(repo.path(), &["rev-list", "--parents", "-n", "1", &sha]).unwrap();
        assert_eq!(parents.split_whitespace().count(), 2);
        assert!(repo.path().join("s.txt").exists() && repo.path().join("s2.txt").exists());
    }

    #[test]
    fn conflicts_are_found_before_anything_is_touched() {
        let repo = init_repo();
        let (_p, wt) = mission_branch(repo.path(), "forge/c", "src/lib.txt", "mission line\ntwo\n");
        commit_all(repo.path(), &wt, "work").unwrap();
        std::fs::write(repo.path().join("src/lib.txt"), "main line\ntwo\n").unwrap();
        git::run(repo.path(), &["add", "-A"]).unwrap();
        git::run(repo.path(), &["commit", "-q", "-m", "main moved"]).unwrap();
        let head = git::head_commit(repo.path()).unwrap();

        let c = conflicts(repo.path(), "main", "forge/c").unwrap();
        assert_eq!(c, vec!["src/lib.txt"]);
        assert_eq!(git::head_commit(repo.path()).unwrap(), head);
        assert!(!git::status(repo.path()).unwrap().dirty, "conflict detection must not touch the working tree");
    }

    #[test]
    fn a_failed_merge_restores_the_checkout() {
        let repo = init_repo();
        let (_p, wt) = mission_branch(repo.path(), "forge/f", "src/lib.txt", "mission line\ntwo\n");
        commit_all(repo.path(), &wt, "work").unwrap();
        std::fs::write(repo.path().join("src/lib.txt"), "main line\ntwo\n").unwrap();
        git::run(repo.path(), &["add", "-A"]).unwrap();
        git::run(repo.path(), &["commit", "-q", "-m", "main moved"]).unwrap();
        let head = git::head_commit(repo.path()).unwrap();
        assert!(merge(repo.path(), "forge/f", "x", false).is_err());
        assert_eq!(git::head_commit(repo.path()).unwrap(), head);
        assert!(!git::status(repo.path()).unwrap().dirty, "no half-merged state may remain");
        assert_eq!(std::fs::read_to_string(repo.path().join("src/lib.txt")).unwrap().replace("\r\n", "\n"), "main line\ntwo\n");
        ensure_ready(repo.path(), "main").unwrap();
    }

    #[test]
    fn dirty_or_wrong_branch_main_checkouts_are_refused() {
        let repo = init_repo();
        std::fs::write(repo.path().join("scratch.txt"), "mine\n").unwrap();
        let e = ensure_ready(repo.path(), "main").unwrap_err().to_string();
        assert!(e.contains("uncommitted") && e.contains("1"), "{e}");
        std::fs::remove_file(repo.path().join("scratch.txt")).unwrap();
        ensure_ready(repo.path(), "main").unwrap();
        git::run(repo.path(), &["switch", "-c", "other"]).unwrap();
        assert!(ensure_ready(repo.path(), "main").unwrap_err().to_string().contains("switch it back"));
    }

    #[test]
    fn an_in_progress_operation_blocks_merging() {
        let repo = init_repo();
        std::fs::write(repo.path().join(".git").join("MERGE_HEAD"), "0000000000000000000000000000000000000000\n").unwrap();
        assert!(ensure_ready(repo.path(), "main").unwrap_err().to_string().contains("MERGE_HEAD"));
    }
}
