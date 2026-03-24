$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

function Find-PayloadRoot {
    $markerFile = "AMAI_WINDOWS_VM_PAYLOAD_MARKER.txt"
    foreach ($driveLetter in [char[]](65..90)) {
        $root = "{0}:\\" -f $driveLetter
        if (Test-Path (Join-Path $root $markerFile)) {
            return $root
        }
    }
    return $null
}

function Write-Utf8File {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path,
        [Parameter(Mandatory = $true)]
        [string[]]$Lines
    )
    $directory = Split-Path -Parent $Path
    if ($directory) {
        New-Item -ItemType Directory -Force -Path $directory | Out-Null
    }
    [System.IO.File]::WriteAllLines($Path, $Lines, [System.Text.UTF8Encoding]::new($false))
}

$localEvidenceDir = "C:\AmaiValidation"
$payloadRoot = Find-PayloadRoot
$payloadEvidenceDir = if ($payloadRoot) { Join-Path $payloadRoot "evidence" } else { $null }

$logCopies = @(
    Join-Path $localEvidenceDir "install_amai_local_fail_closed.txt"
)
$resultCopies = @(
    Join-Path $localEvidenceDir "result.txt"
)
$sentinelCopies = @(
    Join-Path $localEvidenceDir "already_ran.txt"
)

if ($payloadEvidenceDir) {
    $logCopies += Join-Path $payloadEvidenceDir "install_amai_local_fail_closed.txt"
    $resultCopies += Join-Path $payloadEvidenceDir "result.txt"
    $sentinelCopies += Join-Path $payloadEvidenceDir "already_ran.txt"
}

foreach ($sentinelPath in $sentinelCopies) {
    if (Test-Path $sentinelPath) {
        exit 0
    }
}

New-Item -ItemType Directory -Force -Path $localEvidenceDir | Out-Null
if ($payloadEvidenceDir) {
    New-Item -ItemType Directory -Force -Path $payloadEvidenceDir | Out-Null
}

$stdoutPath = Join-Path $localEvidenceDir "install_stdout.txt"
$stderrPath = Join-Path $localEvidenceDir "install_stderr.txt"
$installScript = Join-Path $PSScriptRoot "install_amai.ps1"
$powershellExe = Join-Path $env:SystemRoot "System32\WindowsPowerShell\v1.0\powershell.exe"

$proc = Start-Process `
    -FilePath $powershellExe `
    -ArgumentList @("-NoLogo", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", $installScript) `
    -Wait `
    -PassThru `
    -NoNewWindow `
    -RedirectStandardOutput $stdoutPath `
    -RedirectStandardError $stderrPath

$outputLines = @()
if (Test-Path $stdoutPath) {
    $outputLines += Get-Content -LiteralPath $stdoutPath
}
if (Test-Path $stderrPath) {
    $outputLines += Get-Content -LiteralPath $stderrPath
}

$expectedMessagePrefix = "Local Windows bootstrap install is not supported yet."
$joinedOutput = ($outputLines -join "`n")
$expected = $joinedOutput -like "*$expectedMessagePrefix*"
$result = if ($expected) { "PASS" } else { "FAIL" }

$resultLines = @(
    "validation_mode=windows_vm_local_fail_closed",
    "payload_root=$payloadRoot",
    "install_script=$installScript",
    "exit_code=$($proc.ExitCode)",
    "expected_message_present=$([bool]$expected)",
    "line_count=$($outputLines.Count)",
    "result=$result"
)

foreach ($path in $logCopies) {
    Write-Utf8File -Path $path -Lines $outputLines
}
foreach ($path in $resultCopies) {
    Write-Utf8File -Path $path -Lines $resultLines
}
foreach ($path in $sentinelCopies) {
    Write-Utf8File -Path $path -Lines @("done")
}

Start-Sleep -Seconds 3
Stop-Computer -Force
