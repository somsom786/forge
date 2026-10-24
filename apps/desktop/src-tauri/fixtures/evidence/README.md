# Evidence adapter fixtures

Two kinds, kept apart on purpose:

* `real-*.json` — **captured from real runs** on 2026-10-23 in the Linux cloud environment, with checksum-verified official
  releases, against disposable fixture repositories (a fake `ghp_` token assembled at runtime, a lockfile pinning
  lodash 4.17.20, a two-line Dockerfile, two Playwright specs of which one fails on purpose):

  | File | Tool | Command (exit code) |
  |---|---|---|
  | real-gitleaks-8.30.1.json | gitleaks 8.30.1 | `gitleaks dir . --redact --no-banner --report-format json --report-path -` (1) |
  | real-osv-scanner-2.7.0.json | OSV-Scanner 2.7.0 | `osv-scanner scan source --offline-vulnerabilities --format json -r .` (1) |
  | real-trivy-0.75.0.json | Trivy 0.75.0 | `trivy fs --quiet --format json --scanners vuln,secret,misconfig .` (0, findings present) |
  | real-playwright-1.56.1.json | @playwright/test 1.56.1, Chromium build 1194 | `npx playwright test --reporter=json` (1) |

  Observed behaviours the parsers now handle: OSV exits 127 when api.osv.dev is unreachable (after running) and 128 when
  there is nothing to scan; Trivy exits 0 with findings; Playwright reports a browser that cannot start as failing tests.

* The other files are **hand-written from each tool's documented report format** (written before any tool was run here). They exercise FORGE's parsers; whether the suggested commands produce exactly
these shapes with the tool versions on a user's machine is a Windows-validation item (docs/CODEX_WINDOWS_HANDOFF.md).

| File | Shape documented at |
|---|---|
| gitleaks-findings.json | gitleaks `--report-format json`: array of findings with `RuleID`, `Description`, `File`, `StartLine`, `Secret`, `Match`, `Fingerprint` |
| osv-findings.json | OSV-Scanner `--format json`: `results[].source.path`, `results[].packages[].package`, `.vulnerabilities[]`, `.groups[].max_severity` |
| trivy-findings.json | Trivy `--format json`: `Results[].Target`, `.Vulnerabilities[]`, `.Secrets[]`, `.Misconfigurations[]` |
| playwright-failures.json | Playwright JSON reporter: `stats`, nested `suites[].specs[].tests[].status` |
