@echo off
setlocal
powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File "C:\Windows\Setup\Scripts\run_validation.ps1"
exit /b 0
