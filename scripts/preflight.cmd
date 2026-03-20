@echo off
setlocal
powershell.exe -ExecutionPolicy Bypass -File "%~dp0preflight.ps1" %*
exit /b %ERRORLEVEL%
