//! Suggest verification checks from the files a project already has. Suggestions are never run automatically:
//! the user reviews and enables them, because a check executes code with the user's privileges.

use crate::contract::{CheckKind, CheckSpec};
use serde_json::Value;
use std::path::Path;

fn spec(id: &str, name: &str, command: &str, kind: CheckKind, timeout_secs: u64) -> CheckSpec {
    CheckSpec { id: id.into(), name: name.into(), command: command.into(), kind, timeout_secs, required: false, adapter: None }
}

fn node_runner(root: &Path) -> &'static str {
    if root.join("pnpm-lock.yaml").exists() {
        "pnpm"
    } else if root.join("yarn.lock").exists() {
        "yarn"
    } else {
        "npm"
    }
}

pub fn suggest_checks(root: &Path) -> Vec<CheckSpec> {
    suggest_checks_with(root, &|tool| which::which(tool).is_ok())
}

/// `installed` says whether a scanner is on PATH; injectable so results do not depend on the machine running the tests.
pub fn suggest_checks_with(root: &Path, installed: &dyn Fn(&str) -> bool) -> Vec<CheckSpec> {
    let mut out = vec![];

    if let Ok(text) = std::fs::read_to_string(root.join("package.json")) {
        if let Ok(v) = serde_json::from_str::<Value>(&text) {
            let runner = node_runner(root);
            let scripts = v.get("scripts").and_then(Value::as_object);
            let has = |k: &str| scripts.is_some_and(|s| s.get(k).and_then(Value::as_str).is_some_and(|c| !c.trim().is_empty()));
            let run = |k: &str| if runner == "yarn" { format!("yarn {k}") } else { format!("{runner} run {k}") };
            // `npm test` default stub exits 1 with "no test specified": do not suggest it.
            let real_test = scripts
                .and_then(|s| s.get("test"))
                .and_then(Value::as_str)
                .is_some_and(|c| !c.contains("no test specified"));
            if real_test {
                out.push(spec("test", "Tests", &if runner == "yarn" { "yarn test".into() } else { format!("{runner} test") }, CheckKind::Test, 900));
            }
            if has("typecheck") {
                out.push(spec("typecheck", "Typecheck", &run("typecheck"), CheckKind::Typecheck, 600));
            }
            if has("lint") {
                out.push(spec("lint", "Lint", &run("lint"), CheckKind::Lint, 600));
            }
            if has("build") {
                out.push(spec("build", "Build", &run("build"), CheckKind::Build, 900));
            }
        }
    }
    if root.join("Cargo.toml").exists() {
        out.push(spec("cargo-test", "Cargo tests", "cargo test", CheckKind::Test, 1800));
        out.push(spec("cargo-clippy", "Clippy", "cargo clippy --all-targets", CheckKind::Lint, 900));
    }
    if root.join("pyproject.toml").exists() || root.join("pytest.ini").exists() || root.join("tox.ini").exists() {
        out.push(spec("pytest", "Pytest", "python -m pytest -q", CheckKind::Test, 900));
    }
    if root.join("go.mod").exists() {
        out.push(spec("go-test", "Go tests", "go test ./...", CheckKind::Test, 900));
        out.push(spec("go-vet", "Go vet", "go vet ./...", CheckKind::Lint, 600));
    }
    out.extend(adapter_checks(root, installed));
    out
}

/// Checks with a verification adapter, for tools that are installed (or, for Playwright, declared by the project).
pub fn adapter_checks(root: &Path, installed: &dyn Fn(&str) -> bool) -> Vec<CheckSpec> {
    let uses_playwright = std::fs::read_to_string(root.join("package.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .is_some_and(|v| ["dependencies", "devDependencies"].iter().any(|k| v.get(k).and_then(|d| d.get("@playwright/test")).is_some()));
    crate::evidence::ADAPTERS
        .iter()
        .filter(|a| if a.id == "playwright" { uses_playwright } else { installed(a.id) })
        .map(|a| CheckSpec { adapter: Some(a.id.into()), ..spec(a.id, a.label, a.command, a.kind, 900) })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir_with(files: &[(&str, &str)]) -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        for (n, c) in files {
            std::fs::write(d.path().join(n), c).unwrap();
        }
        d
    }

    #[test]
    fn node_project_scripts_become_suggestions_with_the_right_runner() {
        let d = dir_with(&[("package.json", r#"{"scripts":{"test":"vitest run","lint":"eslint .","build":"tsc","dev":"vite"}}"#), ("pnpm-lock.yaml", "")]);
        let s = suggest_checks_with(d.path(), &|_| false);
        let ids: Vec<_> = s.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, vec!["test", "lint", "build"]);
        assert_eq!(s[0].command, "pnpm test");
        assert_eq!(s[1].command, "pnpm run lint");
        assert!(s.iter().all(|c| !c.required), "suggestions are never required until the user decides");
    }

    #[test]
    fn the_npm_default_test_stub_is_not_suggested() {
        let d = dir_with(&[("package.json", r#"{"scripts":{"test":"echo \"Error: no test specified\" && exit 1"}}"#)]);
        assert!(suggest_checks_with(d.path(), &|_| false).is_empty());
    }

    #[test]
    fn adapter_checks_are_suggested_only_for_installed_tools_or_a_declared_playwright() {
        let d = dir_with(&[("package.json", r#"{"devDependencies":{"@playwright/test":"1.48.0"}}"#)]);
        let ids = |s: Vec<CheckSpec>| s.into_iter().map(|c| (c.id, c.adapter)).collect::<Vec<_>>();
        assert_eq!(ids(adapter_checks(d.path(), &|t| t == "gitleaks")), vec![("gitleaks".into(), Some("gitleaks".into())), ("playwright".into(), Some("playwright".into()))]);
        assert!(adapter_checks(tempfile::tempdir().unwrap().path(), &|_| false).is_empty());
    }

    #[test]
    fn other_ecosystems_and_empty_dirs() {
        let d = dir_with(&[("Cargo.toml", "[package]"), ("go.mod", "module x")]);
        let ids: Vec<_> = suggest_checks_with(d.path(), &|_| false).into_iter().map(|c| c.id).collect();
        assert_eq!(ids, vec!["cargo-test", "cargo-clippy", "go-test", "go-vet"]);
        assert!(suggest_checks_with(tempfile::tempdir().unwrap().path(), &|_| false).is_empty());
        let broken = dir_with(&[("package.json", "{not json")]);
        assert!(suggest_checks_with(broken.path(), &|_| false).is_empty(), "malformed manifests are ignored, not fatal");
    }
}
