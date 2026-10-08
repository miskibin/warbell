"""Replace flat slice grass/flowers with existing scanned CC0 patches.

Run once from the frozen v7 layout. Positions, yaw, ordering, camera, path,
and every non-target instance remain unchanged. The input snapshot makes the
mapping deterministic and prevents a rerun from scaling patches twice.
"""
from pathlib import Path
from collections import Counter
import hashlib
import json

ROOT=Path(__file__).resolve().parents[2]
ART=ROOT/"art/forest_preview"
LAYOUT=ROOT/"assets/models/forest_slice/layout.json"
SNAPSHOT=ART/"layout_v7_before_scanned_groundcover.json"
REPORT=ART/"scanned_groundcover_mapping.json"

def sha(data): return hashlib.sha256(data).hexdigest()
def bound(name):
    doc=json.loads((ROOT/"assets/models/forest_slice"/(name+".json")).read_text(encoding="utf-8"))
    lo,hi=doc["bounds"]["min"],doc["bounds"]["max"]
    return [hi[i]-lo[i] for i in range(3)]
def clip(value,low,high): return max(low,min(high,value))

if not SNAPSHOT.exists():
    SNAPSHOT.write_bytes(LAYOUT.read_bytes())
base_raw=SNAPSHOT.read_bytes()
base=json.loads(base_raw)
current=json.loads(LAYOUT.read_bytes())
assert base["schema"]=="warbell.forest_slice.v1"
assert len(base["instances"])==3433
assert current==base or REPORT.exists(),"Existing layout changed; do not overwrite"

legacy={
    "slice:hero_grass_a":"gltf:cc0_grass_patch_a",
    "slice:hero_grass_b":"gltf:cc0_grass_patch_b",
    "slice:hero_flower_white":"gltf:cc0_heliophila_a",
    "slice:hero_flower_yellow":"gltf:cc0_dandelion_a",
}
scanned_size={
    "gltf:cc0_grass_patch_a":[0.9627245069,0.3658761382,0.7945539355],
    "gltf:cc0_grass_patch_b":[0.9275512695,0.3747071028,0.8725708723],
    "gltf:cc0_heliophila_a":[0.7135484815,0.3975839615,0.6345011592],
    "gltf:cc0_dandelion_a":[0.5790539980,0.1315431446,0.4627446532],
}
old_size={name:bound(name[6:]) for name in legacy}
output=json.loads(base_raw)
changes=[]
for i,(old,new) in enumerate(zip(base["instances"],output["instances"])):
    name=old["model"]
    if name not in legacy:continue
    target=legacy[name]
    src_dims=old_size[name];dst_dims=scanned_size[target]
    sx,sy,sz=old["scale"]
    if "grass" in name:
        new_scale=[sx*src_dims[0]/dst_dims[0]*1.14,
                   clip(sy*src_dims[1]/dst_dims[1]*1.12,.75,1.18),
                   sz*src_dims[2]/dst_dims[2]*1.14]
    elif "white" in name:
        new_scale=[sx*src_dims[0]/dst_dims[0]*.95,
                   clip(sy*src_dims[1]/dst_dims[1]*.64,.78,1.05),
                   sz*src_dims[2]/dst_dims[2]*.95]
    else:  # yellow dandelions: smaller individual flower heads
        new_scale=[sx*src_dims[0]/dst_dims[0]*.90,
                   clip(sy*src_dims[1]/dst_dims[1]*.39,1.15,1.75),
                   sz*src_dims[2]/dst_dims[2]*.90]
    new["model"]=target
    new["scale"]=[round(value,5) for value in new_scale]
    changes.append({"index":i,"from":name,"to":target,
                    "position":old["position"],"rotation_y":old["rotation_y"],
                    "old_scale":old["scale"],"new_scale":new["scale"],
                    "old_height":round(src_dims[1]*sy,5),
                    "new_height":round(dst_dims[1]*new["scale"][1],5)})

assert len(changes)==1747,(len(changes),Counter(x["from"] for x in changes))
assert output["camera"]==base["camera"] and output["closeup_camera"]==base["closeup_camera"]
assert output["path_centerline"]==base["path_centerline"]
for a,b in zip(base["instances"],output["instances"]):
    assert a["position"]==b["position"] and a["rotation_y"]==b["rotation_y"]
    if a["model"] not in legacy: assert a==b
new_raw=(json.dumps(output,indent=2,ensure_ascii=False)+"\n").encode("utf-8")
if current!=base:
    assert current==output,"Rerun would overwrite a different layout"
else:
    LAYOUT.write_bytes(new_raw)
report={"schema":"warbell.scanned_groundcover_mapping.v1",
        "input_snapshot":str(SNAPSHOT.relative_to(ROOT)).replace("\\","/"),
        "input_sha256":sha(base_raw),"output_sha256":sha(LAYOUT.read_bytes()),
        "instances":len(output["instances"]),"changes":changes,
        "counts_before":dict(Counter(row["model"] for row in base["instances"])),
        "counts_after":dict(Counter(row["model"] for row in output["instances"])),
        "unchanged_position_yaw_camera_path":True}
REPORT.write_text(json.dumps(report,indent=2),encoding="utf-8")
print(json.dumps({"replaced":len(changes),
                  "height_range":[min(x["new_height"] for x in changes),
                                  max(x["new_height"] for x in changes)],
                  "layout_sha256":report["output_sha256"]}))
