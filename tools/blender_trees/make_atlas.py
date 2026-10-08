"""Build the single summer-leaf / bark atlas for the Blender-authored trees.

Leaf silhouettes come from dgreenheck/ez-tree (MIT), already shipped by Warbell.
The bark is original procedural artwork. Run with system Python + Pillow/Numpy.
"""
from pathlib import Path
import numpy as np
from PIL import Image

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "assets/models/blender_trees/tree_atlas.png"
LEAVES = ROOT / "assets/textures/leaves"

SIZE = 1024
HALF = 512
atlas = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))

def leaf(source, rect, color, saturation=0.74, base=0.55):
    im = Image.open(LEAVES / source).convert("RGBA")
    a = np.asarray(im, dtype=np.float32).copy()
    rgb = a[..., :3]
    lum = (rgb[..., 0] * .24 + rgb[..., 1] * .62 + rgb[..., 2] * .14) / 255.0
    # Preserve photographed fine contrast but enforce green-dominant summer hue.
    v = np.clip(base + (lum - .5) * saturation, .28, 1.0)
    shade = (v * 255.0)[..., None]
    a[..., :3] = np.clip(shade * np.array(color)[None, None, :], 0, 255)
    im = Image.fromarray(a.astype(np.uint8), "RGBA")
    x, y, w, h = rect
    im.thumbnail((w - 16, h - 16), Image.Resampling.LANCZOS)
    atlas.alpha_composite(im, (x + (w - im.width)//2, y + (h - im.height)//2))

leaf("oakL.png", (0, 0, HALF, HALF), (0.92, 1.00, 0.86))
leaf("aspenL.png", (HALF, 0, HALF, HALF), (0.93, 1.00, 0.88), saturation=.52, base=.68)
leaf("pineL.png", (0, HALF, HALF, HALF), (0.53, 0.86, 0.45))

rng = np.random.default_rng(20261008)
xx = np.arange(HALF, dtype=np.float32)[None, :]
yy = np.arange(HALF, dtype=np.float32)[:, None]
out = np.zeros((HALF, HALF, 4), np.uint8)
bands = [(0, 170, "oak"), (171, 340, "birch"), (341, 511, "pine")]
for lo, hi, species in bands:
    y = yy[lo:hi+1]
    n = rng.normal(0, 1, (hi-lo+1, HALF))
    fissures = np.sin(xx*.19 + np.sin(y*.035)*1.8 + np.sin(xx*.071)*1.2)
    broad = np.sin(xx*.029 + np.sin(y*.025))
    if species == "birch":
        slash = np.sin(y*.23 + xx*.017 + np.sin(xx*.07))
        scars = (slash > .85) & (np.sin(xx*.14 + y*.03) > -.45)
        value = 0.85 + .06*broad + .025*n
        base = np.array([220, 218, 199], float)
        c = base[None,None,:] * value[...,None]
        c[scars] *= np.array([.22, .25, .24])
    else:
        depth = .82 + .13*broad + .09*fissures + .045*n
        if species == "oak":
            depth -= .26 * (fissures < -.86)
            base = np.array([119, 83, 54], float)
        else:
            depth -= .23 * (fissures < -.82)
            base = np.array([106, 78, 54], float)
        c = base[None,None,:] * depth[...,None]
    out[lo:hi+1,:,:3] = np.clip(c, 0, 255).astype(np.uint8)
    out[lo:hi+1,:,3] = 255
atlas.alpha_composite(Image.fromarray(out, "RGBA"), (HALF, HALF))
OUT.parent.mkdir(parents=True, exist_ok=True)
atlas.save(OUT, optimize=True)
print(OUT)
