function Wait-UninstallState {
    param([Parameter(Mandatory = $true)][scriptblock]$Inspect, [int]$TimeoutSeconds = 180, [int]$PollMilliseconds = 500)
    $sw = [System.Diagnostics.Stopwatch]::StartNew()
    do {
        $state = & $Inspect
        if ($state.ExecutableGone -and @($state.Entries).Count -eq 0) { return $state }
        if ($sw.Elapsed.TotalSeconds -ge $TimeoutSeconds) { return $state }
        Start-Sleep -Milliseconds $PollMilliseconds
    } while ($true)
}
