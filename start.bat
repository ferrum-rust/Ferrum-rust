@echo off
setlocal
cd /d "%~dp0"
rem Launch the installed release. Set FERRUM_REBUILD=1 to build source changes first.
if not "%FERRUM_REBUILD%"=="1" if exist "target\release\ferrum189.exe" goto run
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
where cargo >nul 2>nul
if errorlevel 1 (
  echo Rust toolchain is missing. Install Rust from https://rustup.rs first.
  if "%~1"=="" pause
  exit /b 1
)
rem Bound concurrent compilers; the native runtime is large on 16 GiB systems.
cargo build --release --locked --bins --jobs 2 --target-dir "%~dp0target"
if errorlevel 1 (
  echo Build failed. See the error above.
  if "%~1"=="" pause
  exit /b 1
)
:run
"target\release\ferrum189.exe" %*
set "FERRUM_EXIT=%ERRORLEVEL%"
if not "%FERRUM_EXIT%"=="0" if "%~1"=="" pause
exit /b %FERRUM_EXIT%
