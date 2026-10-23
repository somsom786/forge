//! Verification adapters end to end: a check with an adapter runs through the real runner, its stdout report is parsed into
//! findings stored on the run, the adapter's verdict decides the run state, and the Proof Pack lists the findings.
//! The "tools" are node one-liners printing the fixture reports (the real tools are not installed in CI); see
//! fixtures/evidence/README.md.

use super::*;
use crate::contract::{CheckKind, CheckSpec, Contract, RunState};
use crate::evidence::Outcome;

fn print_fixture(name: &str, exit: i32) -> String {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/evidence").join(name);
    let p = dunce::canonicalize(p).unwrap().to_string_lossy().replace('\\', "/");
    format!("node -e \"process.stderr.write('scanning...\\n'); process.stdout.write(require('fs').readFileSync('{p}','utf8')); process.exit({exit})\"")
}

fn with_checks(f: &Fixture, m: &Mission, checks: Vec<CheckSpec>) {
    f.core.set_contract(&m.id, Contract { version: 1, checks, ..Default::default() }).unwrap();
}

fn check(id: &str, command: String, adapter: &str) -> CheckSpec {
    CheckSpec { id: id.into(), name: id.into(), command, kind: CheckKind::Security, timeout_secs: 60, required: true, adapter: Some(adapter.into()) }
}

fn run(f: &Fixture, mid: &str, id: &str) -> crate::store_review::VerificationRun {
    let started = f.core.start_check(mid, id).unwrap();
    wait_until("check finished", || f.core.store.get_run(&started.id).unwrap().state != RunState::Running);
    wait_until("slot released", || f.core.start_check(mid, "nonexistent").map(|_| true).unwrap_or_else(|e| !e.to_string().contains("already running")));
    f.core.store.get_run(&started.id).unwrap()
}

fn reviewed_mission(f: &Fixture) -> Mission {
    let m = new_mission(f, "add a scripted note", PermissionProfile::AcceptEdits);
    f.core.start_mission(&m.id, None).unwrap();
    wait_until("review", || state(f, &m.id) == "awaiting_review");
    wait_until("finalised", || f.core.live_session_count() == 0);
    m
}

#[test]
fn findings_from_a_secret_scan_fail_the_check_and_reach_the_proof_pack() {
    let f = fixture();
    let m = reviewed_mission(&f);
    with_checks(&f, &m, vec![check("secrets", print_fixture("gitleaks-findings.json", 1), "gitleaks"), check("deps", print_fixture("osv-findings.json", 1), "osv-scanner")]);
    let r = run(&f, &m.id, "secrets");
    assert_eq!(r.state, RunState::Failed);
    let ev = r.evidence.expect("evidence stored");
    assert_eq!((ev.outcome, ev.blocking_findings), (Outcome::Findings, 2));
    run(&f, &m.id, "deps");
    let pack = f.core.generate_proof_pack(&m.id).unwrap();
    assert!(pack.markdown.contains("gitleaks") && pack.markdown.contains("src/config.ts:3"), "{}", pack.markdown);
    assert!(pack.markdown.contains("GHSA-35jh-r3h4-6jhm"));
    assert!(!pack.markdown.contains("REDACTED") , "no secret material, even redacted, in the pack");
}

#[test]
fn a_clean_report_passes_and_a_missing_tool_is_an_error_not_a_pass_or_a_finding() {
    let f = fixture();
    let m = reviewed_mission(&f);
    with_checks(&f, &m, vec![
        check("clean", "node -e \"process.stdout.write('[]')\"".into(), "gitleaks"),
        check("missing", "definitely-not-installed-scanner-xyz --format json".into(), "osv-scanner"),
        check("garbage", "node -e \"console.log('all good!')\"".into(), "osv-scanner"),
    ]);
    let clean = run(&f, &m.id, "clean");
    assert_eq!((clean.state, clean.evidence.unwrap().outcome), (RunState::Passed, Outcome::Passed));
    let missing = run(&f, &m.id, "missing");
    assert_eq!((missing.state, missing.evidence.unwrap().outcome), (RunState::Error, Outcome::ToolMissing));
    let garbage = run(&f, &m.id, "garbage");
    assert_eq!((garbage.state, garbage.evidence.unwrap().outcome), (RunState::Error, Outcome::ToolError));
    // A required check that errored blocks approval like a failing one.
    let report = f.core.verification_report(&m.id).unwrap();
    assert!(!report.unresolved.is_empty());
}

#[test]
fn unknown_adapters_are_refused_in_the_contract() {
    let f = fixture();
    let m = reviewed_mission(&f);
    let err = f.core.set_contract(&m.id, Contract { version: 1, checks: vec![check("x", "x".into(), "nope")], ..Default::default() }).unwrap_err();
    assert!(err.to_string().contains("unknown verification adapter"));
}
