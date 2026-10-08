"""Slice-only spring foliage grade, leaving the frozen tree-study atlas intact."""
from pathlib import Path
import bpy
import numpy as np
import hashlib
import json

ROOT=Path(__file__).resolve().parents[2]
SRC=ROOT/"assets/models/blender_trees/tree_atlas.png"
OUT=ROOT/"assets/models/forest_slice/tree_atlas_forest.png"
img=bpy.data.images.load(str(SRC),check_existing=False)
img.colorspace_settings.name="Non-Color"
if tuple(img.size)!=(1024,1024): raise ValueError("Frozen tree atlas dimensions changed")
values=np.empty((1024*1024*4,),dtype=np.float32)
img.pixels.foreach_get(values)
rgba=np.flipud(values.reshape(1024,1024,4)).copy()
original_bark=rgba[512:1024,512:1024].copy()
families={
    "oak":((slice(0,512),slice(0,512)),(.62,1.20,.54)),
    "birch":((slice(0,512),slice(512,1024)),(.72,.97,.55)),
    "pine":((slice(512,1024),slice(0,512)),(1.48,1.40,1.55)),
}
stats={}
for name,((ys,xs),factors) in families.items():
    cell=rgba[ys,xs]
    opaque=cell[...,3]>=.5
    before=(cell[opaque,:3].mean(0)*255).tolist()
    cell[...,:3]=np.clip(cell[...,:3]*np.array(factors,dtype=np.float32),0,1)
    after=(cell[opaque,:3].mean(0)*255).tolist()
    stats[name]={"mean_srgb_before_255":before,"mean_srgb_after_255":after,
                 "rgb_factors":factors,"alpha_coverage":float(opaque.mean())}
if not np.array_equal(rgba[512:1024,512:1024],original_bark):
    raise AssertionError("Bark pixels changed in Blender memory")
result=bpy.data.images.new("Warbell Forest Slice | spring tree atlas",width=1024,height=1024,alpha=True)
result.colorspace_settings.name="sRGB"
result.pixels.foreach_set(np.flipud(rgba).ravel())
result.update()
result.filepath_raw=str(OUT)
result.file_format="PNG"
result.save()
report={"schema":"warbell.forest_slice_tree_atlas.v1",
        "source":"assets/models/blender_trees/tree_atlas.png",
        "source_sha256":hashlib.sha256(SRC.read_bytes()).hexdigest(),
        "output_sha256":hashlib.sha256(OUT.read_bytes()).hexdigest(),
        "size":[1024,1024],"bark_quadrant_memory_exact":True,
        "families":stats}
(ROOT/"art/forest_preview/tree_atlas_report.json").write_text(json.dumps(report,indent=2),encoding="utf-8")
print("WARBELL_FOREST_TREE_ATLAS",json.dumps(report))
