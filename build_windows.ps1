# Build the release binary and copy it into dist\.
#
# The exe is fully self-contained: fonts and the taskbar icon are embedded
# (see crates/colosseum-gui/build.rs), and the GUI subsystem flag means no
# console window opens on double-click.
#
# Usage:  .\build_windows.ps1
# Output: dist\colosseum.exe

$ErrorActionPreference = "Stop"

Set-Location $PSScriptRoot

cargo build --release --bin colosseum
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

New-Item -ItemType Directory -Force -Path dist | Out-Null
Copy-Item target\release\colosseum.exe dist\colosseum.exe -Force

Write-Host "Built dist\colosseum.exe"
