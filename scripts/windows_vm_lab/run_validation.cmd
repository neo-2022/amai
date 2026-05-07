@echo off
setlocal
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "%~dp0run_validation.ps1"
exit /b %ERRORLEVEL%
