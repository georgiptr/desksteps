param([string]$Iscc)
$ErrorActionPreference='Stop'
function Get-ArchiveSha256([string]$Path) {
    $taskAlgorithm=[Security.Cryptography.SHA256]::Create()
    $taskStream=[IO.File]::OpenRead($Path)
    try { ([BitConverter]::ToString($taskAlgorithm.ComputeHash($taskStream))).Replace('-','').ToLowerInvariant() }
    finally { $taskStream.Dispose(); $taskAlgorithm.Dispose() }
}
$taskRoot=Split-Path -Parent $PSScriptRoot
Push-Location -LiteralPath $taskRoot
try {
    & (Join-Path $PSScriptRoot 'build-release.ps1')
    $taskMetadataText=& cargo metadata --locked --format-version 1
    if ($LASTEXITCODE -ne 0) { throw 'Cannot inspect locked dependency metadata.' }
    $taskMetadata=$taskMetadataText | ConvertFrom-Json
    $taskPackage=$taskMetadata.packages | Where-Object { $_.name -eq 'desksteps' }
    $taskVersion=$taskPackage.version
    $taskRelease=Join-Path $taskRoot 'target\release'
    $taskIcon=Get-ChildItem -LiteralPath (Join-Path $taskRelease 'build') -Filter 'DeskSteps.ico' -File -Recurse | Sort-Object LastWriteTimeUtc -Descending | Select-Object -First 1
    if (-not $taskIcon) { throw 'Generated app icon is missing.' }
    Copy-Item -LiteralPath $taskIcon.FullName -Destination (Join-Path $taskRelease 'DeskSteps.ico') -Force
    $taskNotices=[Collections.Generic.List[string]]::new()
    foreach ($taskDependency in $taskMetadata.packages | Where-Object { $_.source } | Sort-Object name,version) {
        $taskNotices.Add("$($taskDependency.name) $($taskDependency.version) - declared license: $($taskDependency.license)")
        $taskFolder=Split-Path -Parent $taskDependency.manifest_path
        $taskLicenseFiles=Get-ChildItem -LiteralPath $taskFolder -File | Where-Object { $_.Name -match '^(LICENSE|COPYING|NOTICE|UNLICENSE)' }
        foreach ($taskLicense in $taskLicenseFiles) { $taskNotices.Add("--- $($taskLicense.Name) ---"); $taskNotices.Add([IO.File]::ReadAllText($taskLicense.FullName)) }
        if (-not $taskLicenseFiles) { throw "Missing dependency license text: $($taskDependency.name)" }
    }
    [IO.File]::WriteAllText((Join-Path $taskRelease 'THIRD_PARTY_LICENSES.txt'),($taskNotices -join "`r`n`r`n"),[Text.UTF8Encoding]::new($false))
    $taskDist=Join-Path $taskRoot 'dist'
    New-Item -ItemType Directory -Force -Path $taskDist | Out-Null
    $taskStage=Join-Path $taskRoot ('target\package\'+[Guid]::NewGuid().ToString()+'\DeskSteps-'+$taskVersion)
    New-Item -ItemType Directory -Path $taskStage | Out-Null
    foreach ($taskFile in @('desksteps.exe','THIRD_PARTY_LICENSES.txt')) { Copy-Item -LiteralPath (Join-Path $taskRelease $taskFile) -Destination $taskStage }
    foreach ($taskFile in @('LICENSE','README.md')) { Copy-Item -LiteralPath (Join-Path $taskRoot $taskFile) -Destination $taskStage }
    Copy-Item -LiteralPath (Join-Path $taskRoot 'docs') -Destination $taskStage -Recurse
    New-Item -ItemType Directory -Path (Join-Path $taskStage 'data') | Out-Null
    $taskZip=Join-Path $taskDist ('DeskSteps-'+$taskVersion+'-windows-x64-portable.zip')
    Compress-Archive -LiteralPath $taskStage -DestinationPath $taskZip -Force
    if (-not $Iscc) {
        $taskCompiler=Get-Command ISCC.exe -ErrorAction SilentlyContinue
        if ($taskCompiler) { $Iscc=$taskCompiler.Source } else { $Iscc=Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6\ISCC.exe' }
    }
    if (-not (Test-Path -LiteralPath $Iscc)) { throw 'Pass -Iscc PATH to an installed Inno Setup 6 compiler.' }
    & $Iscc '/Qp' ('/DAppVersion='+$taskVersion) (Join-Path $taskRoot 'packaging\DeskSteps.iss')
    if ($LASTEXITCODE -ne 0) { throw 'Installer compilation failed.' }
    $taskSetup=Join-Path $taskDist ('DeskSteps-'+$taskVersion+'-windows-x64-setup.exe')
    $taskChecksums=foreach ($taskFile in @($taskZip,$taskSetup)) { (Get-ArchiveSha256 $taskFile)+'  '+[IO.Path]::GetFileName($taskFile) }
    [IO.File]::WriteAllLines((Join-Path $taskDist 'SHA256SUMS.txt'),$taskChecksums,[Text.UTF8Encoding]::new($false))
    Write-Output "Local unsigned packages created in $taskDist. Nothing was uploaded or launched."
} finally { Pop-Location }
