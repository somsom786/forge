//! MCP authorization (see `crate::mcp`). User actions only; every change is audited.

use super::Core;
use crate::error::{ForgeError, Result};
use crate::mcp::{self, McpAuthorization, McpDefinition};
use serde::Serialize;
use serde_json::json;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct McpPreview {
    pub definition: McpDefinition,
    /// Echo this back to authorize: it proves the user saw this exact definition.
    pub definition_sha256: String,
    pub summary: Vec<String>,
    pub authorized: Option<McpAuthorization>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct McpAuthorizationStatus {
    pub authorization: McpAuthorization,
    /// The project's `.mcp.json` now defines this server differently (or not at all). Missions keep receiving the authorized
    /// definition; the user should look and re-authorize or revoke.
    pub changed_since_authorization: bool,
    pub summary: Vec<String>,
}

impl Core {
    fn mcp_root(&self, project_id: &str) -> Result<PathBuf> {
        Ok(PathBuf::from(self.store.get_project(project_id)?.root_path))
    }

    pub fn mcp_preview(&self, project_id: &str, name: &str) -> Result<McpPreview> {
        let def = mcp::from_project_file(&self.mcp_root(project_id)?, name)?;
        let authorized = self.store.mcp_active(project_id)?.into_iter().find(|a| a.name == name);
        Ok(McpPreview { definition_sha256: def.sha256(), summary: def.summary(), definition: def, authorized })
    }

    /// Authorize the server definition the user was shown (`expected_sha256` from the preview) for this project's missions.
    pub fn mcp_authorize(&self, project_id: &str, name: &str, expected_sha256: &str, tool_allowlist: &[String]) -> Result<McpAuthorization> {
        let def = mcp::from_project_file(&self.mcp_root(project_id)?, name)?;
        if def.sha256() != expected_sha256 {
            return Err(ForgeError::InvalidState(".mcp.json changed after you reviewed it; review the server again".into()));
        }
        if tool_allowlist.iter().any(|t| t.trim().is_empty() || t.contains(char::is_whitespace)) {
            return Err(ForgeError::Other("tool names in the allow-list cannot be empty or contain spaces".into()));
        }
        let a = self.store.mcp_authorize(project_id, &def, tool_allowlist)?;
        self.store.audit("user", "mcp.authorize", Some(project_id), &json!({"name": name, "sha256": a.definition_sha256, "tools": tool_allowlist}));
        Ok(a)
    }

    pub fn mcp_revoke(&self, authorization_id: &str) -> Result<McpAuthorization> {
        let a = self.store.mcp_revoke(authorization_id)?;
        self.store.audit("user", "mcp.revoke", Some(&a.project_id), &json!({"name": a.name, "sha256": a.definition_sha256}));
        Ok(a)
    }

    pub fn mcp_authorizations(&self, project_id: &str) -> Result<Vec<McpAuthorizationStatus>> {
        let root = self.mcp_root(project_id)?;
        Ok(self
            .store
            .mcp_active(project_id)?
            .into_iter()
            .map(|a| {
                let now = mcp::from_project_file(&root, &a.name).ok().map(|d| d.sha256());
                McpAuthorizationStatus { changed_since_authorization: now.as_deref() != Some(a.definition_sha256.as_str()), summary: a.definition.summary(), authorization: a }
            })
            .collect())
    }

    /// The `mcpServers` a mission of this project starts with: exactly the active authorizations, from their stored definitions.
    pub(super) fn mcp_servers_for_mission(&self, project_id: &str) -> Result<serde_json::Map<String, serde_json::Value>> {
        let env = |n: &str| std::env::var(n).ok();
        Ok(self.store.mcp_active(project_id)?.into_iter().map(|a| (a.name.clone(), a.definition.to_sdk_config(&env))).collect())
    }

    /// Core-side gate for `mcp__server__tool` calls: the server must be authorized for the mission's project, and the tool on its
    /// allow-list if it has one. `Err(reason)` means deny without asking.
    pub(super) fn mcp_gate(&self, mission_id: &str, tool: &str) -> std::result::Result<(), String> {
        let Some((server, t)) = mcp::split_tool(tool) else { return Err(format!("\"{tool}\" is not a well-formed MCP tool name")) };
        let project = self.store.get_mission(mission_id).map_err(|e| e.to_string())?.project_id;
        let auths = self.store.mcp_active(&project).map_err(|e| e.to_string())?;
        let Some(a) = auths.iter().find(|a| a.name == server) else {
            return Err(format!("the MCP server \"{server}\" is not authorized for this project"));
        };
        if !a.tool_allowlist.is_empty() && !a.tool_allowlist.iter().any(|x| x == t) {
            return Err(format!("the tool \"{t}\" of MCP server \"{server}\" is not on its allow-list"));
        }
        Ok(())
    }
}
