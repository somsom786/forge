//! ACP agent registry (see `crate::acp`). User actions only; audited.

use super::Core;
use crate::acp::{self, AcpAgent};
use crate::error::{ForgeError, Result};
use serde_json::json;

impl Core {
    pub fn acp_agent_register(&self, name: &str, command: &str, args: &[String]) -> Result<AcpAgent> {
        let name = name.trim();
        if !acp::name_ok(name) {
            return Err(ForgeError::Other("agent names use letters, digits, '-', '_' and '.' (at most 48)".into()));
        }
        acp::validate_args(args)?;
        let path = acp::resolve_command(command)?;
        let a = self.store.acp_add(name, &path.to_string_lossy(), args)?;
        self.store.audit("user", "acp.register", None, &json!({"name": a.name, "command": a.command, "args": a.args}));
        Ok(a)
    }

    pub fn acp_agent_remove(&self, id: &str) -> Result<AcpAgent> {
        let a = self.store.acp_remove(id)?;
        self.store.audit("user", "acp.remove", None, &json!({"name": a.name}));
        Ok(a)
    }

    pub fn acp_agents(&self) -> Result<Vec<AcpAgent>> {
        self.store.acp_list()
    }
}
