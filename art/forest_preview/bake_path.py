"""Blender MCP bake of the forest-slice-only warm path PBR trio."""
from pathlib import Path
import bpy
import numpy as np
import hashlib
import json

ROOT=Path(__file__).resolve().parents[2]
SRC=ROOT/"art/blender_environment/imagegen/path-albedo-v3.png"
OUT=ROOT/"assets/models/forest_slice"
OUT.mkdir(parents=True,exist_ok=True)

img=bpy.data.images.load(str(SRC),check_existing=False)
# Preserve source sRGB channel values; Blender can otherwise linearise tagged PNGs.
img.colorspace_settings.name="Non-Color"
img.scale(512,512)
values=np.empty((512*512*4,),dtype=np.float32)
img.pixels.foreach_get(values)
img.pack()
rgb=np.flipud(values.reshape(512,512,4))[...,:3].copy()
for axis in (1,0):
    for k in range(24):
        weight=.5*(1-k/24)
        lo=[slice(None)]*3;hi=[slice(None)]*3
        lo[axis]=k;hi[axis]=-1-k
        first=rgb[tuple(lo)].copy();last=rgb[tuple(hi)].copy()
        rgb[tuple(lo)]=first+(last-first)*weight
        rgb[tuple(hi)]=last+(first-last)*weight
# The afternoon Bevy exposure makes the unmodified photograph a bright
# ochre strip. A restrained slice-only albedo reduction restores dry earth
# and small stones without repainting or changing the source photograph.
rgb=np.clip(rgb*np.array([.82,.83,.93],dtype=np.float32),0,1)

def save(name,data,colorspace):
    image=bpy.data.images.new(name,width=512,height=512,alpha=True)
    image.colorspace_settings.name=colorspace
    image.pixels.foreach_set(np.flipud(data).ravel())
    image.update()
    image.filepath_raw=str(OUT/name)
    image.file_format="PNG"
    image.save()

rgba=np.ones((512,512,4),dtype=np.float32)
rgba[...,:3]=rgb
save("ground_path_albedo.png",rgba,"sRGB")
lum=.2126*rgb[...,0]+.7152*rgb[...,1]+.0722*rgb[...,2]
height=(lum+np.roll(lum,1,0)+np.roll(lum,-1,0)
        +np.roll(lum,1,1)+np.roll(lum,-1,1))/5
gx=np.roll(height,-1,1)-np.roll(height,1,1)
gy=np.roll(height,-1,0)-np.roll(height,1,0)
normal=np.stack((-gx*2.1,-gy*2.1,np.ones_like(gx)),axis=2)
normal/=np.linalg.norm(normal,axis=2)[...,None]
rgba[...,:3]=normal*.5+.5
save("ground_path_normal.png",rgba,"Non-Color")
rough=np.clip(.91+.09*(lum.mean()-lum),.76,.98)
rgba[...,:3]=rough[...,None]
save("ground_path_roughness.png",rgba,"Non-Color")

files={p.name:hashlib.sha256(p.read_bytes()).hexdigest()
       for p in sorted(OUT.glob("ground_path_*.png"))}
report={"schema":"warbell.forest_slice_path.v1",
        "source":"art/blender_environment/imagegen/path-albedo-v3.png",
        "source_sha256":hashlib.sha256(SRC.read_bytes()).hexdigest(),
        "method":"Blender import, 512 resize, 24px seam match, slice-only warm-earth exposure adjustment [0.82,0.83,0.93], luminance-derived OpenGL normal and roughness",
        "mean_rgb_255":[float(x) for x in (rgb.mean((0,1))*255)],
        "file_sha256":files}
(ROOT/"art/forest_preview/path_report.json").write_text(json.dumps(report,indent=2),encoding="utf-8")
print("WARBELL_FOREST_PATH",json.dumps(report))
