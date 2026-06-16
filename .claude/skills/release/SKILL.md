---
name: release
description: Cut a new Warbell release — bump the version, write a player-facing changelog/notes from the ACTUAL diff (not just commit subjects), build the installer, and publish to GitHub + the warbell-game website. Use when the user asks to "cut a release", "create/make a new release", "ship a new version", "release vX", "publish the build", or "update the download on the site".
---

# Cutting a Warbell release

Warbell ships from **two** repos:

- **main game repo** (this one). Pushing a `vX.Y.Z` tag triggers `.github/workflows/release.yml`,
  which builds the canonical GitHub release (Windows + Linux zip + a self-signed MSI).
- **website repo** `miskibin/warbell-game` (local clone usually `D:\warbell-game`). It serves
  `index.html` + `changelog.html`, and **hosts the download** as a release asset
  **`Warbell-Setup.msi`**. The site links to `releases/latest/download/Warbell-Setup.msi` — a
  **stable** filename, so the download links never need editing again.

**One source of version: `Cargo.toml`.** `build.rs` embeds it in the exe; `warbell.wxs` binds the
MSI version to the exe; the MSI filename is fixed. So a release only ever **bumps `Cargo.toml`**
and **writes notes** — no version hand-edited anywhere else.

## You write the changelog — never just echo commit subjects

Commit messages undersell the work. Read the **actual diff** and describe what changed **from the
player's point of view**:

1. Find the baseline. Git tags lag, so use the newest *published website* release as the baseline:
   `gh release list --repo miskibin/warbell-game --limit 3`.
2. `git log --oneline <baseline-tag>..HEAD` for the commits, then **`git diff <baseline>..HEAD`**
   on the changed files to see what really happened. Read any design docs under
   `docs/superpowers/specs/*-design.md` for feature intent.
3. Group into player-facing buckets — **New**, **Combat & balance**, **Fixed**, optionally
   **Under the hood**. Plain, concrete bullets: what the player sees and does, not the code.

## Checklist (create a TodoWrite item per step)

1. **Pick the version.** Ask the user patch vs minor if it's not obvious (features → minor). Bump
   `version` in `Cargo.toml`.
2. **Write release notes** to `dist-notes.md` (markdown — this becomes the website GitHub-release
   body). Use the writing guidance above. End with the self-signed-installer SmartScreen note.
3. **Write the `changelog.html` entry** in the website repo (`D:\warbell-game\changelog.html`):
   insert a new `<article class="rel latest rv">` at the top of `<main class="log">`, and **demote
   the previous latest** (drop its `latest` class and remove its `<span class="rel-badge">Latest
   </span>`). Match the existing markup: `rel-head` (rel-ver / rel-badge / rel-date) · `rel-sum` ·
   `rel-card` with `grp` blocks tagged `tag new` / `tag fix` / `tag perf` · `rel-foot` linking to
   `https://github.com/miskibin/warbell-game/releases/tag/vX`. Download links stay `Warbell-Setup.msi`.
4. **Commit the version bump** on the main repo with explicit paths (never `git add -A` — the
   MSI/PDB are gitignored but stay explicit): `git commit -- Cargo.toml Cargo.lock` with a
   `chore(release): vX.Y.Z` message. Don't push yet — the script pushes.
5. **Commit + push the website changelog**: `git -C D:\warbell-game commit -- changelog.html` then push.
6. **Confirm with the user** before publishing — the next step is public and hard to undo.
7. **Publish** (the mechanical half): `pwsh scripts/release.ps1 -NotesFile dist-notes.md`. It builds
   the exe + `Warbell-Setup.msi`, pushes main + the tag (→ CI canonical release), and creates the
   website release with the MSI attached.
8. **Verify**:
   - `gh release view vX.Y.Z --repo miskibin/warbell-game --json tagName,assets` → shows `Warbell-Setup.msi`.
   - `gh release list --repo miskibin/warbell-game --limit 1` → the new version is `Latest` (so the
     `latest/download` link resolves).
   - `gh run list --repo miskibin/warbell --workflow release.yml --limit 1` → CI build is running.

## Gotchas

- **WiX 6** must be installed: `wix --version`; if missing, `dotnet tool install --global wix --version 6.0.2`.
- `warbell.wxs` builds the website MSI from `target\release\tileworld_bevy_forest.exe` + `assets\**`,
  so a `cargo build --release` must precede the `wix build` (the script does this).
- The MSI is **self-signed** → Windows SmartScreen shows "Unknown Publisher"; say so in the notes
  (a real CA cert is needed to remove it — see `release.yml`'s signing step).
- The main-repo CI release takes ~30–45 min; you don't need to wait for it to finish.
