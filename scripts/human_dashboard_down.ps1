$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $repoRoot

$envPath = Join-Path $repoRoot ".env"
if (Test-Path $envPath) {
    Get-Content $envPath | ForEach-Object {
        if ([string]::IsNullOrWhiteSpace($_) -or $_.StartsWith("#") -or -not $_.Contains("=")) {
            return
        }
        $parts = $_.Split("=", 2)
        $key = $parts[0]
        $value = $parts[1]
        if (-not (Test-Path "env:$key")) {
            Set-Item -Path "env:$key" -Value $value
        }
    }
}

$bind = $env:AMI_OBSERVE_BIND
if ([string]::IsNullOrWhiteSpace($bind)) {
    $bind = "0.0.0.0:9464"
}

$parts = $bind.Split(":", 2)
$host = $parts[0]
$port = $parts[1]
if ([string]::IsNullOrWhiteSpace($host) -or $host -eq "0.0.0.0" -or $host -eq "::") {
    $browserHost = "127.0.0.1"
} else {
    $browserHost = $host
}

$healthUrl = "http://$browserHost`:$port/healthz"
$pidPath = Join-Path $repoRoot "state\human_dashboard.pid"

if (Test-Path $pidPath) {
    $pid = (Get-Content $pidPath -Raw).Trim()
    if ($pid) {
        try {
            Stop-Process -Id ([int]$pid) -ErrorAction SilentlyContinue
        } catch {
        }
    }
    Remove-Item $pidPath -Force -ErrorAction SilentlyContinue
}

try {
    Invoke-WebRequest -UseBasicParsing -Uri $healthUrl -TimeoutSec 2 | Out-Null
    Write-Error "Amai human dashboard is still responding on $healthUrl"
    exit 1
} catch {
    Write-Output "Amai human dashboard stopped"
    exit 0
}
