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

cargo run --release --quiet -- observe serve --bind $bind
exit $LASTEXITCODE
