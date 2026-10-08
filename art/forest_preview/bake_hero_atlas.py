"""Bake the forest-slice-only 2048 PBR atlas in Blender through MCP.

The oak foliage is cropped from the project's MIT leaf photograph; the bark
is the original CC0 Poly Haven Jolcham oak scan at its 1:2 aspect ratio.
"""
from pathlib import Path
import bpy
import numpy as np
import hashlib
import json

ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/"assets/models/forest_slice"
ART=ROOT/"art/forest_preview"
BARK=ART/"polyhaven/jolcham_oak_bark_01"
GRASS=ROOT/"art/blender_environment/polyhaven/grass_medium_01"
N=2048;S=512
OUT.mkdir(parents=True,exist_ok=True)

def load(path):
    im=bpy.data.images.load(str(path),check_existing=False)
    im.colorspace_settings.name="Non-Color"
    w,h=im.size
    values=np.empty((w*h*4,),dtype=np.float32)
    im.pixels.foreach_get(values)
    return np.flipud(values.reshape(h,w,4)).copy()

def resize(src,h,w):
    yi=np.clip(np.rint(np.linspace(0,src.shape[0]-1,h)).astype(np.int32),0,src.shape[0]-1)
    xi=np.clip(np.rint(np.linspace(0,src.shape[1]-1,w)).astype(np.int32),0,src.shape[1]-1)
    return src[yi[:,None],xi[None,:]]

diff=load(BARK/"jolcham_oak_bark_01_diff_1k.png")
normal=load(BARK/"jolcham_oak_bark_01_nor_gl_1k.png")
rough=load(BARK/"jolcham_oak_bark_01_rough_1k.png")
oak=load(ROOT/"assets/textures/leaves/oakL.png")
grass=load(GRASS/"grass_medium_01_diff_1k.png")
grass_alpha=load(GRASS/"grass_medium_01_alpha_1k.png")
stone=load(ROOT/"art/blender_environment/polyhaven/stony_dirt_path/stony_dirt_path_diff_1k.png")

base=np.zeros((N,N,4),dtype=np.float32)
norm=np.zeros_like(base);norm[...,:3]=(.5,.5,1);norm[...,3]=1
orm=np.zeros_like(base);orm[...,:3]=(1,.9,0);orm[...,3]=1

def cell(index,colour,alpha=None,roughness=.88,normal_map=None):
    row,col=divmod(index,4)
    y,x=row*S,col*S
    rgb=resize(colour[...,:3],S,S)
    base[y:y+S,x:x+S,:3]=rgb
    base[y:y+S,x:x+S,3]=resize(alpha,S,S) if alpha is not None else 1
    if normal_map is not None:
        norm[y:y+S,x:x+S,:3]=resize(normal_map[...,:3],S,S)
    if isinstance(roughness,np.ndarray):
        orm[y:y+S,x:x+S,1]=resize(roughness,S,S)
    else:
        orm[y:y+S,x:x+S,1]=roughness

# Source scan is 1024×2048; preserve 1:2 across stacked atlas cells 0 and 4.
base[:1024,:512,:3]=resize(diff[...,:3],1024,512)
base[:1024,:512,3]=1
norm[:1024,:512,:3]=resize(normal[...,:3],1024,512)
orm[:1024,:512,1]=resize(rough[...,0],1024,512)

# Two separately framed small leaf sprays, each retaining source alpha.
cell(1,oak[115:455,610:910],oak[115:455,610:910,3],.78)
cell(2,oak[318:725,240:700],oak[318:725,240:700,3],.81)
cell(5,oak[42:480,120:550],oak[42:480,120:550,3],.82)

# The scanned grass tussock is available for occasional photographic cards.
g=grass[786:915,193:480]
ga=grass_alpha[786:915,193:480,0]
grass_tile=np.zeros((S,S,4),dtype=np.float32)
grass_tile[110:487,21:491,:3]=resize(g[...,:3],377,470)
grass_tile[110:487,21:491,3]=resize(ga,377,470)
cell(3,grass_tile,grass_tile[...,3],.92)

# Mostly opaque surfaces serve individually modelled blades, petals, stones.
leaf_green=np.full((S,S,3),(.30,.49,.20),dtype=np.float32)
rng=np.random.default_rng(241008)
grain=rng.normal(0,.035,(S,S,1)).astype(np.float32)
cell(6,np.clip(leaf_green+grain,0,1),roughness=.83)
cell(7,stone,roughness=.92)
cell(8,diff[150:1174,0:1024],roughness=rough[...,0][150:1174,0:1024])
for index,colour in ((9,(.94,.93,.82)),(10,(.93,.76,.20)),
                     (11,(.55,.36,.73))):
    petal=np.broadcast_to(np.array(colour,dtype=np.float32),(S,S,3)).copy()
    petal=np.clip(petal+grain*.35,0,1)
    cell(index,petal,roughness=.77)
cell(12,load(ROOT/"art/blender_environment/imagegen/path-albedo-v3.png"),roughness=.95)
cell(13,oak[500:900,50:500],oak[500:900,50:500,3],.88)
cell(14,stone*.65,roughness=.96)

def save(filename,arr,colorspace):
    old=bpy.data.images.get(filename)
    if old: bpy.data.images.remove(old)
    image=bpy.data.images.new(filename,width=N,height=N,alpha=True,float_buffer=False)
    image.colorspace_settings.name=colorspace
    image.pixels.foreach_set(np.flipud(np.clip(arr,0,1)).ravel())
    image.update();image.filepath_raw=str(OUT/filename);image.file_format="PNG"
    image.save();image.pack()

save("hero_tree_atlas.png",base,"sRGB")
save("hero_tree_normal.png",norm,"Non-Color")
save("hero_tree_orm.png",orm,"Non-Color")
report={"schema":"warbell.forest_slice_hero_atlas.v1",
        "size":[N,N],"grid":[4,4],"bark_cells":[0,4],
        "sources":{
            "bark":"https://polyhaven.com/a/jolcham_oak_bark_01 (CC0, Charlotte Baglioni)",
            "oak_leaves":"assets/textures/leaves/oakL.png (project MIT attribution)",
            "grass":"https://polyhaven.com/a/grass_medium_01 (CC0)"},
        "source_sha256":{str(p.relative_to(ROOT)):hashlib.sha256(p.read_bytes()).hexdigest()
                         for p in [BARK/"jolcham_oak_bark_01_diff_1k.png",
                                   BARK/"jolcham_oak_bark_01_nor_gl_1k.png",
                                   BARK/"jolcham_oak_bark_01_rough_1k.png",
                                   ROOT/"assets/textures/leaves/oakL.png"]},
        "output_sha256":{name:hashlib.sha256((OUT/name).read_bytes()).hexdigest()
                         for name in ("hero_tree_atlas.png","hero_tree_normal.png","hero_tree_orm.png")}}
(ART/"hero_atlas_report.json").write_text(json.dumps(report,indent=2),encoding="utf-8")
print("WARBELL_FOREST_HERO_ATLAS",json.dumps(report))
