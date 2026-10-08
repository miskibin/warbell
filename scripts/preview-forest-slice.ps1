param(
    [string]$Shot = '',
    [ValidateSet('high', 'ultra')][string]$Quality = 'ultra'
)
$ErrorActionPreference = 'Stop'
$forestRepo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$forestExe = Join-Path $forestRepo 'target\world-preview\bin\forest-slice.exe'
if (-not (Test-Path -LiteralPath $forestExe)) {
    $forestExe = Join-Path $forestRepo 'target\release\tileworld_bevy_forest.exe'
}
if (-not (Test-Path -LiteralPath $forestExe)) { throw 'Build first: cargo build --release --locked' }
$savedForestEnv = @{}
Get-ChildItem Env: | Where-Object {
    $_.Name.StartsWith('FOREST_') -or $_.Name -in @('BEVY_ASSET_ROOT', 'APPDATA')
} | ForEach-Object { $savedForestEnv[$_.Name] = $_.Value }
try {
    Get-ChildItem Env:FOREST_* | Remove-Item
    $env:BEVY_ASSET_ROOT = $forestRepo
    $env:APPDATA = Join-Path $forestRepo 'target\forest-preview\userdata'
    New-Item -ItemType Directory -Force $env:APPDATA | Out-Null
    $env:FOREST_FORESTSLICE = '1'
    $env:FOREST_FREEROAM = '1'
    $env:FOREST_QUALITY = $Quality
    # Use the slice's natural-material lighting defaults from the current build.
    $env:FOREST_TIME = '0.28'
    $env:FOREST_DAY = '1000000'
    $env:FOREST_NOHUD = '1'
    $env:FOREST_MUTE = '1'
    $env:FOREST_RES = '1600x900'
    if ($Shot) {
        $env:FOREST_SHOT = [IO.Path]::GetFullPath($Shot)
        $env:FOREST_SHOT_WARMUP = '60'
    }
    & $forestExe
} finally {
    Get-ChildItem Env: | Where-Object {
        $_.Name.StartsWith('FOREST_') -or $_.Name -in @('BEVY_ASSET_ROOT', 'APPDATA')
    } | Remove-Item
    foreach ($forestEnvName in $savedForestEnv.Keys) {
        Set-Item -LiteralPath ('Env:' + $forestEnvName) -Value $savedForestEnv[$forestEnvName]
    }
}
