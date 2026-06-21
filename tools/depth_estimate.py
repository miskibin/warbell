"""Depth Anything V2 (local, transformers) -> per-pixel relative depth of a parts
sheet. Gives each part's front-face relief/bulge so procedural parts become rounded
solids instead of flat slabs. CPU is fine for one image. Outputs grayscale + colormap
+ raw .npy for the builder."""
import sys, os, numpy as np
from PIL import Image
SRC = sys.argv[1] if len(sys.argv)>1 else r"C:\Users\skibi\Downloads\Gemini_Generated_Image_cuvefjcuvefjcuve.png"
OUT = r"D:\tileworld-bevy-forest\model_proportions"
os.makedirs(OUT, exist_ok=True)
stem = os.path.splitext(os.path.basename(SRC))[0][:16]

from transformers import pipeline
print("loading Depth-Anything-V2-Small (first run downloads ~100MB)...")
pipe = pipeline("depth-estimation", model="depth-anything/Depth-Anything-V2-Small-hf", device=-1)
img = Image.open(SRC).convert("RGB")
res = pipe(img)
depth = np.array(res["depth"], dtype=np.float32)
d = (depth - depth.min())/(np.ptp(depth)+1e-6)        # 0..1, near=1

Image.fromarray((d*255).astype(np.uint8)).save(os.path.join(OUT,f"depth_{stem}.png"))
np.save(os.path.join(OUT,f"depth_{stem}.npy"), d)
try:                                   # colormap is optional (eyeballing only)
    import cv2
    cv2.imwrite(os.path.join(OUT,f"depth_{stem}_color.png"), cv2.applyColorMap((d*255).astype(np.uint8), cv2.COLORMAP_TURBO))
except Exception as e:
    print("colormap skipped:", e)
print(f"depth shape {d.shape}  -> depth_{stem}.png / .npy")
