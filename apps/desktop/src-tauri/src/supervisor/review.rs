//! Contracts, verification, review and merge. A child module of the supervisor so it shares the same
//! reservation (one owner per mission) and error model.

use super::*;
use crate::contract::{CONTRACT_VERSION, CheckSpec, Contract, RunState};
use crate::merge;
use crate::proof::{self, PackInput, ReportInput, VerificationReport};
use crate::store_review::{ProofPackRow, VerificationRun};
use crate::verify;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewDecision {
    Approve,
    Reject,
    RequestChanges,
}

impl ReviewDecision {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "approve" => Some(Self::Approve),
            "reject" => Some(Self::Reject),
            "request_changes" => Some(Self::RequestChanges),
            _ => None,
        }
    }
    fn as_str(self) -> &'static str {
        match self {
            Self::Approve => "approve",
            Self::Reject => "reject",
            Self::RequestChanges => "request_changes",
        }
    }
}

/// What the reviewer was looking at. Approving or giving a verdict is only valid for exactly this state.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Seen {
    pub fingerprint: String,
    pub contract_sha256: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergeOutcome {
    pub commit: String,
    pub branch: String,
    pub target: String,
    pub squashed: bool,
    pub cleaned_up: bool,
    pub notes: Vec<String>,
}

struct MergeGuard<'a> {
    core: &'a Core,
    project_id: String,
}

impl Drop for MergeGuard<'_> {
    fn drop(&mut self) {
        lock(&self.core.merging).remove(&self.project_id);
    }
}

impl Core {
    /// Is anything else (agent, start/discard in flight, another check) using this mission's worktree?
    fn mission_busy(&self, mission_id: &str) -> bool {
        let r = self.reg();
        r.reserved.contains(mission_id) || r.sessions.values().any(|h| h.mission_id == mission_id)
    }

    /// The mission's worktree, proven to be ours, plus its base commit.
    fn workspace(&self, mission_id: &str) -> Result<(Mission, Project, PathBuf, String)> {
        let m = self.store.get_mission(mission_id)?;
        let project = self.store.get_project(&m.project_id)?;
        let (Some(wt), Some(base)) = (m.worktree_path.clone(), m.base_commit.clone()) else {
            return Err(ForgeError::InvalidState("this mission has not produced a workspace yet".into()));
        };
        let wt = PathBuf::from(wt);
        if !wt.exists() {
            return Err(ForgeError::Git("the mission worktree no longer exists".into()));
        }
        git::verify_worktree(Path::new(&project.root_path), &wt, m.branch.as_deref())?;
        Ok((m, project, wt, base))
    }

    // ---- contract -----------------------------------------------------------------------------

    pub fn get_contract(&self, mission_id: &str) -> Result<Contract> {
        self.store.get_contract(mission_id)
    }

    pub fn set_contract(&self, mission_id: &str, mut contract: Contract) -> Result<Mission> {
        let m = self.store.get_mission(mission_id)?;
        if MissionState::parse(&m.state).is_some_and(|s| s.is_terminal()) {
            return Err(ForgeError::InvalidState("a finished mission's contract can no longer be changed".into()));
        }
        contract.version = CONTRACT_VERSION;
        contract.validate()?;
        self.store.set_contract(mission_id, &contract)?;
        self.store.audit("user", "contract.set", Some(mission_id), &json!({"criteria": contract.criteria.len(), "checks": contract.checks.len()}));
        let m = self.store.get_mission(mission_id)?;
        self.emit(UiEvent::Mission { mission: m.clone() });
        Ok(m)
    }

    pub fn suggest_checks(&self, project_id: &str) -> Result<Vec<CheckSpec>> {
        let p = self.store.get_project(project_id)?;
        Ok(crate::detect::suggest_checks(Path::new(&p.root_path)))
    }

    // ---- verification ------------------------------------------------------------------------

    pub fn verification_report(&self, mission_id: &str) -> Result<VerificationReport> {
        let (_m, _p, wt, base) = self.workspace(mission_id)?;
        let contract = self.store.get_contract(mission_id)?;
        let runs = self.store.runs_for_mission(mission_id)?;
        let mut counts: HashMap<String, usize> = HashMap::new();
        for r in &runs {
            *counts.entry(r.check_id.clone()).or_default() += 1;
        }
        proof::build_report(ReportInput {
            contract: &contract,
            latest_runs: &self.store.latest_runs(mission_id)?,
            run_counts: &counts,
            verdicts: &self.store.latest_manual_verdicts(mission_id)?,
            fingerprint: git::fingerprint(&wt, &base)?,
            head_commit: git::head_commit(&wt).ok(),
            files: git::changed_files(&wt, &base)?,
            pending_approvals: self.store.pending_count_for_mission(mission_id)? as usize,
            agent_running: self.is_running(mission_id),
        })
    }

    /// Start a check in the background; the result arrives as a `Verification` event.
    pub fn start_check(self: &Arc<Self>, mission_id: &str, check_id: &str) -> Result<VerificationRun> {
        if self.mission_busy(mission_id) {
            return Err(ForgeError::InvalidState("stop the agent first: running checks while it edits files would make the evidence meaningless".into()));
        }
        let (m, _p, wt, base) = self.workspace(mission_id)?;
        if MissionState::parse(&m.state).is_some_and(|s| s.is_terminal()) {
            return Err(ForgeError::InvalidState("this mission is finished".into()));
        }
        let contract = self.store.get_contract(mission_id)?;
        let spec = contract.checks.iter().find(|c| c.id == check_id).cloned().ok_or_else(|| ForgeError::NotFound(format!("check \"{check_id}\" is not in this mission's contract")))?;

        let cancel = Arc::new(AtomicBool::new(false));
        {
            // Claim the mission for this check in the same critical section that proves nothing else owns it, so a
            // start/merge/discard cannot slip in between the test and the registration (lock order: registry, then checks).
            let reg = self.reg();
            if reg.reserved.contains(mission_id) || reg.sessions.values().any(|h| h.mission_id == mission_id) {
                return Err(ForgeError::InvalidState("another operation owns this mission right now (agent, start, merge or discard); try again when it finishes".into()));
            }
            let mut checks = lock(&self.checks);
            if checks.contains_key(mission_id) {
                return Err(ForgeError::InvalidState("a check is already running on this mission".into()));
            }
            checks.insert(mission_id.to_string(), cancel.clone());
        }
        let started = (|| -> Result<VerificationRun> {
            let pre = git::fingerprint(&wt, &base)?;
            let run = VerificationRun {
                id: crate::store::new_id(),
                mission_id: mission_id.into(),
                check_id: spec.id.clone(),
                name: spec.name.clone(),
                command: spec.command.clone(),
                kind: format!("{:?}", spec.kind).to_lowercase(),
                state: RunState::Running,
                exit_code: None,
                started_at: crate::store::now(),
                ended_at: None,
                duration_ms: None,
                fingerprint: pre,
                head_commit: git::head_commit(&wt).ok(),
                mutated: false,
                output_tail: String::new(),
                output_truncated: false,
                log_path: None,
                spec_rev: spec.revision(),
            };
            self.store.insert_run(&run)?;
            Ok(run)
        })();
        let run = match started {
            Ok(r) => r,
            Err(e) => {
                lock(&self.checks).remove(mission_id);
                return Err(e);
            }
        };
        self.store.audit("user", "check.run", Some(mission_id), &json!({"check": spec.id, "command": spec.command}));
        self.emit(UiEvent::Verification { run: run.clone() });

        let core = self.clone();
        let (run_id, pre, mission) = (run.id.clone(), run.fingerprint.clone(), mission_id.to_string());
        let log = self.cfg.home.join("runs").join(format!("{run_id}.log"));
        std::thread::spawn(move || {
            let outcome = verify::run_check(&spec.command, &wt, Duration::from_secs(spec.timeout_secs), Some(&log), cancel);
            let post = git::fingerprint(&wt, &base).unwrap_or_else(|_| "unknown".into());
            let mutated = post != pre;
            let log_s = outcome.log_path.as_ref().map(|p| p.to_string_lossy().into_owned());
            match core.store.finish_run(&run_id, outcome.state, outcome.exit_code, outcome.duration_ms as i64, &pre, mutated, &outcome.output_tail, outcome.output_truncated, log_s.as_deref()) {
                Ok(done) => core.emit(UiEvent::Verification { run: done }),
                Err(e) => core.notice("error", format!("could not record the result of check {}: {e}", spec.id)),
            }
            lock(&core.checks).remove(&mission);
        });
        Ok(run)
    }

    pub fn cancel_check(&self, mission_id: &str) -> Result<()> {
        match lock(&self.checks).get(mission_id) {
            Some(flag) => {
                flag.store(true, Ordering::SeqCst);
                Ok(())
            }
            None => Err(ForgeError::InvalidState("no check is running on this mission".into())),
        }
    }

    pub fn set_manual_verdict(&self, mission_id: &str, criterion_id: &str, pass: bool, note: Option<&str>, expected_fingerprint: &str) -> Result<()> {
        let contract = self.store.get_contract(mission_id)?;
        let criterion = contract.criteria.iter().find(|c| c.id == criterion_id).ok_or_else(|| ForgeError::NotFound(format!("criterion \"{criterion_id}\"")))?;
        let (_m, _p, wt, base) = self.workspace(mission_id)?;
        let fp = git::fingerprint(&wt, &base)?;
        if fp != expected_fingerprint {
            return Err(ForgeError::InvalidState("the changes are not the ones you were looking at; reload the verification view and judge the current changes".into()));
        }
        self.store.add_manual_verdict(mission_id, criterion_id, pass, note.map(str::trim).filter(|n| !n.is_empty()), &fp, &criterion.revision())?;
        self.store.audit("user", "criterion.verdict", Some(mission_id), &json!({"criterion": criterion_id, "pass": pass}));
        Ok(())
    }

    // ---- review ------------------------------------------------------------------------------

    pub fn review(self: &Arc<Self>, mission_id: &str, decision: ReviewDecision, note: Option<&str>, override_reason: Option<&str>, seen: Option<&Seen>) -> Result<Mission> {
        let note = note.map(str::trim).filter(|n| !n.is_empty());
        let override_reason = override_reason.map(str::trim).filter(|n| !n.is_empty());
        let res = self.reserve(mission_id, false)?;
        let m = self.store.get_mission(mission_id)?;
        if m.state != MissionState::AwaitingReview.as_str() {
            return Err(ForgeError::InvalidState(format!("only a mission awaiting review can be reviewed (it is \"{}\")", m.state)));
        }
        let report = self.verification_report(mission_id)?;
        let record = |ovr: Option<&str>| self.store.add_review_decision(mission_id, decision.as_str(), note, ovr, &report.fingerprint, report.head_commit.as_deref());
        match decision {
            ReviewDecision::Approve => {
                // Approval is a statement about a specific state of the changes and the contract. It must name them.
                let Some(seen) = seen else {
                    return Err(ForgeError::InvalidState("an approval must name the changes and contract that were reviewed".into()));
                };
                if seen.fingerprint != report.fingerprint || seen.contract_sha256 != report.contract_sha256 {
                    return Err(ForgeError::InvalidState("the changes (or the success criteria) are not the ones you reviewed; reload and review the current state before approving".into()));
                }
                if !report.can_approve() {
                    return Err(ForgeError::InvalidState(format!("cannot approve yet: {}", report.hard_blockers.join("; "))));
                }
                if report.needs_override() && override_reason.is_none() {
                    return Err(ForgeError::InvalidState(format!(
                        "{} item(s) are not established: {}. Approve anyway only with a recorded reason.",
                        report.unresolved.len(),
                        report.unresolved.join(" | ")
                    )));
                }
                record(if report.needs_override() { override_reason } else { None })?;
                self.store.audit("user", "review.approve", Some(mission_id), &json!({"unresolved": report.unresolved.len(), "override": override_reason}));
                self.set_state(mission_id, MissionState::Approved, "approved by reviewer")
            }
            ReviewDecision::Reject => {
                record(None)?;
                self.store.audit("user", "review.reject", Some(mission_id), &json!({}));
                self.set_state(mission_id, MissionState::Rejected, "rejected by reviewer")
            }
            ReviewDecision::RequestChanges => {
                let Some(text) = note else {
                    return Err(ForgeError::Other("say what should change; it is sent to the agent as the follow-up".into()));
                };
                record(None)?;
                self.store.audit("user", "review.request_changes", Some(mission_id), &json!({}));
                drop(res);
                self.start_mission(mission_id, Some(text))
            }
        }
    }

    // ---- merge ----------------------------------------------------------------------------------

    pub fn merge_mission(self: &Arc<Self>, mission_id: &str, squash: bool) -> Result<MergeOutcome> {
        let _res = self.reserve(mission_id, false)?;
        let (m, project, wt, base) = self.workspace(mission_id)?;
        if m.state != MissionState::Approved.as_str() {
            return Err(ForgeError::InvalidState(format!("only an approved mission can be merged (it is \"{}\")", m.state)));
        }
        let root = PathBuf::from(&project.root_path);
        {
            let mut busy = lock(&self.merging);
            if !busy.insert(project.id.clone()) {
                return Err(ForgeError::InvalidState("another merge into this project is in progress".into()));
            }
        }
        let _guard = MergeGuard { core: self, project_id: project.id.clone() };

        let branch = m.branch.clone().ok_or_else(|| ForgeError::InvalidState("mission has no branch".into()))?;
        let target = m.base_ref.clone().ok_or_else(|| ForgeError::InvalidState("mission has no base branch".into()))?;

        // What was approved must be exactly what is merged. One read yields both the fingerprint and the reviewed tree.
        let (fp, tree) = git::fingerprint_and_tree(&wt, &base)?;
        let approved = self.store.review_decisions(mission_id)?.into_iter().rev().find(|d| d.decision == "approve");
        if approved.as_ref().map(|d| d.fingerprint.as_str()) != Some(fp.as_str()) {
            self.settle(mission_id, MissionState::AwaitingReview, "changes were made after approval");
            return Err(ForgeError::InvalidState("the changes differ from what was approved; review them again".into()));
        }
        git::ensure_no_external_programs(&wt)?;
        merge::ensure_ready(&root, &target)?;
        let message = format!("{}

FORGE mission {}
Branch: {}", m.title, m.id, branch);
        // Commit the reviewed tree itself (never the worktree's index), then resolve everything to object ids once.
        let (branch_oid, _created) = merge::commit_reviewed_tree(&root, &wt, &branch, &tree, &message)?;
        let target_oid = merge::oid(&root, "HEAD")?;
        let prediction = merge::predict(&root, &target_oid, &branch_oid)?;
        if !prediction.conflicts.is_empty() {
            return Err(ForgeError::Git(format!(
                "merging would conflict in {} file(s): {}. Nothing was changed; resolve by asking the agent to rebase or by merging manually.",
                prediction.conflicts.len(),
                prediction.conflicts.join(", ")
            )));
        }
        let predicted_tree = prediction.tree.ok_or_else(|| ForgeError::Git("git did not report the merged tree".into()))?;
        let commit = merge::merge(&root, &branch_oid, &message, squash, &target_oid, &predicted_tree)?;
        self.store.audit("user", "mission.merge", Some(mission_id), &json!({"commit": commit, "squash": squash, "target": target}));
        self.set_state(mission_id, MissionState::Merged, "merged into the main checkout")?;

        let mut notes = vec![];
        // Seal the evidence while the worktree still exists.
        if let Err(e) = self.generate_proof_pack(mission_id) {
            notes.push(format!("the Proof Pack could not be generated: {e}"));
        }
        // A factual record of what happened, as a *proposed* page: a person decides whether it is worth trusting.
        if let Err(e) = self.capture_mission_record(mission_id, Some(&commit)) {
            notes.push(format!("the mission record could not be saved to memory: {e}"));
        }
        let mut cleaned = false;
        match git::worktree_remove(&root, &wt, false) {
            Ok(()) => {
                let identical = git::run_raw(&root, &["diff", "--quiet", "HEAD", &branch]).map(|o| o.status.success()).unwrap_or(false);
                if !squash || identical {
                    match git::delete_branch(&root, &branch, squash) {
                        Ok(()) => cleaned = true,
                        Err(e) => notes.push(format!("branch {branch} was kept: {e}")),
                    }
                } else {
                    notes.push(format!("branch {branch} was kept because main has moved on since the squash"));
                }
            }
            Err(e) => notes.push(format!("the worktree was kept: {e}")),
        }
        Ok(MergeOutcome { commit, branch, target, squashed: squash, cleaned_up: cleaned, notes })
    }

    /// Remove the worktree and branch of a *finished* mission (rejected, merged, cancelled). Same loss protection as
    /// discard: unsaved or unmerged work is only deleted after the user confirms the stated loss.
    pub fn cleanup_workspace(self: &Arc<Self>, mission_id: &str, confirm_loss: bool) -> Result<git::WorktreeSafety> {
        let _res = self.reserve(mission_id, false)?;
        let m = self.store.get_mission(mission_id)?;
        if !MissionState::parse(&m.state).is_some_and(|s| s.is_terminal()) {
            return Err(ForgeError::InvalidState("only finished missions can be cleaned up; discard or finish this one first".into()));
        }
        let project = self.store.get_project(&m.project_id)?;
        let root = Path::new(&project.root_path);
        let safety = self.worktree_safety(mission_id)?;
        if (safety.dirty_files > 0 || safety.unmerged_commits > 0) && !confirm_loss {
            return Err(ForgeError::InvalidState(format!(
                "cleaning up would permanently delete {} uncommitted file change(s) and {} unmerged commit(s) on branch {}",
                safety.dirty_files, safety.unmerged_commits, m.branch.as_deref().unwrap_or("?")
            )));
        }
        if let Some(wt) = &m.worktree_path {
            git::worktree_remove(root, Path::new(wt), confirm_loss)?;
        }
        if let Some(b) = &m.branch {
            if let Err(e) = git::delete_branch(root, b, safety.unmerged_commits > 0) {
                self.notice("warn", format!("branch {b} was kept: {e}"));
            }
        }
        self.store.audit("user", "mission.cleanup", Some(mission_id), &json!({"lostDirtyFiles": safety.dirty_files, "lostCommits": safety.unmerged_commits}));
        Ok(safety)
    }

    // ---- proof pack ----------------------------------------------------------------------------

    pub fn generate_proof_pack(&self, mission_id: &str) -> Result<ProofPackRow> {
        let (m, project, wt, base) = self.workspace(mission_id)?;
        let report = self.verification_report(mission_id)?;
        let diff = git::diff(&wt, &base)?;
        let sha = proof::sha256_hex(diff.as_bytes());
        let dir = self.cfg.home.join("proofs");
        std::fs::create_dir_all(&dir)?;
        let diff_path = dir.join(format!("{}-{}.diff", mission_id, &sha[..12]));
        std::fs::write(&diff_path, diff.as_bytes())?;
        let diff_path_s = diff_path.to_string_lossy().into_owned();

        let m = self.store.get_mission(&m.id)?;
        let runs = self.store.runs_for_mission(mission_id)?;
        let sessions = self.store.sessions_for_mission(mission_id)?;
        let approvals = self.store.approvals_for_mission(mission_id)?;
        let decisions = self.store.review_decisions(mission_id)?;
        let generated_at = crate::store::now();
        let (content, markdown) = proof::build_pack(PackInput {
            mission: &m,
            project_name: &project.name,
            report: &report,
            runs: &runs,
            sessions: &sessions,
            approvals: &approvals,
            decisions: &decisions,
            diff_sha256: &sha,
            diff_bytes: diff.len(),
            diff_path: Some(&diff_path_s),
            generated_at: &generated_at,
        });
        let content_sha = proof::sha256_hex(content.to_string().as_bytes());
        let row = self.store.insert_proof_pack(mission_id, &content, &content_sha, &markdown, Some(&diff_path_s))?;
        self.store.audit("user", "proofpack.generate", Some(mission_id), &json!({"sha256": content_sha}));
        Ok(row)
    }
}
