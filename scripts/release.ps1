#!/usr/bin/env pwsh
# Warbell release publisher — the mechanical half of the `/release` skill.
#
# The skill (Claude) does the JUDGMENT half BEFORE calling this:
#   * bump `version` in Cargo.toml and commit it on the main repo (this script pushes it),
#   * write player-facing release notes to a markdown file (passed via -NotesFile),
#   * write the changelog.html entry in the website repo and commit/push it there.
#
# This script does the deterministic half:
#   1. read the version from Cargo.toml  -> tag vX.Y.Z
#   2. cargo build --release             (build.rs embeds the version into the exe)
#   3. wix build warbell.wxs             -> Warbell-Setup.msi
#        (version auto-binds from the exe; STABLE filename so the website's
#         releases/latest/download/Warbell-Setup.msi link never changes)
#   4. push main + push the tag          -> the main repo's release.yml CI builds the
#                                            canonical GitHub release (zip + signed MSI)
#   5. gh release create on the website repo with Warbell-Setup.msi attached
#        (the asset the website download resolves to)
#
# Usage:  pwsh scripts/release.ps1 -NotesFile dist-notes.md
param(
    [Parameter(Mandatory)] [string] $NotesFile,
    [string] $SiteRepo = "miskibin/warbell-game"
)
$ErrorActionPreference = 'Stop'
Set-Location (Split-Path $PSScriptRoot -Parent)   # repo root

if (-not (Test-Path $NotesFile)) { throw "notes file not found: $NotesFile" }

$verLine = Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1
if (-not $verLine) { throw "could not read version from Cargo.toml" }
$ver = $verLine.Matches[0].Groups[1].Value
$tag = "v$ver"
Write-Host "==> Releasing Warbell $tag" -ForegroundColor Cyan

# 1+2. Build the release exe (embeds $ver via build.rs).
Write-Host "==> cargo build --release"
cargo build --release
if ($LASTEXITCODE) { throw "cargo build failed ($LASTEXITCODE)" }

# 3. Build the website installer (stable name; version bound to the exe's FileVersion).
Write-Host "==> wix build -> Warbell-Setup.msi"
wix build warbell.wxs -arch x64 -ext WixToolset.UI.wixext -o Warbell-Setup.msi
if ($LASTEXITCODE) { throw "wix build failed ($LASTEXITCODE)" }

# 4. Push main + the tag -> the main-repo CI builds the canonical GitHub release.
Write-Host "==> push main + tag $tag"
git push origin main
git tag $tag
git push origin $tag

# 5. Publish the website release (what the site's download link resolves to).
Write-Host "==> gh release create $tag on $SiteRepo"
gh release create $tag --repo $SiteRepo --title "Warbell $tag" --notes-file $NotesFile "Warbell-Setup.msi"

Write-Host "==> Done." -ForegroundColor Green
Write-Host "    Main-repo CI is building the canonical release for $tag."
Write-Host "    Website installer published as Warbell-Setup.msi (stable download link)."
