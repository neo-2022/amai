@echo off
setlocal
powershell.exe -ExecutionPolicy Bypass -File "%~dp0remove_amai.ps1" %*
exit /b %ERRORLEVEL%
