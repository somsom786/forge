//! ACP (Agent Client Protocol) agents the user registered. FORGE speaks ACP as the *client* (engine `adapters/acp.ts`); an
//! ACP agent is a program FORGE starts for a mission, so registering one is a trust decision like installing a CLI. The
//! command is pinned to an absolute path at registration; a mission refers to the agent by name.

use crate::error::{ForgeError, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AcpAgent {
    pub id: String,
    pub name: String,
    /// Absolute, canonical path resolved when the agent was registered.
    pub command: String,
    pub args: Vec<String>,
    pub added_at: String,
    pub removed_at: Option<String>,
}

pub fn name_ok(n: &str) -> bool {
    !n.is_empty() && n.len() <= 48 && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

/// Resolve and validate what the user typed. Bare names are looked up on PATH; the result must be an existing file.
pub fn resolve_command(command: &str) -> Result<PathBuf> {
    let c = command.trim();
    if c.is_empty() || c.contains(['\n', '\r']) {
        return Err(ForgeError::Other("enter the agent's command".into()));
    }
    let p = if Path::new(c).is_absolute() { PathBuf::from(c) } else { which::which(c).map_err(|_| ForgeError::NotFound(format!("\"{c}\" was not found on PATH; enter its full path")))? };
    let p = dunce::canonicalize(&p).map_err(|_| ForgeError::NotFound(format!("{} does not exist", p.display())))?;
    if !p.is_file() {
        return Err(ForgeError::Other(format!("{} is not a file", p.display())));
    }
    // Node cannot start .cmd/.bat files without a shell (and FORGE never starts agents through one).
    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if matches!(ext.as_str(), "cmd" | "bat" | "ps1") {
        return Err(ForgeError::Other(format!("{} is a script wrapper; register the program it runs (for example node.exe with the script as an argument)", p.display())));
    }
    Ok(p)
}

pub fn validate_args(args: &[String]) -> Result<()> {
    if args.len() > 32 || args.iter().any(|a| a.contains(['\n', '\r', '\0']) || a.len() > 1024) {
        return Err(ForgeError::Other("arguments must be at most 32 single-line values".into()));
    }
    Ok(())
}

/// What the provider list shows for ACP. Capabilities are negotiated per session; this is the static floor.
pub fn provider_info(agents: &[AcpAgent]) -> crate::providers::ProviderInfo {
    crate::providers::ProviderInfo {
        id: "acp".into(),
        name: "ACP agent".into(),
        installed: !agents.is_empty(),
        path: None,
        version: None,
        signed_in: None,
        auth_method: Some("agent-managed".into()),
        plan: None,
        adapter_available: !agents.is_empty(),
        notes: if agents.is_empty() {
            vec!["Register an Agent Client Protocol agent (Agents screen) to run missions with it.".into()]
        } else {
            vec![
                "Each agent signs in and bills on its own; FORGE never authenticates for it and strips API keys from its environment.".into(),
                "FORGE decides every permission request and file read/write the agent asks for, but cannot see a tool the agent runs without asking.".into(),
            ]
        },
        capabilities: capabilities(),
        models: agents.iter().map(|a| a.name.clone()).collect(),
    }
}

pub fn capabilities() -> Value {
    let s = |supported: bool, note: &str| json!({"supported": supported, "note": note});
    json!({
        "sessionStart": s(true, ""),
        "resume": s(false, "Only if the agent advertises loadSession; negotiated per session."),
        "streaming": s(true, ""),
        "interrupt": s(true, "session/cancel, then the agent process is stopped."),
        "toolEvents": s(true, "As reported by the agent."),
        "fileChanges": s(true, "Writes through FORGE and diffs the agent reports; the git diff is authoritative."),
        "approvals": s(true, "Agent-initiated: FORGE decides every request it receives; a tool run without asking is not seen."),
        "usageReporting": s(false, "ACP defines no usage reporting."),
        "modelSelection": s(false, "The registered agent decides its model."),
        "worktreeIntegration": s(true, "The session cwd is the FORGE worktree."),
    })
}

// ---- persistence -------------------------------------------------------------------------------------------------

const COLS: &str = "id, name, command, args_json, added_at, removed_at";

fn row(r: &rusqlite::Row) -> rusqlite::Result<AcpAgent> {
    let args: String = r.get(3)?;
    Ok(AcpAgent { id: r.get(0)?, name: r.get(1)?, command: r.get(2)?, args: serde_json::from_str(&args).unwrap_or_default(), added_at: r.get(4)?, removed_at: r.get(5)? })
}

impl crate::store::Store {
    pub fn acp_add(&self, name: &str, command: &str, args: &[String]) -> Result<AcpAgent> {
        let id = crate::store::new_id();
        let n = self.c().execute(
            "INSERT INTO acp_agent(id, name, command, args_json, added_at) SELECT ?1,?2,?3,?4,?5 WHERE NOT EXISTS (SELECT 1 FROM acp_agent WHERE name=?2 AND removed_at IS NULL)",
            rusqlite::params![id, name, command, serde_json::to_string(args)?, crate::store::now()],
        )?;
        if n != 1 {
            return Err(ForgeError::InvalidState(format!("an ACP agent named \"{name}\" is already registered")));
        }
        self.acp_get(&id)
    }

    pub fn acp_get(&self, id: &str) -> Result<AcpAgent> {
        use rusqlite::OptionalExtension;
        self.c().query_row(&format!("SELECT {COLS} FROM acp_agent WHERE id=?1"), [id], row).optional()?.ok_or_else(|| ForgeError::NotFound(format!("ACP agent {id}")))
    }

    pub fn acp_by_name(&self, name: &str) -> Result<AcpAgent> {
        use rusqlite::OptionalExtension;
        self.c()
            .query_row(&format!("SELECT {COLS} FROM acp_agent WHERE name=?1 AND removed_at IS NULL"), [name], row)
            .optional()?
            .ok_or_else(|| ForgeError::NotFound(format!("no registered ACP agent named \"{name}\"")))
    }

    pub fn acp_remove(&self, id: &str) -> Result<AcpAgent> {
        let n = self.c().execute("UPDATE acp_agent SET removed_at=?2 WHERE id=?1 AND removed_at IS NULL", rusqlite::params![id, crate::store::now()])?;
        if n != 1 {
            return Err(ForgeError::InvalidState("this ACP agent is not registered".into()));
        }
        self.acp_get(id)
    }

    pub fn acp_list(&self) -> Result<Vec<AcpAgent>> {
        let c = self.c();
        let mut s = c.prepare(&format!("SELECT {COLS} FROM acp_agent WHERE removed_at IS NULL ORDER BY name"))?;
        Ok(s.query_map([], row)?.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_are_pinned_to_existing_files_and_wrappers_are_refused() {
        let d = tempfile::tempdir().unwrap();
        let exe = d.path().join("agent-bin");
        std::fs::write(&exe, "x").unwrap();
        assert_eq!(resolve_command(&exe.to_string_lossy()).unwrap(), dunce::canonicalize(&exe).unwrap());
        assert!(resolve_command(&d.path().join("missing").to_string_lossy()).is_err());
        assert!(resolve_command(&d.path().to_string_lossy()).is_err(), "a directory");
        let cmd = d.path().join("agent.cmd");
        std::fs::write(&cmd, "x").unwrap();
        assert!(resolve_command(&cmd.to_string_lossy()).unwrap_err().to_string().contains("script wrapper"));
        assert!(resolve_command("definitely-not-a-real-forge-agent-xyz").is_err());
        assert!(resolve_command("a\nb").is_err());
        assert!(validate_args(&["--acp".into()]).is_ok());
        assert!(validate_args(&["a\nb".into()]).is_err());
        assert!(!name_ok("bad name") && !name_ok("") && name_ok("gemini-acp"));
    }

    #[test]
    fn names_are_unique_among_registered_agents_and_reusable_after_removal() {
        let s = crate::store::Store::in_memory();
        let a = s.acp_add("scripted", "/bin/x", &["--acp".into()]).unwrap();
        assert!(s.acp_add("scripted", "/bin/y", &[]).is_err());
        assert_eq!(s.acp_by_name("scripted").unwrap().args, vec!["--acp".to_string()]);
        s.acp_remove(&a.id).unwrap();
        assert!(s.acp_by_name("scripted").is_err());
        assert!(s.acp_add("scripted", "/bin/y", &[]).is_ok());
        assert_eq!(s.acp_list().unwrap().len(), 1);
    }
}
