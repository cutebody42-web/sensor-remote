param([string]$OutputDirectory)
$ErrorActionPreference = 'Stop'
$repoPath = (Resolve-Path -LiteralPath (Split-Path -Parent $PSScriptRoot)).Path
if (-not $OutputDirectory) {
    $OutputDirectory = Join-Path (Split-Path -Parent $repoPath) 'SENSOR-Windows-0.3.0'
}
$packagePath = [IO.Path]::GetFullPath($OutputDirectory)
if (Test-Path -LiteralPath $packagePath) {
    throw "Package target already exists; choose a new output directory: $packagePath"
}
$env:PATH = (Join-Path $env:USERPROFILE '.cargo\bin') + ';' + $env:PATH
Push-Location $repoPath
try {
    & cargo build --workspace --release --locked
    if ($LASTEXITCODE) { throw 'Release build failed' }
    $metadataText = & cargo metadata --format-version 1 --locked --filter-platform x86_64-pc-windows-msvc
    if ($LASTEXITCODE) { throw 'Dependency metadata failed' }
    $metadata = $metadataText | ConvertFrom-Json
    $null = New-Item -ItemType Directory -Path $packagePath
    Copy-Item -LiteralPath 'target\release\SENSOR-Remote.exe','target\release\SENSOR-CLI.exe','target\release\sensor-relay.exe','target\release\sensor-rendezvous.exe','LICENSE','README.md','.env.example','sensor-network.json','render.yaml','railway.json','Start-SENSOR-Internet.cmd' -Destination $packagePath
    Copy-Item -LiteralPath 'docs' -Destination (Join-Path $packagePath 'docs') -Recurse
    Copy-Item -LiteralPath 'deployment' -Destination (Join-Path $packagePath 'deployment') -Recurse
    $licenseRoot = Join-Path $packagePath 'third-party-licenses'
    $null = New-Item -ItemType Directory -Path $licenseRoot
    $inventory = [Collections.Generic.List[object]]::new()
    foreach ($dependency in ($metadata.packages | Where-Object { $_.source } | Sort-Object name,version)) {
        $sourceRoot = Split-Path -Parent $dependency.manifest_path
        $target = Join-Path $licenseRoot ($dependency.name + '-' + $dependency.version)
        $licenses = @(Get-ChildItem -LiteralPath $sourceRoot -Recurse -File | Where-Object {
            $_.Name -match '^(LICENSE|LICENCE|COPYING|NOTICE|OFL)([-._].*)?$'
        })
        foreach ($license in $licenses) {
            $relative = [IO.Path]::GetRelativePath($sourceRoot, $license.FullName)
            $destination = Join-Path $target $relative
            $null = New-Item -ItemType Directory -Path (Split-Path -Parent $destination) -Force
            Copy-Item -LiteralPath $license.FullName -Destination $destination
        }
        $inventory.Add([pscustomobject]@{
            name = $dependency.name; version = $dependency.version; license = $dependency.license
            repository = $dependency.repository; collectedLicenseFiles = $licenses.Count
        })
    }
    # Generated packaging metadata, not handwritten application configuration.
    $inventory | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath (Join-Path $packagePath 'DEPENDENCIES.json') -Encoding utf8
    $revision = (& git rev-parse HEAD).Trim()
    if ($LASTEXITCODE) { throw 'Cannot record source revision' }
    if ((& git status --porcelain)) {
        "$revision (working tree changes included)" |
            Set-Content -LiteralPath (Join-Path $packagePath 'SOURCE_REVISION.txt') -Encoding utf8
    } else {
        $revision | Set-Content -LiteralPath (Join-Path $packagePath 'SOURCE_REVISION.txt') -Encoding utf8
    }
    $hashes = Get-FileHash -LiteralPath (Join-Path $packagePath 'SENSOR-Remote.exe'),(Join-Path $packagePath 'SENSOR-CLI.exe'),(Join-Path $packagePath 'sensor-relay.exe'),(Join-Path $packagePath 'sensor-rendezvous.exe') -Algorithm SHA256
    $hashes | ForEach-Object { $_.Hash.ToLowerInvariant() + '  ' + [IO.Path]::GetFileName($_.Path) } |
        Set-Content -LiteralPath (Join-Path $packagePath 'SHA256SUMS.txt') -Encoding utf8
    Write-Output "Packaged unsigned Windows development build: $packagePath"
    $hashes | Select-Object Hash,Path
} finally { Pop-Location }
