use super::*;

const GITLEAKS: &str = include_str!("../../fixtures/evidence/gitleaks-findings.json");
const OSV: &str = include_str!("../../fixtures/evidence/osv-findings.json");
const TRIVY: &str = include_str!("../../fixtures/evidence/trivy-findings.json");
const PLAYWRIGHT: &str = include_str!("../../fixtures/evidence/playwright-failures.json");

#[test]
fn gitleaks_findings_fail_the_check_and_never_keep_the_secret() {
    let (e, st) = evaluate("gitleaks", RunState::Failed, Some(1), GITLEAKS, "");
    assert_eq!((e.outcome, st, e.total_findings, e.blocking_findings), (Outcome::Findings, RunState::Failed, 2, 2));
    assert_eq!(e.findings[0].location.as_deref(), Some("src/config.ts:3"));
    let stored = serde_json::to_string(&e).unwrap();
    assert!(!stored.contains("Secret") && !stored.contains("token = "), "match/secret fields are not kept: {stored}");
}

#[test]
fn a_clean_gitleaks_run_passes() {
    for out in ["[]", ""] {
        let (e, st) = evaluate("gitleaks", RunState::Passed, Some(0), out, "");
        assert_eq!((e.outcome, st, e.total_findings), (Outcome::Passed, RunState::Passed, 0), "{out:?}");
    }
}

#[test]
fn osv_severity_comes_from_the_group_score_and_unknown_severity_blocks() {
    let (e, st) = evaluate("osv-scanner", RunState::Failed, Some(1), OSV, "");
    assert_eq!((e.outcome, st, e.total_findings), (Outcome::Findings, RunState::Failed, 3));
    let sev = |id: &str| e.findings.iter().find(|f| f.rule_id == id).unwrap().severity;
    assert_eq!(sev("GHSA-35jh-r3h4-6jhm"), Severity::High);
    assert_eq!(sev("GHSA-29mw-wpgm-hmr9"), Severity::Medium);
    assert_eq!(sev("OSV-2026-1"), Severity::Unknown);
    assert!(e.findings.iter().all(|f| f.blocking), "any known vulnerability blocks by default");
    assert_eq!(e.findings.iter().find(|f| f.rule_id == "OSV-2026-1").unwrap().package.as_deref(), Some("mystery@0.0.1"));
}

#[test]
fn trivy_blocks_on_high_and_lists_lower_findings_as_non_blocking() {
    // Trivy exits 0 by default even with findings; FORGE decides from the report.
    let (e, st) = evaluate("trivy", RunState::Passed, Some(0), TRIVY, "");
    assert_eq!((e.outcome, st), (Outcome::Findings, RunState::Failed));
    assert_eq!(e.total_findings, 4, "the PASS misconfiguration is not a finding");
    assert_eq!(e.blocking_findings, 3);
    let medium = e.findings.iter().find(|f| f.rule_id == "CVE-2020-28500").unwrap();
    assert!(!medium.blocking);
    assert_eq!(medium.fixed_in.as_deref(), Some("4.17.21"));
    assert_eq!(e.by_severity.get("high"), Some(&3));
}

#[test]
fn playwright_failures_are_located_and_flaky_tests_do_not_block() {
    let (e, st) = evaluate("playwright", RunState::Failed, Some(1), PLAYWRIGHT, "");
    assert_eq!((e.outcome, st, e.total_findings), (Outcome::Findings, RunState::Failed, 1));
    assert_eq!(e.findings[0].location.as_deref(), Some("tests/login.spec.ts:12"));
    assert!(e.findings[0].title.contains("shows an error for a wrong password"));
}

#[test]
fn a_missing_tool_is_reported_as_missing_not_as_failing_code() {
    let (e, st) = evaluate("gitleaks", RunState::Failed, Some(127), "", "sh: 1: gitleaks: not found");
    assert_eq!((e.outcome, st), (Outcome::ToolMissing, RunState::Error));
    let (e, _) = evaluate("osv-scanner", RunState::Failed, Some(1), "", "'osv-scanner' is not recognized as an internal or external command,");
    assert_eq!(e.outcome, Outcome::ToolMissing);
}

#[test]
fn unreadable_or_contradictory_output_is_never_a_pass() {
    // Exit 0 but garbage instead of a report.
    let (e, st) = evaluate("osv-scanner", RunState::Passed, Some(0), "Scanning... done", "");
    assert_eq!((e.outcome, st), (Outcome::ToolError, RunState::Error));
    // Non-zero exit but a clean report: something else went wrong.
    let (e, st) = evaluate("gitleaks", RunState::Failed, Some(2), "[]", "");
    assert_eq!((e.outcome, st), (Outcome::ToolError, RunState::Error));
    // Unknown adapter.
    assert_eq!(evaluate("nope", RunState::Passed, Some(0), "[]", "").0.outcome, Outcome::ToolError);
}

#[test]
fn a_report_after_a_preamble_line_is_still_read_and_incomplete_runs_have_no_evidence() {
    let (e, _) = evaluate("gitleaks", RunState::Failed, Some(1), &format!("scanning 12 files\n{GITLEAKS}"), "");
    assert_eq!(e.total_findings, 2);
    let (e, st) = evaluate("gitleaks", RunState::Timeout, None, "", "");
    assert_eq!((e.outcome, st), (Outcome::Incomplete, RunState::Timeout));
}

#[test]
fn findings_are_capped_but_counted() {
    let many: Vec<Value> = (0..250).map(|i| json!({"RuleID": "r", "Description": "d", "File": format!("f{i}"), "StartLine": i})).collect();
    let (e, _) = evaluate("gitleaks", RunState::Failed, Some(1), &serde_json::to_string(&many).unwrap(), "");
    assert_eq!((e.total_findings, e.findings.len(), e.findings_truncated), (250, MAX_FINDINGS, true));
}
