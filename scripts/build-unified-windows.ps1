param([string]$Toolchain = 'nightly-2026-09-08')
$ErrorActionPreference = 'Stop'
if ($Toolchain -notmatch '^nightly-[0-9]{4}-[0-9]{2}-[0-9]{2}$') {
    throw 'Use a pinned nightly with rust-src and Rust >=1.98.'
}
$sensorRepo = (Resolve-Path -LiteralPath (Split-Path -Parent $PSScriptRoot)).Path
Push-Location $sensorRepo
try {
    # Build the actual shipping application, not the local capability probe.
    # Both renderers are in this one executable; ui.rs selects by actual OS.
    & rustup run $Toolchain cargo build -p sensor-desktop -p sensor-client --bin SENSOR-Remote --bin SENSOR-CLI --target x86_64-win7-windows-msvc -Z build-std=std,panic_abort --release --locked
    if ($LASTEXITCODE) { throw 'Unified Windows executable build failed.' }
    & (Join-Path $PSScriptRoot 'audit-windows7-imports.ps1') -Executable 'target\x86_64-win7-windows-msvc\release\SENSOR-Remote.exe'
    & (Join-Path $PSScriptRoot 'audit-windows7-imports.ps1') -Executable 'target\x86_64-win7-windows-msvc\release\SENSOR-CLI.exe'
    Write-Output 'Built one SENSOR-Remote.exe for OS compatibility testing. Import audit and real Win7/10/11 acceptance remain mandatory. This does not mark Win7 supported.'
    Get-FileHash -LiteralPath 'target\x86_64-win7-windows-msvc\release\SENSOR-Remote.exe' -Algorithm SHA256
} finally { Pop-Location }
