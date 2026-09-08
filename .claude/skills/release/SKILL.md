---
name: release
description: Cut a new Warbell release — bump the version, write a player-facing changelog/notes from the ACTUAL diff (not just commit subjects), build the installer, and publish the GitHub release + update the Pages site. Use when the user asks to "cut a release", "create/make a new release", "ship a new version", "release vX", "publish the build", or "update the download on the site".
---

# Cutting a Warbell release

**One repo** now: game code, the website (`site/`), and the GitHub release all live in
`miskibin/warbell`. (The old `warbell-game` website repo was merged in.)

- Pushing a `vX.Y.Z` tag triggers `.github/workflows/release.yml`, which builds the canonical
  release (Windows + Linux zip + a self-signed per-version MSI) on the tag.
- The **website** is served from `site/` via GitHub Pages (`.github/workflows/pages.yml`, on push
  to `main` touching `site/**`). It links the download to
  `releases/latest/download/Warbell-Setup.msi` — a **stable** filename, so the link never changes.
- `scripts/release.ps1` publishes the release with our handwritten notes and attaches the
  stable-name **`Warbell-Setup.msi`** (create-or-update, so it's robust to racing CI for the tag).

**One source of version: `Cargo.toml`.** `build.rs` embeds it in the exe; `warbell.wxs` binds the
MSI version to the exe; the MSI filename is fixed. So a release only ever **bumps `Cargo.toml`**
and **writes notes** — no version hand-edited anywhere else.

## You write the changelog — never just echo commit subjects

Commit messages undersell the work. Read the **actual diff** and describe what changed **from the
player's point of view**:

1. Find the baseline: `gh release list --repo miskibin/warbell --limit 3` → newest published tag.
2. `git log --oneline <baseline-tag>..HEAD` for the commits, then **`git diff <baseline>..HEAD`**
   on the changed files to see what really happened. Read any design docs under
   `docs/superpowers/specs/*-design.md` for feature intent.
3. Group into player-facing buckets — **New**, **Combat & balance**, **Fixed**, optionally
   **Under the hood**. Plain, concrete bullets: what the player sees and does, not the code.

## Checklist (create a TodoWrite item per step)

1. **Pick the version.** Ask the user patch vs minor if it's not obvious (features → minor). Bump
   `version` in `Cargo.toml`.
2. **Write release notes** to **BOTH** paths — they feed two different publish routes and only
   one of them is `dist-notes.md`:
   - `dist-notes.md` — what `scripts/release.ps1 -NotesFile` reads on the dev-machine route. It is
     **gitignored**, so it never reaches CI.
   - `docs/release-notes/vX.Y.Z.md` — what `release.yml`'s "Locate handwritten release notes" step
     looks for, and the ONLY notes CI can see. **Commit it.** Miss it and the CI route silently
     publishes GitHub's auto-generated "What's Changed" PR list as the release body instead of your
     notes (this is exactly what happened to v0.22.1 — fixed after the fact via the releases API).
     This matters most on the web/cloud route, where CI is the only publisher.

   Use the writing guidance above. End with the self-signed-installer SmartScreen note.
3. **Write the `site/changelog.html` entry**: insert a new `<article class="rel latest rv">` at the
   top of `<main class="log">`, and **demote the previous latest** (drop its `latest` class and
   remove its `<span class="rel-badge">Latest</span>`). Match the existing markup: `rel-head`
   (rel-ver / rel-badge / rel-date) · `rel-sum` · `rel-card` with `grp` blocks tagged
   `tag new` / `tag fix` / `tag perf` · `rel-foot` linking to
   `https://github.com/miskibin/warbell/releases/tag/vX`. Download links stay `Warbell-Setup.msi`.
4. **Commit the version bump + changelog + notes** with explicit paths (never `git add -A` — the
   MSI/PDB are gitignored but stay explicit):
   `git commit -- Cargo.toml Cargo.lock site/changelog.html docs/release-notes/vX.Y.Z.md`
   with a `chore(release): vX.Y.Z` message. Don't push yet — the script pushes `main`. (Pushing
   `main` with the changelog also triggers `pages.yml` to redeploy the site.)
5. **Confirm with the user** before publishing — the next step is public and hard to undo.
6. **Publish** (the mechanical half): `pwsh scripts/release.ps1 -NotesFile dist-notes.md`. It builds
   the exe + `Warbell-Setup.msi`, pushes `main` + the tag (→ CI canonical release + Pages redeploy),
   and creates/updates the release with our notes + the MSI attached.
7. **Verify**:
   - `gh release view vX.Y.Z --repo miskibin/warbell --json tagName,assets,body` → shows
     `Warbell-Setup.msi` AND **your** notes in the body, not a "What's Changed" PR list. A PR list
     means `docs/release-notes/vX.Y.Z.md` was missing at the release commit; fix the body with
     `gh release edit vX.Y.Z --notes-file docs/release-notes/vX.Y.Z.md` and commit the file.
   - `gh release list --repo miskibin/warbell --limit 1` → the new version is `Latest` (so the
     `latest/download` link resolves).
   - `gh run list --repo miskibin/warbell --workflow release.yml --limit 1` → CI build is running.

## Gotchas

- **WiX 6** must be installed: `wix --version`; if missing, `dotnet tool install --global wix --version 6.0.2`.
- `warbell.wxs` builds the MSI from `target\release\tileworld_bevy_forest.exe` + `assets\**`, so a
  `cargo build --release` must precede the `wix build` (the script does this).
- The MSI is **self-signed** → Windows SmartScreen shows "Unknown Publisher"; say so in the notes
  (a real CA cert is needed to remove it — see `release.yml`'s signing step).
- The script and CI both target the tag's release on the **same** repo. The script creates it first
  (CI's matrix build takes ~30–45 min); CI then ADDS its zips + per-version signed MSI to the same
  release and leaves our notes/body intact. You don't need to wait for CI to finish.
