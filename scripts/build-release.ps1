param([switch]$Offline)
$ErrorActionPreference = 'Stop'
$taskRoot = Split-Path -Parent $PSScriptRoot
$taskPreviousFlags = $env:CARGO_ENCODED_RUSTFLAGS
# Environment rustflags replace Cargo config flags, so retain the static CRT
# setting from .cargo/config.toml and any caller-supplied flags explicitly.
$taskFlags = @('-C', 'target-feature=+crt-static')
if ($taskPreviousFlags) {
    $taskFlags += $taskPreviousFlags.Split([char]31)
} elseif ($env:RUSTFLAGS) {
    $taskFlags += $env:RUSTFLAGS -split '\s+' | Where-Object { $_ }
}
$taskCargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
$taskRustupHome = if ($env:RUSTUP_HOME) { $env:RUSTUP_HOME } else { Join-Path $env:USERPROFILE '.rustup' }
# Apply the more specific mappings last. Both Windows path spellings can occur
# in compiler diagnostics and dependency panic locations.
foreach ($taskMapping in @(
    @($env:USERPROFILE, '/user'),
    @($taskCargoHome, '/cargo'),
    @($taskRustupHome, '/rustup'),
    @($taskRoot, '/desksteps')
)) {
    if ($taskMapping[0]) {
        $taskPath = [IO.Path]::GetFullPath($taskMapping[0]).TrimEnd('\', '/')
        $taskFlags += "--remap-path-prefix=$taskPath=$($taskMapping[1])"
        $taskFlags += "--remap-path-prefix=$($taskPath.Replace('\', '/'))=$($taskMapping[1])"
    }
}
Push-Location -LiteralPath $taskRoot
try {
    $env:CARGO_ENCODED_RUSTFLAGS = $taskFlags -join [char]31
    $taskArguments = @('build', '--release', '--locked')
    if ($Offline) { $taskArguments += '--offline' }
    & cargo @taskArguments
    if ($LASTEXITCODE -ne 0) { throw 'Release compilation failed.' }
} finally {
    $env:CARGO_ENCODED_RUSTFLAGS = $taskPreviousFlags
    Pop-Location
}
