//! Conflict detection between concurrent missions (Phase 5). Read-only: nothing here changes a worktree, an index, a branch or
//! the user's checkout. Three signals, from cheapest to most specific:
//!
//!   * **file overlap** — two missions change the same path (relative to their own base). Overlap is not a conflict, but it is
//!     where conflicts and semantic clashes come from;
//!   * **predicted textual conflicts** — `git merge-tree --write-tree` on the two missions' current states (their worktree
//!     captured through a temporary index exactly like a review fingerprint, wrapped in an unreferenced commit on the mission's
//!     base), which tells whether landing both would conflict and on which paths;
//!   * **base drift** — the branch a mission was based on has moved since the mission started: how many commits, which of those
//!     commits' files the mission also changed, and whether merging the mission into the current base would conflict.
//!
//! Predictions describe the state at the moment they were computed; a running agent can change it a second later.

use crate::error::Result;
use crate::git;
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::Path;

/// One mission's current changes.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub mission_id: String,
    pub title: String,
    pub base_ref: String,
    pub base_commit: String,
    /// Tree of the worktree's current state (tracked and untracked, honouring .gitignore).
    pub tree: String,
    pub files: BTreeSet<String>,
}

pub fn snapshot(mission_id: &str, title: &str, base_ref: &str, base_commit: &str, worktree: &Path) -> Result<Snapshot> {
    let (_, tree) = git::fingerprint_and_tree(worktree, base_commit)?;
    let files = git::changed_files(worktree, base_commit)?.into_iter().map(|f| f.path).collect();
    Ok(Snapshot { mission_id: mission_id.into(), title: title.into(), base_ref: base_ref.into(), base_commit: base_commit.into(), tree, files })
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Prediction {
    /// clean | conflicts | unknown
    pub outcome: String,
    pub conflicted_files: Vec<String>,
    /// Why the prediction could not be made (outcome `unknown`).
    pub error: Option<String>,
}

/// Wrap a mission's tree in a commit whose parent is its base, so git can merge it (`merge-tree` only accepts trees on newer
/// git versions). This writes one unreferenced commit object; no ref, index or working file changes, and `git gc` discards it.
fn as_commit(repo: &Path, tree: &str, parent: &str) -> Result<String> {
    let out = git::run(repo, &["-c", "user.name=FORGE", "-c", "user.email=forge@localhost", "commit-tree", tree, "-p", parent, "-m", "FORGE conflict prediction (temporary, unreferenced)"])?;
    Ok(out.trim().to_string())
}

/// Merge two commits in memory (`git merge-tree --write-tree`) without touching anything but the object database. Exit code 1
/// means "conflicts" only when git also produced a result tree; otherwise it is an error and the outcome is `unknown`.
pub fn predict(repo: &Path, ours: &str, theirs: &str) -> Prediction {
    let unknown = |e: String| Prediction { outcome: "unknown".into(), conflicted_files: vec![], error: Some(e) };
    match git::run_raw(repo, &["merge-tree", "--write-tree", "--name-only", "--no-messages", ours, theirs]) {
        Ok(out) => {
            let text = String::from_utf8_lossy(&out.stdout);
            let mut lines = text.lines();
            let has_tree = lines.next().is_some_and(|l| l.trim().len() >= 40 && l.trim().chars().all(|c| c.is_ascii_hexdigit()));
            match (out.status.code(), has_tree) {
                (Some(0), true) => Prediction { outcome: "clean".into(), conflicted_files: vec![], error: None },
                (Some(1), true) => {
                    let mut files: Vec<String> = lines.map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect();
                    files.dedup();
                    Prediction { outcome: "conflicts".into(), conflicted_files: files, error: None }
                }
                _ => unknown(String::from_utf8_lossy(&out.stderr).trim().to_string()),
            }
        }
        Err(e) => unknown(e.to_string()),
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PairReport {
    pub a: String,
    pub b: String,
    /// Paths both missions changed.
    pub overlapping_files: Vec<String>,
    /// Landing both (over the merge base of their bases) would conflict on these paths.
    pub prediction: Prediction,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DriftReport {
    pub mission_id: String,
    pub base_ref: String,
    pub base_commit: String,
    /// Where `base_ref` points now (None: the branch no longer exists).
    pub base_head: Option<String>,
    pub commits_behind: u32,
    /// Files changed on the base since the mission started that the mission changed too.
    pub overlapping_files: Vec<String>,
    /// Merging the mission into the current base.
    pub prediction: Option<Prediction>,
}

fn resolve(repo: &Path, rev: &str) -> Option<String> {
    git::run(repo, &["rev-parse", "--verify", "--quiet", &format!("{rev}^{{commit}}")]).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

pub fn pair(repo: &Path, a: &Snapshot, b: &Snapshot) -> PairReport {
    let overlapping_files: Vec<String> = a.files.intersection(&b.files).cloned().collect();
    // Nothing in common and no drift between the bases: the merge is trivially clean, skip the git call.
    let prediction = if a.base_commit == b.base_commit && overlapping_files.is_empty() {
        Prediction { outcome: "clean".into(), conflicted_files: vec![], error: None }
    } else {
        // Each side as a commit on its own base: git then merges over the merge base of the two bases.
        match (as_commit(repo, &a.tree, &a.base_commit), as_commit(repo, &b.tree, &b.base_commit)) {
            (Ok(ca), Ok(cb)) => predict(repo, &ca, &cb),
            (Err(e), _) | (_, Err(e)) => Prediction { outcome: "unknown".into(), conflicted_files: vec![], error: Some(e.to_string()) },
        }
    };
    PairReport { a: a.mission_id.clone(), b: b.mission_id.clone(), overlapping_files, prediction }
}

pub fn drift(repo: &Path, s: &Snapshot) -> DriftReport {
    let mut r = DriftReport { mission_id: s.mission_id.clone(), base_ref: s.base_ref.clone(), base_commit: s.base_commit.clone(), base_head: None, commits_behind: 0, overlapping_files: vec![], prediction: None };
    let Some(head) = resolve(repo, &format!("refs/heads/{}", s.base_ref)) else { return r };
    r.base_head = Some(head.clone());
    if head == s.base_commit {
        return r;
    }
    r.commits_behind = git::run(repo, &["rev-list", "--count", &format!("{}..{head}", s.base_commit)]).ok().and_then(|n| n.trim().parse().ok()).unwrap_or(0);
    let moved: BTreeSet<String> = git::run(repo, &["diff", "--name-only", "-z", "--no-renames", "--no-ext-diff", "--no-textconv", &s.base_commit, &head])
        .map(|o| o.split('\0').filter(|p| !p.is_empty()).map(str::to_string).collect())
        .unwrap_or_default();
    r.overlapping_files = moved.intersection(&s.files).cloned().collect();
    r.prediction = Some(match as_commit(repo, &s.tree, &s.base_commit) {
        Ok(c) => predict(repo, &head, &c),
        Err(e) => Prediction { outcome: "unknown".into(), conflicted_files: vec![], error: Some(e.to_string()) },
    });
    r
}

/// Ids are as given; pairs are in input order (a before b).
pub fn analyse(repo: &Path, snapshots: &[Snapshot]) -> (Vec<PairReport>, Vec<DriftReport>) {
    let mut pairs = vec![];
    for (i, a) in snapshots.iter().enumerate() {
        for b in &snapshots[i + 1..] {
            pairs.push(pair(repo, a, b));
        }
    }
    (pairs, snapshots.iter().map(|s| drift(repo, s)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::testutil::init_repo;
    use std::path::PathBuf;

    fn wt(repo: &Path, name: &str, base: &str) -> PathBuf {
        let p = repo.parent().unwrap().join(format!("{}-{name}", repo.file_name().unwrap().to_string_lossy()));
        git::worktree_add(repo, &p, &format!("m/{name}"), base).unwrap();
        p
    }

    #[test]
    fn overlap_and_textual_conflicts_between_two_missions_are_predicted_without_touching_anything() {
        let d = init_repo();
        let repo = d.path();
        std::fs::write(repo.join("src/lib.txt"), "one\ntwo\nthree\nfour\nfive\nsix\nseven\n").unwrap();
        git::run(repo, &["commit", "-qam", "longer"]).unwrap();
        let base = git::head_commit(repo).unwrap();
        let (a, b, c) = (wt(repo, "a", &base), wt(repo, "b", &base), wt(repo, "c", &base));
        // a and b edit the same line: a textual conflict. c edits a far-away line of the same file: overlap, but clean.
        std::fs::write(a.join("src/lib.txt"), "ONE-a\ntwo\nthree\nfour\nfive\nsix\nseven\n").unwrap();
        std::fs::write(b.join("src/lib.txt"), "ONE-b\ntwo\nthree\nfour\nfive\nsix\nseven\n").unwrap();
        std::fs::write(c.join("src/lib.txt"), "one\ntwo\nthree\nfour\nfive\nsix\nSEVEN-c\n").unwrap();
        // Untracked new files count as changes too; a new file only a has is no overlap.
        std::fs::write(a.join("new.txt"), "only a\n").unwrap();
        let before: Vec<String> = [&a, &b, &c].iter().map(|w| git::run(w, &["status", "--porcelain"]).unwrap()).collect();
        let refs_before = git::run(repo, &["for-each-ref"]).unwrap();
        let snaps = vec![snapshot("a", "A", "main", &base, &a).unwrap(), snapshot("b", "B", "main", &base, &b).unwrap(), snapshot("c", "C", "main", &base, &c).unwrap()];
        assert!(snaps[0].files.contains("new.txt"));
        let (pairs, drift) = analyse(repo, &snaps);
        let ab = &pairs[0];
        assert_eq!((ab.a.as_str(), ab.b.as_str()), ("a", "b"));
        assert_eq!(ab.overlapping_files, vec!["src/lib.txt"]);
        assert_eq!((ab.prediction.outcome.as_str(), ab.prediction.conflicted_files.clone()), ("conflicts", vec!["src/lib.txt".to_string()]));
        let ac = &pairs[1];
        assert_eq!(ac.overlapping_files, vec!["src/lib.txt"]);
        assert_eq!(ac.prediction.outcome, "clean", "{ac:?}");
        assert!(drift.iter().all(|d| d.commits_behind == 0 && d.prediction.is_none()));
        // Read-only: worktree status, the real index of each worktree, and the user's checkout are unchanged.
        let after: Vec<String> = [&a, &b, &c].iter().map(|w| git::run(w, &["status", "--porcelain"]).unwrap()).collect();
        assert_eq!(before, after);
        assert_eq!(git::run(repo, &["for-each-ref"]).unwrap(), refs_before, "no ref is created or moved");
        assert!(!git::status(repo).unwrap().dirty);
        assert_eq!(git::head_commit(repo).unwrap(), base);
    }

    #[test]
    fn base_drift_reports_commits_overlapping_files_and_a_conflict_with_the_new_base() {
        let d = init_repo();
        let repo = d.path();
        let base = git::head_commit(repo).unwrap();
        let m = wt(repo, "m", &base);
        std::fs::write(m.join("src/lib.txt"), "one-mission\ntwo\n").unwrap();
        std::fs::write(m.join("README.md"), "# t\nmission notes\n").unwrap();
        // main moves on: two commits, one touching the same line of src/lib.txt, one unrelated.
        std::fs::write(repo.join("src/lib.txt"), "one-main\ntwo\n").unwrap();
        git::run(repo, &["commit", "-qam", "main edits lib"]).unwrap();
        std::fs::write(repo.join("other.txt"), "x\n").unwrap();
        git::run(repo, &["add", "other.txt"]).unwrap();
        git::run(repo, &["commit", "-qm", "unrelated"]).unwrap();
        let s = snapshot("m", "M", "main", &base, &m).unwrap();
        let r = drift(repo, &s);
        assert_eq!(r.commits_behind, 2);
        assert_eq!(r.base_head.as_deref(), Some(git::head_commit(repo).unwrap().as_str()));
        assert_eq!(r.overlapping_files, vec!["src/lib.txt"]);
        let p = r.prediction.unwrap();
        assert_eq!((p.outcome.as_str(), p.conflicted_files), ("conflicts", vec!["src/lib.txt".to_string()]));

        // A mission whose base branch is gone reports that instead of failing.
        let gone = Snapshot { base_ref: "deleted-branch".into(), ..s.clone() };
        let r = drift(repo, &gone);
        assert_eq!((r.base_head, r.prediction), (None, None));
    }

    #[test]
    fn missions_on_different_bases_are_compared_over_their_common_history() {
        let d = init_repo();
        let repo = d.path();
        let base1 = git::head_commit(repo).unwrap();
        let a = wt(repo, "a", &base1);
        std::fs::write(repo.join("src/lib.txt"), "one\ntwo\nthree-main\n").unwrap();
        git::run(repo, &["commit", "-qam", "main moves"]).unwrap();
        let base2 = git::head_commit(repo).unwrap();
        let b = wt(repo, "b", &base2);
        // a changes line 3 differently from what b's base already has: landing both conflicts even though b changed nothing there.
        std::fs::write(a.join("src/lib.txt"), "one\ntwo\nthree-a\n").unwrap();
        std::fs::write(b.join("README.md"), "# b\n").unwrap();
        let (sa, sb) = (snapshot("a", "A", "main", &base1, &a).unwrap(), snapshot("b", "B", "main", &base2, &b).unwrap());
        let p = pair(repo, &sa, &sb);
        assert!(p.overlapping_files.is_empty());
        assert_eq!(p.prediction.outcome, "conflicts", "{p:?}");
    }

    /// Regression: git versions that refuse an argument also exit with 1, the same code as "conflicts". That must read as
    /// `unknown`, never as a (fabricated) conflict.
    #[test]
    fn a_git_error_is_unknown_not_a_conflict() {
        let d = init_repo();
        let repo = d.path();
        let tree = git::run(repo, &["rev-parse", "HEAD^{tree}"]).unwrap().trim().to_string();
        let p = predict(repo, &tree, "HEAD");
        assert_eq!(p.outcome, "unknown", "{p:?}");
        assert!(p.error.is_some());
        let p = predict(repo, "0000000000000000000000000000000000000001", "HEAD");
        assert_eq!(p.outcome, "unknown", "{p:?}");
    }
}
