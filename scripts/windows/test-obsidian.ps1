#Requires -Version 5.1
<#
.SYNOPSIS
    Runs FORGE's Obsidian / knowledge-vault tests against a temporary fixture vault and, with -Interactive, asks you to
    confirm that Obsidian opens it through an obsidian:// link.

.DESCRIPTION
    Never touches a real vault: the fixture is created under %TEMP% (forge-obsidian-fixture-<timestamp>\FORGE Validation
    Vault, with an empty .obsidian folder and one note) and deleted at the end unless -KeepFixture is given.

    Required (cargo test --lib --locked <filter>, in apps\desktop\src-tauri):
      obsidian      Obsidian import/config tests and the obsidian:// URI encoding
      vault         vault path safety (traversal, .obsidian/.git, junctions, hard links, writer lock) and vault status
      knowledge::   the whole knowledge (memory) module
    Informational:
      obsidian:// handler registered (HKCU\Software\Classes\obsidian or HKCR\obsidian), the same check FORGE makes
    With -Interactive (required when requested):
      opens obsidian://open?path=<fixture note> exactly as FORGE builds it (knowledge\vault.rs obsidian_uri) and asks what
      you saw. FORGE documents that this link works for vaults Obsidian already knows; if Obsidian says the vault is
      unknown you are guided to add the fixture folder as a vault and the link is opened again.

    Exit code: 0 = all required checks passed; 1 = a FAIL; 2 = no FAIL but a required check BLOCKED.
    Report: validation-results\test-obsidian-<timestamp>.json

.PARAMETER Interactive
    Open the fixture in Obsidian and ask you to confirm what happened (Read-Host).
.PARAMETER KeepFixture
    Do not delete the temporary fixture vault at the end.
.PARAMETER SkipRust
    Do not run the Rust tests.

.EXAMPLE
    .\scripts\windows\test-obsidian.ps1 -Interactive
#>
[CmdletBinding()]
param(
    [switch]$Interactive,
    [switch]$KeepFixture,
    [switch]$SkipRust
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Import-Module "$PSScriptRoot\lib\ForgeValidation.psm1" -Force

function ConvertTo-ObsidianUri {
    <# Same encoding as knowledge\vault.rs obsidian_uri(): '\' -> '/', UTF-8 bytes outside [A-Za-z0-9-_.~/] percent-encoded. #>
    param([Parameter(Mandatory = $true)][string]$FilePath)
    $p = $FilePath.Replace('\', '/')
    $sb = New-Object System.Text.StringBuilder
    foreach ($b in [System.Text.Encoding]::UTF8.GetBytes($p)) {
        $c = [char]$b
        if (($b -ge 0x30 -and $b -le 0x39) -or ($b -ge 0x41 -and $b -le 0x5A) -or ($b -ge 0x61 -and $b -le 0x7A) -or $c -eq '-' -or $c -eq '_' -or $c -eq '.' -or $c -eq '~' -or $c -eq '/') {
            [void]$sb.Append([string]$c)
        }
        else {
            [void]$sb.Append(('%{0:X2}' -f $b))
        }
    }
    return ('obsidian://open?path=' + $sb.ToString())
}

function Get-LongPath {
    <# Expands 8.3 short components (e.g. a %TEMP% under C:\Users\LONGNA~1) so the link names the path Obsidian knows. #>
    param([Parameter(Mandatory = $true)][string]$Path)
    $full = [System.IO.Path]::GetFullPath($Path)
    if ($full -notmatch '~') { return $full }
    $rootPart = [System.IO.Path]::GetPathRoot($full)
    $current = $rootPart
    foreach ($part in $full.Substring($rootPart.Length).Split([char[]]@('\'), [System.StringSplitOptions]::RemoveEmptyEntries)) {
        $match = Get-ChildItem -LiteralPath $current -Filter $part -Force -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($null -ne $match) { $current = Join-Path $current $match.Name } else { $current = Join-Path $current $part }
    }
    return $current
}

function Read-Choice {
    param([Parameter(Mandatory = $true)][string]$Prompt, [Parameter(Mandatory = $true)][string[]]$Allowed)
    while ($true) {
        $answer = (Read-Host -Prompt $Prompt).Trim()
        if ($Allowed -contains $answer) { return $answer }
        Write-Host ('Please answer one of: ' + ($Allowed -join ', ')) -ForegroundColor Yellow
    }
}

$ctx = New-ValidationContext -ScriptName 'test-obsidian'
$fixtureRoot = $null
$registeredInObsidian = $false
try {
    $tools = Get-ForgeTools

    # ---- Fixture vault under %TEMP% -----------------------------------------------------------------------------------
    $temp = Get-LongPath -Path $env:TEMP
    $fixtureRoot = Join-Path $temp ('forge-obsidian-fixture-' + $ctx.Timestamp)
    $vault = Join-Path $fixtureRoot 'FORGE Validation Vault'
    if (-not ([System.IO.Path]::GetFullPath($vault)).StartsWith($temp, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw ('refusing to create the fixture outside %TEMP%: ' + $vault)
    }
    $token = 'FORGE-VALIDATION-' + [guid]::NewGuid().ToString('N').Substring(0, 12)
    New-Item -ItemType Directory -Path (Join-Path $vault '.obsidian') -Force | Out-Null
    $utf8 = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText((Join-Path $vault '.obsidian\app.json'), '{}', $utf8)
    $note = Join-Path $vault 'FORGE validation note.md'
    $noteText = "# FORGE validation note`r`n`r`nIf you can read this in Obsidian, the obsidian:// link worked.`r`n`r`nToken: $token`r`n"
    [System.IO.File]::WriteAllText($note, $noteText, $utf8)
    Add-CheckResult -Context $ctx -Name 'Fixture vault created under %TEMP%' -Status PASS -Detail $vault
    Add-ValidationNote -Context $ctx -Text ('Only a temporary fixture vault is used: ' + $vault + '. No real vault is opened, read or written by this script.')

    # ---- obsidian:// handler (informational; mirrors vault.rs obsidian_installed) -----------------------------------
    $handlerKeys = @('HKCU:\Software\Classes\obsidian', 'Registry::HKEY_CLASSES_ROOT\obsidian')
    $handler = $null
    foreach ($k in $handlerKeys) {
        if (Test-Path -LiteralPath $k) { $handler = $k; break }
    }
    if ($null -ne $handler) {
        Add-CheckResult -Context $ctx -Name 'Obsidian installed: obsidian:// handler registered (informational)' -Status PASS -Informational -Detail $handler
    }
    else {
        Add-CheckResult -Context $ctx -Name 'Obsidian installed: obsidian:// handler registered (informational)' -Status BLOCKED -Informational -Detail 'no obsidian:// handler (HKCU\Software\Classes\obsidian, HKCR\obsidian): Obsidian is not installed'
    }

    # ---- Rust tests ------------------------------------------------------------------------------------------------
    $filters = @(
        @{ Filter = 'obsidian'; What = 'Obsidian import and URI encoding' },
        @{ Filter = 'vault'; What = 'vault path safety and vault status' },
        @{ Filter = 'knowledge::'; What = 'knowledge (memory) module' }
    )
    $engineMain = Join-Path $ctx.RepoRoot 'packages\engine\dist\main.js'
    if (-not $SkipRust -and $null -ne $tools.Cargo -and -not (Test-Path -LiteralPath $engineMain)) {
        [void](Test-EngineReady -Context $ctx -Tools $tools)
    }
    foreach ($f in $filters) {
        $name = 'Rust: cargo test --lib ' + $f.Filter + ' (' + $f.What + ')'
        if ($SkipRust) { Add-CheckResult -Context $ctx -Name $name -Status 'NOT RUN' -Detail '-SkipRust' }
        elseif ($null -eq $tools.Cargo) { Add-CheckResult -Context $ctx -Name $name -Status BLOCKED -Detail 'cargo not found on PATH' }
        elseif (-not (Test-Path -LiteralPath $engineMain)) { Add-CheckResult -Context $ctx -Name $name -Status BLOCKED -Detail 'engine not built (packages\engine\dist\main.js); a vault status test starts it' }
        else { [void](Invoke-CargoTestCheck -Context $ctx -Name $name -Filter $f.Filter -Cargo $tools.Cargo) }
    }

    # ---- Human confirmation ------------------------------------------------------------------------------------------
    $humanName = 'Obsidian opens the fixture note via obsidian://open?path=... (human-confirmed)'
    $uri = ConvertTo-ObsidianUri -FilePath $note
    if (-not $Interactive) {
        Add-CheckResult -Context $ctx -Name $humanName -Status 'NOT RUN' -Detail 'use -Interactive' -Command $uri
    }
    elseif ($null -eq $handler) {
        Add-CheckResult -Context $ctx -Name $humanName -Status BLOCKED -Detail 'Obsidian is not installed (no obsidian:// handler)' -Command $uri
    }
    else {
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        Write-Host ''
        Write-Host 'Opening this link (exactly what FORGE''s "Open in Obsidian" builds):' -ForegroundColor Cyan
        Write-Host ('  ' + $uri)
        Write-Host ('Expected: Obsidian shows "FORGE validation note" containing the line  Token: ' + $token) -ForegroundColor Cyan
        Start-Process -FilePath $uri
        Write-Host ''
        Write-Host '  1 = Obsidian opened the note and I can see the token above'
        Write-Host '  2 = Obsidian opened but said the vault was not found / unknown'
        Write-Host '  3 = Obsidian did not open (or Windows asked which app to use)'
        Write-Host '  4 = something else'
        Write-Host '  s = skip this check'
        $answer = Read-Choice -Prompt 'What did you see? [1/2/3/4/s]' -Allowed @('1', '2', '3', '4', 's')
        $detail = ''
        $status = 'FAIL'
        if ($answer -eq '1') {
            $status = 'PASS'; $detail = 'person confirmed the note opened and showed the token'
        }
        elseif ($answer -eq '2') {
            Write-Host ''
            Write-Host 'FORGE''s link only works for vaults Obsidian already knows. Add the fixture as a vault:' -ForegroundColor Yellow
            Write-Host '  In Obsidian: vault switcher (bottom left) > Manage vaults > "Open folder as vault" > choose:'
            Write-Host ('    ' + $vault)
            Write-Host '  (This adds a temporary entry to Obsidian''s vault list; remove it afterwards with "Remove from list".)'
            [void](Read-Host -Prompt 'Press Enter when Obsidian has opened that folder as a vault')
            $registeredInObsidian = $true
            Start-Process -FilePath $uri
            $again = Read-Choice -Prompt 'Link opened again. Did Obsidian now show the note with the token? [y/n]' -Allowed @('y', 'n')
            if ($again -eq 'y') {
                $status = 'PASS'; $detail = 'opened only after the fixture folder was added as a vault in Obsidian (expected: the link works for vaults Obsidian knows)'
            }
            else {
                $status = 'FAIL'; $detail = 'did not open the note even after the folder was added as a vault'
            }
        }
        elseif ($answer -eq '3') {
            $detail = 'Obsidian did not open from the obsidian:// link'
        }
        elseif ($answer -eq '4') {
            $detail = 'person reported something else'
        }
        else {
            $status = 'NOT RUN'; $detail = 'skipped by the person running the script'
        }
        $comment = Read-Host -Prompt 'Optional: describe what you saw (Enter to skip)'
        if (-not [string]::IsNullOrWhiteSpace($comment)) { $detail = $detail + '; person said: ' + $comment.Trim() }
        $sw.Stop()
        Add-CheckResult -Context $ctx -Name $humanName -Status $status -Detail $detail -Command $uri -DurationSeconds $sw.Elapsed.TotalSeconds
    }
}
catch {
    Add-CheckResult -Context $ctx -Name 'Unexpected script error' -Status FAIL -Detail ($_.Exception.Message + ' at ' + $_.InvocationInfo.PositionMessage)
}
finally {
    if ($null -ne $fixtureRoot -and (Test-Path -LiteralPath $fixtureRoot)) {
        if ($KeepFixture) {
            Add-ValidationNote -Context $ctx -Text ('Fixture kept (-KeepFixture): ' + $fixtureRoot)
        }
        else {
            try {
                Remove-Item -LiteralPath $fixtureRoot -Recurse -Force -ErrorAction Stop
                Write-Host ('Removed the fixture: ' + $fixtureRoot)
            }
            catch {
                Add-ValidationNote -Context $ctx -Text ('Could not remove the fixture (close it in Obsidian, then delete it): ' + $fixtureRoot)
            }
        }
    }
    if ($registeredInObsidian) {
        Add-ValidationNote -Context $ctx -Text 'The fixture folder was added to Obsidian''s vault list; remove it there (Manage vaults > Remove from list).'
    }
}
$code = Complete-Validation -Context $ctx
exit $code
