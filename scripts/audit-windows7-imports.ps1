param(
    [Parameter(Mandatory)][string]$Executable,
    [string]$Dumpbin,
    [string]$Report
)
$ErrorActionPreference = 'Stop'
$sensorExe = (Resolve-Path -LiteralPath $Executable).Path
if (!$Dumpbin) {
    $sensorVsWhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (!(Test-Path -LiteralPath $sensorVsWhere)) { throw 'Specify a Visual Studio dumpbin.exe path.' }
    $sensorVs = & $sensorVsWhere -latest -products '*' -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    $sensorDumpbins = @(Get-ChildItem -LiteralPath (Join-Path $sensorVs 'VC\Tools\MSVC') -Directory |
        ForEach-Object { Get-Item -LiteralPath (Join-Path $_.FullName 'bin\Hostx64\x64\dumpbin.exe') -ErrorAction SilentlyContinue } |
        Sort-Object FullName -Descending)
    if (!$sensorDumpbins) { throw 'dumpbin.exe was not found.' }
    $Dumpbin = $sensorDumpbins[0].FullName
}
$sensorImports = @(& $Dumpbin /imports $sensorExe)
if ($LASTEXITCODE) { throw 'Could not inspect PE imports.' }
$sensorHeaders = @(& $Dumpbin /headers $sensorExe)
if ($LASTEXITCODE) { throw 'Could not inspect PE headers.' }
if (!($sensorHeaders -match '8664 machine \(x64\)')) { throw 'Unified SENSOR requires an x64 PE.' }
if (!($sensorHeaders -match 'NX compatible') -or !($sensorHeaders -match 'Dynamic base')) { throw 'DEP/ASLR must remain enabled.' }
$sensorSubsystem = ($sensorHeaders | Select-String '^\s*([0-9.]+) subsystem version$').Matches
if (!$sensorSubsystem -or [version]$sensorSubsystem[0].Groups[1].Value -gt [version]'6.1') {
    throw 'PE subsystem version would prevent Windows 7 loading.'
}
# This is a fail-closed gate for known static loader blockers, not an API or OS
# certification. Dynamic imports and runtime behavior still need a real Win7 OS.
$sensorAllowedDlls = @('kernel32.dll','user32.dll','ole32.dll','gdi32.dll','d3d11.dll','wtsapi32.dll','dxgi.dll','mfplat.dll','ntdll.dll','uiautomationcore.dll','oleaut32.dll','advapi32.dll','shell32.dll','crypt32.dll','bcrypt.dll','shlwapi.dll','ws2_32.dll','opengl32.dll','imm32.dll','dwmapi.dll','uxtheme.dll','comdlg32.dll','comctl32.dll','version.dll','userenv.dll','psapi.dll','normaliz.dll','winmm.dll')
$sensorForbidden = @('ProcessPrng','WaitOnAddress','WakeByAddressAll','WakeByAddressSingle','GetSystemTimePreciseAsFileTime','CreateFile2','CreateDXGIFactory2','D3D12CreateDevice','SetThreadDescription','GetDpiForWindow','GetDpiForSystem','GetSystemMetricsForDpi','AdjustWindowRectExForDpi','SetProcessDpiAwarenessContext','SetThreadDpiAwarenessContext','RoInitialize','RoOriginateError','WindowsCreateString','WindowsDeleteString','WindowsGetStringRawBuffer','WindowsDuplicateString','GetCurrentThreadStackLimits')
$sensorDll = $null
$sensorEntries = [Collections.Generic.List[object]]::new()
$sensorBlockers = [Collections.Generic.List[string]]::new()
foreach ($sensorLine in $sensorImports) {
    if ($sensorLine -match '^\s+([\w.-]+\.dll)\s*$') {
        $sensorDll = $Matches[1].ToLowerInvariant()
        if ($sensorDll -notin $sensorAllowedDlls) { $sensorBlockers.Add("Unreviewed or incompatible static DLL: $sensorDll") }
    } elseif ($sensorDll -and $sensorLine -match '^\s+[0-9A-F]+\s+([A-Za-z_?][^ ]*)\s*$') {
        $sensorSymbol = $Matches[1]
        $sensorEntries.Add([pscustomobject]@{dll=$sensorDll; symbol=$sensorSymbol})
        if ($sensorSymbol -in $sensorForbidden) { $sensorBlockers.Add("Post-Win7 static import: $sensorDll!$sensorSymbol") }
    }
}
if ($sensorEntries.Count -lt 10) { throw 'Import parser did not obtain a credible symbol inventory.' }
$sensorResult = [ordered]@{
    executable = [IO.Path]::GetFileName($sensorExe)
    sha256 = (Get-FileHash -LiteralPath $sensorExe -Algorithm SHA256).Hash.ToLowerInvariant()
    machine = 'x64'
    subsystem_version = $sensorSubsystem[0].Groups[1].Value
    known_loader_blockers_passed = ($sensorBlockers.Count -eq 0)
    actual_windows7_runtime_verified = $false
    limitations = 'Static known-blocker audit only. Does not prove dynamic APIs, OpenGL driver, TLS roots, codecs, remote input, consent or OS compatibility.'
    blockers = @($sensorBlockers)
    imports = @($sensorEntries)
}
if ($Report) {
    $sensorReportPath = [IO.Path]::GetFullPath($Report)
    if (Test-Path -LiteralPath $sensorReportPath) { throw 'Choose a new report path; existing evidence is preserved.' }
    # Generated diagnostic inventory from the actual binary, never a support badge.
    $sensorResult | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $sensorReportPath -Encoding utf8
}
if ($sensorBlockers.Count) { throw ($sensorBlockers -join '; ') }
Write-Output "WIN7_STATIC_LOADER_GATE_PASS symbols=$($sensorEntries.Count) sha256=$($sensorResult.sha256) actual_win7_runtime_verified=FALSE"
