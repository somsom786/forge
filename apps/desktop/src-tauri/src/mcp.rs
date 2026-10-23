//! MCP authorization. A repository can *declare* MCP servers (`.mcp.json`), but nothing it declares runs in a FORGE mission
//! unless the user authorized that exact definition for that project. What is pinned is the definition FORGE read and showed
//! the user (command, arguments, URL, names of the environment variables it gets); missions receive the *stored* definition, so
//! editing `.mcp.json` afterwards cannot change what runs. Every tool call of an authorized server still goes through FORGE
//! policy and asks a human; an optional tool allow-list narrows it further. Calls to servers or tools that are not authorized
//! are denied by the core, whatever the agent or the server claims.
//!
//! Honest limits: an authorized stdio server runs on the host with the user's privileges (not in an execution backend), it starts
//! when the agent session starts (before any tool call is approved), and it inherits the agent process environment (from which
//! FORGE already strips API-billing keys). Authorizing is a trust decision about that program.

use crate::error::{ForgeError, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct McpDefinition {
    pub name: String,
    /// stdio | http
    pub transport: String,
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    pub url: Option<String>,
    /// Names of environment variables FORGE passes to the server (values come from FORGE's environment at mission start and
    /// are never stored).
    #[serde(default)]
    pub env_names: Vec<String>,
}

fn name_ok(n: &str) -> bool {
    !n.is_empty() && n.len() <= 64 && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') && !n.contains("__")
}

fn env_name_ok(n: &str) -> bool {
    !n.is_empty() && n.len() <= 128 && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !n.starts_with(|c: char| c.is_ascii_digit())
}

/// Environment variables an MCP server may never receive by name: they redirect or bill model traffic.
const FORBIDDEN_ENV: &[&str] = &["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_BASE_URL", "OPENAI_API_KEY", "OPENAI_BASE_URL", "CODEX_API_KEY"];

impl McpDefinition {
    pub fn validate(&self) -> Result<()> {
        let bad = |m: String| Err(ForgeError::Other(m));
        if !name_ok(&self.name) {
            return bad(format!("\"{}\" is not a server name FORGE accepts (letters, digits, '-', '_'; no '__')", self.name));
        }
        match self.transport.as_str() {
            "stdio" => {
                if self.command.as_deref().is_none_or(|c| c.trim().is_empty()) {
                    return bad(format!("{}: a stdio server needs a command", self.name));
                }
                if self.url.is_some() {
                    return bad(format!("{}: a stdio server has no URL", self.name));
                }
            }
            "http" => {
                let u = self.url.as_deref().unwrap_or("");
                let loopback = u.starts_with("http://127.0.0.1") || u.starts_with("http://localhost") || u.starts_with("http://[::1]");
                if !(u.starts_with("https://") || loopback) {
                    return bad(format!("{}: an HTTP server must use https (or plain http on loopback)", self.name));
                }
                if self.command.is_some() || !self.args.is_empty() {
                    return bad(format!("{}: an HTTP server has no command", self.name));
                }
            }
            t => return bad(format!("{}: transport \"{t}\" is not supported", self.name)),
        }
        for e in &self.env_names {
            if !env_name_ok(e) {
                return bad(format!("{}: \"{e}\" is not an environment variable name", self.name));
            }
            if FORBIDDEN_ENV.iter().any(|f| f.eq_ignore_ascii_case(e)) {
                return bad(format!("{}: {e} redirects or bills model traffic and is never passed to an MCP server", self.name));
            }
        }
        if self.args.iter().chain(self.command.iter()).any(|a| a.contains('\0') || a.contains('\n')) {
            return bad(format!("{}: commands and arguments must be single-line", self.name));
        }
        Ok(())
    }

    /// Stable identity of the definition (sorted keys), shown to the user and pinned on authorization.
    pub fn sha256(&self) -> String {
        let mut env = self.env_names.clone();
        env.sort();
        let doc = json!({"name": self.name, "transport": self.transport, "command": self.command, "args": self.args, "url": self.url, "envNames": env});
        format!("{:x}", Sha256::digest(doc.to_string().as_bytes()))
    }

    /// What authorizing means, in plain words.
    pub fn summary(&self) -> Vec<String> {
        let mut s = vec![];
        match self.transport.as_str() {
            "stdio" => {
                let cmd = std::iter::once(self.command.clone().unwrap_or_default()).chain(self.args.iter().cloned()).collect::<Vec<_>>().join(" ");
                s.push(format!("Starts the program `{cmd}` on this computer, with your privileges, whenever a Claude mission in this project starts — before any tool call is approved. It is not sandboxed."));
                if self.command.as_deref().is_some_and(|c| ["npx", "uvx", "bunx", "pnpx"].iter().any(|r| c == *r || c.ends_with(&format!("/{r}")) || c.ends_with(&format!("\\{r}.cmd")))) {
                    s.push("It is fetched by a package runner: unless the arguments pin an exact version, a different version can run next time.".into());
                }
            }
            _ => s.push(format!("Connects to {} on every Claude mission in this project.", self.url.clone().unwrap_or_default())),
        }
        if self.env_names.is_empty() {
            s.push("FORGE passes it no extra environment variables.".into());
        } else {
            s.push(format!("FORGE passes it these environment variables from its own environment: {}.", self.env_names.join(", ")));
        }
        s.push("Its tools can only be used with your approval, one call at a time; what they return is treated as untrusted data.".into());
        s.push("The server can still use the network and read files your account can read; FORGE cannot restrict a program it does not run.".into());
        s
    }

    /// The `mcpServers` entry handed to the Claude Agent SDK. Environment values are read now, never stored.
    pub fn to_sdk_config(&self, env: &dyn Fn(&str) -> Option<String>) -> Value {
        let envs: serde_json::Map<String, Value> = self.env_names.iter().filter_map(|n| env(n).map(|v| (n.clone(), Value::String(v)))).collect();
        match self.transport.as_str() {
            "stdio" => json!({"type": "stdio", "command": self.command, "args": self.args, "env": envs}),
            _ => json!({"type": "http", "url": self.url}),
        }
    }
}

/// Read one server definition from the project's `.mcp.json`. Literal environment *values* in the file are not carried over (they
/// would end up stored or shown); only their names are, and FORGE supplies the values from its own environment.
pub fn from_project_file(root: &Path, name: &str) -> Result<McpDefinition> {
    let p = root.join(".mcp.json");
    let meta = std::fs::symlink_metadata(&p).map_err(|_| ForgeError::NotFound("this project has no .mcp.json".into()))?;
    if !meta.is_file() || meta.len() > 256 * 1024 {
        return Err(ForgeError::PathRejected(".mcp.json is not a regular file of reasonable size".into()));
    }
    let v: Value = serde_json::from_str(&std::fs::read_to_string(&p)?).map_err(|e| ForgeError::Other(format!(".mcp.json is not valid JSON: {e}")))?;
    let entry = v.get("mcpServers").and_then(|s| s.get(name)).ok_or_else(|| ForgeError::NotFound(format!("no server \"{name}\" in .mcp.json")))?;
    let transport = match entry.get("type").and_then(Value::as_str) {
        Some("http") | Some("streamable-http") => "http",
        Some("sse") => "sse",
        Some("stdio") | None => {
            if entry.get("url").is_some() && entry.get("command").is_none() { "http" } else { "stdio" }
        }
        Some(other) => other,
    };
    let def = McpDefinition {
        name: name.into(),
        transport: transport.into(),
        command: entry.get("command").and_then(Value::as_str).map(String::from),
        args: entry.get("args").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(String::from).collect()).unwrap_or_default(),
        url: entry.get("url").and_then(Value::as_str).map(String::from),
        env_names: entry.get("env").and_then(Value::as_object).map(|o| o.keys().cloned().collect()).unwrap_or_default(),
    };
    def.validate()?;
    Ok(def)
}

/// `mcp__<server>__<tool>` → (server, tool). None for anything else.
pub fn split_tool(tool: &str) -> Option<(&str, &str)> {
    let rest = tool.strip_prefix("mcp__")?;
    let (server, t) = rest.split_once("__")?;
    (!server.is_empty() && !t.is_empty()).then_some((server, t))
}

// ---- persistence -------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct McpAuthorization {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub definition: McpDefinition,
    pub definition_sha256: String,
    /// Empty: every tool of the server may be *asked* for. Otherwise only these.
    pub tool_allowlist: Vec<String>,
    pub authorized_at: String,
    pub revoked_at: Option<String>,
}

const COLS: &str = "id, project_id, name, definition_json, definition_sha256, tool_allowlist_json, authorized_at, revoked_at";

fn row(r: &rusqlite::Row) -> rusqlite::Result<McpAuthorization> {
    let def: String = r.get(3)?;
    let tools: String = r.get(5)?;
    Ok(McpAuthorization {
        id: r.get(0)?,
        project_id: r.get(1)?,
        name: r.get(2)?,
        definition: serde_json::from_str(&def).map_err(|e| rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e)))?,
        definition_sha256: r.get(4)?,
        tool_allowlist: serde_json::from_str(&tools).unwrap_or_default(),
        authorized_at: r.get(6)?,
        revoked_at: r.get(7)?,
    })
}

impl crate::store::Store {
    pub fn mcp_authorize(&self, project_id: &str, def: &McpDefinition, tools: &[String]) -> Result<McpAuthorization> {
        let id = crate::store::new_id();
        let mut c = self.c();
        let tx = c.transaction()?;
        // Re-authorizing replaces the active authorization for that name.
        tx.execute("UPDATE mcp_authorization SET revoked_at=?3 WHERE project_id=?1 AND name=?2 AND revoked_at IS NULL", rusqlite::params![project_id, def.name, crate::store::now()])?;
        tx.execute(
            "INSERT INTO mcp_authorization(id, project_id, name, definition_json, definition_sha256, tool_allowlist_json, authorized_at) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            rusqlite::params![id, project_id, def.name, serde_json::to_string(def)?, def.sha256(), serde_json::to_string(tools)?, crate::store::now()],
        )?;
        tx.commit()?;
        drop(c);
        self.mcp_get(&id)
    }

    pub fn mcp_get(&self, id: &str) -> Result<McpAuthorization> {
        use rusqlite::OptionalExtension;
        self.c().query_row(&format!("SELECT {COLS} FROM mcp_authorization WHERE id=?1"), [id], row).optional()?.ok_or_else(|| ForgeError::NotFound(format!("MCP authorization {id}")))
    }

    pub fn mcp_revoke(&self, id: &str) -> Result<McpAuthorization> {
        let n = self.c().execute("UPDATE mcp_authorization SET revoked_at=?2 WHERE id=?1 AND revoked_at IS NULL", rusqlite::params![id, crate::store::now()])?;
        if n != 1 {
            return Err(ForgeError::InvalidState("this authorization is not active".into()));
        }
        self.mcp_get(id)
    }

    /// Active authorizations of a project.
    pub fn mcp_active(&self, project_id: &str) -> Result<Vec<McpAuthorization>> {
        let c = self.c();
        let mut s = c.prepare(&format!("SELECT {COLS} FROM mcp_authorization WHERE project_id=?1 AND revoked_at IS NULL ORDER BY name"))?;
        Ok(s.query_map([project_id], row)?.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, json: &str) {
        std::fs::write(dir.join(".mcp.json"), json).unwrap();
    }

    #[test]
    fn a_definition_is_read_without_its_secret_values_and_pinned_by_hash() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), r#"{"mcpServers":{"docs":{"command":"node","args":["server.js","--port","0"],"env":{"DOCS_TOKEN":"s3cret-literal"}}}}"#);
        let def = from_project_file(d.path(), "docs").unwrap();
        assert_eq!((def.transport.as_str(), def.command.as_deref(), def.env_names.clone()), ("stdio", Some("node"), vec!["DOCS_TOKEN".to_string()]));
        assert!(!serde_json::to_string(&def).unwrap().contains("s3cret-literal"), "literal env values are never kept");
        let sha = def.sha256();
        // Any change to what would run changes the pin.
        write(d.path(), r#"{"mcpServers":{"docs":{"command":"node","args":["evil.js"],"env":{"DOCS_TOKEN":"x"}}}}"#);
        assert_ne!(from_project_file(d.path(), "docs").unwrap().sha256(), sha);
        // A changed literal value alone does not (it is not part of what FORGE passes).
        write(d.path(), r#"{"mcpServers":{"docs":{"command":"node","args":["server.js","--port","0"],"env":{"DOCS_TOKEN":"other"}}}}"#);
        assert_eq!(from_project_file(d.path(), "docs").unwrap().sha256(), sha);
        let cfg = def.to_sdk_config(&|n| (n == "DOCS_TOKEN").then(|| "from-forge-env".to_string()));
        assert_eq!(cfg["env"]["DOCS_TOKEN"], "from-forge-env");
    }

    #[test]
    fn dangerous_or_malformed_definitions_are_refused() {
        let base = McpDefinition { name: "ok".into(), transport: "stdio".into(), command: Some("node".into()), args: vec![], url: None, env_names: vec![] };
        assert!(base.validate().is_ok());
        let cases: Vec<(&str, McpDefinition)> = vec![
            ("name with __", McpDefinition { name: "a__b".into(), ..base.clone() }),
            ("no command", McpDefinition { command: None, ..base.clone() }),
            ("billing env", McpDefinition { env_names: vec!["anthropic_api_key".into()], ..base.clone() }),
            ("bad env name", McpDefinition { env_names: vec!["A B".into()], ..base.clone() }),
            ("newline arg", McpDefinition { args: vec!["a\nb".into()], ..base.clone() }),
            ("plain http remote", McpDefinition { transport: "http".into(), command: None, url: Some("http://example.com/mcp".into()), ..base.clone() }),
            ("sse", McpDefinition { transport: "sse".into(), ..base.clone() }),
        ];
        for (why, d) in cases {
            assert!(d.validate().is_err(), "{why}");
        }
        let https = McpDefinition { transport: "http".into(), command: None, url: Some("https://mcp.example.com".into()), ..base.clone() };
        assert!(https.validate().is_ok());
        let local = McpDefinition { url: Some("http://127.0.0.1:3000/mcp".into()), ..https };
        assert!(local.validate().is_ok());
    }

    #[test]
    fn the_summary_says_what_authorizing_means() {
        let d = McpDefinition { name: "fs".into(), transport: "stdio".into(), command: Some("npx".into()), args: vec!["-y".into(), "@x/server-fs".into()], url: None, env_names: vec![] };
        let s = d.summary().join(" ");
        assert!(s.contains("npx -y @x/server-fs") && s.contains("not sandboxed") && s.contains("package runner") && s.contains("your approval"));
    }

    #[test]
    fn tool_names_split_on_the_first_double_underscore() {
        assert_eq!(split_tool("mcp__docs__search"), Some(("docs", "search")));
        assert_eq!(split_tool("mcp__docs__a__b"), Some(("docs", "a__b")));
        assert_eq!(split_tool("Bash"), None);
        assert_eq!(split_tool("mcp____x"), None);
    }
}
