"""Compose scanned CC0 understory among broad existing Blender grass patches.

Only the single forest approval layout changes. Original procedural mesh kit
and campaign scatter remain untouched; direct Bevy QA decides further tuning.
"""
from pathlib import Path
import json,hashlib,math,random
from collections import Counter

ROOT=Path(__file__).resolve().parents[2]
FILE=ROOT/"assets/models/forest_slice/layout.json"
SOURCE=ROOT/"art/forest_preview/layout_after_cc0_trees.json"
if not SOURCE.exists():SOURCE.write_bytes(FILE.read_bytes())
layout=json.loads(SOURCE.read_text(encoding="utf-8"))
rows=layout["instances"]
rng=random.Random(80201008)
path=[(9,24),(6,15),(1,6),(-2,-4),(-1,-14),(-7,-24)]
def path_distance(x,z):
    best=1e9
    for (ax,az),(bx,bz) in zip(path,path[1:]):
        dx,dz=bx-ax,bz-az
        t=max(0,min(1,((x-ax)*dx+(z-az)*dz)/(dx*dx+dz*dz)))
        best=min(best,math.hypot(x-(ax+t*dx),z-(az+t*dz)))
    return best

grass=[i for i,r in enumerate(rows) if r["model"] in
       ("slice:hero_grass_a","slice:hero_grass_b")]
assert len(grass)>2400,len(grass)
near=sorted(grass,key=lambda i:(-rows[i]["position"][2],
                                path_distance(rows[i]["position"][0],rows[i]["position"][2])))
scanned=set(near[:850])
for i in grass:
    row=rows[i]
    if i in scanned:
        row["model"]="gltf:cc0_grass_patch_a" if i%2 else "gltf:cc0_grass_patch_b"
        horizontal=round(rng.uniform(1.02,1.28),5)
        row["scale"]=[horizontal,round(rng.uniform(.91,1.08),5),horizontal]
    else:
        sx,sy,sz=row["scale"]
        row["scale"]=[round(sx*1.70,5),round(sy,5),round(sz*1.70,5)]

shrubs=[i for i,r in enumerate(rows) if r["model"] in
        ("slice:hero_shrub_a","slice:hero_shrub_b")]
for i in shrubs:
    if i%3==0:continue
    row=rows[i];row["model"]="gltf:cc0_shrub_patch_a"
    s=round(rng.uniform(1.20,1.48),5);row["scale"]=[s,s,s]

flowers=[i for i,r in enumerate(rows) if r["model"].startswith("slice:hero_flower_")]
for j,i in enumerate(flowers):
    row=rows[i]
    if j%3==0:
        row["scale"]=[round(v*.68,5) for v in row["scale"]]
    elif j%3==1:
        row["model"]="gltf:cc0_dandelion_a"
        s=round(rng.uniform(1.32,1.62),5);row["scale"]=[s,s,s]
    else:
        row["model"]="gltf:cc0_heliophila_a"
        s=round(rng.uniform(.85,1.12),5);row["scale"]=[s,s,s]

rocks=[i for i,r in enumerate(rows) if r["model"] in ("rock_a","rock_b","rock_c")]
rocks=sorted(rocks,key=lambda i:(path_distance(rows[i]["position"][0],rows[i]["position"][2]),
                                 -rows[i]["position"][2]))
for j,i in enumerate(rocks[:24]):
    row=rows[i]
    row["model"]="gltf:cc0_moss_rock_a" if j%2 else "gltf:cc0_moss_rock_b"
    s=round(rng.uniform(.84,1.14),5);row["scale"]=[s,s,s]

stumps=[i for i,r in enumerate(rows) if r["model"]=="stump_a"]
for i in stumps:
    row=rows[i];row["model"]="gltf:cc0_stump_a"
    s=round(rng.uniform(.78,1.02),5);row["scale"]=[s,s,s]

FILE.write_text(json.dumps(layout,indent=2),encoding="utf-8")
counts=Counter(r["model"] for r in rows if r["model"].startswith("gltf:"))
print({"layout_sha256":hashlib.sha256(FILE.read_bytes()).hexdigest(),
       "cc0_instances":dict(sorted(counts.items())),"total_instances":len(rows)})
