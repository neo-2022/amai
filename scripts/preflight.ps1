$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $repoRoot

$explicitProfile = $false
$stackProfile = ""
$installArgs = New-Object System.Collections.Generic.List[string]

for ($i = 0; $i -lt $args.Count; $i++) {
    $arg = [string]$args[$i]
    if ($arg -eq "--stack-profile") {
        $explicitProfile = $true
        if ($i + 1 -ge $args.Count) {
            throw "missing value for --stack-profile"
        }
        $stackProfile = [string]$args[$i + 1]
        $i++
        continue
    }
    if ($arg.StartsWith("--stack-profile=")) {
        $explicitProfile = $true
        $stackProfile = $arg.Substring("--stack-profile=".Length)
        continue
    }
    [void]$installArgs.Add($arg)
}

function Run-Preflight {
    param([string]$Profile)

    $output = & cargo run --quiet -- bootstrap preflight --stack-profile $Profile 2>&1
    $exitCode = $LASTEXITCODE
    $text = (($output | ForEach-Object { "$_" }) -join "`n").TrimEnd()
    return @{
        Output = $text
        ExitCode = $exitCode
    }
}

function Get-Field {
    param(
        [string]$Prefix,
        [string]$Text
    )

    foreach ($line in ($Text -split "`r?`n")) {
        if ($line.StartsWith($Prefix)) {
            return $line.Substring($Prefix.Length)
        }
    }
    return ""
}

function Get-Line {
    param(
        [string]$Prefix,
        [string]$Text
    )

    foreach ($line in ($Text -split "`r?`n")) {
        if ($line.StartsWith($Prefix)) {
            return $line
        }
    }
    return ""
}

function Normalize-Verdict {
    param([string]$Title)

    switch ($Title) {
        "машина подходит" { return "pass" }
        "машина подходит с оговорками" { return "warn" }
        "машина не подходит для этого режима" { return "fail" }
        default { return "unknown" }
    }
}

function Verdict-Short {
    param([string]$Code)

    switch ($Code) {
        "pass" { return "подходит" }
        "warn" { return "подходит с оговорками" }
        "fail" { return "не подходит" }
        default { return "статус неясен" }
    }
}

function Test-InteractivePrompt {
    if ($env:AMAI_NO_INSTALL_PROMPT -eq "1") {
        return $false
    }
    if ($env:AMAI_FORCE_INTERACTIVE_PROMPT -eq "1") {
        return $true
    }
    try {
        return (-not [Console]::IsInputRedirected) -and (-not [Console]::IsOutputRedirected)
    } catch {
        return $false
    }
}

function Confirm-Install {
    param(
        [string]$ChosenProfile,
        [string]$ChosenLabel,
        [string]$ChosenVerdict
    )

    Write-Host ""
    if ($ChosenVerdict -eq "warn") {
        Write-Host "ПРЕДУПРЕЖДЕНИЕ: профиль $ChosenLabel этой машине подходит, но без запаса."
        Write-Host "Такой режим можно ставить, если вас устраивает более скромный запас по тяжёлым сценариям."
    }
    if ($ChosenVerdict -eq "fail") {
        Write-Host "ПРЕДУПРЕЖДЕНИЕ: профиль $ChosenLabel этой машине не подходит."
        Write-Host "Установка не начата. Выберите другой профиль или более сильную машину."
        return
    }

    $answer = Read-Host "Напишите ДА, если хотите установить Amai в режиме $ChosenLabel"
    switch ($answer) {
        "ДА" { }
        "да" { }
        "Да" { }
        "YES" { }
        "Yes" { }
        "yes" { }
        "Y" { }
        "y" { }
        default {
            Write-Host "Установка не запущена. Когда захотите продолжить, снова запустите проверку."
            return
        }
    }

    & "$repoRoot/scripts/install_amai.ps1" @installArgs --stack-profile $ChosenProfile --yes
    exit $LASTEXITCODE
}

if ($explicitProfile) {
    $result = Run-Preflight $stackProfile
    Write-Output $result.Output
    if ($result.ExitCode -ne 0) {
        exit $result.ExitCode
    }

    if (-not (Test-InteractivePrompt)) {
        exit 0
    }

    $profileLabel = Get-Field "Профиль: " $result.Output
    $verdictTitle = Get-Field "Итог: " $result.Output
    $verdictCode = Normalize-Verdict $verdictTitle
    Confirm-Install $stackProfile $profileLabel $verdictCode
    exit 0
}

$defaultResult = Run-Preflight "default"
if ($defaultResult.ExitCode -ne 0) {
    Write-Output $defaultResult.Output
    exit $defaultResult.ExitCode
}
$liteResult = Run-Preflight "lite_vps"
if ($liteResult.ExitCode -ne 0) {
    Write-Output $liteResult.Output
    exit $liteResult.ExitCode
}

$defaultLabel = Get-Field "Профиль: " $defaultResult.Output
$liteLabel = Get-Field "Профиль: " $liteResult.Output
$defaultVerdictTitle = Get-Field "Итог: " $defaultResult.Output
$liteVerdictTitle = Get-Field "Итог: " $liteResult.Output
$defaultVerdict = Normalize-Verdict $defaultVerdictTitle
$liteVerdict = Normalize-Verdict $liteVerdictTitle

Write-Host "Amai preflight"
Write-Host ""
Write-Host "Эта команда сразу проверила два режима установки и покажет, что ваша машина реально тянет."
Write-Host ""
Write-Host "Что увидела машина:"
Write-Host (Get-Line "- CPU:" $defaultResult.Output)
Write-Host (Get-Line "- Память:" $defaultResult.Output)
Write-Host (Get-Line "- Диск:" $defaultResult.Output)
Write-Host ""
Write-Host "Профили установки:"
Write-Host "1. $defaultLabel — $(Verdict-Short $defaultVerdict)"
Write-Host "2. $liteLabel — $(Verdict-Short $liteVerdict)"
Write-Host ""

$recommendedChoice = ""
$recommendedReason = ""
if ($defaultVerdict -eq "pass") {
    $recommendedChoice = "1"
    $recommendedReason = "Это основной полноценный режим, и у этой машины для него есть хороший запас."
} elseif ($defaultVerdict -eq "warn") {
    $recommendedChoice = "1"
    $recommendedReason = "Полноценный режим возможен, но уже без большого запаса. Если нужен более лёгкий вариант, можно выбрать 2."
} elseif ($liteVerdict -in @("pass", "warn")) {
    $recommendedChoice = "2"
    $recommendedReason = "Полноценный локальный режим сейчас тяжёлый, зато лёгкий удалённый режим машина тянет."
}

Write-Host "Рекомендуемый выбор:"
if ($recommendedChoice -eq "1") {
    Write-Host "- 1. $defaultLabel"
    Write-Host "- $recommendedReason"
} elseif ($recommendedChoice -eq "2") {
    Write-Host "- 2. $liteLabel"
    Write-Host "- $recommendedReason"
} else {
    Write-Host "- Сейчас нет профиля, который эта машина тянет без блокирующих ограничений."
}

Write-Host ""
Write-Host "Если хотите только посмотреть результат, можно остановиться здесь."
Write-Host "Если хотите установить Amai, ниже можно выбрать профиль."

if (-not (Test-InteractivePrompt)) {
    exit 0
}

$choice = Read-Host "Введите 1 или 2, чтобы начать установку. Нажмите Enter, если пока ставить не нужно"
switch ($choice) {
    "1" { Confirm-Install "default" $defaultLabel $defaultVerdict }
    "2" { Confirm-Install "lite_vps" $liteLabel $liteVerdict }
    "" { Write-Host "Установка не запущена. Вы просто посмотрели, что тянет машина." }
    default { Write-Host "Непонятный выбор. Установка не запущена." }
}
