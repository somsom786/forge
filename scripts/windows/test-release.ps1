#Requires -Version 5.1
<#
.SYNOPSIS
    Builds the FORGE NSIS installer, hashes it, installs it silently into a temporary folder, optionally launches the
    installed app, then uninstalls it silently and checks that it is gone.

.DESCRIPTION
    Steps (each one check):
      1. npm run package            (repository root; = tauri build --config src-tauri\tauri.bundle.conf.json: NSIS,
                                     per-user install mode, engine bundle + node.exe staged by tools\package\prepare.mjs)
      2. SHA-256 of the produced installer (<target>\release\bundle\nsis\*-setup.exe) and of the app executable
         (<target>\release\forge-desktop.exe); written to SHA256SUMS.txt in the log folder
      3. <installer> /S /D=%TEMP%\forge-release-test-<timestamp>\FORGE       (silent per-user install, NSIS flags)
      4. the installed app executable, uninstall.exe, engine\main.js and node\node.exe exist; the uninstall entry exists
      5. with -Launch: start the installed app with FORGE_HOME and WEBVIEW2_USER_DATA_FOLDER pointing into the temporary
         folder (your real FORGE data is never opened), check it is still running after -LaunchSeconds, then stop it
      6. <install dir>\uninstall.exe /S, then check the executable and the uninstall entry are gone

    The installer is unsigned. Nothing is signed, published or uploaded anywhere.

    Safety: a per-user FORGE installation shares the uninstall entry and Start-menu shortcut with this test install, so if
    FORGE is already installed for this user the install steps are BLOCKED unless you pass -ReplaceExistingInstall (the
    test's uninstall would then remove that registration; reinstall FORGE afterwards). Your FORGE data folder
    (%USERPROFILE%\.forge) is never used by this script.

    Exit code: 0 = all required checks passed; 1 = a FAIL; 2 = no FAIL but a required check BLOCKED.
    Report: validation-results\test-release-<timestamp>.json

.PARAMETER InstallerPath
    Use this already-built installer instead of building one (skips step 1).
.PARAMETER Launch
    Start the installed app and check that it keeps running for -LaunchSeconds.
.PARAMETER LaunchSeconds
    How long the launched app must stay up (default 20).
.PARAMETER ReplaceExistingInstall
    Proceed even though FORGE is already installed for this user (see Safety above).

.EXAMPLE
    .\scripts\windows\test-release.ps1 -Launch
.EXAMPLE
    .\scripts\windows\test-release.ps1 -InstallerPath .\apps\desktop\src-tauri\target\release\bundle\nsis\FORGE_0.1.0_x64-setup.exe
#>
[CmdletBinding()]
param(
    [string]$InstallerPath = '',
    [switch]$Launch,
    [ValidateRange(5, 600)][int]$LaunchSeconds = 20,
    [switch]$ReplaceExistingInstall
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Import-Module "$PSScriptRoot\lib\ForgeValidation.psm1" -Force

function Get-UninstallEntry {
    <# Uninstall registry entries whose DisplayName is the product name (per-user and per-machine views). #>
    param([Parameter(Mandatory = $true)][string]$ProductName)
    $roots = @(
        'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall',
        'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall',
        'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall'
    )
    $found = New-Object System.Collections.ArrayList
    foreach ($root in $roots) {
        if (-not (Test-Path -LiteralPath $root)) { continue }
        foreach ($key in (Get-ChildItem -LiteralPath $root -ErrorAction SilentlyContinue)) {
            $p = Get-ItemProperty -LiteralPath $key.PSPath -ErrorAction SilentlyContinue
            if ($null -eq $p -or $null -eq $p.PSObject.Properties['DisplayName']) { continue }
            if ([string]$p.DisplayName -ne $ProductName) { continue }
            $loc = ''
            if ($null -ne $p.PSObject.Properties['InstallLocation']) { $loc = [string]$p.InstallLocation }
            $ver = ''
            if ($null -ne $p.PSObject.Properties['DisplayVersion']) { $ver = [string]$p.DisplayVersion }
            $uninst = ''
            if ($null -ne $p.PSObject.Properties['UninstallString']) { $uninst = [string]$p.UninstallString }
            [void]$found.Add([pscustomobject]@{ Key = $key.Name; InstallLocation = $loc; Version = $ver; UninstallString = $uninst })
        }
    }
    return , $found
}

function Test-EntryPointsAt {
    <# Does an uninstall entry belong to the installation in this directory? (NSIS may store the location quoted.) #>
    param([Parameter(Mandatory = $true)]$Entry, [Parameter(Mandatory = $true)][string]$Directory)
    $dir = $Directory.TrimEnd('\')
    $loc = $Entry.InstallLocation.Trim().Trim('"').TrimEnd('\')
    if ($loc -ne '' -and $loc -ieq $dir) { return $true }
    return ($Entry.UninstallString.IndexOf($dir, [System.StringComparison]::OrdinalIgnoreCase) -ge 0)
}

function Stop-ProcessTree {
    param([Parameter(Mandatory = $true)][int]$ProcessId)
    $taskkill = Join-Path $env:SystemRoot 'System32\taskkill.exe'
    $null = Invoke-NativeCapture -FilePath $taskkill -ArgumentList @('/PID', [string]$ProcessId, '/T', '/F') -TimeoutSeconds 30
}

function Wait-PathGone {
    param([Parameter(Mandatory = $true)][string]$Path, [int]$TimeoutSeconds = 120)
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    while ((Test-Path -LiteralPath $Path) -and $sw.Elapsed.TotalSeconds -lt $TimeoutSeconds) {
        Start-Sleep -Milliseconds 500
    }
    return (-not (Test-Path -LiteralPath $Path))
}

$ctx = New-ValidationContext -ScriptName 'test-release'
$installRoot = $null
$installDir = $null
$installed = $false
$uninstallDone = $false
$appExe = $null
try {
    Add-ValidationNote -Context $ctx -Text 'The installer is unsigned. Nothing is signed, published or uploaded by this script.'
    $tools = Get-ForgeTools
    $conf = Get-Content -LiteralPath (Join-Path $ctx.TauriDir 'tauri.conf.json') -Raw -Encoding UTF8 | ConvertFrom-Json
    $productName = [string]$conf.productName
    $version = [string]$conf.version

    # ---- 0. existing installation ------------------------------------------------------------------------------------
    $existing = Get-UninstallEntry -ProductName $productName
    $installAllowed = $true
    if ($existing.Count -gt 0) {
        $where = (($existing | ForEach-Object { $_.Key + ' -> ' + $_.InstallLocation + ' (' + $_.Version + ')' }) -join '; ')
        if ($ReplaceExistingInstall) {
            Add-CheckResult -Context $ctx -Name 'No existing FORGE installation for this user' -Status PASS -Detail ('existing installation found but -ReplaceExistingInstall was given; its registration will be removed by the test uninstall: ' + $where)
        }
        else {
            $installAllowed = $false
            Add-CheckResult -Context $ctx -Name 'No existing FORGE installation for this user' -Status BLOCKED -Detail ('FORGE is already installed (' + $where + '). Uninstall it first, or rerun with -ReplaceExistingInstall')
        }
    }
    else {
        Add-CheckResult -Context $ctx -Name 'No existing FORGE installation for this user' -Status PASS -Detail ('no uninstall entry named ' + $productName)
    }

    # ---- 1. build -----------------------------------------------------------------------------------------------------
    $buildName = 'Build installer: npm run package (NSIS, per-user)'
    $installer = $null
    if (-not [string]::IsNullOrEmpty($InstallerPath)) {
        if (Test-Path -LiteralPath $InstallerPath -PathType Leaf) {
            $installer = (Resolve-Path -LiteralPath $InstallerPath).Path
            Add-CheckResult -Context $ctx -Name $buildName -Status 'NOT RUN' -Detail ('-InstallerPath given: ' + $installer)
        }
        else {
            Add-CheckResult -Context $ctx -Name $buildName -Status BLOCKED -Detail ('-InstallerPath does not exist: ' + $InstallerPath)
        }
    }
    elseif ($null -eq $tools.Node -or $null -eq $tools.Npm -or $null -eq $tools.Cargo) {
        Add-CheckResult -Context $ctx -Name $buildName -Status BLOCKED -Detail 'node, npm.cmd and cargo are required (run verify-windows.ps1 to see which is missing)'
    }
    elseif (-not (Test-Path -LiteralPath (Join-Path $ctx.RepoRoot 'node_modules'))) {
        Add-CheckResult -Context $ctx -Name $buildName -Status BLOCKED -Detail 'node_modules is missing; run verify-windows.ps1 (npm ci) first'
    }
    else {
        $targetDir = Join-Path $ctx.TauriDir 'target'
        if (-not [string]::IsNullOrEmpty($env:CARGO_TARGET_DIR)) {
            $targetDir = $env:CARGO_TARGET_DIR
            if (-not [System.IO.Path]::IsPathRooted($targetDir)) { $targetDir = Join-Path $ctx.TauriDir $targetDir }
        }
        $nsisDir = Join-Path $targetDir 'release\bundle\nsis'
        $buildStart = (Get-Date).AddSeconds(-5)
        $r = Invoke-LoggedCommand -Context $ctx -Name $buildName -FilePath $tools.Npm -ArgumentList @('run', 'package') -WorkingDirectory $ctx.RepoRoot
        $produced = @()
        if (Test-Path -LiteralPath $nsisDir) {
            $produced = @(Get-ChildItem -LiteralPath $nsisDir -Filter '*-setup.exe' -File | Where-Object { $_.LastWriteTime -ge $buildStart })
        }
        if ($r.ExitCode -ne 0) {
            Add-CheckResult -Context $ctx -Name $buildName -Status FAIL -Detail ('exit code ' + $r.ExitCode + '; see log') -Command $r.Command -LogPath $r.LogPath -DurationSeconds $r.DurationSeconds
        }
        elseif ($produced.Count -ne 1) {
            Add-CheckResult -Context $ctx -Name $buildName -Status FAIL -Detail ('expected exactly one new *-setup.exe in ' + $nsisDir + ', found ' + $produced.Count) -Command $r.Command -LogPath $r.LogPath -DurationSeconds $r.DurationSeconds
        }
        else {
            $installer = $produced[0].FullName
            $mib = [math]::Round($produced[0].Length / 1MB, 2)
            Add-CheckResult -Context $ctx -Name $buildName -Status PASS -Detail ($installer + ' (' + $mib + ' MiB; tauri.conf.json version ' + $version + ')') -Command $r.Command -LogPath $r.LogPath -DurationSeconds $r.DurationSeconds
        }
    }

    # ---- 2. hashes ----------------------------------------------------------------------------------------------------
    $hashName = 'SHA-256 of the produced artifacts'
    if ($null -eq $installer) {
        Add-CheckResult -Context $ctx -Name $hashName -Status BLOCKED -Detail 'no installer'
    }
    else {
        $files = New-Object System.Collections.ArrayList
        [void]$files.Add($installer)
        $releaseDir = Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $installer))
        $rawExe = Join-Path $releaseDir 'forge-desktop.exe'
        if (Test-Path -LiteralPath $rawExe -PathType Leaf) { [void]$files.Add($rawExe) }
        $lines = New-Object System.Collections.Generic.List[string]
        foreach ($f in $files) {
            $h = Get-FileHash -LiteralPath $f -Algorithm SHA256
            $lines.Add(($h.Hash.ToLowerInvariant() + '  ' + (Split-Path -Leaf $f)))
        }
        $sumsPath = Join-Path $ctx.LogDir 'SHA256SUMS.txt'
        [System.IO.File]::WriteAllText($sumsPath, (($lines -join "`r`n") + "`r`n"), (New-Object System.Text.UTF8Encoding($false)))
        Add-CheckResult -Context $ctx -Name $hashName -Status PASS -Detail ($lines -join '; ') -Command 'Get-FileHash -Algorithm SHA256' -LogPath $sumsPath
    }

    # ---- 3. silent install --------------------------------------------------------------------------------------------
    $installName = 'Silent install into a temporary folder (/S /D=...)'
    if ($null -eq $installer) {
        Add-CheckResult -Context $ctx -Name $installName -Status BLOCKED -Detail 'no installer'
    }
    elseif (-not $installAllowed) {
        Add-CheckResult -Context $ctx -Name $installName -Status BLOCKED -Detail 'FORGE is already installed for this user (see above)'
    }
    else {
        $installRoot = Join-Path ([System.IO.Path]::GetFullPath($env:TEMP)) ('forge-release-test-' + $ctx.Timestamp)
        $installDir = Join-Path $installRoot $productName
        New-Item -ItemType Directory -Path $installRoot -Force | Out-Null
        # NSIS: /D= must be the last argument and must not be quoted, even if the path has spaces.
        $argLine = '/S /D=' + $installDir
        $cmdText = '"' + $installer + '" ' + $argLine
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        $p = Start-Process -FilePath $installer -ArgumentList $argLine -PassThru
        $null = $p.Handle  # keep a handle so ExitCode is available after the process ends
        $finished = $p.WaitForExit(600000)
        $sw.Stop()
        if (-not $finished) {
            Stop-ProcessTree -ProcessId $p.Id
            Add-CheckResult -Context $ctx -Name $installName -Status FAIL -Detail 'the installer did not finish within 10 minutes' -Command $cmdText -DurationSeconds $sw.Elapsed.TotalSeconds
        }
        elseif ($p.ExitCode -ne 0) {
            Add-CheckResult -Context $ctx -Name $installName -Status FAIL -Detail ('installer exit code ' + $p.ExitCode) -Command $cmdText -DurationSeconds $sw.Elapsed.TotalSeconds
        }
        else {
            $installed = $true
            Add-CheckResult -Context $ctx -Name $installName -Status PASS -Detail $installDir -Command $cmdText -DurationSeconds $sw.Elapsed.TotalSeconds
        }
    }

    # ---- 4. installed files ------------------------------------------------------------------------------------------
    $filesName = 'Installed files and manifest checksums; uninstall entry'
    if (-not $installed) {
        Add-CheckResult -Context $ctx -Name $filesName -Status BLOCKED -Detail 'not installed'
    }
    else {
        $exes = @(Get-ChildItem -LiteralPath $installDir -Filter '*.exe' -File -ErrorAction SilentlyContinue | Where-Object { $_.Name -ine 'uninstall.exe' })
        $preferred = @($exes | Where-Object { $_.Name -ieq ($productName + '.exe') -or $_.Name -ieq 'forge-desktop.exe' })
        if ($preferred.Count -ge 1) { $appExe = $preferred[0].FullName } elseif ($exes.Count -eq 1) { $appExe = $exes[0].FullName }
        $missing = New-Object System.Collections.Generic.List[string]
        if ($null -eq $appExe) { $missing.Add('app executable (found: ' + (($exes | ForEach-Object { $_.Name }) -join ', ') + ')') }
        foreach ($rel in @('uninstall.exe', 'engine\main.js', 'engine\inputs.json', 'node\node.exe', 'THIRD_PARTY_NOTICES.txt', 'BUILD_MANIFEST.json', 'CLAUDE_SDK_SETUP.txt')) {
            if (-not (Test-Path -LiteralPath (Join-Path $installDir $rel) -PathType Leaf)) { $missing.Add($rel) }
        }
        $entries = @((Get-UninstallEntry -ProductName $productName) | Where-Object { Test-EntryPointsAt -Entry $_ -Directory $installDir })
        if ($entries.Count -eq 0) { $missing.Add('uninstall registry entry pointing at ' + $installDir) }
        if ($missing.Count -eq 0) {
            try {
                $manifest = Get-Content -LiteralPath (Join-Path $installDir 'BUILD_MANIFEST.json') -Raw | ConvertFrom-Json
                foreach ($pair in @(@('engine\main.js', $manifest.engineSha256), @('engine\inputs.json', $manifest.engineInputsSha256), @('node\node.exe', $manifest.node.sha256))) {
                    $actual = (Get-FileHash -LiteralPath (Join-Path $installDir $pair[0]) -Algorithm SHA256).Hash.ToLowerInvariant()
                    if ($actual -ne $pair[1]) { $missing.Add('build manifest hash mismatch: ' + $pair[0]) }
                }
                $inputs = @(Get-Content -LiteralPath (Join-Path $installDir 'engine\inputs.json') -Raw | ConvertFrom-Json)
                if (@($inputs | Where-Object { $_ -match 'node_modules[/\\]@anthropic-ai[/\\]' }).Count -gt 0) { $missing.Add('Anthropic SDK code included in engine inputs') }
            }
            catch { $missing.Add('invalid build manifest: ' + $_.Exception.Message) }
        }
        if ($missing.Count -eq 0) {
            Add-CheckResult -Context $ctx -Name $filesName -Status PASS -Detail ($appExe + '; uninstall entry ' + $entries[0].Key + ' (version ' + $entries[0].Version + ')')
        }
        else {
            Add-CheckResult -Context $ctx -Name $filesName -Status FAIL -Detail ('missing: ' + ($missing -join ', '))
        }
    }

    # ---- 5. launch ---------------------------------------------------------------------------------------------------
    $launchName = 'Launch the installed app (process stays up for ' + $LaunchSeconds + ' s)'
    if (-not $Launch) {
        Add-CheckResult -Context $ctx -Name $launchName -Status 'NOT RUN' -Detail 'use -Launch'
    }
    elseif ($null -eq $appExe) {
        Add-CheckResult -Context $ctx -Name $launchName -Status BLOCKED -Detail 'no installed app executable'
    }
    else {
        $forgeHome = Join-Path $installRoot 'forge-home'
        $webviewData = Join-Path $installRoot 'webview-data'
        New-Item -ItemType Directory -Path $forgeHome -Force | Out-Null
        $savedHome = $env:FORGE_HOME
        $savedWebview = $env:WEBVIEW2_USER_DATA_FOLDER
        $proc = $null
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        try {
            # The launched app inherits these: a fresh FORGE home and WebView2 profile inside the temporary folder.
            $env:FORGE_HOME = $forgeHome
            $env:WEBVIEW2_USER_DATA_FOLDER = $webviewData
            $proc = Start-Process -FilePath $appExe -WindowStyle Hidden -PassThru
            $null = $proc.Handle
        }
        finally {
            if ($null -eq $savedHome) { Remove-Item -LiteralPath 'Env:FORGE_HOME' -ErrorAction SilentlyContinue } else { $env:FORGE_HOME = $savedHome }
            if ($null -eq $savedWebview) { Remove-Item -LiteralPath 'Env:WEBVIEW2_USER_DATA_FOLDER' -ErrorAction SilentlyContinue } else { $env:WEBVIEW2_USER_DATA_FOLDER = $savedWebview }
        }
        $exitedEarly = $false
        while ($sw.Elapsed.TotalSeconds -lt $LaunchSeconds) {
            if ($proc.HasExited) { $exitedEarly = $true; break }
            Start-Sleep -Milliseconds 500
        }
        $sw.Stop()
        if ($exitedEarly) {
            Add-CheckResult -Context $ctx -Name $launchName -Status FAIL -Detail ('the app exited after ' + [math]::Round($sw.Elapsed.TotalSeconds, 1) + ' s with exit code ' + $proc.ExitCode) -Command $appExe -DurationSeconds $sw.Elapsed.TotalSeconds
        }
        else {
            $proc.Refresh()
            $window = 'no'
            if ($proc.MainWindowHandle -ne [IntPtr]::Zero) { $window = 'yes (title: ' + $proc.MainWindowTitle + ')' }
            $homeFiles = @(Get-ChildItem -LiteralPath $forgeHome -Recurse -File -ErrorAction SilentlyContinue).Count
            Stop-ProcessTree -ProcessId $proc.Id
            [void]$proc.WaitForExit(15000)
            Add-CheckResult -Context $ctx -Name $launchName -Status PASS -Detail ('running after ' + $LaunchSeconds + ' s; main window: ' + $window + '; files created in the temporary FORGE home: ' + $homeFiles + '; stopped') -Command ('FORGE_HOME=' + $forgeHome + ' ' + $appExe) -DurationSeconds $sw.Elapsed.TotalSeconds
        }
    }

    # ---- 6. uninstall ------------------------------------------------------------------------------------------------
    $uninstallName = 'Silent uninstall (uninstall.exe /S) and removal check'
    if (-not $installed) {
        Add-CheckResult -Context $ctx -Name $uninstallName -Status BLOCKED -Detail 'not installed'
    }
    else {
        $uninstallDone = $true
        $uninstaller = Join-Path $installDir 'uninstall.exe'
        $sw = [System.Diagnostics.Stopwatch]::StartNew()
        if (-not (Test-Path -LiteralPath $uninstaller -PathType Leaf)) {
            Add-CheckResult -Context $ctx -Name $uninstallName -Status FAIL -Detail ('uninstall.exe not found in ' + $installDir)
        }
        else {
            # The NSIS uninstaller copies itself to %TEMP% and returns at once, so removal is polled for afterwards.
            $u = Start-Process -FilePath $uninstaller -ArgumentList '/S' -PassThru
            $null = $u.Handle
            [void]$u.WaitForExit(300000)
            $target = $uninstaller
            if ($null -ne $appExe) { $target = $appExe }
            $gone = Wait-PathGone -Path $target -TimeoutSeconds 180
            $left = @((Get-UninstallEntry -ProductName $productName) | Where-Object { Test-EntryPointsAt -Entry $_ -Directory $installDir })
            $sw.Stop()
            $dirState = 'install folder removed'
            if (Test-Path -LiteralPath $installDir) {
                $remaining = @(Get-ChildItem -LiteralPath $installDir -Recurse -Force -ErrorAction SilentlyContinue).Count
                $dirState = 'install folder still exists with ' + $remaining + ' entries'
            }
            if ($gone -and $left.Count -eq 0) {
                Add-CheckResult -Context $ctx -Name $uninstallName -Status PASS -Detail ('app executable and uninstall entry removed; ' + $dirState) -Command ('"' + $uninstaller + '" /S') -DurationSeconds $sw.Elapsed.TotalSeconds
            }
            else {
                $why = New-Object System.Collections.Generic.List[string]
                if (-not $gone) { $why.Add('app executable still present after 180 s') }
                if ($left.Count -gt 0) { $why.Add('uninstall entry still present: ' + $left[0].Key) }
                Add-CheckResult -Context $ctx -Name $uninstallName -Status FAIL -Detail (($why -join '; ') + '; ' + $dirState) -Command ('"' + $uninstaller + '" /S') -DurationSeconds $sw.Elapsed.TotalSeconds
            }
        }
    }
}
catch {
    Add-CheckResult -Context $ctx -Name 'Unexpected script error' -Status FAIL -Detail ($_.Exception.Message + ' at ' + $_.InvocationInfo.PositionMessage)
}
finally {
    # Never leave a test installation behind, even after an unexpected error.
    if ($installed -and -not $uninstallDone -and $null -ne $installDir) {
        $uninstaller = Join-Path $installDir 'uninstall.exe'
        if (Test-Path -LiteralPath $uninstaller -PathType Leaf) {
            try {
                $u = Start-Process -FilePath $uninstaller -ArgumentList '/S' -PassThru
                [void]$u.WaitForExit(300000)
                Add-ValidationNote -Context $ctx -Text ('Cleaned up after an error: ran ' + $uninstaller + ' /S')
            }
            catch {
                Add-ValidationNote -Context $ctx -Text ('Could not uninstall the test installation; run it by hand: "' + $uninstaller + '" /S')
            }
        }
    }
    if ($null -ne $installRoot -and (Test-Path -LiteralPath $installRoot)) {
        $leftUninstaller = Join-Path $installDir 'uninstall.exe'
        $stillInstalled = $installed -and -not (Wait-PathGone -Path $leftUninstaller -TimeoutSeconds 30)
        if ($stillInstalled) {
            # Deleting the folder would leave a dangling uninstall entry and Start-menu shortcut; keep it for a manual uninstall.
            Add-ValidationNote -Context $ctx -Text ('The test installation is still present. Remove it with: "' + $leftUninstaller + '" /S   then delete ' + $installRoot)
        }
        else {
            try {
                Remove-Item -LiteralPath $installRoot -Recurse -Force -ErrorAction Stop
            }
            catch {
                Add-ValidationNote -Context $ctx -Text ('Temporary folder left behind (delete it by hand): ' + $installRoot)
            }
        }
    }
}
$code = Complete-Validation -Context $ctx
exit $code
