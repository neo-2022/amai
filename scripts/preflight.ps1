$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $repoRoot

cargo run --quiet -- bootstrap preflight @args
exit $LASTEXITCODE
