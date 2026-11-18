@echo off
setlocal
cd /d "%~dp0"
python scripts\prepare_reference.py %*
if errorlevel 1 pause
