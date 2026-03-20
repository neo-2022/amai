$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $repoRoot

$envPath = Join-Path $repoRoot ".env"
$examplePath = Join-Path $repoRoot ".env.example"

if (-not (Test-Path $envPath)) {
    Copy-Item $examplePath $envPath
}

$exampleLines = Get-Content $examplePath
$envLines = Get-Content $envPath
$existingKeys = @{}
foreach ($line in $envLines) {
    if ([string]::IsNullOrWhiteSpace($line) -or $line.StartsWith("#") -or -not $line.Contains("=")) {
        continue
    }
    $parts = $line.Split("=", 2)
    $existingKeys[$parts[0].Trim()] = $true
}

foreach ($line in $exampleLines) {
    if ([string]::IsNullOrWhiteSpace($line) -or $line.StartsWith("#") -or -not $line.Contains("=")) {
        continue
    }
    $parts = $line.Split("=", 2)
    $key = $parts[0].Trim()
    if (-not $existingKeys.ContainsKey($key)) {
        Add-Content -Path $envPath -Value $line
        $existingKeys[$key] = $true
    }
}

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

$dashboardUrl = "http://$browserHost`:$port/"
$healthUrl = "http://$browserHost`:$port/healthz"
$pidPath = Join-Path $repoRoot "state\human_dashboard.pid"
$logPath = Join-Path $repoRoot "tmp\human_dashboard.log"

New-Item -ItemType Directory -Force -Path (Join-Path $repoRoot "state") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $repoRoot "tmp") | Out-Null

try {
    Invoke-WebRequest -UseBasicParsing -Uri $healthUrl -TimeoutSec 2 | Out-Null
    Write-Output "Amai human dashboard already running"
    Write-Output "URL: $dashboardUrl"
    if (Test-Path $pidPath) {
        Write-Output ("PID: " + (Get-Content $pidPath -Raw).Trim())
    }
    exit 0
} catch {
}

$process = Start-Process -FilePath "cargo" -ArgumentList @("run", "--release", "--quiet", "--", "observe", "serve", "--bind", $bind) -WorkingDirectory $repoRoot -RedirectStandardOutput $logPath -RedirectStandardError $logPath -WindowStyle Hidden -PassThru
Set-Content -Path $pidPath -Value $process.Id

for ($i = 0; $i -lt 120; $i++) {
    try {
        Invoke-WebRequest -UseBasicParsing -Uri $healthUrl -TimeoutSec 2 | Out-Null
        Write-Output "Amai human dashboard started"
        Write-Output "URL: $dashboardUrl"
        Write-Output "PID: $($process.Id)"
        Write-Output "Log: $logPath"
        exit 0
    } catch {
        if ($process.HasExited) {
            Write-Error "Amai human dashboard failed to start. See $logPath"
            exit 1
        }
        Start-Sleep -Milliseconds 500
    }
}

Write-Error "Amai human dashboard did not become healthy in time. See $logPath"
exit 1
