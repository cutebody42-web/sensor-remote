param([switch]$Release, [switch]$ReleaseTests)
$ErrorActionPreference = 'Stop'
$repoPath = Split-Path -Parent $PSScriptRoot
$cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
$env:PATH = $cargoBin + ';' + $env:PATH
if ($IsWindows -or $env:OS -eq 'Windows_NT') {
    $vswherePath = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (Test-Path -LiteralPath $vswherePath) {
        $vsPath = & $vswherePath -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
        if ($vsPath) {
            Import-Module (Join-Path $vsPath 'Common7\Tools\Microsoft.VisualStudio.DevShell.dll')
            Enter-VsDevShell -VsInstallPath $vsPath -SkipAutomaticLocation -DevCmdArguments '-arch=x64 -host_arch=x64' | Out-Null
        }
    }
}
Push-Location $repoPath
try {
    & cargo fmt --all -- --check
    if ($LASTEXITCODE) { throw 'Formatting failed' }
    if ($ReleaseTests) {
        & cargo test --workspace --release --locked
    } else {
        & cargo test --workspace --locked
    }
    if ($LASTEXITCODE) { throw 'Tests failed' }
    & cargo clippy --workspace --all-targets --locked -- -D warnings
    if ($LASTEXITCODE) { throw 'Clippy failed' }
    if ($Release) {
        & cargo build --workspace --release --locked
        if ($LASTEXITCODE) { throw 'Release build failed' }
    }
} finally { Pop-Location }
