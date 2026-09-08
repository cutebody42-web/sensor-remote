param([long]$RunId = 34153021049, [string]$GuiViewerPublic)
$ErrorActionPreference = 'Stop'
$sensorRepo = (Resolve-Path -LiteralPath (Split-Path -Parent $PSScriptRoot)).Path
$sensorViewerRoot = Join-Path (Split-Path -Parent $sensorRepo) 'native-cross-video-0.3.2\viewer'
$sensorExe = Join-Path $sensorRepo 'target\release\examples\cross_desktop.exe'
if (!$GuiViewerPublic -and (!(Test-Path -LiteralPath $sensorExe) -or !(Test-Path -LiteralPath (Join-Path $sensorViewerRoot 'identity.bin')))) { throw 'Prepared viewer fixture is missing' }
$sensorEvidence = Join-Path (Split-Path -Parent $sensorRepo) ('two-computer-input-' + [Guid]::NewGuid().ToString('N'))
$null = New-Item -ItemType Directory -Path $sensorEvidence
$sensorCredential = "protocol=https`nhost=github.com`n`n" | git credential fill
$sensorFields = @{}
foreach ($sensorLine in $sensorCredential) { if ($sensorLine.Contains('=')) { $sensorParts = $sensorLine.Split('=',2); $sensorFields[$sensorParts[0]] = $sensorParts[1] } }
$sensorHeaders = @{ Authorization = ('Bearer ' + $sensorFields['password']); Accept = 'application/vnd.github+json' }
$sensorStarted = [DateTime]::UtcNow
if ($GuiViewerPublic) {
    $sensorGui = Get-Content -LiteralPath $GuiViewerPublic -Raw | ConvertFrom-Json
    $sensorGuiId = $sensorGui.device_id.Replace(' ','')
    if ($sensorGuiId -notmatch '^[1-9][0-9]{8}$' -or $sensorGui.public_key -notmatch '^[0-9a-f]{64}$' -or $sensorGui.server -ne 'https://sensor-rendezvous-production.up.railway.app') { throw 'Invalid GUI fixture identity' }
    $sensorWorkflow = 'https://api.github.com/repos/cutebody42-web/sensor-remote/actions/workflows/cross-input.yml'
    $sensorBody = @{ref='main';inputs=@{viewer_id=$sensorGuiId;viewer_key=$sensorGui.public_key}} | ConvertTo-Json
    Invoke-RestMethod -Method Post "$sensorWorkflow/dispatches" -Headers $sensorHeaders -ContentType 'application/json' -Body $sensorBody | Out-Null
    $sensorRuns = @()
    for ($sensorPoll=0; $sensorPoll -lt 12 -and $sensorRuns.Count -eq 0; $sensorPoll++) {
        Start-Sleep -Seconds 5
        $sensorRuns = @((Invoke-RestMethod "$sensorWorkflow/runs?per_page=5" -Headers $sensorHeaders).workflow_runs | Where-Object { [DateTime]$_.created_at -ge $sensorStarted.AddSeconds(-2) })
    }
    if ($sensorRuns.Count -ne 1) { throw 'Could not uniquely resolve dispatched GUI run' }
    $RunId = [long]$sensorRuns[0].id
    $sensorAttempt = 1
    $sensorApi = "https://api.github.com/repos/cutebody42-web/sensor-remote/actions/runs/$RunId"
} else {
    $sensorApi = "https://api.github.com/repos/cutebody42-web/sensor-remote/actions/runs/$RunId"
    $sensorBefore = Invoke-RestMethod $sensorApi -Headers $sensorHeaders
    $sensorAttempt = [int]$sensorBefore.run_attempt + 1
    Invoke-RestMethod -Method Post "$sensorApi/rerun" -Headers $sensorHeaders | Out-Null
}
Write-Output "Cloud fixture rerun dispatched. Run=$RunId attempt=$sensorAttempt evidence=$sensorEvidence"
$sensorDeadline = [DateTime]::UtcNow.AddMinutes(16)
$sensorArtifact = $null
do {
    Start-Sleep -Seconds 15
    $sensorRun = Invoke-RestMethod $sensorApi -Headers $sensorHeaders
    if ([int]$sensorRun.run_attempt -lt $sensorAttempt) { continue }
    $sensorArtifacts = (Invoke-RestMethod "$sensorApi/artifacts" -Headers $sensorHeaders).artifacts
    $sensorArtifact = $sensorArtifacts | Where-Object { $_.name -eq 'sensor-input-public' -and [DateTime]$_.created_at -ge $sensorStarted.AddSeconds(-2) } | Select-Object -First 1
    if ($sensorArtifact) { break }
    if ($sensorRun.status -eq 'completed') { throw "Cloud preparation failed: $($sensorRun.conclusion)" }
} while ([DateTime]::UtcNow -lt $sensorDeadline)
if (!$sensorArtifact) { throw 'Timed out waiting for actual cloud desktop' }
$sensorZip = Join-Path $sensorEvidence 'public.zip'
Invoke-WebRequest $sensorArtifact.archive_download_url -Headers $sensorHeaders -OutFile $sensorZip
$sensorPublicRoot = Join-Path $sensorEvidence 'public'
Expand-Archive -LiteralPath $sensorZip -DestinationPath $sensorPublicRoot
$sensorPublic = @(Get-ChildItem -LiteralPath $sensorPublicRoot -Recurse -File -Filter 'sensor-input-public.json')
$sensorTarget = @(Get-ChildItem -LiteralPath $sensorPublicRoot -Recurse -File -Filter 'target-public.json')
if ($sensorPublic.Count -ne 1 -or $sensorTarget.Count -ne 1) { throw 'Missing or ambiguous public fixture metadata' }
if ($GuiViewerPublic) {
    Write-Output 'GUI_TARGET_READY. Connect through the installed native app using this public metadata:'
    Get-Content -LiteralPath $sensorPublic[0].FullName
    Get-Content -LiteralPath $sensorTarget[0].FullName
    Write-Output 'The gate requires native mouse, exact synthetic text SENSOR QA مرحبا 123, wheel, then disconnect.'
} else {
    Write-Output 'Cloud target prepared. Sending real native mouse, keyboard and wheel through SENSOR.'
    & $sensorExe view-control $sensorViewerRoot $sensorPublic[0].FullName $sensorTarget[0].FullName 2>&1 | Tee-Object -FilePath (Join-Path $sensorEvidence 'laptop-viewer.log')
    if ($LASTEXITCODE) { throw 'Laptop native viewer/input failed' }
}
do {
    Start-Sleep -Seconds 5
    $sensorRun = Invoke-RestMethod $sensorApi -Headers $sensorHeaders
} while ($sensorRun.status -ne 'completed' -and [DateTime]::UtcNow -lt $sensorDeadline)
$sensorArtifact = (Invoke-RestMethod "$sensorApi/artifacts" -Headers $sensorHeaders).artifacts | Where-Object name -eq 'sensor-input-evidence' | Select-Object -First 1
if (!$sensorArtifact) { throw 'Cloud acknowledgement evidence is missing' }
$sensorZip = Join-Path $sensorEvidence 'host.zip'
Invoke-WebRequest $sensorArtifact.archive_download_url -Headers $sensorHeaders -OutFile $sensorZip
Expand-Archive -LiteralPath $sensorZip -DestinationPath (Join-Path $sensorEvidence 'host')
$sensorHostLog = Get-Content (Join-Path $sensorEvidence 'host\sensor-input.log') -Raw
Write-Output $sensorHostLog
if ($sensorRun.conclusion -ne 'success' -or $sensorHostLog -notmatch 'CROSS_COMPUTER_INPUT_PASS' -or $sensorHostLog -notmatch 'CROSS_DESKTOP_HOST_PASS') { throw 'Both computers have not passed the gate' }
Write-Output "TWO_COMPUTER_CONTROL_VERIFIED evidence=$sensorEvidence"
