param([switch]$OriginalWorld)
$ErrorActionPreference = 'Stop'
$previewRepo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$previewExe = Join-Path $previewRepo 'target\world-preview\bin\warbell-world.exe'
if (-not (Test-Path -LiteralPath $previewExe)) {
    $previewExe = Join-Path $previewRepo 'target\release\tileworld_bevy_forest.exe'
}
if (-not (Test-Path -LiteralPath $previewExe)) {
    throw 'Build first: cargo build --release --locked'
}
$savedPreviewEnv = @{}
Get-ChildItem Env: | Where-Object {
    $_.Name.StartsWith('FOREST_') -or $_.Name -in @('BEVY_ASSET_ROOT', 'APPDATA')
} | ForEach-Object { $savedPreviewEnv[$_.Name] = $_.Value }
try {
    Get-ChildItem Env:FOREST_* | Remove-Item
    $env:BEVY_ASSET_ROOT = $previewRepo
    $env:APPDATA = Join-Path $previewRepo 'target\world-preview\interactive-userdata'
    New-Item -ItemType Directory -Force $env:APPDATA | Out-Null
    $env:FOREST_QUALITY = 'high'
    if (-not $OriginalWorld) {
        $env:FOREST_BLENDERWORLD = '1'
        $env:FOREST_BLENDERTREES = '1'
    }
    & $previewExe
} finally {
    Get-ChildItem Env: | Where-Object {
        $_.Name.StartsWith('FOREST_') -or $_.Name -in @('BEVY_ASSET_ROOT', 'APPDATA')
    } | Remove-Item
    foreach ($previewEnvName in $savedPreviewEnv.Keys) {
        Set-Item -LiteralPath ('Env:' + $previewEnvName) -Value $savedPreviewEnv[$previewEnvName]
    }
}
