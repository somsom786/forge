# Evidence adapter fixtures

These JSON files are **hand-written from each tool's documented report format**, not captured from real runs (none of the
tools is installed in the build environment). They exercise FORGE's parsers; whether the suggested commands produce exactly
these shapes with the tool versions on a user's machine is a Windows-validation item (docs/CODEX_WINDOWS_HANDOFF.md).

| File | Shape documented at |
|---|---|
| gitleaks-findings.json | gitleaks `--report-format json`: array of findings with `RuleID`, `Description`, `File`, `StartLine`, `Secret`, `Match`, `Fingerprint` |
| osv-findings.json | OSV-Scanner `--format json`: `results[].source.path`, `results[].packages[].package`, `.vulnerabilities[]`, `.groups[].max_severity` |
| trivy-findings.json | Trivy `--format json`: `Results[].Target`, `.Vulnerabilities[]`, `.Secrets[]`, `.Misconfigurations[]` |
| playwright-failures.json | Playwright JSON reporter: `stats`, nested `suites[].specs[].tests[].status` |
