param([Parameter(Mandatory)][string]$Installer)
$ErrorActionPreference = 'Stop'
$sensorInstaller = (Resolve-Path -LiteralPath $Installer).Path
$sensorRegistry = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\SENSORRemote'
if (Test-Path -LiteralPath $sensorRegistry) { throw 'An existing registered installation must not be displaced by this isolated test.' }
$sensorOutputs = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..\..')).Path
$sensorTarget = [IO.Path]::GetFullPath((Join-Path $sensorOutputs ('installer-qa-' + [Guid]::NewGuid().ToString('N'))))
if (!( $sensorTarget.StartsWith($sensorOutputs + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase))) { throw 'Unsafe test installation target' }
$sensorProfile = Join-Path $env:LOCALAPPDATA 'SENSOR Technology\Remote'
function Get-SensorProfileFingerprint {
    if (Test-Path -LiteralPath $sensorProfile) {
        @(Get-ChildItem -LiteralPath $sensorProfile -File -Recurse | Where-Object Name -ne 'desktop.lock' |
            ForEach-Object { [pscustomobject]@{ Path = $_.FullName; Hash = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash } }) |
            ConvertTo-Json -Compress
    }
}
$sensorBefore = Get-SensorProfileFingerprint
foreach ($sensorPass in 1..2) {
    $sensorProcess = Start-Process -FilePath $sensorInstaller -ArgumentList ('/S /D=' + $sensorTarget) -WindowStyle Hidden -Wait -PassThru
    if ($sensorProcess.ExitCode -ne 0) { throw "Installer pass $sensorPass failed: $($sensorProcess.ExitCode)" }
    $sensorInstall = Get-ItemProperty -LiteralPath $sensorRegistry
    if ($sensorInstall.InstallLocation -ne $sensorTarget -or $sensorInstall.DisplayVersion -ne '0.3.1') { throw 'Installation registration mismatch' }
    if ($sensorInstall.UninstallString -ne ('"' + $sensorTarget + '\Uninstall.exe"')) { throw 'Uninstall command is incorrectly quoted' }
    foreach ($sensorFile in @('SENSOR-Remote.exe','SENSOR-CLI.exe','sensor-network.json','sbom.cdx.json','Uninstall.exe')) {
        if (!(Test-Path -LiteralPath (Join-Path $sensorTarget $sensorFile))) { throw "Missing installed file: $sensorFile" }
    }
    if ($sensorPass -eq 1) {
        # Synthetic preservation sentinel, not a real user file.
        'SENSOR installer preservation fixture' | Set-Content -LiteralPath (Join-Path $sensorTarget 'unrelated-file.txt')
    }
    Write-Output "INSTALLER_PASS install_or_upgrade=$sensorPass"
}
$sensorUninstaller = Join-Path $sensorTarget 'Uninstall.exe'
$sensorProcess = Start-Process -FilePath $sensorUninstaller -ArgumentList '/S' -WindowStyle Hidden -PassThru
$sensorDeadline = [DateTime]::UtcNow.AddSeconds(40)
while ((Test-Path -LiteralPath $sensorUninstaller) -and [DateTime]::UtcNow -lt $sensorDeadline) { Start-Sleep -Milliseconds 200 }
if (Test-Path -LiteralPath $sensorRegistry) { throw 'Uninstall registration remains' }
if (Test-Path -LiteralPath (Join-Path $sensorTarget 'SENSOR-Remote.exe')) { throw 'Uninstall left the installed application' }
if (!(Test-Path -LiteralPath (Join-Path $sensorTarget 'unrelated-file.txt'))) { throw 'Uninstaller removed an unrelated file' }
if ((Get-SensorProfileFingerprint) -ne $sensorBefore) { throw 'Installer lifecycle changed the existing user profile' }
$sensorRemaining = @(Get-ChildItem -LiteralPath $sensorTarget -Force)
if ($sensorRemaining.Count -ne 1 -or $sensorRemaining[0].Name -ne 'unrelated-file.txt') { throw 'Unexpected installer payload remains' }
Write-Output "INSTALLER_PASS uninstall=true unrelated_file_preserved=true existing_profile_unchanged=true fixture=$sensorTarget"
