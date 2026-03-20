$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $repoRoot

& cargo run --quiet -- bootstrap remove @args
exit $LASTEXITCODE
