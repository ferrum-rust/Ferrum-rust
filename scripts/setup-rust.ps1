$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$installer = Join-Path $projectRoot '.local\tools\rustup-init.exe'
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $installer) | Out-Null
Invoke-WebRequest 'https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe' -OutFile $installer
& $installer -y --profile minimal --default-toolchain stable --no-modify-path
if ($LASTEXITCODE -ne 0) { throw 'Rust installation failed.' }
Write-Host 'Rust installed. Run start.bat. Visual Studio C++ build tools are required.'
