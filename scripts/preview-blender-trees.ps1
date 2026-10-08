param([switch]$OriginalTrees)
$ErrorActionPreference = 'Stop'
$taskRepo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$taskExe = Join-Path $taskRepo 'target\tree-study\bin\trees.exe'
if (-not (Test-Path -LiteralPath $taskExe)) {
    $taskExe = Join-Path $taskRepo 'target\release\tileworld_bevy_forest.exe'
}
if (-not (Test-Path -LiteralPath $taskExe)) {
    throw 'Build the game first: cargo build --release --locked'
}
$previousTreeEnv = @{}
Get-ChildItem Env: | Where-Object {
    $_.Name.StartsWith('FOREST_') -or $_.Name -in @('BEVY_ASSET_ROOT', 'APPDATA')
} | ForEach-Object { $previousTreeEnv[$_.Name] = $_.Value }
try {
    Get-ChildItem Env:FOREST_* | Remove-Item
    $env:BEVY_ASSET_ROOT = $taskRepo
    $env:APPDATA = Join-Path $taskRepo 'target\tree-study\interactive-userdata'
    New-Item -ItemType Directory -Force $env:APPDATA | Out-Null
    $env:FOREST_QUALITY = 'high'
    if (-not $OriginalTrees) { $env:FOREST_BLENDERTREES = '1' }
    & $taskExe
} finally {
    Get-ChildItem Env: | Where-Object {
        $_.Name.StartsWith('FOREST_') -or $_.Name -in @('BEVY_ASSET_ROOT', 'APPDATA')
    } | Remove-Item
    foreach ($treeEnvName in $previousTreeEnv.Keys) {
        Set-Item -LiteralPath ('Env:' + $treeEnvName) -Value $previousTreeEnv[$treeEnvName]
    }
}
