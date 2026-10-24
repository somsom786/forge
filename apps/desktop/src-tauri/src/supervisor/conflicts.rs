//! Conflict report across a project's active missions (see `crate::conflicts`). Read-only.

use super::Core;
use crate::conflicts::{self, DriftReport, PairReport};
use crate::error::Result;
use crate::git;
use crate::mission::MissionState;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MissionChangeSet {
    pub mission_id: String,
    pub title: String,
    pub state: String,
    pub base_ref: String,
    pub base_commit: String,
    pub changed_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SkippedMission {
    pub mission_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConflictReport {
    pub project_id: String,
    pub generated_at: String,
    pub missions: Vec<MissionChangeSet>,
    /// Every pair of analysed missions, in mission order.
    pub pairs: Vec<PairReport>,
    pub drift: Vec<DriftReport>,
    /// Missions with a workspace that could not be analysed, and why.
    pub skipped: Vec<SkippedMission>,
}

impl Core {
    /// Overlap, predicted textual conflicts and base drift across the project's missions that still have work to land (not
    /// merged, rejected or cancelled) and a worktree. Nothing is modified.
    pub fn mission_conflicts(&self, project_id: &str) -> Result<ConflictReport> {
        let project = self.store.get_project(project_id)?;
        let repo = PathBuf::from(&project.root_path);
        let done = [MissionState::Merged, MissionState::Rejected, MissionState::Cancelled].map(MissionState::as_str);
        let mut missions = self.store.list_missions(Some(project_id))?;
        missions.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
        let (mut snaps, mut skipped) = (vec![], vec![]);
        for m in missions.into_iter().filter(|m| !done.contains(&m.state.as_str())) {
            let (Some(wt), Some(base), Some(base_ref)) = (m.worktree_path.clone(), m.base_commit.clone(), m.base_ref.clone()) else { continue };
            let wt = PathBuf::from(wt);
            let snap = (|| {
                if !wt.exists() {
                    return Err(crate::error::ForgeError::Git("the mission worktree no longer exists".into()));
                }
                git::verify_worktree(Path::new(&project.root_path), &wt, m.branch.as_deref())?;
                conflicts::snapshot(&m.id, &m.title, &base_ref, &base, &wt)
            })();
            match snap {
                Ok(s) => snaps.push((s, m.state)),
                Err(e) => skipped.push(SkippedMission { mission_id: m.id, reason: e.to_string() }),
            }
        }
        let only: Vec<conflicts::Snapshot> = snaps.iter().map(|(s, _)| s.clone()).collect();
        let (pairs, drift) = conflicts::analyse(&repo, &only);
        Ok(ConflictReport {
            project_id: project_id.to_string(),
            generated_at: crate::store::now(),
            missions: snaps
                .into_iter()
                .map(|(s, state)| MissionChangeSet { mission_id: s.mission_id, title: s.title, state, base_ref: s.base_ref, base_commit: s.base_commit, changed_files: s.files.into_iter().collect() })
                .collect(),
            pairs,
            drift,
            skipped,
        })
    }
}
