$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $repoRoot

$cleanupScript = Join-Path $repoRoot "scripts\\cleanup_mcp_orphans.ps1"
if (Test-Path $cleanupScript) {
    & $cleanupScript -RepoRoot $repoRoot *> $null
}

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

$releaseBinary = Join-Path $repoRoot "target\release\amai.exe"
$manifestPath = Join-Path $repoRoot "Cargo.toml"
$cargoCommand = Get-Command cargo -ErrorAction SilentlyContinue
if ($null -ne $cargoCommand) {
    cargo run --manifest-path $manifestPath --release --quiet -- mcp serve
    exit $LASTEXITCODE
}

if (Test-Path $releaseBinary) {
    & $releaseBinary mcp serve
    exit $LASTEXITCODE
}

Write-Error "Amai MCP runner requires cargo or target\release\amai.exe"
exit 1
