//! Inventory of everything that configures an agent in a project: instruction files, Claude settings (permissions, hooks),
//! rules, skills, subagents, commands and MCP servers. Read-only, bounded, and honest about what FORGE missions load.
//!
//! What FORGE missions actually do with these (see `adapters/claude.ts`):
//!   * CLAUDE.md, rules, skills, subagents, commands from the PROJECT are loaded (`settingSources:['project']`);
//!   * project hooks and `.mcp.json` servers are NOT run (neutralised: a repository must not execute code on session start);
//!   * user-level (~/.claude) configuration is not loaded at all.
//! Every item carries `loaded_in_missions` so the UI can say exactly that instead of implying hooks "work" in missions.

use crate::knowledge::page;
use crate::paths;
use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

const MAX_FILE_BYTES: u64 = 512 * 1024;
const MAX_ITEMS: usize = 400;

/// Exact lifecycle event names the Claude Agent SDK / Claude Code accept (from the SDK's `HookEvent` type).
pub const CLAUDE_HOOK_EVENTS: &[&str] = &[
    "PreToolUse", "PostToolUse", "PostToolUseFailure", "PostToolBatch", "Notification", "UserPromptSubmit", "UserPromptExpansion",
    "SessionStart", "SessionEnd", "Stop", "StopFailure", "SubagentStart", "SubagentStop", "PreCompact", "PostCompact", "PreModelSwitch",
    "PostModelSwitch", "PermissionRequest", "PermissionDenied", "Setup", "TeammateIdle", "TaskCreated", "TaskCompleted", "Elicitation",
    "ElicitationResult", "ConfigChange", "WorktreeCreate", "WorktreeRemove", "InstructionsLoaded", "CwdChanged", "FileChanged",
    "DirectoryAdded", "MessageDisplay",
];

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HarnessItem {
    /// Stable id: `<kind>:<scope>:<path>`.
    pub id: String,
    /// instructions | settings | rule | skill | subagent | command | mcp
    pub kind: String,
    /// project | local | user
    pub scope: String,
    pub path: String,
    /// Path relative to the project root (None for user-level items).
    pub rel: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub bytes: u64,
    pub sha256: String,
    /// FORGE may edit this (project scope, inside the allowlist). User-level items are never editable here.
    pub editable: bool,
    /// Does a FORGE mission read this? (instructions/rules/skills/agents/commands: yes; settings/hooks/mcp: no.)
    pub loaded_in_missions: bool,
    pub warnings: Vec<String>,
    /// Kind-specific extras (allowed tools, model, tools list, ...).
    pub details: Value,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HookDef {
    /// `<settings item id>#<event>#<group index>#<hook index>`
    pub id: String,
    pub event: String,
    pub matcher: Option<String>,
    /// command | prompt | http | other
    pub kind: String,
    pub command: String,
    pub timeout_secs: Option<u64>,
    pub source_item: String,
    pub scope: String,
    /// Valid Claude event name?
    pub known_event: bool,
    /// FORGE missions do not run project hooks.
    pub runs_in_forge_missions: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct McpServerDef {
    pub name: String,
    /// stdio | http | sse | unknown
    pub transport: String,
    pub command: Option<String>,
    pub url: Option<String>,
    pub source_item: String,
    pub scope: String,
    pub runs_in_forge_missions: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Inventory {
    pub items: Vec<HarnessItem>,
    pub hooks: Vec<HookDef>,
    pub mcp_servers: Vec<McpServerDef>,
    /// Plain-language notes about how FORGE treats this configuration.
    pub notes: Vec<String>,
    pub truncated: bool,
}

/// Which project-relative paths FORGE is willing to edit (everything else is read-only here).
pub fn editable_rel(rel: &str) -> bool {
    let r = rel.replace('\\', "/");
    if r.contains("..") || r.starts_with('/') {
        return false;
    }
    let lower = r.to_lowercase();
    if matches!(lower.as_str(), "claude.md" | "agents.md" | "gemini.md" | ".claude/claude.md" | ".claude/settings.json" | ".mcp.json") {
        // `.mcp.json` is inventoried but not editable: it launches programs, and FORGE does not manage that.
        return lower != ".mcp.json";
    }
    let parts: Vec<&str> = lower.split('/').collect();
    match parts.as_slice() {
        [".claude", "rules", f] | [".claude", "commands", f] | [".claude", "agents", f] => f.ends_with(".md"),
        [".claude", "skills", _dir, rest @ ..] => !rest.is_empty() && rest.iter().all(|p| !p.is_empty() && !p.starts_with('.')),
        _ => false,
    }
}

fn read_capped(path: &Path) -> Option<(String, u64)> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
        return None;
    }
    // Refuse links: the inventory must describe files that are really in the project.
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return None;
        }
    }
    let bytes = std::fs::read(path).ok()?;
    Some((String::from_utf8_lossy(&bytes).into_owned(), meta.len()))
}

fn item(kind: &str, scope: &str, path: &Path, rel: Option<String>, name: String, description: Option<String>, text: &str, size: u64, loaded: bool, warnings: Vec<String>, details: Value) -> HarnessItem {
    let editable = scope == "project" && rel.as_deref().is_some_and(editable_rel);
    HarnessItem {
        id: format!("{kind}:{scope}:{}", rel.clone().unwrap_or_else(|| path.to_string_lossy().replace('\\', "/"))),
        kind: kind.into(),
        scope: scope.into(),
        path: path.to_string_lossy().into_owned(),
        rel,
        name,
        description,
        bytes: size,
        sha256: page::content_hash(text),
        editable,
        loaded_in_missions: loaded,
        warnings,
        details,
    }
}

fn rel_of(root: &Path, p: &Path) -> String {
    p.strip_prefix(root).unwrap_or(p).components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/")
}

fn list_dir(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > 4 || out.len() > MAX_ITEMS {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let Ok(m) = std::fs::symlink_metadata(e.path()) else { continue };
        if m.is_dir() {
            list_dir(&e.path(), depth + 1, out);
        } else if m.is_file() {
            out.push(e.path());
        }
    }
}

fn parse_hooks(settings: &Value, source: &str, scope: &str) -> (Vec<HookDef>, Vec<String>) {
    let mut out = vec![];
    let mut warnings = vec![];
    let Some(hooks) = settings.get("hooks") else { return (out, warnings) };
    let Some(map) = hooks.as_object() else {
        warnings.push("`hooks` is not an object".into());
        return (out, warnings);
    };
    for (event, groups) in map {
        let known = CLAUDE_HOOK_EVENTS.contains(&event.as_str());
        if !known {
            warnings.push(format!("`{event}` is not a Claude hook event name (events are case-sensitive; see the supported list)"));
        }
        let Some(groups) = groups.as_array() else {
            warnings.push(format!("hooks.{event} is not a list"));
            continue;
        };
        for (gi, g) in groups.iter().enumerate() {
            let matcher = g.get("matcher").and_then(Value::as_str).map(str::to_string);
            let Some(list) = g.get("hooks").and_then(Value::as_array) else {
                warnings.push(format!("hooks.{event}[{gi}] has no `hooks` list"));
                continue;
            };
            for (hi, h) in list.iter().enumerate() {
                let kind = h.get("type").and_then(Value::as_str).unwrap_or("command").to_string();
                out.push(HookDef {
                    id: format!("{source}#{event}#{gi}#{hi}"),
                    event: event.clone(),
                    matcher: matcher.clone(),
                    command: h.get("command").and_then(Value::as_str).or_else(|| h.get("prompt").and_then(Value::as_str)).or_else(|| h.get("url").and_then(Value::as_str)).unwrap_or("").to_string(),
                    timeout_secs: h.get("timeout").and_then(Value::as_u64),
                    kind: if matches!(kind.as_str(), "command" | "prompt" | "http") { kind } else { "other".into() },
                    source_item: source.to_string(),
                    scope: scope.to_string(),
                    known_event: known,
                    runs_in_forge_missions: false,
                });
            }
        }
    }
    (out, warnings)
}

fn parse_mcp(v: &Value, source: &str, scope: &str) -> Vec<McpServerDef> {
    let Some(servers) = v.get("mcpServers").and_then(Value::as_object) else { return vec![] };
    servers
        .iter()
        .map(|(name, s)| {
            let command = s.get("command").and_then(Value::as_str).map(str::to_string);
            let url = s.get("url").and_then(Value::as_str).map(str::to_string);
            let transport = match (s.get("type").and_then(Value::as_str), &command, &url) {
                (Some(t), _, _) => t.to_string(),
                (None, Some(_), _) => "stdio".into(),
                (None, None, Some(_)) => "http".into(),
                _ => "unknown".into(),
            };
            McpServerDef { name: name.clone(), transport, command, url, source_item: source.to_string(), scope: scope.to_string(), runs_in_forge_missions: false }
        })
        .collect()
}

fn scan_project(root: &Path, inv: &mut Inventory) {
    let mut push = |i: HarnessItem, inv: &mut Inventory| {
        if inv.items.len() >= MAX_ITEMS {
            inv.truncated = true;
        } else {
            inv.items.push(i);
        }
    };

    for rel in ["CLAUDE.md", "AGENTS.md", "GEMINI.md", ".claude/CLAUDE.md", ".cursorrules", ".github/copilot-instructions.md"] {
        let p = root.join(rel);
        if let Some((text, size)) = read_capped(&p) {
            let provider = match rel {
                "AGENTS.md" => "Codex and other agents",
                "GEMINI.md" => "Gemini CLI",
                ".cursorrules" => "Cursor",
                ".github/copilot-instructions.md" => "GitHub Copilot",
                _ => "Claude Code",
            };
            push(item("instructions", "project", &p, Some(rel.into()), rel.into(), Some(format!("Project instructions read by {provider}")), &text, size, rel.contains("CLAUDE"), vec![], serde_json::json!({"provider": provider, "lines": text.lines().count()})), inv);
        }
    }
    // Nested CLAUDE.md files (monorepos), bounded.
    let mut all = vec![];
    list_dir(root, 0, &mut all);
    for p in all.iter().filter(|p| p.file_name().is_some_and(|n| n == "CLAUDE.md" || n == "AGENTS.md")) {
        let rel = rel_of(root, p);
        if rel.contains('/') && !rel.starts_with(".claude/") && !rel.starts_with("node_modules/") && !rel.contains("/node_modules/") && !rel.starts_with(".git/") {
            if let Some((text, size)) = read_capped(p) {
                push(item("instructions", "project", p, Some(rel.clone()), rel.clone(), Some("Nested instructions (loaded when the agent works in that folder)".into()), &text, size, rel.ends_with("CLAUDE.md"), vec![], serde_json::json!({"nested": true, "lines": text.lines().count()})), inv);
            }
        }
    }

    for (rel, scope) in [(".claude/settings.json", "project"), (".claude/settings.local.json", "local")] {
        let p = root.join(rel);
        if let Some((text, size)) = read_capped(&p) {
            let mut warnings = vec![];
            let v: Value = match serde_json::from_str(&text) {
                Ok(v) => v,
                Err(e) => {
                    warnings.push(format!("invalid JSON: {e}"));
                    Value::Null
                }
            };
            let id = format!("settings:{scope}:{rel}");
            let (hooks, hw) = parse_hooks(&v, &id, scope);
            warnings.extend(hw);
            let allow = v.pointer("/permissions/allow").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect::<Vec<_>>()).unwrap_or_default();
            let deny = v.pointer("/permissions/deny").and_then(Value::as_array).map(|a| a.len()).unwrap_or(0);
            let mode = v.pointer("/permissions/defaultMode").and_then(Value::as_str).map(str::to_string);
            inv.hooks.extend(hooks);
            inv.mcp_servers.extend(parse_mcp(&v, &id, scope));
            push(item("settings", scope, &p, Some(rel.into()), rel.into(), Some("Claude Code settings: permissions, hooks, environment".into()), &text, size, false, warnings, serde_json::json!({"allow": allow, "denyCount": deny, "defaultMode": mode, "model": v.get("model"), "hasEnv": v.get("env").is_some()})), inv);
        }
    }

    let mcp = root.join(".mcp.json");
    if let Some((text, size)) = read_capped(&mcp) {
        let mut warnings = vec![];
        let v: Value = serde_json::from_str(&text).unwrap_or_else(|e| {
            warnings.push(format!("invalid JSON: {e}"));
            Value::Null
        });
        inv.mcp_servers.extend(parse_mcp(&v, "mcp:project:.mcp.json", "project"));
        push(item("mcp", "project", &mcp, Some(".mcp.json".into()), ".mcp.json".into(), Some("MCP servers declared by the repository (stdio servers launch programs)".into()), &text, size, false, warnings, serde_json::json!({})), inv);
    }

    for (sub, kind) in [("rules", "rule"), ("commands", "command"), ("agents", "subagent")] {
        let dir = root.join(".claude").join(sub);
        let mut files = vec![];
        list_dir(&dir, 0, &mut files);
        for p in files.into_iter().filter(|p| p.extension().is_some_and(|e| e == "md")) {
            if let Some((text, size)) = read_capped(&p) {
                let parsed = page::parse(&text);
                let rel = rel_of(root, &p);
                let name = parsed.meta.title.clone().or_else(|| parsed.meta.extra.get("name").cloned()).unwrap_or_else(|| p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
                let desc = parsed.meta.extra.get("description").cloned();
                push(item(kind, "project", &p, Some(rel), name, desc, &text, size, true, parsed.warnings, serde_json::json!({"tools": parsed.meta.extra.get("tools"), "model": parsed.meta.extra.get("model"), "paths": parsed.meta.extra.get("paths")})), inv);
            }
        }
    }
    let skills = root.join(".claude").join("skills");
    let mut sk = vec![];
    list_dir(&skills, 0, &mut sk);
    for p in sk.into_iter().filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("SKILL.md"))) {
        if let Some((text, size)) = read_capped(&p) {
            let parsed = page::parse(&text);
            let rel = rel_of(root, &p);
            let dir_name = p.parent().and_then(|d| d.file_name()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let name = parsed.meta.extra.get("name").cloned().unwrap_or(dir_name);
            let mut warnings = parsed.warnings.clone();
            if parsed.meta.extra.get("description").is_none() {
                warnings.push("a skill needs a `description`: it is how the agent decides when to use it".into());
            }
            push(item("skill", "project", &p, Some(rel), name, parsed.meta.extra.get("description").cloned(), &text, size, true, warnings, serde_json::json!({"allowedTools": parsed.meta.extra.get("allowed-tools"), "bodyChars": parsed.body.chars().count()})), inv);
        }
    }
}

fn scan_user(home: &Path, inv: &mut Inventory) {
    let base = home.join(".claude");
    for (rel, kind, what) in [("CLAUDE.md", "instructions", "Your global Claude Code instructions"), ("settings.json", "settings", "Your global Claude Code settings")] {
        let p = base.join(rel);
        if let Some((text, size)) = read_capped(&p) {
            let mut warnings = vec![];
            if kind == "settings" {
                let v: Value = serde_json::from_str(&text).unwrap_or_else(|e| {
                    warnings.push(format!("invalid JSON: {e}"));
                    Value::Null
                });
                let id = format!("settings:user:{}", p.to_string_lossy().replace('\\', "/"));
                let (hooks, hw) = parse_hooks(&v, &id, "user");
                warnings.extend(hw);
                inv.hooks.extend(hooks);
                inv.mcp_servers.extend(parse_mcp(&v, &id, "user"));
            }
            inv.items.push(item(kind, "user", &p, None, rel.into(), Some(format!("{what} — never loaded by FORGE missions, never edited by FORGE")), &text, size, false, warnings, serde_json::json!({})));
        }
    }
    for (sub, kind) in [("skills", "skill"), ("agents", "subagent")] {
        let mut files = vec![];
        list_dir(&base.join(sub), 0, &mut files);
        for p in files.into_iter().filter(|p| p.extension().is_some_and(|e| e == "md")).take(100) {
            if let Some((text, size)) = read_capped(&p) {
                let parsed = page::parse(&text);
                let name = parsed.meta.extra.get("name").cloned().unwrap_or_else(|| p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
                inv.items.push(item(kind, "user", &p, None, name, parsed.meta.extra.get("description").cloned(), &text, size, false, parsed.warnings, serde_json::json!({})));
            }
        }
    }
}

pub fn scan(root: &Path, user_home: Option<&Path>) -> Inventory {
    let mut inv = Inventory::default();
    scan_project(root, &mut inv);
    if let Some(h) = user_home {
        scan_user(h, &mut inv);
    }
    inv.notes = vec![
        "FORGE missions load this project's CLAUDE.md, rules, skills, subagents and commands.".into(),
        "FORGE missions do NOT run project hooks or .mcp.json servers: a repository must not execute code when a session starts. Hooks defined here run in your own Claude Code sessions.".into(),
        "Your user-level (~/.claude) configuration is shown for reference only; FORGE never loads or edits it.".into(),
    ];
    inv
}

/// Safe resolution of a project-relative path for reading/writing: no traversal, no links, inside the root.
pub fn resolve(root: &Path, rel: &str) -> crate::error::Result<PathBuf> {
    paths::resolve_within(root, Path::new(rel))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        let w = |rel: &str, text: &str| {
            let p = d.path().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        };
        w("CLAUDE.md", "# Project\nAlways run tests.\n");
        w("AGENTS.md", "# Agents\nBe brief.\n");
        w("packages/web/CLAUDE.md", "# Web\n");
        w(".claude/settings.json", r#"{"permissions":{"allow":["Bash(npm test)","Read"],"deny":["Bash(rm *)"],"defaultMode":"default"},"hooks":{"PostToolUse":[{"matcher":"Write|Edit","hooks":[{"type":"command","command":"npx prettier --write","timeout":30}]}],"pretooluse":[{"hooks":[{"type":"command","command":"echo hi"}]}]}}"#);
        w(".claude/settings.local.json", r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"echo done"}]}]}}"#);
        w(".claude/rules/testing.md", "---\npaths: src/**\n---\nWrite tests first.\n");
        w(".claude/skills/release/SKILL.md", "---\nname: release\ndescription: Cut a release\nallowed-tools: Bash(git tag *)\n---\nSteps...\n");
        w(".claude/skills/broken/SKILL.md", "---\nname: broken\n---\nno description\n");
        w(".claude/agents/reviewer.md", "---\nname: reviewer\ndescription: Reviews diffs\ntools: Read, Grep\nmodel: sonnet\n---\nYou review.\n");
        w(".mcp.json", r#"{"mcpServers":{"fs":{"command":"node","args":["server.js"]},"remote":{"type":"http","url":"https://example.com/mcp"}}}"#);
        d
    }

    #[test]
    fn inventories_every_kind_with_scope_and_mission_loading_truth() {
        let d = project();
        let inv = scan(d.path(), None);
        let by_kind = |k: &str| inv.items.iter().filter(|i| i.kind == k).count();
        assert_eq!((by_kind("instructions"), by_kind("settings"), by_kind("rule"), by_kind("skill"), by_kind("subagent"), by_kind("mcp")), (3, 2, 1, 2, 1, 1), "{:#?}", inv.items.iter().map(|i| &i.id).collect::<Vec<_>>());
        let claude = inv.items.iter().find(|i| i.rel.as_deref() == Some("CLAUDE.md")).unwrap();
        assert!(claude.loaded_in_missions && claude.editable);
        let agents = inv.items.iter().find(|i| i.rel.as_deref() == Some("AGENTS.md")).unwrap();
        assert!(!agents.loaded_in_missions, "AGENTS.md is for other agents; Claude missions do not read it");
        let settings = inv.items.iter().find(|i| i.kind == "settings" && i.scope == "project").unwrap();
        assert!(!settings.loaded_in_missions && settings.editable);
        assert_eq!(settings.details["allow"].as_array().unwrap().len(), 2);
        assert!(inv.items.iter().find(|i| i.kind == "mcp").is_some_and(|m| !m.editable && !m.loaded_in_missions), ".mcp.json launches programs: inventoried, never edited, never run in missions");
        assert!(inv.notes.iter().any(|n| n.contains("do NOT run project hooks")));
    }

    #[test]
    fn hooks_use_exact_event_names_and_say_they_do_not_run_in_missions() {
        let d = project();
        let inv = scan(d.path(), None);
        assert_eq!(inv.hooks.len(), 3);
        let post = inv.hooks.iter().find(|h| h.event == "PostToolUse").unwrap();
        assert_eq!((post.matcher.as_deref(), post.command.as_str(), post.timeout_secs, post.known_event, post.runs_in_forge_missions), (Some("Write|Edit"), "npx prettier --write", Some(30), true, false));
        let wrong = inv.hooks.iter().find(|h| h.event == "pretooluse").unwrap();
        assert!(!wrong.known_event, "lifecycle names are exact and case-sensitive");
        let settings = inv.items.iter().find(|i| i.kind == "settings" && i.scope == "project").unwrap();
        assert!(settings.warnings.iter().any(|w| w.contains("pretooluse") && w.contains("not a Claude hook event")), "{:?}", settings.warnings);
        assert!(inv.hooks.iter().any(|h| h.event == "Stop" && h.scope == "local"));
    }

    #[test]
    fn skills_subagents_and_mcp_servers_are_described() {
        let d = project();
        let inv = scan(d.path(), None);
        let release = inv.items.iter().find(|i| i.kind == "skill" && i.name == "release").unwrap();
        assert_eq!((release.description.as_deref(), release.editable), (Some("Cut a release"), true));
        assert_eq!(release.details["allowedTools"], "Bash(git tag *)");
        let broken = inv.items.iter().find(|i| i.kind == "skill" && i.name == "broken").unwrap();
        assert!(broken.warnings.iter().any(|w| w.contains("needs a `description`")));
        let rev = inv.items.iter().find(|i| i.kind == "subagent").unwrap();
        assert_eq!((rev.name.as_str(), rev.details["model"].as_str()), ("reviewer", Some("sonnet")));
        let mut m: Vec<_> = inv.mcp_servers.iter().map(|s| (s.name.as_str(), s.transport.as_str())).collect();
        m.sort();
        assert_eq!(m, vec![("fs", "stdio"), ("remote", "http")]);
        assert!(inv.mcp_servers.iter().all(|s| !s.runs_in_forge_missions));
    }

    #[test]
    fn user_level_configuration_is_shown_but_never_editable_or_loaded() {
        let d = project();
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join(".claude/skills/mine")).unwrap();
        std::fs::write(home.path().join(".claude/CLAUDE.md"), "global").unwrap();
        std::fs::write(home.path().join(".claude/settings.json"), r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"x"}]}]}}"#).unwrap();
        std::fs::write(home.path().join(".claude/skills/mine/SKILL.md"), "---\nname: mine\ndescription: d\n---\n").unwrap();
        let inv = scan(d.path(), Some(home.path()));
        let user: Vec<_> = inv.items.iter().filter(|i| i.scope == "user").collect();
        assert_eq!(user.len(), 2 + 1 - 0 - 0, "{:?}", user.iter().map(|i| &i.id).collect::<Vec<_>>());
        assert!(user.iter().all(|i| !i.editable && !i.loaded_in_missions));
        assert!(inv.hooks.iter().any(|h| h.scope == "user" && h.event == "SessionStart"));
    }

    #[test]
    fn editable_allowlist_is_tight() {
        for ok in ["CLAUDE.md", "AGENTS.md", ".claude/settings.json", ".claude/rules/x.md", ".claude/agents/a.md", ".claude/commands/c.md", ".claude/skills/s/SKILL.md", ".claude/skills/s/scripts/run.sh"] {
            assert!(editable_rel(ok), "{ok}");
        }
        for no in [".mcp.json", ".claude/settings.local.json", "../CLAUDE.md", "src/main.rs", ".claude/hooks/x.sh", ".claude/rules/x.sh", ".git/config", "/etc/passwd", ".claude/skills/s/.hidden", ".claude/skills/"] {
            assert!(!editable_rel(no), "{no}");
        }
    }

    #[test]
    fn oversized_and_linked_files_are_skipped_not_fatal() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("CLAUDE.md"), vec![b'x'; (MAX_FILE_BYTES + 1) as usize]).unwrap();
        assert!(scan(d.path(), None).items.is_empty());
        let empty = tempfile::tempdir().unwrap();
        assert!(scan(empty.path(), None).items.is_empty());
    }
}
