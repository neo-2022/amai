$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $repoRoot

$hasStackProfile = $false
foreach ($arg in $args) {
    $value = [string]$arg
    if ($value -eq "--stack-profile" -or $value.StartsWith("--stack-profile=")) {
        $hasStackProfile = $true
    }
}

$interactivePrompt = $false
if ($env:AMAI_NO_INSTALL_PROMPT -ne "1") {
    if ($env:AMAI_FORCE_INTERACTIVE_PROMPT -eq "1") {
        $interactivePrompt = $true
    } else {
        try {
            $interactivePrompt = (-not [Console]::IsInputRedirected) -and (-not [Console]::IsOutputRedirected)
        } catch {
            $interactivePrompt = $false
        }
    }
}

if ((-not $hasStackProfile) -and $interactivePrompt) {
    & "$repoRoot/scripts/preflight.ps1" @args
    exit $LASTEXITCODE
}

& cargo run --quiet -- bootstrap install @args
exit $LASTEXITCODE
