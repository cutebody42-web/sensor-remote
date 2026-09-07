param(
    [Parameter(Mandatory)][string]$PackageDirectory,
    [Parameter(Mandatory)][string]$Compiler,
    [Parameter(Mandatory)][string]$OutputFile
)
$ErrorActionPreference = 'Stop'
$sensorPackage = (Resolve-Path -LiteralPath $PackageDirectory).Path
$sensorCompiler = (Resolve-Path -LiteralPath $Compiler).Path
$sensorRepo = (Resolve-Path -LiteralPath (Split-Path -Parent $PSScriptRoot)).Path
$sensorOutput = [IO.Path]::GetFullPath($OutputFile)
if (Test-Path -LiteralPath $sensorOutput) { throw 'Installer output already exists; use a new path.' }
$sensorTemp = Join-Path ([IO.Path]::GetTempPath()) ('sensor-installer-' + [Guid]::NewGuid().ToString('N'))
$null = New-Item -ItemType Directory -Path $sensorTemp
Copy-Item -LiteralPath (Join-Path $sensorRepo 'installer\sensor.nsi') -Destination $sensorTemp
$sensorInstallLines = [Collections.Generic.List[string]]::new()
$sensorRemoveLines = [Collections.Generic.List[string]]::new()
$sensorDirectories = [Collections.Generic.HashSet[string]]::new()
foreach ($sensorSubdir in @('docs', 'third-party-licenses')) {
    foreach ($sensorFile in (Get-ChildItem -LiteralPath (Join-Path $sensorPackage $sensorSubdir) -File -Recurse)) {
        $sensorRelative = [IO.Path]::GetRelativePath($sensorPackage, $sensorFile.FullName)
        if ($sensorRelative.Contains('$') -or $sensorRelative.Contains('"') -or $sensorRelative.Contains("`n")) { throw 'Unsafe NSIS payload filename' }
        $sensorDirectory = Split-Path -Parent $sensorRelative
        $sensorInstallLines.Add('SetOutPath "$INSTDIR\' + $sensorDirectory + '"')
        $sensorInstallLines.Add('File "${PACKAGE}\' + $sensorRelative + '"')
        $sensorRemoveLines.Add('Delete "$INSTDIR\' + $sensorRelative + '"')
        while ($sensorDirectory) {
            $null = $sensorDirectories.Add($sensorDirectory)
            $sensorDirectory = Split-Path -Parent $sensorDirectory
        }
    }
}
foreach ($sensorDirectory in ($sensorDirectories | Sort-Object Length -Descending)) {
    $sensorRemoveLines.Add('RMDir "$INSTDIR\' + $sensorDirectory + '"')
}
# Mechanical manifest generation from exact packaged filenames.
$sensorInstallLines | Set-Content -LiteralPath (Join-Path $sensorTemp 'payload-install.nsh') -Encoding utf8
$sensorRemoveLines | Set-Content -LiteralPath (Join-Path $sensorTemp 'payload-uninstall.nsh') -Encoding utf8
& $sensorCompiler /V2 ("/DPACKAGE=" + $sensorPackage) ("/DOUTPUT=" + $sensorOutput) (Join-Path $sensorTemp 'sensor.nsi')
if ($LASTEXITCODE) { throw 'NSIS compilation failed' }
Get-FileHash -LiteralPath $sensorOutput -Algorithm SHA256
Write-Output "Built unsigned per-user installer: $sensorOutput"
