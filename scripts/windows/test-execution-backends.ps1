#Requires -Version 5.1
<#
.SYNOPSIS
    Reports Windows Sandbox and WSL on this machine and runs FORGE's execution, path-containment and Windows-specific tests.

.DESCRIPTION
    IMPORTANT: of FORGE's execution backends only `trusted_local` (the user's shell, NOT isolated; Job Object for clean-up
    and a timeout) runs on Windows. `windows_sandbox` and `isolated_wsl` are designed (docs\EXECUTION.md) but NOT
    implemented, and `linux_namespaces` is Linux only. The Windows Sandbox and WSL states below are informational: they
    tell you whether the machine could host those backends later; they test nothing in FORGE.

    Informational:
      Get-WindowsOptionalFeature -Online -FeatureName Containers-DisposableClientVM   (needs an elevated prompt; BLOCKED
                                                                                         with the reason otherwise)
      wsl.exe --status

    Required (cargo test --lib --locked <filter>, in apps\desktop\src-tauri; one check per filter):
      execution                     execution::tests + the check-runner test that records the backend (trusted_local)
      execution::tests::unimplemented_backends_are_never_available_and_say_so
      paths::                       path containment (all path tests, incl. the Windows-only ones)
      proc::                        process containment (on Windows: the Job Object / suspended-start test)
      policy::                      policy tests (incl. the Windows real-path test)
      Windows-only tests (#[cfg(windows)]), each by name:
        paths::tests::windows_case_insensitive_and_rejects_streams_and_devices
        paths::tests::a_root_spelled_with_a_short_name_still_contains_its_files_but_short_names_below_it_are_refused
        policy::tests::protection_follows_the_real_path_not_the_spelling
        proc::tests::a_suspended_process_runs_only_after_it_is_contained_and_resumed
      Tests that use real NTFS junctions or Job Objects on Windows, each by name:
        paths::tests::resolve_within_catches_directory_link_escape
        knowledge::vault::tests::a_directory_junction_inside_the_tree_is_refused
        knowledge::vault::tests::a_junction_that_points_elsewhere_inside_the_same_tree_is_still_refused
        harness::edit::tests::links_in_the_path_are_refused
        supervisor::tests::stopping_a_hung_agent_marks_it_interrupted_and_cleans_up_the_process
        supervisor::tests::regressions::a_descendant_holding_the_pipes_cannot_wedge_finalisation_and_is_killed
        harness::dryrun::tests      (hook dry-run: created suspended, put in the Job, then resumed; needs Git Bash)

    The bubblewrap tests (execution_tests, Linux only) do not exist in a Windows build.
    Exit code: 0 = all required checks passed; 1 = a FAIL; 2 = no FAIL but a required check BLOCKED.
    Report: validation-results\test-execution-backends-<timestamp>.json

.PARAMETER SkipRust
    Only report Windows Sandbox and WSL.

.EXAMPLE
    .\scripts\windows\test-execution-backends.ps1
#>
[CmdletBinding()]
param(
    [switch]$SkipRust
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Import-Module "$PSScriptRoot\lib\ForgeValidation.psm1" -Force

function Test-IsAdministrator {
    $identity = [System.Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object System.Security.Principal.WindowsPrincipal($identity)
    return $principal.IsInRole([System.Security.Principal.WindowsBuiltInRole]::Administrator)
}

$ctx = New-ValidationContext -ScriptName 'test-execution-backends'
try {
    Add-ValidationNote -Context $ctx -Text 'Only the trusted_local backend (NOT isolated) runs on Windows. windows_sandbox and isolated_wsl are designed but NOT implemented (docs/EXECUTION.md); the Sandbox/WSL states below are informational.'
    $tools = Get-ForgeTools

    # ---- Windows Sandbox (informational) ---------------------------------------------------------------------------
    $wsName = 'Windows Sandbox feature Containers-DisposableClientVM (informational)'
    $wsCmd = 'Get-WindowsOptionalFeature -Online -FeatureName Containers-DisposableClientVM'
    if (-not (Test-IsAdministrator)) {
        Add-CheckResult -Context $ctx -Name $wsName -Status BLOCKED -Informational -Command $wsCmd -Detail 'not checked: needs an elevated (Run as administrator) PowerShell; not required for FORGE today'
    }
    else {
        try {
            $feature = Get-WindowsOptionalFeature -Online -FeatureName 'Containers-DisposableClientVM' -ErrorAction Stop
            if ($null -eq $feature) {
                Add-CheckResult -Context $ctx -Name $wsName -Status BLOCKED -Informational -Command $wsCmd -Detail 'feature not present on this Windows edition (Windows Sandbox needs Pro, Enterprise or Education)'
            }
            else {
                Add-CheckResult -Context $ctx -Name $wsName -Status PASS -Informational -Command $wsCmd -Detail ('state: ' + [string]$feature.State + ' (FORGE does not use it yet: backend not implemented)')
            }
        }
        catch {
            Add-CheckResult -Context $ctx -Name $wsName -Status BLOCKED -Informational -Command $wsCmd -Detail ('not available: ' + $_.Exception.Message)
        }
    }

    # ---- WSL (informational) ---------------------------------------------------------------------------------------
    $wslName = 'WSL status (informational)'
    $wsl = Join-Path $env:SystemRoot 'System32\wsl.exe'
    if (-not (Test-Path -LiteralPath $wsl -PathType Leaf)) {
        Add-CheckResult -Context $ctx -Name $wslName -Status BLOCKED -Informational -Command 'wsl --status' -Detail 'wsl.exe not present; not required for FORGE today'
    }
    else {
        # WSL_UTF8=1 makes wsl.exe print UTF-8 instead of UTF-16.
        $r = Invoke-NativeCapture -FilePath $wsl -ArgumentList @('--status') -TimeoutSeconds 30 -Environment @{ WSL_UTF8 = '1' }
        $summary = ((($r.Output -split "`r?`n") | ForEach-Object { $_.Trim() } | Where-Object { $_ -ne '' } | Select-Object -First 3) -join ' | ')
        $logPath = Join-Path $ctx.LogDir 'wsl-status.log'
        [System.IO.File]::WriteAllText($logPath, ('> wsl --status' + "`r`n" + $r.Output + "`r`nexit code: " + $r.ExitCode), (New-Object System.Text.UTF8Encoding($false)))
        if ($r.TimedOut) {
            Add-CheckResult -Context $ctx -Name $wslName -Status BLOCKED -Informational -Command 'wsl --status' -LogPath $logPath -Detail 'wsl --status timed out'
        }
        elseif ($r.ExitCode -eq 0) {
            Add-CheckResult -Context $ctx -Name $wslName -Status PASS -Informational -Command 'wsl --status' -LogPath $logPath -Detail ('installed: ' + $summary + ' (FORGE does not use it yet: isolated_wsl not implemented)')
        }
        else {
            Add-CheckResult -Context $ctx -Name $wslName -Status BLOCKED -Informational -Command 'wsl --status' -LogPath $logPath -Detail ('WSL not installed or not ready (exit code ' + $r.ExitCode + '): ' + $summary)
        }
    }

    # ---- Rust tests --------------------------------------------------------------------------------------------------
    $filters = @(
        @{ Filter = 'execution'; What = 'execution backends and the check runner' },
        @{ Filter = 'execution::tests::unimplemented_backends_are_never_available_and_say_so'; What = 'windows_sandbox / isolated_wsl are refused as not implemented' },
        @{ Filter = 'paths::'; What = 'path containment' },
        @{ Filter = 'proc::'; What = 'process containment' },
        @{ Filter = 'policy::'; What = 'policy' },
        @{ Filter = 'paths::tests::windows_case_insensitive_and_rejects_streams_and_devices'; What = 'Windows only: case, ADS, device names' },
        @{ Filter = 'paths::tests::a_root_spelled_with_a_short_name_still_contains_its_files_but_short_names_below_it_are_refused'; What = 'Windows only: 8.3 short names' },
        @{ Filter = 'policy::tests::protection_follows_the_real_path_not_the_spelling'; What = 'Windows only: policy on the real path' },
        @{ Filter = 'proc::tests::a_suspended_process_runs_only_after_it_is_contained_and_resumed'; What = 'Windows only: Job Object + suspended start' },
        @{ Filter = 'paths::tests::resolve_within_catches_directory_link_escape'; What = 'real junction escape' },
        @{ Filter = 'knowledge::vault::tests::a_directory_junction_inside_the_tree_is_refused'; What = 'real junction in a vault' },
        @{ Filter = 'knowledge::vault::tests::a_junction_that_points_elsewhere_inside_the_same_tree_is_still_refused'; What = 'real junction inside the same tree' },
        @{ Filter = 'harness::edit::tests::links_in_the_path_are_refused'; What = 'real junction in a harness edit path' },
        @{ Filter = 'supervisor::tests::stopping_a_hung_agent_marks_it_interrupted_and_cleans_up_the_process'; What = 'stop kills the agent process tree' },
        @{ Filter = 'supervisor::tests::regressions::a_descendant_holding_the_pipes_cannot_wedge_finalisation_and_is_killed'; What = 'Job kills lingering descendants' },
        @{ Filter = 'harness::dryrun::tests'; What = 'hook dry-run containment (needs Git Bash)' }
    )
    $engineMain = Join-Path $ctx.RepoRoot 'packages\engine\dist\main.js'
    if (-not $SkipRust -and $null -ne $tools.Cargo -and -not (Test-Path -LiteralPath $engineMain)) {
        [void](Test-EngineReady -Context $ctx -Tools $tools)
    }
    foreach ($f in $filters) {
        $name = 'Rust: cargo test --lib ' + $f.Filter + ' (' + $f.What + ')'
        if ($SkipRust) { Add-CheckResult -Context $ctx -Name $name -Status 'NOT RUN' -Detail '-SkipRust' }
        elseif ($null -eq $tools.Cargo) { Add-CheckResult -Context $ctx -Name $name -Status BLOCKED -Detail 'cargo not found on PATH' }
        elseif (-not (Test-Path -LiteralPath $engineMain)) { Add-CheckResult -Context $ctx -Name $name -Status BLOCKED -Detail 'engine not built (packages\engine\dist\main.js); the supervisor tests start it' }
        else { [void](Invoke-CargoTestCheck -Context $ctx -Name $name -Filter $f.Filter -Cargo $tools.Cargo) }
    }
}
catch {
    Add-CheckResult -Context $ctx -Name 'Unexpected script error' -Status FAIL -Detail ($_.Exception.Message + ' at ' + $_.InvocationInfo.PositionMessage)
}
$code = Complete-Validation -Context $ctx
exit $code
