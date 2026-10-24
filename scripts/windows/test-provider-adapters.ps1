#Requires -Version 5.1
<#
.SYNOPSIS
    Reports the Claude Code and Codex CLIs on this machine and runs FORGE's provider-adapter tests. No live mission is run.

.DESCRIPTION
    Provider CLIs (informational; a missing CLI never fails this script):
      claude --version, codex --version
      Sign-in status, the same way FORGE's Rust core detects it (apps\desktop\src-tauri\src\providers.rs):
        claude auth status   -> JSON; only loggedIn / authMethod / subscriptionType are read
        codex login status   -> "Logged in using ChatGPT" / "Logged in using an API key ..." / "Not logged in"
      The raw output of these status commands is never printed or written to a log (it can contain an account e-mail,
      and for an API-key login Codex prints a partially masked key). Only the derived yes/no/method is reported.
      This script never reads ~/.codex/auth.json, ~/.claude* credential files or token environment variables.
      FORGE needs a real codex.exe (it never starts an npm .cmd/.ps1 shim); that is reported too.

    Tests (required):
      npm run test -w @forge/engine
          engine adapter tests: Claude options/env scrubbing, the Codex adapter against a scripted app-server
          (test\codex.test.ts), ACP against a scripted agent built on the official SDK (test\acp.test.ts), host tests
      cargo test --lib --locked <filter> in apps\desktop\src-tauri, one check per filter:
          supervisor::tests::acp        full ACP missions through the core with the scripted agent
          supervisor::tests::mcp        MCP authorization with the scripted agent requesting mcp__ tools
          supervisor::tests::approvals  approval binding / expiry / decision-time policy re-check
          acp::tests, mcp::tests, providers::tests
      There is no Rust test that drives a scripted Codex app-server; the Codex adapter's scripted tests are the engine's
      codex.test.ts. The Rust live tests (live_codex_*, live_claude_*) are #[ignore] and are never selected.

    NO LIVE PROVIDER MISSION IS RUN. No provider quota is spent.

    Exit code: 0 = all required checks passed; 1 = a FAIL; 2 = no FAIL but a required check BLOCKED.
    Report: validation-results\test-provider-adapters-<timestamp>.json

.PARAMETER SkipEngineTests
    Do not run the engine tests (for example when verify-windows.ps1 just ran them).
.PARAMETER SkipRust
    Do not run the Rust tests.

.EXAMPLE
    .\scripts\windows\test-provider-adapters.ps1
#>
[CmdletBinding()]
param(
    [switch]$SkipEngineTests,
    [switch]$SkipRust
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Import-Module "$PSScriptRoot\lib\ForgeValidation.psm1" -Force

function Get-ClaudeSignIn {
    <# Mirrors providers.rs detect_claude: `claude auth status` prints JSON with loggedIn/authMethod/subscriptionType. #>
    param([Parameter(Mandatory = $true)][string]$Claude)
    $r = Invoke-NativeCapture -FilePath $Claude -ArgumentList @('auth', 'status') -TimeoutSeconds 15
    if (-not $r.Started) { return [pscustomobject]@{ Known = $false; SignedIn = $null; Text = ('not checked: could not start (' + $r.Error + ')') } }
    if ($r.TimedOut) { return [pscustomobject]@{ Known = $false; SignedIn = $null; Text = 'not checked: claude auth status timed out after 15 s' } }
    $parsed = $null
    try { $parsed = ($r.Output.Trim() | ConvertFrom-Json) } catch { $parsed = $null }
    if ($null -eq $parsed -or $null -eq $parsed.PSObject.Properties['loggedIn']) {
        return [pscustomobject]@{ Known = $false; SignedIn = $null; Text = 'not checked: claude auth status did not print the expected JSON (older CLI?)' }
    }
    $signedIn = [bool]$parsed.loggedIn
    $parts = New-Object System.Collections.Generic.List[string]
    if ($signedIn) { $parts.Add('signed in: yes') } else { $parts.Add('signed in: no') }
    foreach ($field in @('authMethod', 'subscriptionType')) {
        if ($null -ne $parsed.PSObject.Properties[$field] -and $null -ne $parsed.$field -and ([string]$parsed.$field) -match '^[A-Za-z0-9_.-]{1,40}$') {
            $parts.Add($field + ': ' + [string]$parsed.$field)
        }
    }
    return [pscustomobject]@{ Known = $true; SignedIn = $signedIn; Text = ($parts -join '; ') }
}

function Get-CodexSignIn {
    <#
    Mirrors providers.rs detect_codex (`codex login status`), with one correction: FORGE's check is
    lower.contains("logged in"), which is also true for "Not logged in"; here "not logged in" or a non-zero exit is "no".
    #>
    param([Parameter(Mandatory = $true)][string]$Codex)
    $r = Invoke-NativeCapture -FilePath $Codex -ArgumentList @('login', 'status') -TimeoutSeconds 15
    if (-not $r.Started) { return [pscustomobject]@{ Known = $false; SignedIn = $null; Text = ('not checked: could not start (' + $r.Error + ')') } }
    if ($r.TimedOut) { return [pscustomobject]@{ Known = $false; SignedIn = $null; Text = 'not checked: codex login status timed out after 15 s' } }
    $lower = $r.Output.ToLowerInvariant()
    if ($lower.Contains('not logged in')) {
        return [pscustomobject]@{ Known = $true; SignedIn = $false; Text = 'signed in: no' }
    }
    if ($r.ExitCode -eq 0 -and $lower.Contains('logged in')) {
        $method = 'unknown'
        if ($lower.Contains('chatgpt')) { $method = 'chatgpt' } elseif ($lower.Contains('api key')) { $method = 'api_key (FORGE refuses API-key logins for missions)' }
        return [pscustomobject]@{ Known = $true; SignedIn = $true; Text = ('signed in: yes; method: ' + $method) }
    }
    return [pscustomobject]@{ Known = $false; SignedIn = $null; Text = ('not checked: unrecognised output (exit code ' + $r.ExitCode + ')') }
}

$ctx = New-ValidationContext -ScriptName 'test-provider-adapters'
try {
    Add-ValidationNote -Context $ctx -Text 'NO LIVE PROVIDER MISSION IS RUN by this script: only --version / status commands and scripted-agent tests. No provider quota is spent.'
    Add-ValidationNote -Context $ctx -Text 'Sign-in status is derived from claude auth status / codex login status; their raw output is never printed or logged, and no credential file or token variable is read.'
    Add-ValidationNote -Context $ctx -Text 'The Rust core has no test with a scripted Codex app-server; the Codex adapter is covered by the engine tests (packages/engine/test/codex.test.ts).'
    $tools = Get-ForgeTools

    # ---- Claude Code CLI ------------------------------------------------------------------------------------------
    $claude = Find-ForgeExecutable -Name 'claude'
    if ($null -eq $claude) {
        Add-CheckResult -Context $ctx -Name 'Claude Code CLI installed (informational)' -Status BLOCKED -Informational -Detail 'claude not found on PATH; FORGE cannot run Claude missions here'
        Add-CheckResult -Context $ctx -Name 'Claude Code sign-in (informational)' -Status BLOCKED -Informational -Detail 'not checked: CLI not installed'
    }
    else {
        $v = Invoke-NativeCapture -FilePath $claude -ArgumentList @('--version') -TimeoutSeconds 15
        if ($v.ExitCode -eq 0) {
            Add-CheckResult -Context $ctx -Name 'Claude Code CLI installed (informational)' -Status PASS -Informational -Detail ((Get-FirstLine -Text $v.Output) + ' at ' + $claude) -Command 'claude --version'
        }
        else {
            Add-CheckResult -Context $ctx -Name 'Claude Code CLI installed (informational)' -Status BLOCKED -Informational -Detail ('found at ' + $claude + ' but claude --version failed ' + $v.Error) -Command 'claude --version'
        }
        $s = Get-ClaudeSignIn -Claude $claude
        $status = 'BLOCKED'
        if ($s.Known -and $s.SignedIn) { $status = 'PASS' }
        Add-CheckResult -Context $ctx -Name 'Claude Code sign-in (informational)' -Status $status -Informational -Detail $s.Text -Command 'claude auth status (output not shown)'
    }

    # ---- Codex CLI ------------------------------------------------------------------------------------------------
    # FORGE: FORGE_CODEX_PATH overrides PATH, and only a real .exe counts (providers.rs locate_codex).
    $codexOverride = $env:FORGE_CODEX_PATH
    $codex = $null
    $codexSource = 'PATH'
    if (-not [string]::IsNullOrEmpty($codexOverride)) {
        $codexSource = 'FORGE_CODEX_PATH'
        if (Test-Path -LiteralPath $codexOverride -PathType Leaf) { $codex = (Resolve-Path -LiteralPath $codexOverride).Path }
    }
    else {
        $codex = Find-ForgeExecutable -Name 'codex'
    }
    if ($null -eq $codex) {
        Add-CheckResult -Context $ctx -Name 'Codex CLI installed (informational)' -Status BLOCKED -Informational -Detail ('codex not found (' + $codexSource + '); FORGE cannot run Codex missions here')
        Add-CheckResult -Context $ctx -Name 'Codex sign-in (informational)' -Status BLOCKED -Informational -Detail 'not checked: CLI not installed'
    }
    else {
        $isExe = ([System.IO.Path]::GetExtension($codex) -ieq '.exe')
        $v = Invoke-NativeCapture -FilePath $codex -ArgumentList @('--version') -TimeoutSeconds 15
        $ver = Get-FirstLine -Text $v.Output
        if ($v.ExitCode -ne 0) {
            Add-CheckResult -Context $ctx -Name 'Codex CLI installed (informational)' -Status BLOCKED -Informational -Detail ('found at ' + $codex + ' but codex --version failed ' + $v.Error) -Command 'codex --version'
        }
        elseif (-not $isExe) {
            Add-CheckResult -Context $ctx -Name 'Codex CLI installed (informational)' -Status BLOCKED -Informational -Detail ($ver + ' at ' + $codex + ' is a script shim; FORGE only starts a real codex.exe (set FORGE_CODEX_PATH to the .exe)') -Command 'codex --version'
        }
        else {
            Add-CheckResult -Context $ctx -Name 'Codex CLI installed (informational)' -Status PASS -Informational -Detail ($ver + ' at ' + $codex + ' (from ' + $codexSource + ')') -Command 'codex --version'
        }
        $s = Get-CodexSignIn -Codex $codex
        $status = 'BLOCKED'
        if ($s.Known -and $s.SignedIn) { $status = 'PASS' }
        Add-CheckResult -Context $ctx -Name 'Codex sign-in (informational)' -Status $status -Informational -Detail $s.Text -Command 'codex login status (output not shown)'
    }

    # ---- Engine adapter tests -------------------------------------------------------------------------------------
    $engineName = 'Engine adapter tests: npm run test -w @forge/engine (scripted Codex app-server, scripted ACP agent)'
    $haveModules = Test-Path -LiteralPath (Join-Path $ctx.RepoRoot 'node_modules')
    if ($SkipEngineTests) {
        Add-CheckResult -Context $ctx -Name $engineName -Status 'NOT RUN' -Detail '-SkipEngineTests'
    }
    elseif ($null -eq $tools.Node -or $null -eq $tools.Npm) {
        Add-CheckResult -Context $ctx -Name $engineName -Status BLOCKED -Detail 'node and npm.cmd are required'
    }
    elseif (-not $haveModules) {
        Add-CheckResult -Context $ctx -Name $engineName -Status BLOCKED -Detail 'node_modules is missing; run verify-windows.ps1 (npm ci) first'
    }
    else {
        # `npm run test` builds the engine first (packages\engine\package.json), which the Rust tests below need.
        [void](Invoke-CommandCheck -Context $ctx -Name $engineName -FilePath $tools.Npm -ArgumentList @('run', 'test', '-w', '@forge/engine') -WorkingDirectory $ctx.RepoRoot)
    }

    # ---- Rust supervisor tests with scripted agents -----------------------------------------------------------------
    $filters = @(
        @{ Filter = 'supervisor::tests::acp'; What = 'ACP missions through the core (scripted ACP agent)' },
        @{ Filter = 'supervisor::tests::mcp'; What = 'MCP authorization (scripted agent)' },
        @{ Filter = 'supervisor::tests::approvals'; What = 'approval binding and expiry (scripted agent)' },
        @{ Filter = 'acp::tests'; What = 'ACP registry unit tests' },
        @{ Filter = 'mcp::tests'; What = 'MCP definition unit tests' },
        @{ Filter = 'providers::tests'; What = 'provider detection unit tests' }
    )
    $engineMain = Join-Path $ctx.RepoRoot 'packages\engine\dist\main.js'
    if (-not $SkipRust -and $null -ne $tools.Cargo -and -not (Test-Path -LiteralPath $engineMain)) {
        [void](Test-EngineReady -Context $ctx -Tools $tools)
    }
    foreach ($f in $filters) {
        $name = 'Rust: cargo test --lib ' + $f.Filter + ' (' + $f.What + ')'
        if ($SkipRust) { Add-CheckResult -Context $ctx -Name $name -Status 'NOT RUN' -Detail '-SkipRust' }
        elseif ($null -eq $tools.Cargo) { Add-CheckResult -Context $ctx -Name $name -Status BLOCKED -Detail 'cargo not found on PATH' }
        elseif (-not (Test-Path -LiteralPath $engineMain)) { Add-CheckResult -Context $ctx -Name $name -Status BLOCKED -Detail 'engine not built (packages\engine\dist\main.js)' }
        else { [void](Invoke-CargoTestCheck -Context $ctx -Name $name -Filter $f.Filter -Cargo $tools.Cargo) }
    }
    Write-Host ''
    Write-Host 'Reminder: no live provider mission was run. Live tests (#[ignore]) were not selected.' -ForegroundColor Cyan
}
catch {
    Add-CheckResult -Context $ctx -Name 'Unexpected script error' -Status FAIL -Detail ($_.Exception.Message + ' at ' + $_.InvocationInfo.PositionMessage)
}
$code = Complete-Validation -Context $ctx
exit $code
