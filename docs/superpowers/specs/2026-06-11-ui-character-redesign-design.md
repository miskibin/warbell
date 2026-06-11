# UI character redesign — medieval chrome + upgrade-tree graph

**Date:** 2026-06-11 · **Status:** approved (user waived per-section review after §2; "just implement")

## Goal

The UI reads "generic web app": blue-grey translucent rounded rects, Twemoji icons, and an
upgrade "tree" that is a flat 4-column card list whose Defense column (11 nodes) overflows the
screen. Redesign every surface with a **restrained medieval chrome** — readability and UX first,
decor second — and rebuild the upgrade panel as a **real tree graph that fits one screen**
(min target 1280×720).

User decisions: medieval chrome, *not* over-decorated · real tree graph layout · scope =
all surfaces · icons = game-icons.net if good, else keep Twemoji · UX fixes: tooltips/hover
detail, affordability clarity, keyboard/controller nav, HUD readability.
Approach **A** chosen: vector chrome + startup-rasterized texture, no 9-slice art packs.

## 1. Visual language

- **Palette shift:** dark panels go warm iron/charcoal-brown (`rgb(26,22,17)` family) instead of
  blue-grey `rgb(22,28,40)`. Gold accents (`GOLD`, `GOLD_DEEP`) and the parchment palette stay.
- **Chrome frame (signature, restrained):** outer 2px near-black border + inner 1px dim-gold
  hairline + 6px gold corner notches. No scrollwork.
- **Fonts:** add **Cinzel** (OFL) for titles/headers only; EB Garamond keeps parchment body;
  Inter keeps numerals/small text.
- **Texture:** startup-rasterized tiling noise (parchment grain on boards, faint linen on dark
  panels, ~5% alpha) — same technique as the icon fallback rasterizer.
- **New primitives:** `chrome_panel`, `parchment_board`, `banner` header, `medallion`
  (framed icon + state ring), `cost_chip`, global tooltip, focus ring.

## 2. Upgrade tree (`Modal::UpgradeTree`)

- Centered framed parchment sheet over dark scrim (not a full-screen wash). Header: title +
  gold/stone treasury chips.
- **Graph layout:** branch areas sized to content — Economy (3 roots) and Arsenal (1 chain)
  narrow; Defense (6 roots, depth ≤3) and Hero (3 roots, depth ≤3) wide. Tier rows by prereq
  depth. 48px icon medallions, cost label beneath. **Ink prereq lines** drawn as thin positioned
  nodes (vertical + elbow): faded while parent unowned, solid once owned. Board ≈ 1000×420px.
- **Detail strip** fixed at board bottom: focused node name, desc, cost chip, "Requires: X",
  buy hint. No inline text walls, no layout jumps.
- **State language (shared with shop):** owned = filled, branch-color ring, seal-check ·
  buyable = bright + gold ring · too-poor = normal + red cost chip · locked = faded + lock badge.

## 3. HUD

- Resource strip top-left in one chrome panel: tinted icons + Inter numerals, larger and evenly
  spaced.
- Wave banner top-center as `banner` widget: phase name big (Cinzel), timer/objective small;
  night = iron/deep-red + gold, day = parchment tint.
- HP/stamina/XP bottom-left: framed bars. Quickslots: medallion slots + keycap chips (Z/X/C/Q),
  cooldown sweep stays. Top-right buttons: small chrome icon buttons. Interact prompt: keycap +
  verb in mini banner.

## 4. Other surfaces

- **Satchel:** chrome panel, medallion grid, bottom detail strip (replaces inline text), equip =
  gold ring, pin keys as keycaps.
- **Shop:** chrome panel, medallion + name + cost-chip rows, same affordability language as tree.
- **Start / pause / game-over / settings / notices / toasts / subtitles:** same primitives —
  Cinzel titles, chrome buttons, banner toasts. Layouts unchanged where they already work.

## 5. Keyboard/controller nav (`src/ui/focus.rs`)

- `Focusable` component on actionable nodes; per-panel focus state. Arrow keys / gamepad d-pad
  move focus (nearest-in-direction by node center); Enter/E/gamepad-South activates. Activation
  routes through one event consumed alongside `Interaction::Pressed` so mouse and keys share the
  buy/equip code path. Mouse hover moves focus — hover == focus, so tooltips and detail strips
  have one driver. Focus ring = gold outline.

## 6. Icons

- Source ~45 monochrome SVGs from game-icons.net (CC-BY 3.0; add ATTRIBUTION like Twemoji's),
  pre-rasterized offline to white 64px PNGs in `assets/icons/gameicons/`, tinted at use via
  `ImageNode.color`. `IconAtlas` resolution order: gameicons map → Twemoji → procedural fallback.
  Any id without a good gameicons match simply stays Twemoji (user-approved fallback).

## 7. Files / architecture

- `src/ui/theme.rs` rework · `src/ui/widgets.rs` new primitives · new `src/ui/texture.rs`,
  `src/ui/focus.rs`, `src/ui/tooltip.rs` · `src/ui/fonts.rs` +Cinzel · `src/ui/icons.rs`
  gameicons layer.
- New `src/tree_ui.rs` plugin: tree panel UI (layout, lines, detail strip, nav); purchase
  logic stays in `economy.rs` (`try_purchase` unchanged). Shop UI restyled in place.
- Touched surfaces: `hud.rs`, `inventory.rs`, `economy.rs` (shop), start/pause/game-over screens,
  `ui/settings.rs`, `ui/notice.rs`, `subtitles.rs`.
- **No `crates/core` changes** — graph computed from existing `prereq_id` data; parity untouched.

## Verification

`cargo test` (core untouched) · `FOREST_SHOT` + `FOREST_PANEL=tree|inv|shop` screenshots per
surface, day + night HUD · board must fit 1280×720 by construction (fixed budget ≈ 1000×560
including header/detail strip).
