# Ground albedo source images

Generated on 2026-10-08 with the built-in `image_gen` tool through the `imagegen`
skill, at the user's request for realistic ground. These are AI-generated base
color inputs, not photographs or measured photogrammetry scans. Originals are
retained here; the Blender MCP pipeline imports them and produces runtime ground
maps and approximate normal/roughness detail.

Each request specified one seamless square, orthographic 2.5 × 2.5 m ground
surface with even diffuse illumination, no directional shadows, no highlights,
no vignette, no perspective, no labels, and a restrained natural palette.

| Source | Requested surface | Generation ID |
|---|---|---|
| `grass-albedo-source.png` | Fine short European meadow grass, olive/fresh green, intertwined dry grass, 15% exposed soil; no flowers or dominant stones. | `exec-f57cc94f-9e4e-4762-8d10-fdf6ee5756fc` |
| `forest-albedo-source.png` | Brown woodland loam, flattened decomposing oak/birch litter, twigs, needles, small pebbles and sparse moss. | `exec-c982f8d8-9dc9-47fc-a6a8-7134214e4f56` |
| `path-albedo-source.png` | Neutral grey-brown compact earth, crushed gravel, small embedded limestone pebbles, subtle cracks and wear. | `exec-3952bad6-fd68-4ac3-95c3-459a8e03e026` |

The generated texture previews are material inputs only. All pictures in the
world comparison gallery are unretouched screenshots captured by the game.

## Selected color revisions

The first in-game capture showed that the olive grass and warm gravel became
mustard under the existing warm sunlight. Two imagegen edits preserve the
material detail while selecting genuinely green grass and neutral grey gravel:

- `grass-albedo-v2.png`: `exec-9749f957-76ff-4114-9468-c0886954053b`.
  Healthy muted green blades, minimal dry straw, neutral brown soil.
- `path-albedo-v2.png`: `exec-62323048-73ab-4e31-8d08-0ab6f5522170`.
  Neutral cool grey mineral gravel, preserving grain, cracks and pebbles.

These revisions are the selected runtime inputs. The original source images
remain here for provenance. Lighting and time of day are unchanged for capture.
