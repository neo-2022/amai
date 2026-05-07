@echo off
setlocal
powershell.exe -ExecutionPolicy Bypass -File "%~dp0install_amai.ps1" %*
exit /b %ERRORLEVEL%
