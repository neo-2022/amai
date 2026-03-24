$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $repoRoot

$hasStackProfile = $false
$hasRemoteDestination = $false
foreach ($arg in $args) {
    $value = [string]$arg
    if ($value -eq "--stack-profile" -or $value.StartsWith("--stack-profile=")) {
        $hasStackProfile = $true
    }
    if ($value -eq "--ssh-destination" -or $value.StartsWith("--ssh-destination=")) {
        $hasRemoteDestination = $true
    }
}

if (-not $hasRemoteDestination) {
    Write-Error "Local Windows bootstrap install is not supported yet. Use WSL2 for a local stack or pass --ssh-destination for a remote Amai host."
    exit 1
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
    $env:AMAI_SELECTOR_MODE = "install"
    & "$repoRoot/scripts/preflight.ps1" @args
    exit $LASTEXITCODE
}

& cargo run --quiet -- bootstrap install @args
exit $LASTEXITCODE
