//! Verification report and Proof Pack assembly. Pure functions: given recorded facts, derive what is and is not
//! established. Nothing here trusts an agent's claim of completion, and nothing produces a score or a percentage.

use crate::contract::{self, CheckSpec, Contract, Criterion, CriterionStatus, EvalContext, ManualVerdict, RunEvidence, RunState, ScopeReport};
use crate::git::ChangedFile;
use crate::store::{AgentSession, Approval, Mission};
use crate::store_review::{ManualVerdictRow, ReviewDecisionRow, VerificationRun};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CriterionReport {
    pub id: String,
    pub text: String,
    pub status: CriterionStatus,
    pub manual: bool,
    pub scope_rule: bool,
    pub verified_by: Vec<String>,
    pub manual_verdict: Option<ManualVerdictRow>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckReport {
    pub spec: CheckSpec,
    pub latest: Option<VerificationRun>,
    /// The latest run passed/failed against the changes as they are now, under the check's current definition.
    pub fresh: bool,
    pub runs: usize,
    /// Changed files that define or configure this check (its own command's files, package.json, test configs, CI...).
    /// An agent that edits them can make a check pass without the code being correct.
    pub touched_inputs: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationReport {
    pub fingerprint: String,
    /// Revision hash of the contract this report was computed for; approval is bound to it.
    pub contract_sha256: String,
    pub head_commit: Option<String>,
    pub files: Vec<ChangedFile>,
    pub agent_instruction_files: Vec<String>,
    pub scope: ScopeReport,
    pub scope_configured: bool,
    pub criteria: Vec<CriterionReport>,
    pub checks: Vec<CheckReport>,
    /// Required checks not passing on the current changes.
    pub blocking_checks: Vec<String>,
    /// Everything that is not established, in plain words.
    pub unresolved: Vec<String>,
    /// Things that can never be overridden (agent still running, unanswered approvals, a check in flight).
    pub hard_blockers: Vec<String>,
    pub contract: Contract,
}

impl VerificationReport {
    /// Approval is possible when nothing hard blocks it; unresolved items additionally need a recorded reason.
    pub fn can_approve(&self) -> bool {
        self.hard_blockers.is_empty()
    }
    pub fn needs_override(&self) -> bool {
        !self.unresolved.is_empty()
    }
}

pub struct ReportInput<'a> {
    pub contract: &'a Contract,
    pub latest_runs: &'a HashMap<String, VerificationRun>,
    pub run_counts: &'a HashMap<String, usize>,
    pub verdicts: &'a HashMap<String, ManualVerdictRow>,
    pub fingerprint: String,
    pub head_commit: Option<String>,
    pub files: Vec<ChangedFile>,
    pub pending_approvals: usize,
    pub agent_running: bool,
}

pub fn build_report(i: ReportInput) -> crate::error::Result<VerificationReport> {
    let paths: Vec<String> = i.files.iter().map(|f| f.path.clone()).collect();
    let agent_instruction_files = paths.iter().filter(|p| is_agent_instruction_file(p)).cloned().collect();
    let scope = contract::check_scope(i.contract, &paths)?;
    let scope_configured = !i.contract.in_scope.is_empty() || !i.contract.out_of_scope.is_empty();

    let evidence: HashMap<String, RunEvidence> = i
        .latest_runs
        .iter()
        .map(|(k, r)| (k.clone(), RunEvidence { check_id: r.check_id.clone(), run_id: r.id.clone(), state: r.state, fingerprint: r.fingerprint.clone(), spec_rev: r.spec_rev.clone(), mutated: r.mutated }))
        .collect();
    let manual: HashMap<String, ManualVerdict> = i
        .verdicts
        .iter()
        .map(|(k, v)| (k.clone(), ManualVerdict { pass: v.pass, note: v.note.clone(), fingerprint: v.fingerprint.clone(), criterion_rev: v.criterion_rev.clone() }))
        .collect();
    let check_revs: HashMap<String, String> = i.contract.checks.iter().map(|c| (c.id.clone(), c.revision())).collect();
    let ctx = EvalContext { runs: &evidence, check_revs: &check_revs, manual: &manual, fingerprint: &i.fingerprint, scope: &scope };

    let criteria: Vec<CriterionReport> = i
        .contract
        .criteria
        .iter()
        .map(|c: &Criterion| CriterionReport {
            id: c.id.clone(),
            text: c.text.clone(),
            status: contract::evaluate(c, &ctx),
            manual: c.manual,
            scope_rule: c.scope_rule,
            verified_by: c.verified_by.clone(),
            manual_verdict: i.verdicts.get(&c.id).cloned(),
        })
        .collect();

    let checks: Vec<CheckReport> = i
        .contract
        .checks
        .iter()
        .map(|spec| {
            let latest = i.latest_runs.get(&spec.id).cloned();
            let fresh = latest.as_ref().is_some_and(|r| r.fingerprint == i.fingerprint && r.state != RunState::Running && !r.mutated && r.spec_rev == spec.revision());
            let touched_inputs = touched_inputs(spec, &i.files);
            CheckReport { spec: spec.clone(), latest, fresh, runs: i.run_counts.get(&spec.id).copied().unwrap_or(0), touched_inputs }
        })
        .collect();

    let blocking_checks = contract::blocking_checks(i.contract, &evidence, &i.fingerprint);

    let mut unresolved = vec![];
    for c in &criteria {
        match &c.status {
            CriterionStatus::NotChecked => unresolved.push(format!("Criterion \"{}\": not checked (nothing verifies it)", c.text)),
            CriterionStatus::CheckPending { pending } => unresolved.push(format!("Criterion \"{}\": check pending ({})", c.text, pending.join(", "))),
            CriterionStatus::Failed { reason } => unresolved.push(format!("Criterion \"{}\": FAILED — {reason}", c.text)),
            CriterionStatus::Inconclusive { reason } => unresolved.push(format!("Criterion \"{}\": inconclusive — {reason}", c.text)),
            CriterionStatus::RequiresManualReview { reason } => unresolved.push(format!("Criterion \"{}\": requires manual review — {reason}", c.text)),
            CriterionStatus::VerifiedByChecks { .. } | CriterionStatus::VerifiedManually { .. } | CriterionStatus::VerifiedByScopeCheck => {}
        }
    }
    for name in &blocking_checks {
        unresolved.push(format!("Required check \"{name}\" is not passing on the current changes"));
    }
    if scope_configured && !scope.clean() {
        unresolved.push(format!(
            "Scope: {} file(s) outside the contract scope, {} in excluded areas",
            scope.outside_in_scope.len(),
            scope.in_out_of_scope.len()
        ));
    }
    for c in &checks {
        if !c.touched_inputs.is_empty() {
            unresolved.push(format!("Check \"{}\" may be affected by changes to files that define or configure it ({}); a passing result is weaker evidence", c.spec.name, c.touched_inputs.iter().take(4).cloned().collect::<Vec<_>>().join(", ")));
        }
        if c.latest.as_ref().is_some_and(|r| r.spec_rev != c.spec.revision()) {
            unresolved.push(format!("Check \"{}\" was edited after its last run; that result no longer counts", c.spec.name));
        }
        if c.latest.as_ref().is_some_and(|r| r.mutated) {
            unresolved.push(format!("Check \"{}\" modified files while running, so its result does not match the current changes", c.spec.name));
        }
    }
    unresolved.dedup();

    let mut hard_blockers = vec![];
    if i.agent_running {
        hard_blockers.push("The agent is still running".to_string());
    }
    if i.pending_approvals > 0 {
        hard_blockers.push(format!("{} permission request(s) are waiting for an answer", i.pending_approvals));
    }
    if checks.iter().any(|c| c.latest.as_ref().is_some_and(|r| r.state == RunState::Running)) {
        hard_blockers.push("A verification check is still running".to_string());
    }

    Ok(VerificationReport {
        contract_sha256: i.contract.revision(),
        fingerprint: i.fingerprint,
        head_commit: i.head_commit,
        files: i.files,
        agent_instruction_files,
        scope,
        scope_configured,
        criteria,
        checks,
        blocking_checks,
        unresolved,
        hard_blockers,
        contract: i.contract.clone(),
    })
}

/// Conservative path-based identification; renames are represented as delete + add by changed_files.
pub fn is_agent_instruction_file(path: &str) -> bool {
    let p = path.replace('\\', "/").to_lowercase();
    let parts: Vec<&str> = p.split('/').collect();
    let name = parts.last().copied().unwrap_or("");
    matches!(name, "claude.md" | "claude.local.md" | "agents.md" | "agents.override.md" | "gemini.md" | ".mcp.json")
        || parts.iter().any(|part| matches!(*part, ".claude" | ".codex" | ".gemini" | ".cursor"))
        || parts.windows(2).any(|p| p == [".agents", "skills"])
        || p == ".github/copilot-instructions.md"
        || p.starts_with(".github/instructions/")
        || p.starts_with(".github/agents/")
        || p.starts_with(".github/prompts/")
}

/// Files the agent changed that define or configure a check: the check's own command tokens, build/test manifests and CI.
pub fn touched_inputs(spec: &CheckSpec, files: &[ChangedFile]) -> Vec<String> {
    const DEFINITIONS: &[&str] = &["package.json", "cargo.toml", "makefile", "justfile", "pyproject.toml", "pytest.ini", "tox.ini", "setup.cfg", "go.mod", "build.gradle", "pom.xml", "noxfile.py", "conftest.py"];
    let tokens: Vec<String> = spec.command.split(|c: char| c.is_whitespace() || "\"'=(),;".contains(c)).map(|t| t.trim_start_matches("./").replace('\\', "/").to_lowercase()).filter(|t| t.len() > 2).collect();
    let mut out = vec![];
    for f in files {
        let path = f.path.replace('\\', "/");
        let lower = path.to_lowercase();
        let name = lower.rsplit('/').next().unwrap_or("");
        let config_like = name.contains(".config.") || name.starts_with(".eslintrc") || name.starts_with("jest.config") || name.starts_with("vitest.config") || name.starts_with("tsconfig");
        // Only SCRIPT-like files named in the command count: `node scripts/check.js` is defined by that file, whereas a path
        // the command merely inspects (`existsSync('out.txt')`) is the subject of the check, not part of its definition.
        let script_like = [".js", ".mjs", ".cjs", ".ts", ".py", ".sh", ".ps1", ".bat", ".cmd", ".rb", ".pl"].iter().any(|e| name.ends_with(e));
        if DEFINITIONS.contains(&name) || config_like || lower.starts_with(".github/workflows/") || (script_like && tokens.iter().any(|t| t == &lower || t == name)) {
            out.push(path);
        }
    }
    out
}

pub struct PackInput<'a> {
    pub mission: &'a Mission,
    pub project_name: &'a str,
    pub report: &'a VerificationReport,
    pub runs: &'a [VerificationRun],
    pub sessions: &'a [AgentSession],
    pub approvals: &'a [Approval],
    pub decisions: &'a [ReviewDecisionRow],
    pub diff_sha256: &'a str,
    pub diff_bytes: usize,
    pub diff_path: Option<&'a str>,
    pub generated_at: &'a str,
}

/// Immutable snapshot of what happened and what is established. Returns the JSON content and a Markdown rendering.
pub fn build_pack(i: PackInput) -> (Value, String) {
    let m = i.mission;
    let policy_denials = i.approvals.iter().filter(|a| a.state == "auto_denied").count();
    let content = json!({
        "version": 1,
        "generatedAt": i.generated_at,
        "mission": {
            "id": m.id, "title": m.title, "objective": m.objective, "project": i.project_name,
            "provider": m.provider, "model": m.model, "permissionProfile": m.permission_profile,
            "branch": m.branch, "baseRef": m.base_ref, "baseCommit": m.base_commit,
            "headCommit": i.report.head_commit, "state": m.state,
        },
        "changesFingerprint": i.report.fingerprint,
        "contract": i.report.contract,
        "sessions": i.sessions.iter().map(|s| json!({
            "id": s.id, "providerSessionId": s.provider_session_id, "state": s.state, "startedAt": s.started_at,
            "endedAt": s.ended_at, "endReason": s.end_reason, "usage": s.usage,
            // Where it ran, only for a session that actually started (a session refused at launch ran nowhere).
            "ranOn": s.pid.map(|_| s.executes_on.clone()),
            "usageNote": "token and cost figures are provider-reported estimates, not billing",
        })).collect::<Vec<_>>(),
        "changes": {
            "files": i.report.files, "agentInstructionFiles": i.report.agent_instruction_files, "diffSha256": i.diff_sha256, "diffBytes": i.diff_bytes, "diffPath": i.diff_path,
        },
        "scope": { "configured": i.report.scope_configured, "report": i.report.scope },
        "criteria": i.report.criteria,
        "checks": i.report.checks.iter().map(|c| json!({
            "id": c.spec.id, "name": c.spec.name, "command": c.spec.command, "kind": c.spec.kind, "required": c.spec.required,
            "fresh": c.fresh, "runs": c.runs,
            "latest": c.latest.as_ref().map(|r| json!({
                "runId": r.id, "state": r.state, "exitCode": r.exit_code, "durationMs": r.duration_ms,
                "ranAgainst": r.fingerprint, "mutatedWorktree": r.mutated, "startedAt": r.started_at, "logPath": r.log_path,
                "backend": r.backend,
                "evidence": r.evidence,
            })),
            "adapter": c.spec.adapter,
        })).collect::<Vec<_>>(),
        "permissions": {
            "total": i.approvals.len(), "policyDenials": policy_denials,
            "items": i.approvals.iter().map(|a| json!({
                "tool": a.tool, "summary": a.summary, "risk": a.risk, "state": a.state, "decidedBy": a.decided_by, "note": a.decision_note,
            })).collect::<Vec<_>>(),
        },
        "reviewDecisions": i.decisions,
        "unresolved": i.report.unresolved,
        "allRunsRecorded": i.runs.len(),
    });
    let md = render_markdown(&content);
    (content, md)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn status_label(s: &Value) -> String {
    match s.get("status").and_then(Value::as_str).unwrap_or("?") {
        "not_checked" => "Not checked".into(),
        "check_pending" => "Check pending".into(),
        "verified_by_checks" => format!("Verified by checks: {}", s["evidence"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")).unwrap_or_default()),
        "verified_manually" => "Verified manually by a human".into(),
        "verified_by_scope_check" => "Verified by scope check".into(),
        "failed" => format!("FAILED — {}", s["reason"].as_str().unwrap_or("")),
        "inconclusive" => format!("Inconclusive — {}", s["reason"].as_str().unwrap_or("")),
        "requires_manual_review" => format!("Requires manual review — {}", s["reason"].as_str().unwrap_or("")),
        other => other.into(),
    }
}

/// Where an agent session ran, in words.
fn agent_place(executes_on: &str) -> String {
    match executes_on {
        "agent_host" => "on the host, as the user (not isolated)".to_string(),
        other => crate::execution::Backend::parse(other).map(|b| b.label().to_string()).unwrap_or_else(|_| other.to_string()),
    }
}

fn cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

pub fn render_markdown(c: &Value) -> String {
    let mut o = String::new();
    let m = &c["mission"];
    o.push_str(&format!("# Proof Pack — {}\n\n", m["title"].as_str().unwrap_or("")));
    o.push_str(&format!(
        "Project **{}** · branch `{}` · base `{}` · state **{}**  \nProvider {} {} · generated {}\n\n",
        m["project"].as_str().unwrap_or(""),
        m["branch"].as_str().unwrap_or("-"),
        m["baseCommit"].as_str().map(|s| &s[..s.len().min(10)]).unwrap_or("-"),
        m["state"].as_str().unwrap_or(""),
        m["provider"].as_str().unwrap_or(""),
        m["model"].as_str().unwrap_or(""),
        c["generatedAt"].as_str().unwrap_or("")
    ));
    let ran: Vec<String> = {
        let mut v: Vec<String> = vec![];
        for p in c["sessions"].as_array().into_iter().flatten().filter_map(|s| s["ranOn"].as_str()).map(agent_place) {
            if !v.contains(&p) {
                v.push(p);
            }
        }
        v
    };
    if !ran.is_empty() {
        o.push_str(&format!("Agent ran: {}\n\n", ran.join("; ")));
    }
    o.push_str("> This records evidence, not a guarantee. A passing check is evidence for the criterion it is linked to; it does not prove correctness. Token and cost figures are provider estimates.\n\n");
    o.push_str(&format!("## Objective\n\n{}\n\n", m["objective"].as_str().unwrap_or("")));

    o.push_str("## Not established\n\n");
    let un = c["unresolved"].as_array().cloned().unwrap_or_default();
    if un.is_empty() {
        o.push_str("Nothing outstanding: every criterion and required check is established on the current changes.\n\n");
    } else {
        for u in un {
            o.push_str(&format!("- {}\n", u.as_str().unwrap_or("")));
        }
        o.push('\n');
    }

    o.push_str("## Acceptance criteria\n\n");
    let crit = c["criteria"].as_array().cloned().unwrap_or_default();
    if crit.is_empty() {
        o.push_str("_No acceptance criteria were defined for this mission._\n\n");
    } else {
        o.push_str("| Criterion | Status |\n|---|---|\n");
        for k in crit {
            o.push_str(&format!("| {} | {} |\n", cell(k["text"].as_str().unwrap_or("")), cell(&status_label(&k["status"]))));
        }
        o.push('\n');
    }

    o.push_str("## Checks\n\n");
    let checks = c["checks"].as_array().cloned().unwrap_or_default();
    if checks.is_empty() {
        o.push_str("_No checks were defined._\n\n");
    } else {
        o.push_str("| Check | Command | Result | Ran in | Fresh |\n|---|---|---|---|---|\n");
        for k in checks {
            let res = match k["latest"].as_object() {
                None => "never run".to_string(),
                Some(l) => format!("{} (exit {}, {} ms){}", l["state"].as_str().unwrap_or("?"), l["exitCode"], l["durationMs"], if l["mutatedWorktree"] == true { ", modified worktree" } else { "" }),
            };
            o.push_str(&format!(
                "| {}{} | `{}` | {} | {} | {} |\n",
                cell(k["name"].as_str().unwrap_or("")),
                if k["required"] == true { " (required)" } else { "" },
                cell(k["command"].as_str().unwrap_or("")),
                cell(&res),
                match k["latest"]["backend"].as_str() {
                    Some(b) => crate::execution::Backend::parse(b).map(|b| b.label()).unwrap_or(b),
                    None => "—",
                },
                if k["fresh"] == true { "yes" } else { "no" }
            ));
        }
        o.push('\n');
        // Structured findings from verification adapters (secret scans, dependency audits, end-to-end tests).
        for k in c["checks"].as_array().cloned().unwrap_or_default() {
            let ev = &k["latest"]["evidence"];
            if !ev.is_object() {
                continue;
            }
            o.push_str(&format!("### {} — {} ({})

", cell(k["name"].as_str().unwrap_or("")), ev["adapter"].as_str().unwrap_or(""), cell(ev["summary"].as_str().unwrap_or(""))));
            let findings = ev["findings"].as_array().cloned().unwrap_or_default();
            if !findings.is_empty() {
                o.push_str("| Severity | Blocking | Rule | Title | Location | Package |\n|---|---|---|---|---|---|\n");
                for f in &findings {
                    o.push_str(&format!(
                        "| {} | {} | {} | {} | {} | {} |\n",
                        f["severity"].as_str().unwrap_or(""),
                        if f["blocking"] == true { "yes" } else { "no" },
                        cell(f["ruleId"].as_str().unwrap_or("")),
                        cell(f["title"].as_str().unwrap_or("")),
                        cell(f["location"].as_str().unwrap_or("")),
                        cell(f["package"].as_str().unwrap_or("")),
                    ));
                }
                if ev["findingsTruncated"] == true {
                    o.push_str(&format!("\n_{} findings in total; the first {} are listed._\n", ev["totalFindings"], findings.len()));
                }
                o.push('\n');
            }
        }
    }

    o.push_str("## Changes\n\n");
    let sensitive = c["changes"]["agentInstructionFiles"].as_array().cloned().unwrap_or_default();
    if !sensitive.is_empty() {
        o.push_str("**Agent instruction/configuration changes:** These files can affect future agent behavior. Review their changes in the accompanying diff.\n\n");
        for p in sensitive { o.push_str(&format!("- `{}`\n", p.as_str().unwrap_or(""))); }
        o.push('\n');
    }
    let files = c["changes"]["files"].as_array().cloned().unwrap_or_default();
    o.push_str(&format!("{} file(s) changed · diff sha256 `{}`\n\n", files.len(), c["changes"]["diffSha256"].as_str().unwrap_or("")));
    for f in files {
        o.push_str(&format!("- `{}` (+{} −{}){}\n", f["path"].as_str().unwrap_or(""), f["added"].as_u64().unwrap_or(0), f["removed"].as_u64().unwrap_or(0), if f["binary"] == true { " binary" } else { "" }));
    }
    o.push('\n');

    let sc = &c["scope"];
    if sc["configured"] == true {
        o.push_str("## Scope\n\n");
        let out = sc["report"]["outsideInScope"].as_array().cloned().unwrap_or_default();
        let ex = sc["report"]["inOutOfScope"].as_array().cloned().unwrap_or_default();
        if out.is_empty() && ex.is_empty() {
            o.push_str("All changed files are within the contract scope.\n\n");
        } else {
            for p in out {
                o.push_str(&format!("- outside scope: `{}`\n", p.as_str().unwrap_or("")));
            }
            for p in ex {
                o.push_str(&format!("- in excluded area: `{}`\n", p.as_str().unwrap_or("")));
            }
            o.push('\n');
        }
    }

    o.push_str(&format!("## Permissions\n\n{} request(s), {} blocked by policy.\n\n", c["permissions"]["total"], c["permissions"]["policyDenials"]));
    for p in c["permissions"]["items"].as_array().cloned().unwrap_or_default() {
        o.push_str(&format!("- {} — {} ({}, {})\n", p["tool"].as_str().unwrap_or(""), p["summary"].as_str().unwrap_or(""), p["risk"].as_str().unwrap_or(""), p["state"].as_str().unwrap_or("")));
    }
    o.push('\n');

    o.push_str("## Review\n\n");
    let ds = c["reviewDecisions"].as_array().cloned().unwrap_or_default();
    if ds.is_empty() {
        o.push_str("_No review decision recorded yet._\n");
    }
    for d in ds {
        if d["agentInstructionsReviewed"] == true {
            o.push_str("- Explicitly acknowledged agent instruction/configuration changes for this review fingerprint.\n");
        }
        o.push_str(&format!(
            "- {} at {}{}{}\n",
            d["decision"].as_str().unwrap_or(""),
            d["at"].as_str().unwrap_or(""),
            d["note"].as_str().map(|n| format!(" — {n}")).unwrap_or_default(),
            d["overrideReason"].as_str().map(|n| format!(" (approved with unresolved items; reason: {n})")).unwrap_or_default()
        ));
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::CheckKind;

    #[test]
    fn agent_instruction_paths_include_nested_and_case_variants() {
        for path in ["AGENTS.md", "src/AGENTS.override.md", "src/CLAUDE.md", ".claude/rules/review.md", "nested/.codex/config.toml", ".agents/skills/release/SKILL.md", "nested/.AGENTS/skills/release/scripts/run.sh", ".github/copilot-instructions.md", ".github/instructions/a.md", ".github/agents/reviewer.md", ".mcp.json", "x\\.gemini\\settings.json"] {
            assert!(is_agent_instruction_file(path), "{path}");
        }
        for path in ["src/agents.rs", "docs/claude.md.txt", ".github/workflows/ci.yml", "claude/file.md"] {
            assert!(!is_agent_instruction_file(path), "{path}");
        }
        let r = report(vec![], vec![], &["AGENTS.md", "src/main.rs"], 0, false);
        assert_eq!(r.agent_instruction_files, vec!["AGENTS.md"]);
    }

    fn contract() -> Contract {
        Contract {
            version: 1,
            in_scope: vec!["src/".into()],
            checks: vec![
                CheckSpec { id: "t".into(), name: "Tests".into(), command: "npm test".into(), kind: CheckKind::Test, timeout_secs: 60, required: true, adapter: None },
                CheckSpec { id: "l".into(), name: "Lint".into(), command: "npm run lint".into(), kind: CheckKind::Lint, timeout_secs: 60, required: false, adapter: None },
            ],
            criteria: vec![
                Criterion { id: "exp".into(), text: "Reset tokens expire".into(), verified_by: vec!["t".into()], manual: false, scope_rule: false },
                Criterion { id: "enum".into(), text: "No account enumeration".into(), verified_by: vec![], manual: true, scope_rule: false },
                Criterion { id: "scope".into(), text: "No unrelated modifications".into(), verified_by: vec![], manual: false, scope_rule: true },
                Criterion { id: "doc".into(), text: "Docs updated".into(), verified_by: vec![], manual: false, scope_rule: false },
            ],
            ..Default::default()
        }
    }

    fn vrun(check: &str, state: RunState, fp: &str, mutated: bool) -> VerificationRun {
        VerificationRun { id: format!("{check}-run-123456"), mission_id: "m".into(), check_id: check.into(), name: check.into(), command: "x".into(), kind: "test".into(), state, exit_code: Some(0), started_at: "t".into(), ended_at: None, duration_ms: Some(10), fingerprint: fp.into(), head_commit: None, mutated, output_tail: String::new(), output_truncated: false, log_path: None, spec_rev: contract().checks.iter().find(|c| c.id == check).map(|c| c.revision()).unwrap_or_default(), backend: "trusted_local".into(), evidence: None }
    }

    fn report(runs: Vec<VerificationRun>, verdicts: Vec<ManualVerdictRow>, files: &[&str], pending: usize, running: bool) -> VerificationReport {
        let c = contract();
        let latest: HashMap<_, _> = runs.into_iter().map(|r| (r.check_id.clone(), r)).collect();
        let counts: HashMap<_, _> = latest.keys().map(|k| (k.clone(), 1)).collect();
        let v: HashMap<_, _> = verdicts.into_iter().map(|x| (x.criterion_id.clone(), x)).collect();
        build_report(ReportInput {
            contract: &c, latest_runs: &latest, run_counts: &counts, verdicts: &v, fingerprint: "F".into(), head_commit: Some("abc".into()),
            files: files.iter().map(|p| ChangedFile { path: p.to_string(), added: Some(1), removed: Some(0), binary: false }).collect(),
            pending_approvals: pending, agent_running: running,
        })
        .unwrap()
    }

    fn status<'a>(r: &'a VerificationReport, id: &str) -> &'a CriterionStatus {
        &r.criteria.iter().find(|c| c.id == id).unwrap().status
    }

    #[test]
    fn the_password_reset_example_reports_exactly_what_is_and_is_not_established() {
        let r = report(vec![vrun("t", RunState::Passed, "F", false)], vec![], &["src/auth/reset.rs", "src/auth/reset_test.rs"], 0, false);
        assert!(matches!(status(&r, "exp"), CriterionStatus::VerifiedByChecks { .. }), "linked test passed on current changes");
        assert!(matches!(status(&r, "enum"), CriterionStatus::RequiresManualReview { .. }));
        assert!(matches!(status(&r, "scope"), CriterionStatus::VerifiedByScopeCheck));
        assert!(matches!(status(&r, "doc"), CriterionStatus::NotChecked), "unlinked criterion is NOT assumed fine");
        assert!(r.unresolved.iter().any(|u| u.contains("No account enumeration")) && r.unresolved.iter().any(|u| u.contains("Docs updated")));
        assert!(r.needs_override() && r.can_approve());
        assert!(r.blocking_checks.is_empty());
    }

    #[test]
    fn a_failing_or_stale_required_check_blocks_and_is_listed() {
        let failing = report(vec![vrun("t", RunState::Failed, "F", false)], vec![], &["src/a.rs"], 0, false);
        assert!(matches!(status(&failing, "exp"), CriterionStatus::Failed { .. }));
        assert_eq!(failing.blocking_checks, vec!["Tests"]);
        let stale = report(vec![vrun("t", RunState::Passed, "OLD", false)], vec![], &["src/a.rs"], 0, false);
        assert!(matches!(status(&stale, "exp"), CriterionStatus::Inconclusive { .. }));
        assert_eq!(stale.blocking_checks, vec!["Tests"]);
        assert!(!stale.checks[0].fresh);
    }

    #[test]
    fn scope_violations_fail_the_scope_criterion() {
        let r = report(vec![vrun("t", RunState::Passed, "F", false)], vec![], &["src/a.rs", "package-lock.json"], 0, false);
        assert!(matches!(status(&r, "scope"), CriterionStatus::Failed { .. }));
        assert!(r.unresolved.iter().any(|u| u.starts_with("Scope:")));
    }

    #[test]
    fn hard_blockers_cannot_be_overridden() {
        let r = report(vec![vrun("t", RunState::Running, "F", false)], vec![], &["src/a.rs"], 2, true);
        assert!(!r.can_approve());
        assert_eq!(r.hard_blockers.len(), 3, "{:?}", r.hard_blockers);
    }

    #[test]
    fn a_check_that_dirtied_the_worktree_is_called_out() {
        let r = report(vec![vrun("t", RunState::Passed, "F", true)], vec![], &["src/a.rs"], 0, false);
        assert!(r.unresolved.iter().any(|u| u.contains("modified files while running")));
    }

    #[test]
    fn fully_established_mission_has_nothing_unresolved() {
        let mut c = contract();
        c.criteria.retain(|k| k.id == "exp" || k.id == "scope");
        c.checks.truncate(1);
        let latest: HashMap<_, _> = [("t".to_string(), vrun("t", RunState::Passed, "F", false))].into();
        let counts = HashMap::new();
        let v = HashMap::new();
        let r = build_report(ReportInput { contract: &c, latest_runs: &latest, run_counts: &counts, verdicts: &v, fingerprint: "F".into(), head_commit: None, files: vec![ChangedFile { path: "src/x.rs".into(), added: Some(1), removed: Some(0), binary: false }], pending_approvals: 0, agent_running: false }).unwrap();
        assert!(r.unresolved.is_empty(), "{:?}", r.unresolved);
        assert!(!r.needs_override() && r.can_approve());
    }

    #[test]
    fn lightweight_missions_without_a_contract_are_not_blocked() {
        let c = Contract::default();
        let (l, n, v) = (HashMap::new(), HashMap::new(), HashMap::new());
        let r = build_report(ReportInput { contract: &c, latest_runs: &l, run_counts: &n, verdicts: &v, fingerprint: "F".into(), head_commit: None, files: vec![], pending_approvals: 0, agent_running: false }).unwrap();
        assert!(r.can_approve() && !r.needs_override());
    }

    #[test]
    fn markdown_states_evidence_honestly_and_contains_no_scores() {
        let r = report(vec![vrun("t", RunState::Passed, "F", false)], vec![], &["src/auth/reset.rs", "AGENTS.md"], 0, false);
        let m = Mission { id: "m1".into(), project_id: "p".into(), title: "Implement password reset".into(), objective: "Do it".into(), state: "awaiting_review".into(), provider: "claude".into(), model: None, permission_profile: "default".into(), base_ref: Some("main".into()), base_commit: Some("0123456789abcdef".into()), branch: Some("forge/x".into()), worktree_path: None, provider_session_id: None, last_error: None, created_at: "t".into(), updated_at: "t".into(), use_memory: true, memory_pins: vec![] };
        let (content, md) = build_pack(PackInput { mission: &m, project_name: "demo", report: &r, runs: &[], sessions: &[], approvals: &[], decisions: &[], diff_sha256: "deadbeef", diff_bytes: 10, diff_path: Some("/x.diff"), generated_at: "2026-10-23T00:00:00Z" });
        assert!(md.contains("# Proof Pack — Implement password reset"));
        assert!(md.contains("evidence, not a guarantee"));
        assert!(md.contains("Verified by checks") && md.contains("Not checked") && md.contains("Requires manual review"));
        assert!(md.contains("## Not established"));
        assert!(!md.to_lowercase().contains("confidence") && !md.contains('%'), "no invented scores");
        assert_eq!(content["changes"]["diffSha256"], "deadbeef");
        assert_eq!(content["changes"]["agentInstructionFiles"][0], "AGENTS.md");
        assert!(md.contains("future agent behavior") && md.contains("AGENTS.md"));
        assert_eq!(content["mission"]["baseCommit"], "0123456789abcdef");
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn markdown_escapes_table_breaking_characters() {
        assert_eq!(cell("a|b\nc"), "a\\|b c");
    }
}
