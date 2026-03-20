@echo off
setlocal
powershell.exe -ExecutionPolicy Bypass -File "%~dp0human_dashboard.ps1"
exit /b %ERRORLEVEL%
