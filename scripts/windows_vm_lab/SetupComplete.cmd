@echo off
setlocal

set "STARTUP_DIR=%ProgramData%\Microsoft\Windows\Start Menu\Programs\Startup"
set "LAUNCHER_PATH=%STARTUP_DIR%\AmaiValidation.cmd"

if not exist "%STARTUP_DIR%" (
    mkdir "%STARTUP_DIR%"
)

(
    echo @echo off
    echo powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File C:\Windows\Setup\Scripts\run_validation.ps1
) > "%LAUNCHER_PATH%"

reg add "HKLM\SOFTWARE\Microsoft\Windows\CurrentVersion\RunOnce" ^
    /v AmaiValidation ^
    /t REG_SZ ^
    /d "\"%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe\" -NoLogo -NoProfile -ExecutionPolicy Bypass -File C:\Windows\Setup\Scripts\run_validation.ps1" ^
    /f >nul

exit /b 0
