$ErrorActionPreference = "Stop"

param(
    [string]$RepoRoot = $(Split-Path -Parent $PSScriptRoot)
)

if ($env:AMAI_SKIP_MCP_ORPHAN_CLEANUP -eq "1") {
    exit 0
}

$repoRootPath = [System.IO.Path]::GetFullPath($RepoRoot)
$manifestPath = [System.IO.Path]::GetFullPath((Join-Path $repoRootPath "Cargo.toml"))
$currentPid = $PID

$processes = Get-CimInstance Win32_Process
$processMap = @{}
foreach ($process in $processes) {
    $processMap[[int]$process.ProcessId] = $process
}

function Test-TargetCommand {
    param([object]$Process)
    $cmd = $Process.CommandLine
    if ([string]::IsNullOrWhiteSpace($cmd)) {
        return $false
    }
    if ($cmd.IndexOf("mcp serve", [System.StringComparison]::OrdinalIgnoreCase) -lt 0) {
        return $false
    }
    return (
        $cmd.IndexOf("amai", [System.StringComparison]::OrdinalIgnoreCase) -ge 0 -or
        $cmd.IndexOf("cargo", [System.StringComparison]::OrdinalIgnoreCase) -ge 0
    )
}

function Test-RepoMatch {
    param([object]$Process)
    $cmd = $Process.CommandLine
    if (-not [string]::IsNullOrWhiteSpace($cmd)) {
        if (
            $cmd.IndexOf($repoRootPath, [System.StringComparison]::OrdinalIgnoreCase) -ge 0 -or
            $cmd.IndexOf($manifestPath, [System.StringComparison]::OrdinalIgnoreCase) -ge 0
        ) {
            return $true
        }
    }
    $exe = $Process.ExecutablePath
    if ([string]::IsNullOrWhiteSpace($exe)) {
        return $false
    }
    $exePath = [System.IO.Path]::GetFullPath($exe)
    return $exePath.StartsWith($repoRootPath, [System.StringComparison]::OrdinalIgnoreCase)
}

function Test-Orphan {
    param(
        [object]$Process,
        [hashtable]$ProcessMap
    )
    $parentId = [int]$Process.ParentProcessId
    if ($parentId -le 0) {
        return $true
    }
    return -not $ProcessMap.ContainsKey($parentId)
}

foreach ($process in $processes) {
    $pid = [int]$process.ProcessId
    if ($pid -eq $currentPid) {
        continue
    }
    if (-not (Test-TargetCommand -Process $process)) {
        continue
    }
    if (-not (Test-RepoMatch -Process $process)) {
        continue
    }
    if (-not (Test-Orphan -Process $process -ProcessMap $processMap)) {
        continue
    }
    Stop-Process -Id $pid -Force -ErrorAction SilentlyContinue
}
