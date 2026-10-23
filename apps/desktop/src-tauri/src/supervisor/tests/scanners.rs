//! Real scanners through the real FORGE check path: the installed gitleaks / osv-scanner binaries run on the mission
//! worktree via `start_check`, their reports are parsed into stored evidence, and the tool version is recorded.
//!
//! Needs the tools on PATH. Without them each test says so and returns, unless FORGE_REQUIRE_SCANNER_TESTS=1 (CI installs
//! pinned, checksum-verified releases and sets it), in which case a missing tool fails the test instead of looking green.
//! The secret is a fake token assembled at runtime so this source file never contains one.

use super::*;
use crate::contract::{CheckKind, CheckSpec, Contract, RunState};
use crate::evidence::Outcome;

fn tool(name: &str) -> bool {
    if which::which(name).is_ok() {
        return true;
    }
    assert!(std::env::var("FORGE_REQUIRE_SCANNER_TESTS").as_deref() != Ok("1"), "{name} is required here but not on PATH");
    eprintln!("SKIPPED: {name} not on PATH (set FORGE_REQUIRE_SCANNER_TESTS=1 to make this a failure)");
    false
}

fn reviewed(f: &Fixture) -> (Mission, PathBuf) {
    let m = new_mission(f, "add a scripted note", PermissionProfile::AcceptEdits);
    f.core.start_mission(&m.id, None).unwrap();
    wait_until("review", || state(f, &m.id) == "awaiting_review");
    wait_until("finalised", || f.core.live_session_count() == 0);
    let wt = PathBuf::from(f.core.store.get_mission(&m.id).unwrap().worktree_path.unwrap());
    (m, wt)
}

fn run(f: &Fixture, m: &Mission, spec: CheckSpec) -> crate::store_review::VerificationRun {
    let id = spec.id.clone();
    f.core.set_contract(&m.id, Contract { version: 1, checks: vec![spec], ..Default::default() }).unwrap();
    let started = f.core.start_check(&m.id, &id).unwrap();
    let t0 = Instant::now();
    while f.core.store.get_run(&started.id).unwrap().state == RunState::Running {
        assert!(t0.elapsed() < Duration::from_secs(240), "scanner did not finish");
        std::thread::sleep(Duration::from_millis(50));
    }
    wait_until("slot released", || f.core.start_check(&m.id, "nonexistent").map(|_| true).unwrap_or_else(|e| !e.to_string().contains("already running")));
    f.core.store.get_run(&started.id).unwrap()
}

fn spec(adapter: &str, command: &str) -> CheckSpec {
    CheckSpec { id: adapter.into(), name: adapter.into(), command: command.into(), kind: CheckKind::Security, timeout_secs: 240, required: true, adapter: Some(adapter.into()) }
}

#[test]
fn real_gitleaks_finds_a_planted_token_and_passes_a_clean_tree() {
    if !tool("gitleaks") {
        return;
    }
    let f = fixture();
    let (m, wt) = reviewed(&f);
    let cmd = crate::evidence::adapter("gitleaks").unwrap().command;
    let clean = run(&f, &m, spec("gitleaks", cmd));
    let ev = clean.evidence.clone().expect("evidence");
    assert_eq!((clean.state, ev.outcome), (RunState::Passed, Outcome::Passed), "{ev:?}\n{}", clean.output_tail);
    assert!(ev.tool_version.as_deref().is_some_and(|v| v.chars().next().is_some_and(|c| c.is_ascii_digit())), "version recorded: {:?}", ev.tool_version);

    let token = format!("ghp_{}", "A1b2C3d4E5f6G7h8I9j0K1l2M3n4O5p6Q7r8");
    std::fs::create_dir_all(wt.join("src")).unwrap();
    std::fs::write(wt.join("src/config.ts"), format!("export const token = \"{token}\";\n")).unwrap();
    let dirty = run(&f, &m, spec("gitleaks", cmd));
    let ev = dirty.evidence.clone().expect("evidence");
    assert_eq!((dirty.state, ev.outcome), (RunState::Failed, Outcome::Findings), "{ev:?}");
    assert!(ev.findings.iter().any(|x| x.rule_id == "github-pat" && x.location.as_deref() == Some("src/config.ts:1")), "{ev:?}");
    let stored = serde_json::to_string(&f.core.store.get_run(&dirty.id).unwrap()).unwrap();
    assert!(!stored.contains(&token), "the secret never reaches the database");
    let stdout = std::fs::read_to_string(f.core.cfg.home.join("runs").join(format!("{}.stdout", dirty.id))).unwrap();
    assert!(!stdout.contains(&token), "nor the captured report");
}

#[test]
fn real_osv_scanner_reports_a_known_vulnerable_dependency() {
    if !tool("osv-scanner") {
        return;
    }
    let f = fixture();
    let (m, wt) = reviewed(&f);
    std::fs::write(
        wt.join("package-lock.json"),
        r#"{"name":"fx","version":"1.0.0","lockfileVersion":3,"requires":true,"packages":{"":{"name":"fx","version":"1.0.0","dependencies":{"lodash":"4.17.20"}},"node_modules/lodash":{"version":"4.17.20","resolved":"https://registry.npmjs.org/lodash/-/lodash-4.17.20.tgz","license":"MIT"}}}"#,
    )
    .unwrap();
    // Offline matching against OSV's downloadable database (fetched by the scan itself), so the result does not depend on the
    // api.osv.dev query endpoint.
    let r = run(&f, &m, spec("osv-scanner", "osv-scanner scan source --offline-vulnerabilities --download-offline-databases --format json -r ."));
    let ev = r.evidence.clone().expect("evidence");
    assert_eq!((r.state, ev.outcome), (RunState::Failed, Outcome::Findings), "{ev:?}\n{}", r.output_tail);
    assert!(ev.findings.iter().any(|x| x.rule_id == "GHSA-35jh-r3h4-6jhm" && x.package.as_deref() == Some("lodash@4.17.20")), "{ev:?}");
    assert!(ev.tool_version.as_deref().is_some_and(|v| v.contains("2.")), "{:?}", ev.tool_version);
}
