//! Verification adapters: turn the machine-readable output of known tools into structured evidence (findings with severity
//! and location, and an outcome category), instead of a pass/fail exit code and a log.
//!
//! The command stays user-authored (FORGE suggests one per adapter); the adapter only reads the tool's **stdout**, captured
//! separately from stderr, redacted line by line and capped. Secret values are never stored: gitleaks findings keep the rule,
//! file and line, not the match. Outcome rules are deliberately strict: output that cannot be parsed, or that contradicts the
//! exit code, is a tool error (not a pass), and a tool that is not installed is reported as missing (not as a failure of the
//! code under test).

use crate::contract::RunState;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Findings kept per run; the counts always cover all of them.
pub const MAX_FINDINGS: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// Parsed, nothing at or above the adapter's threshold.
    Passed,
    /// Parsed, with blocking findings (vulnerabilities, secrets, failing tests).
    Findings,
    /// The tool is not installed or not on PATH.
    ToolMissing,
    /// The tool ran but its output could not be trusted (unparseable, or inconsistent with its exit code).
    ToolError,
    /// The run did not complete (timeout, cancel, runner error); no evidence.
    Incomplete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Low,
    Medium,
    High,
    Critical,
    /// The tool did not say; treated as blocking.
    Unknown,
}

impl Severity {
    fn parse(s: &str) -> Severity {
        match s.trim().to_ascii_lowercase().as_str() {
            "critical" => Severity::Critical,
            "high" => Severity::High,
            "medium" | "moderate" => Severity::Medium,
            "low" => Severity::Low,
            "info" | "negligible" | "none" => Severity::Info,
            _ => Severity::Unknown,
        }
    }
    fn blocks_at(self, threshold: Severity) -> bool {
        self == Severity::Unknown || self >= threshold
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub rule_id: String,
    pub severity: Severity,
    pub title: String,
    /// `path` or `path:line`
    pub location: Option<String>,
    /// Package name@version for dependency findings.
    pub package: Option<String>,
    pub fixed_in: Option<String>,
    pub blocking: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Evidence {
    pub adapter: String,
    pub outcome: Outcome,
    pub summary: String,
    pub total_findings: usize,
    pub blocking_findings: usize,
    pub by_severity: std::collections::BTreeMap<String, usize>,
    pub findings: Vec<Finding>,
    pub findings_truncated: bool,
}

pub struct AdapterInfo {
    pub id: &'static str,
    pub label: &'static str,
    /// Suggested command (stdout must carry the JSON report). Validated against each tool's documentation; see docs.
    pub command: &'static str,
    pub threshold: Severity,
    pub kind: crate::contract::CheckKind,
}

pub const ADAPTERS: &[AdapterInfo] = &[
    AdapterInfo { id: "gitleaks", label: "Secret scan (gitleaks)", command: "gitleaks dir . --redact --no-banner --report-format json --report-path -", threshold: Severity::Info, kind: crate::contract::CheckKind::Security },
    AdapterInfo { id: "osv-scanner", label: "Dependency vulnerabilities (OSV-Scanner)", command: "osv-scanner scan source --format json -r .", threshold: Severity::Info, kind: crate::contract::CheckKind::Security },
    AdapterInfo { id: "trivy", label: "Vulnerabilities, secrets, misconfiguration (Trivy)", command: "trivy fs --quiet --format json --scanners vuln,secret,misconfig .", threshold: Severity::High, kind: crate::contract::CheckKind::Security },
    AdapterInfo { id: "playwright", label: "End-to-end tests (Playwright)", command: "npx playwright test --reporter=json", threshold: Severity::Info, kind: crate::contract::CheckKind::Test },
];

pub fn adapter(id: &str) -> Option<&'static AdapterInfo> {
    ADAPTERS.iter().find(|a| a.id == id)
}

/// Exit codes shells use for "command not found" (sh: 127; cmd.exe: 9009), or the matching messages.
fn tool_missing(exit: Option<i32>, stderr_tail: &str) -> bool {
    matches!(exit, Some(127) | Some(9009))
        || stderr_tail.contains("is not recognized as an internal or external command")
        || (stderr_tail.contains("not found") && (stderr_tail.contains("command not found") || stderr_tail.contains(": not found")))
}

/// The first JSON value in `text` (tools sometimes print a line before it).
fn json_in(text: &str) -> Option<Value> {
    let t = text.trim();
    if let Ok(v) = serde_json::from_str(t) {
        return Some(v);
    }
    let start = t.find(['{', '['])?;
    let mut de = serde_json::Deserializer::from_str(&t[start..]).into_iter::<Value>();
    de.next()?.ok()
}

fn s(v: &Value, k: &str) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or("").to_string()
}

fn gitleaks(v: &Value) -> Option<Vec<Finding>> {
    let arr = v.as_array()?;
    Some(
        arr.iter()
            .map(|f| {
                let line = f.get("StartLine").and_then(Value::as_u64);
                // Never `Secret` / `Match`: even redacted, they are not evidence FORGE needs to keep.
                Finding {
                    rule_id: s(f, "RuleID"),
                    severity: Severity::High,
                    title: s(f, "Description"),
                    location: Some(match line {
                        Some(l) => format!("{}:{l}", s(f, "File")),
                        None => s(f, "File"),
                    }),
                    package: None,
                    fixed_in: None,
                    blocking: true,
                }
            })
            .collect(),
    )
}

fn osv(v: &Value) -> Option<Vec<Finding>> {
    let results = v.get("results")?.as_array()?;
    let mut out = vec![];
    for r in results {
        let path = r.get("source").map(|src| s(src, "path")).unwrap_or_default();
        for p in r.get("packages").and_then(Value::as_array).into_iter().flatten() {
            let pkg = p.get("package").cloned().unwrap_or(Value::Null);
            let package = format!("{}@{}", s(&pkg, "name"), s(&pkg, "version"));
            // Groups carry the aggregated severity (CVSS score as a string) for aliases of one vulnerability.
            let groups: Vec<&Value> = p.get("groups").and_then(Value::as_array).map(|g| g.iter().collect()).unwrap_or_default();
            for vuln in p.get("vulnerabilities").and_then(Value::as_array).into_iter().flatten() {
                let id = s(vuln, "id");
                let score = groups
                    .iter()
                    .find(|g| g.get("ids").and_then(Value::as_array).is_some_and(|ids| ids.iter().any(|i| i.as_str() == Some(id.as_str()))))
                    .and_then(|g| g.get("max_severity"))
                    .and_then(|m| m.as_str().and_then(|x| x.parse::<f64>().ok()).or_else(|| m.as_f64()));
                let severity = match score {
                    Some(x) if x >= 9.0 => Severity::Critical,
                    Some(x) if x >= 7.0 => Severity::High,
                    Some(x) if x >= 4.0 => Severity::Medium,
                    Some(x) if x > 0.0 => Severity::Low,
                    _ => vuln.get("database_specific").map(|d| Severity::parse(&s(d, "severity"))).unwrap_or(Severity::Unknown),
                };
                out.push(Finding { rule_id: id, severity, title: s(vuln, "summary"), location: (!path.is_empty()).then(|| path.clone()), package: Some(package.clone()), fixed_in: None, blocking: false });
            }
        }
    }
    Some(out)
}

fn trivy(v: &Value) -> Option<Vec<Finding>> {
    let results = v.get("Results")?.as_array();
    let mut out = vec![];
    for r in results.into_iter().flatten() {
        let target = s(r, "Target");
        for x in r.get("Vulnerabilities").and_then(Value::as_array).into_iter().flatten() {
            out.push(Finding {
                rule_id: s(x, "VulnerabilityID"),
                severity: Severity::parse(&s(x, "Severity")),
                title: s(x, "Title"),
                location: Some(target.clone()),
                package: Some(format!("{}@{}", s(x, "PkgName"), s(x, "InstalledVersion"))),
                fixed_in: x.get("FixedVersion").and_then(Value::as_str).filter(|f| !f.is_empty()).map(String::from),
                blocking: false,
            });
        }
        for x in r.get("Secrets").and_then(Value::as_array).into_iter().flatten() {
            out.push(Finding { rule_id: s(x, "RuleID"), severity: Severity::parse(&s(x, "Severity")), title: s(x, "Title"), location: Some(format!("{target}:{}", x.get("StartLine").and_then(Value::as_u64).unwrap_or(0))), package: None, fixed_in: None, blocking: false });
        }
        for x in r.get("Misconfigurations").and_then(Value::as_array).into_iter().flatten() {
            if s(x, "Status") == "PASS" {
                continue;
            }
            out.push(Finding { rule_id: s(x, "ID"), severity: Severity::parse(&s(x, "Severity")), title: s(x, "Title"), location: Some(target.clone()), package: None, fixed_in: None, blocking: false });
        }
    }
    Some(out)
}

fn playwright(v: &Value) -> Option<Vec<Finding>> {
    let stats = v.get("stats")?;
    let mut out = vec![];
    fn walk(suite: &Value, path: &str, out: &mut Vec<Finding>) {
        let title = s(suite, "title");
        let here = if path.is_empty() { title } else { format!("{path} › {title}") };
        for spec in suite.get("specs").and_then(Value::as_array).into_iter().flatten() {
            for t in spec.get("tests").and_then(Value::as_array).into_iter().flatten() {
                if s(t, "status") == "unexpected" {
                    out.push(Finding {
                        rule_id: "unexpected".into(),
                        severity: Severity::High,
                        title: format!("{here} › {}", s(spec, "title")),
                        location: Some(format!("{}:{}", s(spec, "file"), spec.get("line").and_then(Value::as_u64).unwrap_or(0))),
                        package: Some(s(t, "projectName")).filter(|p| !p.is_empty()),
                        fixed_in: None,
                        blocking: true,
                    });
                }
            }
        }
        for child in suite.get("suites").and_then(Value::as_array).into_iter().flatten() {
            walk(child, &here, out);
        }
    }
    for suite in v.get("suites").and_then(Value::as_array).into_iter().flatten() {
        walk(suite, "", &mut out);
    }
    // The stats are authoritative for the count; a failure without a locatable spec is still a failure.
    let unexpected = stats.get("unexpected").and_then(Value::as_u64).unwrap_or(0) as usize;
    while out.len() < unexpected {
        out.push(Finding { rule_id: "unexpected".into(), severity: Severity::High, title: "failing test (not located in the report)".into(), location: None, package: None, fixed_in: None, blocking: true });
    }
    Some(out)
}

/// Turn a finished run into evidence and the run state FORGE records. `stdout` is the captured standard output; `tail` is the
/// combined output tail (used only to recognise a missing tool).
pub fn evaluate(adapter_id: &str, state: RunState, exit: Option<i32>, stdout: &str, tail: &str) -> (Evidence, RunState) {
    let info = adapter(adapter_id);
    let mk = |outcome: Outcome, summary: String, findings: Vec<Finding>| {
        let mut by = std::collections::BTreeMap::new();
        for f in &findings {
            *by.entry(serde_json::to_value(f.severity).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default()).or_insert(0) += 1;
        }
        let blocking = findings.iter().filter(|f| f.blocking).count();
        let total = findings.len();
        let truncated = total > MAX_FINDINGS;
        let mut kept = findings;
        kept.sort_by(|a, b| b.blocking.cmp(&a.blocking).then(b.severity.cmp(&a.severity)));
        kept.truncate(MAX_FINDINGS);
        Evidence { adapter: adapter_id.into(), outcome, summary, total_findings: total, blocking_findings: blocking, by_severity: by, findings: kept, findings_truncated: truncated }
    };
    let Some(info) = info else {
        return (mk(Outcome::ToolError, format!("unknown adapter \"{adapter_id}\""), vec![]), RunState::Error);
    };
    if !matches!(state, RunState::Passed | RunState::Failed) {
        return (mk(Outcome::Incomplete, "the run did not complete, so there is no evidence".into(), vec![]), state);
    }
    if exit != Some(0) && tool_missing(exit, tail) {
        return (mk(Outcome::ToolMissing, format!("{} is not installed or not on PATH for this backend", info.id), vec![]), RunState::Error);
    }
    let parsed = json_in(stdout).and_then(|v| match info.id {
        "gitleaks" => gitleaks(&v),
        "osv-scanner" => osv(&v),
        "trivy" => trivy(&v),
        "playwright" => playwright(&v),
        _ => None,
    });
    // gitleaks prints nothing (or `[]`) when clean with some flag combinations.
    let parsed = match parsed {
        None if info.id == "gitleaks" && exit == Some(0) && stdout.trim().is_empty() => Some(vec![]),
        p => p,
    };
    let Some(mut findings) = parsed else {
        return (mk(Outcome::ToolError, format!("{} ran (exit code {}) but its JSON report could not be read from standard output", info.id, exit.map_or("none".into(), |c| c.to_string())), vec![]), RunState::Error);
    };
    for f in &mut findings {
        f.blocking = f.blocking || f.severity.blocks_at(info.threshold);
    }
    let blocking = findings.iter().filter(|f| f.blocking).count();
    match (blocking, exit) {
        (0, Some(0)) => {
            let n = findings.len();
            (mk(Outcome::Passed, if n == 0 { "no findings".into() } else { format!("{n} finding(s) below the blocking threshold") }, findings), RunState::Passed)
        }
        (0, _) => (mk(Outcome::ToolError, format!("{} exited with code {} but reported no blocking findings; treating the result as untrustworthy", info.id, exit.map_or("none".into(), |c| c.to_string())), findings), RunState::Error),
        (n, _) => (mk(Outcome::Findings, format!("{n} blocking finding(s)"), findings), RunState::Failed),
    }
}

pub fn adapters_json() -> Value {
    Value::Array(ADAPTERS.iter().map(|a| json!({"id": a.id, "label": a.label, "command": a.command, "kind": a.kind})).collect())
}

#[cfg(test)]
mod tests;
