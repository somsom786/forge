#Requires -Version 5.1
<#
.SYNOPSIS
    Shared helpers for the FORGE Windows validation scripts in scripts\windows.

.DESCRIPTION
    Every script creates a validation context (New-ValidationContext), records each check with Add-CheckResult (or one of
    the Invoke-* helpers that run a command and record it), and ends with Complete-Validation, which prints the summary
    table, writes the JSON report to <repo>\validation-results\<script>-<timestamp>.json and returns the exit code.

    State lives in the context object, not in the module, so re-importing the module (Import-Module -Force) in a child
    script never loses results.

    Status labels are exactly one of:
        PASS <em dash> Physical Windows   a check passed on a machine that is not GitHub Actions
        PASS <em dash> Windows CI         a check passed inside GitHub Actions ($env:GITHUB_ACTIONS -eq 'true')
        FAIL                              the check ran and failed
        BLOCKED                           a prerequisite is missing, so the check could not run (or could not decide)
        NOT RUN                           skipped on purpose by a parameter

    Exit codes (Complete-Validation): 0 = no FAIL and no required check BLOCKED; 1 = at least one FAIL;
    2 = no FAIL, but at least one required check BLOCKED. Informational checks never change the exit code unless they FAIL.

    This file is pure ASCII on purpose: Windows PowerShell 5.1 reads a BOM-less script as the ANSI code page, so the em dash
    is built from its code point instead of being typed.
#>

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$script:EmDash = [string][char]0x2014
$script:StatusSet = @('PASS', 'FAIL', 'BLOCKED', 'NOT RUN')

function Test-ForgeIsWindows {
    return ([System.Environment]::OSVersion.Platform -eq [System.PlatformID]::Win32NT)
}

function Test-ForgeInCI {
    return ($env:GITHUB_ACTIONS -eq 'true')
}

function Get-ForgeRepoRoot {
    # lib -> windows -> scripts -> repository root
    $scriptsWindows = Split-Path -Parent $PSScriptRoot
    $scripts = Split-Path -Parent $scriptsWindows
    return (Split-Path -Parent $scripts)
}

function Get-StatusLabel {
    param(
        [Parameter(Mandatory = $true)][ValidateSet('PASS', 'FAIL', 'BLOCKED', 'NOT RUN')][string]$Status
    )
    if ($Status -ne 'PASS') { return $Status }
    if (Test-ForgeInCI) { return ('PASS ' + $script:EmDash + ' Windows CI') }
    return ('PASS ' + $script:EmDash + ' Physical Windows')
}

function ConvertTo-SafeFileName {
    param([Parameter(Mandatory = $true)][string]$Name)
    $safe = ($Name -replace '[^A-Za-z0-9._-]+', '-').Trim('-')
    if ($safe.Length -gt 80) { $safe = $safe.Substring(0, 80) }
    if ([string]::IsNullOrEmpty($safe)) { $safe = 'check' }
    return $safe
}

function ConvertTo-ArgumentString {
    <# Quotes arguments the way the Windows C runtime parses them (CommandLineToArgvW rules). #>
    param([string[]]$ArgumentList = @())
    $parts = New-Object System.Collections.Generic.List[string]
    foreach ($a in $ArgumentList) {
        if ($null -eq $a) { continue }
        if ($a.Length -gt 0 -and $a -notmatch '[\s"]') {
            $parts.Add($a)
            continue
        }
        $sb = New-Object System.Text.StringBuilder
        [void]$sb.Append('"')
        $backslashes = 0
        foreach ($ch in $a.ToCharArray()) {
            if ($ch -eq '\') {
                $backslashes++
                continue
            }
            if ($ch -eq '"') {
                [void]$sb.Append(('\' * ($backslashes * 2 + 1)))
                [void]$sb.Append('"')
            }
            else {
                [void]$sb.Append(('\' * $backslashes))
                [void]$sb.Append([string]$ch)
            }
            $backslashes = 0
        }
        [void]$sb.Append(('\' * ($backslashes * 2)))
        [void]$sb.Append('"')
        $parts.Add($sb.ToString())
    }
    return ($parts -join ' ')
}

function Format-CommandLine {
    param([Parameter(Mandatory = $true)][string]$FilePath, [string[]]$ArgumentList = @())
    $exe = $FilePath
    if ($exe -match '\s') { $exe = '"' + $exe + '"' }
    $rest = ConvertTo-ArgumentString -ArgumentList $ArgumentList
    if ([string]::IsNullOrEmpty($rest)) { return $exe }
    return ($exe + ' ' + $rest)
}

function Find-ForgeExecutable {
    <#
    .SYNOPSIS
        Full path of an executable on PATH (.exe, .cmd, .bat; never a .ps1 script), or $null.
    #>
    param([Parameter(Mandatory = $true)][string]$Name)
    $cmd = Get-Command -Name $Name -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($null -eq $cmd) { return $null }
    return $cmd.Source
}

function Get-FirstLine {
    param([string]$Text)
    if ([string]::IsNullOrEmpty($Text)) { return '' }
    foreach ($line in ($Text -split "`r?`n")) {
        $t = $line.Trim()
        if ($t.Length -gt 0) { return $t }
    }
    return ''
}

function New-ValidationContext {
    <#
    .SYNOPSIS
        Creates the results folder, the per-run log folder and the in-memory list of checks.
    #>
    param([Parameter(Mandatory = $true)][string]$ScriptName)

    if (-not (Test-ForgeIsWindows)) {
        throw "$ScriptName validates FORGE on Windows and must run on Windows. Nothing was run."
    }

    $root = Get-ForgeRepoRoot
    $stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
    $resultsDir = Join-Path $root 'validation-results'
    if (-not (Test-Path -LiteralPath $resultsDir)) {
        New-Item -ItemType Directory -Path $resultsDir -Force | Out-Null
    }
    # Keep results out of git without touching the repository's .gitignore.
    $ignoreFile = Join-Path $resultsDir '.gitignore'
    if (-not (Test-Path -LiteralPath $ignoreFile)) {
        [System.IO.File]::WriteAllText($ignoreFile, "*`r`n", (New-Object System.Text.UTF8Encoding($false)))
    }
    $logDir = Join-Path $resultsDir ($ScriptName + '-' + $stamp)
    New-Item -ItemType Directory -Path $logDir -Force | Out-Null

    $looksVirtual = $null
    try {
        $cs = Get-CimInstance -ClassName Win32_ComputerSystem -ErrorAction Stop
        $model = ([string]$cs.Manufacturer + ' ' + [string]$cs.Model)
        $looksVirtual = ($model -match 'Virtual|VMware|QEMU|KVM|Xen|innotek|VirtualBox|Parallels|Hyper-V')
    }
    catch {
        $looksVirtual = $null
    }

    $ctx = [pscustomobject]@{
        ScriptName       = $ScriptName
        Timestamp        = $stamp
        RepoRoot         = $root
        TauriDir         = (Join-Path $root 'apps\desktop\src-tauri')
        ResultsDir       = $resultsDir
        LogDir           = $logDir
        ReportPath       = (Join-Path $resultsDir ($ScriptName + '-' + $stamp + '.json'))
        StartedAt        = (Get-Date)
        InCI             = (Test-ForgeInCI)
        HostLooksVirtual = $looksVirtual
        Checks           = (New-Object System.Collections.ArrayList)
        Notes            = (New-Object System.Collections.ArrayList)
    }

    Write-Host ''
    Write-Host ('=' * 100)
    Write-Host ("FORGE validation: {0}" -f $ScriptName)
    Write-Host ("Repository:       {0}" -f $root)
    Write-Host ("Logs:             {0}" -f $logDir)
    if ($ctx.InCI) {
        Write-Host 'Environment:      GitHub Actions (passes are labelled Windows CI)'
    }
    else {
        Write-Host 'Environment:      not CI (passes are labelled Physical Windows)'
        if ($looksVirtual -eq $true) {
            Write-Host 'WARNING: this machine reports a virtual model. Results are labelled Physical Windows because this is not CI; record that it was a VM.' -ForegroundColor Yellow
        }
    }
    Write-Host ('=' * 100)
    return $ctx
}

function Add-CheckResult {
    <#
    .SYNOPSIS
        Records one check and prints its status line.
    .PARAMETER Informational
        The check reports a fact about the machine; a BLOCKED result does not change the exit code.
    #>
    param(
        [Parameter(Mandatory = $true)]$Context,
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][ValidateSet('PASS', 'FAIL', 'BLOCKED', 'NOT RUN')][string]$Status,
        [string]$Detail = '',
        [string]$Command = '',
        [string]$LogPath = '',
        [double]$DurationSeconds = 0,
        [switch]$Informational
    )
    $label = Get-StatusLabel -Status $Status
    $entry = [pscustomobject]@{
        name            = $Name
        status          = $label
        outcome         = $Status
        required        = (-not $Informational.IsPresent)
        durationSeconds = [math]::Round($DurationSeconds, 1)
        command         = $Command
        logPath         = $LogPath
        detail          = $Detail
    }
    [void]$Context.Checks.Add($entry)

    $color = 'Gray'
    switch ($Status) {
        'PASS' { $color = 'Green' }
        'FAIL' { $color = 'Red' }
        'BLOCKED' { $color = 'Yellow' }
        'NOT RUN' { $color = 'DarkGray' }
    }
    $line = '[' + $label + '] ' + $Name
    if (-not [string]::IsNullOrEmpty($Detail)) { $line = $line + ' -- ' + $Detail }
    Write-Host $line -ForegroundColor $color
}

function Add-ValidationNote {
    <# A statement that belongs in the report (for example what is deliberately not tested). Printed and kept in the JSON. #>
    param([Parameter(Mandatory = $true)]$Context, [Parameter(Mandatory = $true)][string]$Text)
    [void]$Context.Notes.Add($Text)
    Write-Host ('NOTE: ' + $Text) -ForegroundColor Cyan
}

function Get-CheckOutcome {
    <# The plain outcome (PASS/FAIL/BLOCKED/NOT RUN) of the last check recorded with this name, or $null. #>
    param([Parameter(Mandatory = $true)]$Context, [Parameter(Mandatory = $true)][string]$Name)
    $found = $null
    foreach ($c in $Context.Checks) {
        if ($c.name -eq $Name) { $found = $c.outcome }
    }
    return $found
}

function Invoke-LoggedCommand {
    <#
    .SYNOPSIS
        Runs a native command from a working directory, streams its output to the console and to a log file, and returns
        the exit code. Never throws for a failing command; returns ExitCode -1 if the command could not be started.
    #>
    param(
        [Parameter(Mandatory = $true)]$Context,
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][string]$FilePath,
        [string[]]$ArgumentList = @(),
        [string]$WorkingDirectory = '',
        [switch]$Quiet
    )
    if ([string]::IsNullOrEmpty($WorkingDirectory)) { $WorkingDirectory = $Context.RepoRoot }
    $logPath = Join-Path $Context.LogDir ((ConvertTo-SafeFileName -Name $Name) + '.log')
    $display = Format-CommandLine -FilePath $FilePath -ArgumentList $ArgumentList
    $exitCode = -1
    $startError = ''
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    $writer = New-Object System.IO.StreamWriter($logPath, $false, (New-Object System.Text.UTF8Encoding($false)))
    $previousPreference = $ErrorActionPreference
    $previousEncoding = $null
    try {
        # PowerShell decodes native output with the console encoding; npm, vitest and cargo write UTF-8.
        $previousEncoding = [Console]::OutputEncoding
        [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
    }
    catch {
        $previousEncoding = $null  # no console (for example a redirected CI step); keep the default
    }
    Push-Location -LiteralPath $WorkingDirectory
    try {
        $writer.WriteLine('> ' + $display)
        $writer.WriteLine('cwd: ' + $WorkingDirectory)
        $writer.WriteLine('')
        $writer.Flush()
        Write-Host ''
        Write-Host ('>>> ' + $Name) -ForegroundColor Cyan
        Write-Host ('    ' + $display + '   (in ' + $WorkingDirectory + ')') -ForegroundColor DarkCyan
        # Windows PowerShell turns every stderr line of a native command into an error record when 2>&1 is used;
        # with 'Stop' the first progress line from cargo or npm would abort the script.
        $ErrorActionPreference = 'Continue'
        $global:LASTEXITCODE = 0
        & $FilePath @ArgumentList 2>&1 | ForEach-Object {
            $text = $null
            if ($_ -is [System.Management.Automation.ErrorRecord]) { $text = $_.Exception.Message } else { $text = [string]$_ }
            $writer.WriteLine($text)
            if (-not $Quiet) { Write-Host $text }
        }
        $exitCode = $LASTEXITCODE
        if ($null -eq $exitCode) { $exitCode = 0 }
    }
    catch {
        $startError = $_.Exception.Message
        $writer.WriteLine('FAILED TO RUN: ' + $startError)
        $exitCode = -1
    }
    finally {
        $ErrorActionPreference = $previousPreference
        $sw.Stop()
        $writer.WriteLine('')
        $writer.WriteLine('exit code: ' + $exitCode + '; duration: ' + [math]::Round($sw.Elapsed.TotalSeconds, 1) + ' s')
        $writer.Dispose()
        Pop-Location
        if ($null -ne $previousEncoding) {
            try { [Console]::OutputEncoding = $previousEncoding } catch { $null = $_ }
        }
    }
    return [pscustomobject]@{
        ExitCode        = $exitCode
        LogPath         = $logPath
        Command         = $display
        DurationSeconds = $sw.Elapsed.TotalSeconds
        StartError      = $startError
    }
}

function Invoke-NativeCapture {
    <#
    .SYNOPSIS
        Runs a short native command with stdin closed and a hard timeout, and returns its exit code and combined output
        in memory. Nothing is written to disk; callers decide what (if anything) to print.
    #>
    param(
        [Parameter(Mandatory = $true)][string]$FilePath,
        [string[]]$ArgumentList = @(),
        [int]$TimeoutSeconds = 30,
        [string]$WorkingDirectory = '',
        [hashtable]$Environment = @{}
    )
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $FilePath
    $psi.Arguments = (ConvertTo-ArgumentString -ArgumentList $ArgumentList)
    $psi.UseShellExecute = $false
    $psi.RedirectStandardInput = $true
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.CreateNoWindow = $true
    if (-not [string]::IsNullOrEmpty($WorkingDirectory)) { $psi.WorkingDirectory = $WorkingDirectory }
    foreach ($k in $Environment.Keys) { $psi.EnvironmentVariables[[string]$k] = [string]$Environment[$k] }

    $p = New-Object System.Diagnostics.Process
    $p.StartInfo = $psi
    try {
        [void]$p.Start()
    }
    catch {
        return [pscustomobject]@{ Started = $false; TimedOut = $false; ExitCode = -1; Output = ''; Error = $_.Exception.Message }
    }
    try {
        $p.StandardInput.Close()
        $outTask = $p.StandardOutput.ReadToEndAsync()
        $errTask = $p.StandardError.ReadToEndAsync()
        if (-not $p.WaitForExit($TimeoutSeconds * 1000)) {
            $taskkill = Join-Path $env:SystemRoot 'System32\taskkill.exe'
            try { & $taskkill /PID $p.Id /T /F 2>&1 | Out-Null } catch { $null = $_ }
            [void]$p.WaitForExit(5000)
            return [pscustomobject]@{ Started = $true; TimedOut = $true; ExitCode = -1; Output = ''; Error = ('timed out after ' + $TimeoutSeconds + ' s') }
        }
        $p.WaitForExit()
        $text = ($outTask.Result + "`n" + $errTask.Result) -replace "`0", ''
        return [pscustomobject]@{ Started = $true; TimedOut = $false; ExitCode = $p.ExitCode; Output = $text; Error = '' }
    }
    finally {
        $p.Dispose()
    }
}

function Get-CargoTestSummary {
    <# Sums every "test result:" line of a cargo test log. #>
    param([Parameter(Mandatory = $true)][string]$LogPath)
    $passed = 0; $failed = 0; $ignored = 0; $filtered = 0; $found = $false
    if (Test-Path -LiteralPath $LogPath) {
        foreach ($line in (Get-Content -LiteralPath $LogPath -Encoding UTF8)) {
            $m = [regex]::Match($line, 'test result: \w+\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out')
            if ($m.Success) {
                $found = $true
                $passed += [int]$m.Groups[1].Value
                $failed += [int]$m.Groups[2].Value
                $ignored += [int]$m.Groups[3].Value
                $filtered += [int]$m.Groups[5].Value
            }
        }
    }
    return [pscustomobject]@{ Found = $found; Passed = $passed; Failed = $failed; Ignored = $ignored; FilteredOut = $filtered }
}

function Invoke-CommandCheck {
    <#
    .SYNOPSIS
        Runs a command as a check: PASS when the exit code is 0, FAIL otherwise.
    #>
    param(
        [Parameter(Mandatory = $true)]$Context,
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][string]$FilePath,
        [string[]]$ArgumentList = @(),
        [string]$WorkingDirectory = '',
        [string]$PassDetail = ''
    )
    $r = Invoke-LoggedCommand -Context $Context -Name $Name -FilePath $FilePath -ArgumentList $ArgumentList -WorkingDirectory $WorkingDirectory
    if ($r.ExitCode -eq 0) {
        $detail = $PassDetail
        $vitest = Select-String -LiteralPath $r.LogPath -Pattern '^\s*Tests\s+(.+)$' -ErrorAction SilentlyContinue | Select-Object -Last 1
        if ($null -ne $vitest -and [string]::IsNullOrEmpty($detail)) { $detail = 'Tests ' + $vitest.Matches[0].Groups[1].Value.Trim() }
        Add-CheckResult -Context $Context -Name $Name -Status PASS -Detail $detail -Command $r.Command -LogPath $r.LogPath -DurationSeconds $r.DurationSeconds
        return $true
    }
    $why = 'exit code ' + $r.ExitCode
    if (-not [string]::IsNullOrEmpty($r.StartError)) { $why = 'could not start: ' + $r.StartError }
    Add-CheckResult -Context $Context -Name $Name -Status FAIL -Detail ($why + '; see log') -Command $r.Command -LogPath $r.LogPath -DurationSeconds $r.DurationSeconds
    return $false
}

function Invoke-CargoTestCheck {
    <#
    .SYNOPSIS
        Runs `cargo test --lib --locked [filter]` in apps\desktop\src-tauri. Never passes --ignored or --include-ignored:
        tests marked #[ignore] are live tests that need provider logins or spend quota and are reported, never run.
        A filter that matches no test is a FAIL (a green run that tested nothing proves nothing).
    #>
    param(
        [Parameter(Mandatory = $true)]$Context,
        [Parameter(Mandatory = $true)][string]$Name,
        [string]$Filter = '',
        [Parameter(Mandatory = $true)][string]$Cargo
    )
    $cargoArgs = @('test', '--lib', '--locked')
    if (-not [string]::IsNullOrEmpty($Filter)) { $cargoArgs += $Filter }
    foreach ($a in $cargoArgs) {
        if ($a -match 'ignored') { throw 'refusing to run ignored (live) tests' }
    }
    $r = Invoke-LoggedCommand -Context $Context -Name $Name -FilePath $Cargo -ArgumentList $cargoArgs -WorkingDirectory $Context.TauriDir
    $s = Get-CargoTestSummary -LogPath $r.LogPath
    $counts = '{0} passed, {1} failed, {2} ignored (live tests, not run), {3} filtered out' -f $s.Passed, $s.Failed, $s.Ignored, $s.FilteredOut
    if ($r.ExitCode -ne 0) {
        $why = 'exit code ' + $r.ExitCode
        if ($s.Found) { $why = $why + '; ' + $counts }
        Add-CheckResult -Context $Context -Name $Name -Status FAIL -Detail ($why + '; see log') -Command $r.Command -LogPath $r.LogPath -DurationSeconds $r.DurationSeconds
        return $false
    }
    if (-not $s.Found -or $s.Passed -eq 0) {
        Add-CheckResult -Context $Context -Name $Name -Status FAIL -Detail ('no test ran (the filter matched nothing); ' + $counts) -Command $r.Command -LogPath $r.LogPath -DurationSeconds $r.DurationSeconds
        return $false
    }
    Add-CheckResult -Context $Context -Name $Name -Status PASS -Detail $counts -Command $r.Command -LogPath $r.LogPath -DurationSeconds $r.DurationSeconds
    return $true
}

function Get-ForgeTools {
    <# Locates the tools most checks need. Values are full paths or $null. #>
    return [pscustomobject]@{
        Git   = (Find-ForgeExecutable -Name 'git')
        Node  = (Find-ForgeExecutable -Name 'node')
        Npm   = (Find-ForgeExecutable -Name 'npm.cmd')
        Cargo = (Find-ForgeExecutable -Name 'cargo')
    }
}

function Test-EngineReady {
    <#
    .SYNOPSIS
        Makes sure packages\engine\dist\main.js exists (the Rust supervisor tests start it). Builds it when node_modules is
        present; records a BLOCKED check when it cannot. Returns $true when the engine is ready.
    #>
    param([Parameter(Mandatory = $true)]$Context, [Parameter(Mandatory = $true)]$Tools)
    $engineMain = Join-Path $Context.RepoRoot 'packages\engine\dist\main.js'
    $nodeModules = Join-Path $Context.RepoRoot 'node_modules'
    if ($null -eq $Tools.Node -or $null -eq $Tools.Npm) {
        Add-CheckResult -Context $Context -Name 'Engine build (npm run build:engine)' -Status BLOCKED -Detail 'node or npm.cmd not found on PATH'
        return $false
    }
    if (-not (Test-Path -LiteralPath $nodeModules)) {
        Add-CheckResult -Context $Context -Name 'Engine build (npm run build:engine)' -Status BLOCKED -Detail 'node_modules is missing; run scripts\windows\verify-windows.ps1 (it runs npm ci) first'
        return $false
    }
    $ok = Invoke-CommandCheck -Context $Context -Name 'Engine build (npm run build:engine)' -FilePath $Tools.Npm -ArgumentList @('run', 'build:engine')
    return ($ok -and (Test-Path -LiteralPath $engineMain))
}

function Get-GitState {
    param([Parameter(Mandatory = $true)][string]$RepoRoot)
    $git = Find-ForgeExecutable -Name 'git'
    if ($null -eq $git) { return [pscustomobject]@{ commit = $null; branch = $null; dirty = $null } }
    $c = Invoke-NativeCapture -FilePath $git -ArgumentList @('-C', $RepoRoot, 'rev-parse', 'HEAD') -TimeoutSeconds 20
    $b = Invoke-NativeCapture -FilePath $git -ArgumentList @('-C', $RepoRoot, 'rev-parse', '--abbrev-ref', 'HEAD') -TimeoutSeconds 20
    $s = Invoke-NativeCapture -FilePath $git -ArgumentList @('-C', $RepoRoot, 'status', '--porcelain', '--untracked-files=no') -TimeoutSeconds 60
    $dirty = $null
    if ($s.ExitCode -eq 0) { $dirty = (-not [string]::IsNullOrWhiteSpace($s.Output)) }
    return [pscustomobject]@{
        commit = (Get-FirstLine -Text $c.Output)
        branch = (Get-FirstLine -Text $b.Output)
        dirty  = $dirty
    }
}

function Complete-Validation {
    <#
    .SYNOPSIS
        Prints the summary table, writes the JSON report and returns the exit code (0, 1 or 2; see the module help).
    #>
    param([Parameter(Mandatory = $true)]$Context)
    $finished = Get-Date
    $fails = @($Context.Checks | Where-Object { $_.outcome -eq 'FAIL' }).Count
    $blockedRequired = @($Context.Checks | Where-Object { $_.outcome -eq 'BLOCKED' -and $_.required }).Count
    $blockedInfo = @($Context.Checks | Where-Object { $_.outcome -eq 'BLOCKED' -and -not $_.required }).Count
    $passes = @($Context.Checks | Where-Object { $_.outcome -eq 'PASS' }).Count
    $notRun = @($Context.Checks | Where-Object { $_.outcome -eq 'NOT RUN' }).Count

    $exitCode = 0
    $overall = 'PASS'
    if ($fails -gt 0) { $exitCode = 1; $overall = 'FAIL' }
    elseif ($blockedRequired -gt 0) { $exitCode = 2; $overall = 'INCOMPLETE (required checks BLOCKED)' }

    $git = Get-GitState -RepoRoot $Context.RepoRoot
    $os = [System.Environment]::OSVersion.VersionString
    try {
        $cv = Get-ItemProperty -LiteralPath 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion' -ErrorAction Stop
        $os = ([string]$cv.ProductName + ' ' + [string]$cv.DisplayVersion + ' (build ' + [string]$cv.CurrentBuild + ')')
    }
    catch { $null = $_ }

    $report = [pscustomobject]@{
        script            = $Context.ScriptName
        startedAt         = $Context.StartedAt.ToString('o')
        finishedAt        = $finished.ToString('o')
        durationSeconds   = [math]::Round(($finished - $Context.StartedAt).TotalSeconds, 1)
        overall           = $overall
        exitCode          = $exitCode
        counts            = [pscustomobject]@{ pass = $passes; fail = $fails; blockedRequired = $blockedRequired; blockedInformational = $blockedInfo; notRun = $notRun }
        environment       = [pscustomobject]@{
            githubActions      = $Context.InCI
            hostLooksVirtual   = $Context.HostLooksVirtual
            os                 = $os
            powershell         = $PSVersionTable.PSVersion.ToString()
            powershellEdition  = [string]$PSVersionTable.PSEdition
        }
        repository        = [pscustomobject]@{ root = $Context.RepoRoot; commit = $git.commit; branch = $git.branch; trackedChanges = $git.dirty }
        logDirectory      = $Context.LogDir
        statusLabels      = @((Get-StatusLabel -Status PASS), 'FAIL', 'BLOCKED', 'NOT RUN')
        notes             = @($Context.Notes)
        checks            = @($Context.Checks)
    }
    $json = $report | ConvertTo-Json -Depth 8
    [System.IO.File]::WriteAllText($Context.ReportPath, $json, (New-Object System.Text.UTF8Encoding($false)))

    Write-Host ''
    Write-Host ('=' * 100)
    Write-Host ('Summary: ' + $Context.ScriptName)
    Write-Host ('=' * 100)
    $Context.Checks | Format-Table -AutoSize -Wrap -Property @(
        @{ Label = 'Status'; Expression = { $_.status } },
        @{ Label = 'Check'; Expression = { $_.name } },
        @{ Label = 'Seconds'; Expression = { $_.durationSeconds } },
        @{ Label = 'Detail'; Expression = { $_.detail } }
    ) | Out-String -Width 220 | Write-Host
    Write-Host ('Passed: {0}   Failed: {1}   Blocked (required): {2}   Blocked (informational): {3}   Not run: {4}' -f $passes, $fails, $blockedRequired, $blockedInfo, $notRun)
    if ($null -ne $git.commit) {
        $dirtyText = ''
        if ($git.dirty -eq $true) { $dirtyText = ' (with uncommitted changes to tracked files)' }
        Write-Host ('Commit: {0} on {1}{2}' -f $git.commit, $git.branch, $dirtyText)
    }
    Write-Host ('Report: ' + $Context.ReportPath)
    Write-Host ('Logs:   ' + $Context.LogDir)
    $color = 'Green'
    if ($exitCode -eq 1) { $color = 'Red' } elseif ($exitCode -eq 2) { $color = 'Yellow' }
    Write-Host ('Overall: ' + $overall + ' (exit code ' + $exitCode + ')') -ForegroundColor $color
    return $exitCode
}

function Invoke-ChildValidationScript {
    <#
    .SYNOPSIS
        Runs another validation script in a separate PowerShell process (same edition as this one) and records its result
        as one check: exit 0 -> PASS, 1 -> FAIL, 2 -> BLOCKED. The child writes its own JSON report.
    #>
    param(
        [Parameter(Mandatory = $true)]$Context,
        [Parameter(Mandatory = $true)][string]$ScriptFile,
        [string[]]$ArgumentList = @()
    )
    $scriptPath = Join-Path (Split-Path -Parent $PSScriptRoot) $ScriptFile
    $psExe = (Get-Process -Id $PID).Path
    $childArgs = @('-NoProfile', '-File', $scriptPath) + $ArgumentList
    $display = Format-CommandLine -FilePath $psExe -ArgumentList $childArgs
    $baseName = [System.IO.Path]::GetFileNameWithoutExtension($ScriptFile)
    $before = Get-Date
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    Write-Host ''
    Write-Host ('>>> ' + $ScriptFile + ' (child process)') -ForegroundColor Cyan
    $exitCode = -1
    try {
        # -NoNewWindow: the child shares this console (it may ask the person questions with Read-Host).
        # WaitForExit() rather than -Wait: Windows PowerShell's -Wait also waits for every descendant, such as an
        # Obsidian window opened by test-obsidian.ps1.
        $child = Start-Process -FilePath $psExe -ArgumentList (ConvertTo-ArgumentString -ArgumentList $childArgs) -NoNewWindow -PassThru
        $null = $child.Handle  # keep a handle so ExitCode is available after the process ends
        $child.WaitForExit()
        $exitCode = $child.ExitCode
    }
    catch {
        Write-Host ('Could not run ' + $ScriptFile + ': ' + $_.Exception.Message) -ForegroundColor Red
        $exitCode = -1
    }
    finally {
        $sw.Stop()
    }
    $childReport = Get-ChildItem -LiteralPath $Context.ResultsDir -Filter ($baseName + '-*.json') -File -ErrorAction SilentlyContinue |
        Where-Object { $_.LastWriteTime -ge $before } | Sort-Object LastWriteTime | Select-Object -Last 1
    $reportPath = ''
    if ($null -ne $childReport) { $reportPath = $childReport.FullName }
    $name = $ScriptFile + ' (see its own report)'
    switch ($exitCode) {
        0 { Add-CheckResult -Context $Context -Name $name -Status PASS -Detail 'all required checks passed' -Command $display -LogPath $reportPath -DurationSeconds $sw.Elapsed.TotalSeconds }
        2 { Add-CheckResult -Context $Context -Name $name -Status BLOCKED -Detail 'no failure, but required checks were BLOCKED' -Command $display -LogPath $reportPath -DurationSeconds $sw.Elapsed.TotalSeconds }
        default { Add-CheckResult -Context $Context -Name $name -Status FAIL -Detail ('exit code ' + $exitCode) -Command $display -LogPath $reportPath -DurationSeconds $sw.Elapsed.TotalSeconds }
    }
}

Export-ModuleMember -Function @(
    'Test-ForgeIsWindows', 'Test-ForgeInCI', 'Get-ForgeRepoRoot', 'Get-StatusLabel', 'ConvertTo-SafeFileName',
    'ConvertTo-ArgumentString', 'Format-CommandLine', 'Find-ForgeExecutable', 'Get-FirstLine', 'New-ValidationContext',
    'Add-CheckResult', 'Add-ValidationNote', 'Get-CheckOutcome', 'Invoke-LoggedCommand', 'Invoke-NativeCapture', 'Get-CargoTestSummary',
    'Invoke-CommandCheck', 'Invoke-CargoTestCheck', 'Get-ForgeTools', 'Test-EngineReady', 'Get-GitState',
    'Complete-Validation', 'Invoke-ChildValidationScript'
)
