"""Combine original Poly Haven 1K diffuse JPG and separate 16-bit leaf mask."""
from pathlib import Path
from PIL import Image
import hashlib
import numpy as np
import sys

ROOT=Path(__file__).resolve().parents[2]
ASSET=sys.argv[1] if len(sys.argv)>1 else "tree_small_02"
SOURCE=ROOT/f"target/world-preview/cc0-sources/{ASSET}/textures"
OUT=ROOT/f"art/forest_preview/cc0_prepared/{ASSET}_leaves_rgba_1k.png"
OUT.parent.mkdir(parents=True,exist_ok=True)
color=np.asarray(Image.open(SOURCE/f"{ASSET}_leaves_diff_1k.jpg").convert("RGB"),dtype=np.uint8)
alpha16=np.asarray(Image.open(SOURCE/f"{ASSET}_leaves_alpha_1k.png"),dtype=np.uint16)
if color.shape[:2]!=alpha16.shape:
    raise ValueError("Leaf diffuse/alpha dimensions differ")
rgba=np.empty((*alpha16.shape,4),dtype=np.uint8)
rgba[:,:,:3]=color
rgba[:,:,3]=np.rint(alpha16.astype(np.float32)/257.0).astype(np.uint8)
Image.fromarray(rgba,"RGBA").save(OUT,optimize=True)
print({"path":str(OUT.relative_to(ROOT)),"size":OUT.stat().st_size,
       "sha256":hashlib.sha256(OUT.read_bytes()).hexdigest(),
       "visible_fraction":float((rgba[:,:,3]>=128).mean()),
       "alpha_minmax":[int(rgba[:,:,3].min()),int(rgba[:,:,3].max())]})
