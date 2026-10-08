"""Replace visible legacy oak/birch trunks with original-PBR CC0 forest trees.

The dense back rows remain inexpensive study trees. Deterministic instance
indices refer to the frozen forest-slice v4 composition; rerun is idempotent.
"""
from pathlib import Path
import json,hashlib

ROOT=Path(__file__).resolve().parents[2]
FILE=ROOT/"assets/models/forest_slice/layout.json"
SOURCE=ROOT/"art/forest_preview/layout_v4_before_cc0.json"
if not SOURCE.exists():SOURCE.write_bytes(FILE.read_bytes())
layout=json.loads(SOURCE.read_text(encoding="utf-8"))
instances=layout["instances"]
hero=[i for i,row in enumerate(instances) if row["model"] in
      ("slice:hero_oak_a","slice:hero_oak_b")]
assert hero==[29,31,38,41,45,48,50,55,60,62,63,66,69,71],hero
# Three island oaks create the natural frame and its gnarled near-right trunk.
island={36,41,62}
secondary={5,1,27,44,51,40,59,56,54,70,58,33,3,49}
assert all(instances[i]["model"].startswith("tree:") for i in secondary|{36})
for index in hero:
    row=instances[index]
    row["model"]=("gltf:island_tree_01_optimized" if index in island else
                  "gltf:tree_small_02_optimized")
    scale=(1.04 if index==62 else .95 if index in island else
           .88+.04*(index%5))
    row["scale"]=[round(scale,5)]*3
for index in secondary|{36}:
    row=instances[index]
    row["model"]=("gltf:island_tree_01_optimized" if index in island else
                  "gltf:tree_small_02_optimized")
    scale=1.0 if index==36 else .86+.045*(index%5)
    row["scale"]=[round(scale,5)]*3
for index,yaw in {62:3.55,36:4.10,41:.55}.items():
    instances[index]["rotation_y"]=yaw

FILE.write_text(json.dumps(layout,indent=2),encoding="utf-8")
counts={name:sum(i["model"]==name for i in instances)
        for name in sorted({i["model"] for i in instances if i["model"].startswith("gltf:")})}
print({"layout_sha256":hashlib.sha256(FILE.read_bytes()).hexdigest(),
       "gltf_instances":counts,"total_instances":len(instances)})
