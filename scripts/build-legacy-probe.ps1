param([Parameter(Mandatory=$true)][string]$Toolchain)
$ErrorActionPreference = 'Stop'
if ($Toolchain -notmatch '^nightly-[0-9]{4}-[0-9]{2}-[0-9]{2}$') { throw 'Use an explicitly pinned nightly with rust-src and Rust >=1.98. No stable pc-windows-msvc masquerading as Windows 7.' }
$sensorRepo = (Resolve-Path -LiteralPath (Split-Path -Parent $PSScriptRoot)).Path
& rustup run $Toolchain cargo build --manifest-path (Join-Path $sensorRepo 'legacy\Cargo.toml') --target x86_64-win7-windows-msvc -Z build-std=std,panic_abort --release --locked
if ($LASTEXITCODE) { throw 'Legacy capability probe build failed. No Windows 7 package was produced.' }
Write-Output 'Only the capability probe was built. Import audit and actual Win7 SP1 launch/capture/codec/security testing are mandatory; no remote application support is claimed.'
