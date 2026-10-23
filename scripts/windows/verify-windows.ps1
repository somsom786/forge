#Requires -Version 5.1
<#
.SYNOPSIS
    Main entry point for validating FORGE on Windows: prerequisites, install, engine, UI and Rust core tests.

.DESCRIPTION
    Runs, in order, and records each step as one check:

      Prerequisites  git, Git Bash (hook tests), Node 22+, npm, Rust (cargo/rustc) with the MSVC host toolchain,
                     Visual Studio Build Tools with the C++ x64 tools, WebView2 runtime (registry), git identity (informational)
      Install        npm ci                                            (repository root)
      Engine         npm run build -w @forge/engine
                     npm run typecheck -w @forge/engine
                     npm run test -w @forge/engine
      UI             npm run typecheck -w forge-desktop
                     npm run test -w forge-desktop
      Rust core      cargo test --lib --locked                         (apps\desktop\src-tauri)

    Tests marked #[ignore] are live tests that need provider logins or spend provider quota (for example
    live_claude_mission_end_to_end). They are never selected: no script passes --ignored or --include-ignored, and no script
    starts a live Claude or Codex mission.

    With -IncludeRelease it then runs test-release.ps1; with -All it runs test-provider-adapters.ps1,
    test-execution-backends.ps1, test-obsidian.ps1 and test-release.ps1, each as a child process with its own report.

    Status labels: "PASS <em dash> Physical Windows" (not in CI), "PASS <em dash> Windows CI" (GitHub Actions), FAIL,
    BLOCKED (prerequisite missing), NOT RUN (skipped by a parameter).
    Exit code: 0 = everything required passed; 1 = at least one FAIL; 2 = no FAIL but a required check was BLOCKED.
    Report: validation-results\verify-windows-<timestamp>.json; logs: validation-results\verify-windows-<timestamp>\.

.PARAMETER SkipInstall
    Do not run npm ci (use the node_modules already present).
.PARAMETER SkipEngine
    Do not build, typecheck or test the engine.
.PARAMETER SkipUi
    Do not typecheck or test the UI.
.PARAMETER SkipRust
    Do not run the Rust core tests.
.PARAMETER IncludeRelease
    Also run test-release.ps1 (builds the NSIS installer, installs it silently into a temporary folder, uninstalls it).
.PARAMETER All
    Also run every other script: test-provider-adapters.ps1, test-execution-backends.ps1, test-obsidian.ps1, test-release.ps1.
.PARAMETER Interactive
    Passed to test-obsidian.ps1 with -All: open the fixture vault in Obsidian and ask you what you saw.
.PARAMETER Launch
    Passed to test-release.ps1: start the installed app for -LaunchSeconds and check that it keeps running.
.PARAMETER LaunchSeconds
    Passed to test-release.ps1 (default 20).
.PARAMETER ReplaceExistingInstall
    Passed to test-release.ps1: proceed even though FORGE is already installed for this user (see that script's help).

.EXAMPLE
    powershell -NoProfile -ExecutionPolicy Bypass -File .\scripts\windows\verify-windows.ps1
.EXAMPLE
    .\scripts\windows\verify-windows.ps1 -All -Interactive -Launch
.EXAMPLE
    .\scripts\windows\verify-windows.ps1 -SkipInstall -SkipEngine -SkipUi -SkipRust     # prerequisites only
#>
[CmdletBinding()]
param(
    [switch]$SkipInstall,
    [switch]$SkipEngine,
    [switch]$SkipUi,
    [switch]$SkipRust,
    [switch]$IncludeRelease,
    [switch]$All,
    [switch]$Interactive,
    [switch]$Launch,
    [ValidateRange(5, 600)][int]$LaunchSeconds = 20,
    [switch]$ReplaceExistingInstall
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Import-Module "$PSScriptRoot\lib\ForgeValidation.psm1" -Force

$WebView2ClientKey = '{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'

function Test-Prerequisites {
    param([Parameter(Mandatory = $true)]$Context, [Parameter(Mandatory = $true)]$Tools)
    $state = @{ Git = $false; Node = $false; Npm = $false; Cargo = $false; Msvc = $false }

    # git
    if ($null -eq $Tools.Git) {
        Add-CheckResult -Context $Context -Name 'Prerequisite: git' -Status BLOCKED -Detail 'git not found on PATH (install Git for Windows)'
    }
    else {
        $r = Invoke-NativeCapture -FilePath $Tools.Git -ArgumentList @('--version')
        if ($r.ExitCode -eq 0) {
            $state.Git = $true
            Add-CheckResult -Context $Context -Name 'Prerequisite: git' -Status PASS -Detail ((Get-FirstLine -Text $r.Output) + ' at ' + $Tools.Git) -Command 'git --version'
        }
        else {
            Add-CheckResult -Context $Context -Name 'Prerequisite: git' -Status BLOCKED -Detail ('git --version failed: ' + $r.Error) -Command 'git --version'
        }
    }

    # Git Bash: the Rust hook tests run hook commands the way Claude Code does, in Git for Windows' bash (mirrors find_bash()).
    $bashCandidates = New-Object System.Collections.Generic.List[string]
    if (-not [string]::IsNullOrEmpty($env:CLAUDE_CODE_GIT_BASH_PATH)) { $bashCandidates.Add($env:CLAUDE_CODE_GIT_BASH_PATH) }
    foreach ($var in @('ProgramFiles', 'ProgramFiles(x86)', 'LOCALAPPDATA')) {
        $base = [System.Environment]::GetEnvironmentVariable($var)
        if (-not [string]::IsNullOrEmpty($base)) {
            $bashCandidates.Add((Join-Path $base 'Git\bin\bash.exe'))
            $bashCandidates.Add((Join-Path $base 'Programs\Git\bin\bash.exe'))
        }
    }
    $bash = $null
    foreach ($c in $bashCandidates) {
        if (Test-Path -LiteralPath $c -PathType Leaf) { $bash = $c; break }
    }
    if ($null -eq $bash) {
        Add-CheckResult -Context $Context -Name 'Prerequisite: Git Bash (hook tests)' -Status BLOCKED -Detail 'Git for Windows bash.exe not found (checked CLAUDE_CODE_GIT_BASH_PATH, Program Files, LOCALAPPDATA); Rust hook tests will fail without it'
    }
    else {
        Add-CheckResult -Context $Context -Name 'Prerequisite: Git Bash (hook tests)' -Status PASS -Detail $bash
    }

    # Node 22+
    if ($null -eq $Tools.Node) {
        Add-CheckResult -Context $Context -Name 'Prerequisite: Node.js 22+' -Status BLOCKED -Detail 'node not found on PATH (install Node.js 22 LTS)'
    }
    else {
        $r = Invoke-NativeCapture -FilePath $Tools.Node -ArgumentList @('--version')
        $v = Get-FirstLine -Text $r.Output
        $m = [regex]::Match($v, '^v(\d+)\.')
        if ($r.ExitCode -eq 0 -and $m.Success -and [int]$m.Groups[1].Value -ge 22) {
            $state.Node = $true
            $note = ''
            if ([int]$m.Groups[1].Value -ne 22) { $note = ' (CI uses Node 22)' }
            Add-CheckResult -Context $Context -Name 'Prerequisite: Node.js 22+' -Status PASS -Detail ($v + ' at ' + $Tools.Node + $note) -Command 'node --version'
        }
        else {
            Add-CheckResult -Context $Context -Name 'Prerequisite: Node.js 22+' -Status BLOCKED -Detail ('found "' + $v + '"; package.json requires node >= 22') -Command 'node --version'
        }
    }

    # npm (npm.cmd: npm.ps1 would depend on the execution policy)
    if ($null -eq $Tools.Npm) {
        Add-CheckResult -Context $Context -Name 'Prerequisite: npm' -Status BLOCKED -Detail 'npm.cmd not found on PATH (it ships with Node.js)'
    }
    else {
        $r = Invoke-NativeCapture -FilePath $Tools.Npm -ArgumentList @('--version') -TimeoutSeconds 60
        if ($r.ExitCode -eq 0) {
            $state.Npm = $true
            Add-CheckResult -Context $Context -Name 'Prerequisite: npm' -Status PASS -Detail ((Get-FirstLine -Text $r.Output) + ' at ' + $Tools.Npm) -Command 'npm --version'
        }
        else {
            Add-CheckResult -Context $Context -Name 'Prerequisite: npm' -Status BLOCKED -Detail ('npm --version failed: ' + $r.Error) -Command 'npm --version'
        }
    }

    # Rust: cargo and rustc with an MSVC host (the toolchain Tauri builds Windows apps with)
    $rustc = Find-ForgeExecutable -Name 'rustc'
    if ($null -eq $Tools.Cargo -or $null -eq $rustc) {
        Add-CheckResult -Context $Context -Name 'Prerequisite: Rust (cargo, rustc)' -Status BLOCKED -Detail 'cargo/rustc not found on PATH (install Rust with rustup, stable MSVC toolchain)'
        Add-CheckResult -Context $Context -Name 'Prerequisite: Rust MSVC target' -Status BLOCKED -Detail 'Rust is not installed'
    }
    else {
        $cv = Invoke-NativeCapture -FilePath $Tools.Cargo -ArgumentList @('--version') -TimeoutSeconds 60
        $rv = Invoke-NativeCapture -FilePath $rustc -ArgumentList @('-vV') -TimeoutSeconds 60
        if ($cv.ExitCode -eq 0 -and $rv.ExitCode -eq 0) {
            $state.Cargo = $true
            Add-CheckResult -Context $Context -Name 'Prerequisite: Rust (cargo, rustc)' -Status PASS -Detail ((Get-FirstLine -Text $cv.Output) + '; ' + (Get-FirstLine -Text $rv.Output)) -Command 'cargo --version; rustc -vV'
        }
        else {
            Add-CheckResult -Context $Context -Name 'Prerequisite: Rust (cargo, rustc)' -Status BLOCKED -Detail 'cargo --version or rustc -vV failed (is a default toolchain set? rustup default stable)' -Command 'cargo --version; rustc -vV'
        }
        $hostMatch = [regex]::Match($rv.Output, '(?m)^host:\s*(\S+)')
        $hostTriple = ''
        if ($hostMatch.Success) { $hostTriple = $hostMatch.Groups[1].Value }
        $installed = ''
        $rustup = Find-ForgeExecutable -Name 'rustup'
        if ($null -ne $rustup) {
            $tl = Invoke-NativeCapture -FilePath $rustup -ArgumentList @('target', 'list', '--installed') -TimeoutSeconds 60
            if ($tl.ExitCode -eq 0) { $installed = ((($tl.Output -split "`r?`n") | Where-Object { $_.Trim() -ne '' }) -join ', ') }
        }
        if ($hostTriple -like '*-pc-windows-msvc') {
            $state.Msvc = $true
            $d = 'host ' + $hostTriple
            if ($installed -ne '') { $d = $d + '; installed targets: ' + $installed }
            Add-CheckResult -Context $Context -Name 'Prerequisite: Rust MSVC target' -Status PASS -Detail $d -Command 'rustc -vV; rustup target list --installed'
        }
        else {
            Add-CheckResult -Context $Context -Name 'Prerequisite: Rust MSVC target' -Status BLOCKED -Detail ('Rust host is "' + $hostTriple + '"; FORGE builds with the MSVC toolchain (rustup default stable-x86_64-pc-windows-msvc)') -Command 'rustc -vV'
        }
    }

    # Visual Studio Build Tools with the C++ x64/x86 tools (the MSVC linker Rust needs)
    $pf86 = [System.Environment]::GetEnvironmentVariable('ProgramFiles(x86)')
    $vswhere = $null
    if (-not [string]::IsNullOrEmpty($pf86)) { $vswhere = Join-Path $pf86 'Microsoft Visual Studio\Installer\vswhere.exe' }
    if ($null -eq $vswhere -or -not (Test-Path -LiteralPath $vswhere -PathType Leaf)) {
        Add-CheckResult -Context $Context -Name 'Prerequisite: Visual Studio Build Tools (C++)' -Status BLOCKED -Detail 'vswhere.exe not found: Visual Studio / Build Tools 2022 with "Desktop development with C++" is not installed'
    }
    else {
        $vsArgs = @('-products', '*', '-requires', 'Microsoft.VisualStudio.Component.VC.Tools.x86.x64', '-latest', '-format', 'value', '-property', 'displayName')
        $r = Invoke-NativeCapture -FilePath $vswhere -ArgumentList $vsArgs -TimeoutSeconds 60
        $name = Get-FirstLine -Text $r.Output
        if ($r.ExitCode -eq 0 -and $name -ne '') {
            Add-CheckResult -Context $Context -Name 'Prerequisite: Visual Studio Build Tools (C++)' -Status PASS -Detail ($name + ' with MSVC x64/x86 build tools') -Command (Format-CommandLine -FilePath 'vswhere.exe' -ArgumentList $vsArgs)
        }
        else {
            Add-CheckResult -Context $Context -Name 'Prerequisite: Visual Studio Build Tools (C++)' -Status BLOCKED -Detail 'no Visual Studio installation with the MSVC x64/x86 C++ build tools (component Microsoft.VisualStudio.Component.VC.Tools.x86.x64)' -Command (Format-CommandLine -FilePath 'vswhere.exe' -ArgumentList $vsArgs)
        }
    }

    # WebView2 runtime (registry; per-machine 64-bit view, per-machine 32-bit, per-user)
    $wvKeys = @(
        ('HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\' + $WebView2ClientKey),
        ('HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\' + $WebView2ClientKey),
        ('HKCU:\Software\Microsoft\EdgeUpdate\Clients\' + $WebView2ClientKey)
    )
    $wvVersion = $null
    $wvWhere = $null
    foreach ($k in $wvKeys) {
        $props = Get-ItemProperty -LiteralPath $k -ErrorAction SilentlyContinue
        if ($null -ne $props -and $null -ne $props.PSObject.Properties['pv']) {
            $pv = [string]$props.pv
            if ($pv -ne '' -and $pv -ne '0.0.0.0') { $wvVersion = $pv; $wvWhere = $k; break }
        }
    }
    if ($null -ne $wvVersion) {
        Add-CheckResult -Context $Context -Name 'Prerequisite: WebView2 runtime' -Status PASS -Detail ('version ' + $wvVersion + ' (' + $wvWhere + ')') -Command 'registry: EdgeUpdate\Clients\{F3017226-...}\pv'
    }
    else {
        Add-CheckResult -Context $Context -Name 'Prerequisite: WebView2 runtime' -Status BLOCKED -Detail 'WebView2 Evergreen runtime not registered (HKLM/HKCU EdgeUpdate\Clients); the app window cannot open without it' -Command 'registry: EdgeUpdate\Clients\{F3017226-...}\pv'
    }

    # git identity (informational: CI sets one before the Rust tests; FORGE never changes your git config)
    if ($state.Git) {
        $n = Invoke-NativeCapture -FilePath $Tools.Git -ArgumentList @('config', '--get', 'user.name')
        $e = Invoke-NativeCapture -FilePath $Tools.Git -ArgumentList @('config', '--get', 'user.email')
        if ($n.ExitCode -eq 0 -and $e.ExitCode -eq 0 -and (Get-FirstLine -Text $n.Output) -ne '' -and (Get-FirstLine -Text $e.Output) -ne '') {
            Add-CheckResult -Context $Context -Name 'Prerequisite: git identity (informational)' -Status PASS -Detail 'user.name and user.email are set' -Informational -Command 'git config --get user.name; git config --get user.email'
        }
        else {
            Add-CheckResult -Context $Context -Name 'Prerequisite: git identity (informational)' -Status BLOCKED -Detail 'user.name/user.email not set; if Rust tests fail on commits, set them with git config --global' -Informational -Command 'git config --get user.name; git config --get user.email'
        }
    }
    return $state
}

$ctx = New-ValidationContext -ScriptName 'verify-windows'
try {
    Add-ValidationNote -Context $ctx -Text 'No live provider test is run: #[ignore] tests are never selected and no Claude or Codex mission is started.'
    $tools = Get-ForgeTools
    $pre = Test-Prerequisites -Context $ctx -Tools $tools
    $jsReady = ($pre.Node -and $pre.Npm)
    $root = $ctx.RepoRoot

    # Install
    $installName = 'Install: npm ci'
    if ($SkipInstall) {
        Add-CheckResult -Context $ctx -Name $installName -Status 'NOT RUN' -Detail '-SkipInstall'
    }
    elseif (-not $jsReady) {
        Add-CheckResult -Context $ctx -Name $installName -Status BLOCKED -Detail 'Node 22+ and npm are required'
    }
    else {
        [void](Invoke-CommandCheck -Context $ctx -Name $installName -FilePath $tools.Npm -ArgumentList @('ci') -WorkingDirectory $root)
    }
    $installFailed = ((Get-CheckOutcome -Context $ctx -Name $installName) -eq 'FAIL')
    $haveModules = Test-Path -LiteralPath (Join-Path $root 'node_modules')
    $jsBlockedReason = ''
    if (-not $jsReady) { $jsBlockedReason = 'Node 22+ and npm are required' }
    elseif ($installFailed) { $jsBlockedReason = 'npm ci failed' }
    elseif (-not $haveModules) { $jsBlockedReason = 'node_modules is missing (run without -SkipInstall)' }

    # Engine
    $engineSteps = @(
        @{ Name = 'Engine: build (npm run build -w @forge/engine)'; Args = @('run', 'build', '-w', '@forge/engine') },
        @{ Name = 'Engine: typecheck (npm run typecheck -w @forge/engine)'; Args = @('run', 'typecheck', '-w', '@forge/engine') },
        @{ Name = 'Engine: tests (npm run test -w @forge/engine)'; Args = @('run', 'test', '-w', '@forge/engine') }
    )
    foreach ($s in $engineSteps) {
        if ($SkipEngine) { Add-CheckResult -Context $ctx -Name $s.Name -Status 'NOT RUN' -Detail '-SkipEngine' }
        elseif ($jsBlockedReason -ne '') { Add-CheckResult -Context $ctx -Name $s.Name -Status BLOCKED -Detail $jsBlockedReason }
        else { [void](Invoke-CommandCheck -Context $ctx -Name $s.Name -FilePath $tools.Npm -ArgumentList $s.Args -WorkingDirectory $root) }
    }

    # UI
    $uiSteps = @(
        @{ Name = 'UI: typecheck (npm run typecheck -w forge-desktop)'; Args = @('run', 'typecheck', '-w', 'forge-desktop') },
        @{ Name = 'UI: tests (npm run test -w forge-desktop)'; Args = @('run', 'test', '-w', 'forge-desktop') }
    )
    foreach ($s in $uiSteps) {
        if ($SkipUi) { Add-CheckResult -Context $ctx -Name $s.Name -Status 'NOT RUN' -Detail '-SkipUi' }
        elseif ($jsBlockedReason -ne '') { Add-CheckResult -Context $ctx -Name $s.Name -Status BLOCKED -Detail $jsBlockedReason }
        else { [void](Invoke-CommandCheck -Context $ctx -Name $s.Name -FilePath $tools.Npm -ArgumentList $s.Args -WorkingDirectory $root) }
    }

    # Rust core
    $rustName = 'Rust core: cargo test --lib (ignored live tests stay ignored)'
    $engineMain = Join-Path $root 'packages\engine\dist\main.js'
    if ($SkipRust) {
        Add-CheckResult -Context $ctx -Name $rustName -Status 'NOT RUN' -Detail '-SkipRust'
    }
    elseif (-not ($pre.Cargo -and $pre.Msvc)) {
        Add-CheckResult -Context $ctx -Name $rustName -Status BLOCKED -Detail 'Rust with the MSVC host toolchain is required'
    }
    elseif (-not $pre.Node) {
        Add-CheckResult -Context $ctx -Name $rustName -Status BLOCKED -Detail 'node is required (the supervisor tests start the engine)'
    }
    elseif (-not (Test-Path -LiteralPath $engineMain)) {
        Add-CheckResult -Context $ctx -Name $rustName -Status BLOCKED -Detail 'packages\engine\dist\main.js is missing (run the engine build; do not use -SkipEngine on a fresh checkout)'
    }
    else {
        [void](Invoke-CargoTestCheck -Context $ctx -Name $rustName -Cargo $tools.Cargo)
    }

    # Other scripts
    $releaseArgs = New-Object System.Collections.Generic.List[string]
    if ($Launch) { $releaseArgs.Add('-Launch'); $releaseArgs.Add('-LaunchSeconds'); $releaseArgs.Add([string]$LaunchSeconds) }
    if ($ReplaceExistingInstall) { $releaseArgs.Add('-ReplaceExistingInstall') }
    if ($All) {
        Invoke-ChildValidationScript -Context $ctx -ScriptFile 'test-provider-adapters.ps1'
        Invoke-ChildValidationScript -Context $ctx -ScriptFile 'test-execution-backends.ps1'
        $obsArgs = @()
        if ($Interactive) { $obsArgs = @('-Interactive') }
        Invoke-ChildValidationScript -Context $ctx -ScriptFile 'test-obsidian.ps1' -ArgumentList $obsArgs
    }
    else {
        foreach ($f in @('test-provider-adapters.ps1', 'test-execution-backends.ps1', 'test-obsidian.ps1')) {
            Add-CheckResult -Context $ctx -Name ($f + ' (see its own report)') -Status 'NOT RUN' -Detail 'use -All'
        }
    }
    if ($All -or $IncludeRelease) {
        Invoke-ChildValidationScript -Context $ctx -ScriptFile 'test-release.ps1' -ArgumentList $releaseArgs.ToArray()
    }
    else {
        Add-CheckResult -Context $ctx -Name 'test-release.ps1 (see its own report)' -Status 'NOT RUN' -Detail 'use -IncludeRelease or -All'
    }
}
catch {
    Add-CheckResult -Context $ctx -Name 'Unexpected script error' -Status FAIL -Detail ($_.Exception.Message + ' at ' + $_.InvocationInfo.PositionMessage)
}
$code = Complete-Validation -Context $ctx
exit $code
