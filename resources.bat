@echo off
setlocal
cd /d "%~dp0"
python scripts\prepare_resources.py %*
if errorlevel 1 pause
