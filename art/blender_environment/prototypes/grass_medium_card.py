"""Blender MCP-only preview crop; does not change runtime atlases or kit."""
from pathlib import Path
import bpy
import numpy as np
import hashlib
import json

ROOT=Path(__file__).resolve().parents[3]
HERE=Path(__file__).resolve().parent
SOURCE=ROOT/"art/blender_environment/polyhaven/grass_medium_01"
OUT=HERE/"grass_medium_card_preview.png"
HERE.mkdir(parents=True,exist_ok=True)

def load_raw(path):
    image=bpy.data.images.load(str(path),check_existing=False)
    image.colorspace_settings.name="Non-Color"
    width,height=image.size
    values=np.empty((width*height*4,),dtype=np.float32)
    image.pixels.foreach_get(values)
    return np.flipud(values.reshape(height,width,4)).copy()

diff_path=SOURCE/"grass_medium_01_diff_1k.png"
mask_path=SOURCE/"grass_medium_01_alpha_1k.png"
diff=load_raw(diff_path)
mask=load_raw(mask_path)
# One complete photographic tussock from the lower strip of the source atlas.
# Crop excludes the large disjoint blades above and the second tuft below.
x0,y0,x1,y1=193,786,480,915
crop=diff[y0:y1,x0:x1,:3]
alpha=mask[y0:y1,x0:x1,0]
width,height=222,184
sx=np.linspace(0,crop.shape[1]-1,width)
sy=np.linspace(0,crop.shape[0]-1,height)
xi=np.clip(np.rint(sx).astype(int),0,crop.shape[1]-1)
yi=np.clip(np.rint(sy).astype(int),0,crop.shape[0]-1)
out=np.zeros((256,256,4),dtype=np.float32)
out[57:57+height,17:17+width,:3]=crop[yi[:,None],xi[None,:]]
out[57:57+height,17:17+width,3]=alpha[yi[:,None],xi[None,:]]
image=bpy.data.images.new("Grass Medium CC0 Crop Prototype",width=256,height=256,alpha=True)
image.colorspace_settings.name="sRGB"
image.pixels.foreach_set(np.flipud(out).ravel())
image.update()
image.filepath_raw=str(OUT)
image.file_format="PNG"
image.save()
report={"schema":"warbell.blender_environment_grass_proto.v1",
        "source_page":"https://polyhaven.com/a/grass_medium_01",
        "license":"CC0-1.0",
        "source_diff_sha256":hashlib.sha256(diff_path.read_bytes()).hexdigest(),
        "source_alpha_sha256":hashlib.sha256(mask_path.read_bytes()).hexdigest(),
        "crop_xyxy":[x0,y0,x1,y1],
        "alpha_coverage_0p5":float((out[...,3]>=.5).mean()),
        "top_middle_bottom_coverage":[float((out[i,:,3]>=.5).mean()) for i in (16,128,239)],
        "prototype_sha256":hashlib.sha256(OUT.read_bytes()).hexdigest()}
(HERE/"grass_medium_card_report.json").write_text(json.dumps(report,indent=2),encoding="utf-8")
print("WARBELL_GRASS_PROTO",json.dumps(report))
