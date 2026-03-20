@echo off
setlocal
powershell.exe -ExecutionPolicy Bypass -File "%~dp0run_mcp_stdio.ps1"
exit /b %ERRORLEVEL%
