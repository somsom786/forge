# Fixture only: does not install software or touch the registry.
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\..\lib\WaitUninstall.ps1"

$script:checks = 0
$result = Wait-UninstallState -TimeoutSeconds 2 -PollMilliseconds 10 -Inspect {
    $script:checks++
    $entries = if ($script:checks -lt 3) { @('still present') } else { @() }
    [pscustomobject]@{ ExecutableGone = $true; Entries = $entries }
}
if ($script:checks -ne 3 -or @($result.Entries).Count -ne 0) { throw 'did not wait for the registry entry to disappear' }

$result = Wait-UninstallState -TimeoutSeconds 0 -Inspect { [pscustomobject]@{ ExecutableGone = $true; Entries = @('still present') } }
if (@($result.Entries).Count -ne 1) { throw 'a persistent registry entry was incorrectly accepted' }
$result = Wait-UninstallState -TimeoutSeconds 0 -Inspect { [pscustomobject]@{ ExecutableGone = $false; Entries = @() } }
if ($result.ExecutableGone) { throw 'an executable that remains was incorrectly accepted' }
Write-Host 'uninstall polling fixtures passed'
