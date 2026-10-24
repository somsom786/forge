//! Choosing where a project's checks and experiments run. User action only; every change is audited.

use super::Core;
use crate::error::{ForgeError, Result};
use crate::execution::{self, Backend, BackendInfo};
use crate::store::Project;
use serde_json::json;

impl Core {
    pub fn execution_backends(&self) -> Vec<BackendInfo> {
        execution::all()
    }

    /// Select the backend for a project. A backend that cannot run on this machine cannot be selected: choosing it would only
    /// make every later check fail, and the user must know now, not at run time.
    pub fn project_set_execution_backend(&self, project_id: &str, backend: &str) -> Result<Project> {
        let b = Backend::parse(backend)?;
        if let Err(why) = execution::availability(b) {
            return Err(ForgeError::InvalidState(format!("\"{}\" cannot be used on this machine: {why}", b.label())));
        }
        let before = self.store.get_project(project_id)?.execution_backend;
        self.store.set_project_backend(project_id, b.as_str())?;
        self.store.audit("user", "project.execution_backend", Some(project_id), &json!({"from": before, "to": b.as_str()}));
        self.store.get_project(project_id)
    }

    /// Select where a project's *agents* run. Same rule: only a backend that can run here; never a silent fallback later.
    pub fn project_set_agent_backend(&self, project_id: &str, backend: &str) -> Result<Project> {
        let b = Backend::parse(backend)?;
        if let Err(why) = execution::availability(b) {
            return Err(ForgeError::InvalidState(format!("\"{}\" cannot be used for agents on this machine: {why}", b.label())));
        }
        let before = self.store.get_project(project_id)?.agent_backend;
        self.store.set_project_agent_backend(project_id, b.as_str())?;
        self.store.audit("user", "project.agent_backend", Some(project_id), &json!({"from": before, "to": b.as_str()}));
        self.store.get_project(project_id)
    }
}
