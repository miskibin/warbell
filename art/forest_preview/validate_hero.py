"""Independent stdlib parity audit for Blender forest-slice hero GLB/JSON."""
from pathlib import Path
import hashlib
import json
import math
import struct

ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/"assets/models/forest_slice"
ART=ROOT/"art/forest_preview"
manifest=json.loads((ART/"hero_manifest.json").read_text(encoding="utf-8"))
raw=(OUT/"hero_forest_kit.glb").read_bytes()
assert raw[:4]==b"glTF"
jlen,jtag=struct.unpack_from("<I4s",raw,12)
assert jtag==b"JSON"
doc=json.loads(raw[20:20+jlen])
boff=20+jlen
_,btag=struct.unpack_from("<I4s",raw,boff)
assert btag==b"BIN\x00"
binary=raw[boff+8:]

def accessor(index):
    acc=doc["accessors"][index];view=doc["bufferViews"][acc["bufferView"]]
    fmt={5121:"B",5123:"H",5125:"I",5126:"f"}[acc["componentType"]]
    n={"SCALAR":1,"VEC2":2,"VEC3":3,"VEC4":4}[acc["type"]]
    stride=view.get("byteStride",struct.calcsize("<"+fmt*n))
    start=view.get("byteOffset",0)+acc.get("byteOffset",0)
    scale=({5121:255,5123:65535}.get(acc["componentType"],1)
           if acc.get("normalized") else 1)
    values=[[x/scale for x in struct.unpack_from("<"+fmt*n,binary,start+i*stride)]
            for i in range(acc["count"])]
    return [x[0] for x in values] if n==1 else values

prims={n["name"]:doc["meshes"][n["mesh"]]["primitives"][0]
       for n in doc["nodes"] if "mesh" in n and "name" in n}
report={"schema":"warbell.forest_slice_hero_validation.v1","models":[],
        "max_delta":0,"issues":[]}
for entry in manifest["models"]:
    name=entry["name"]
    data=json.loads((OUT/(name+".json")).read_text(encoding="utf-8"))
    prim=prims[name]
    if data["schema"]!="warbell.forest_slice_tree.v1":report["issues"].append(name+" schema")
    if data["material"]!=entry["material"]:report["issues"].append(name+" material")
    for key,attribute in (("positions","POSITION"),("normals","NORMAL"),
                          ("tangents","TANGENT"),("uvs","TEXCOORD_0"),
                          ("colors","COLOR_0")):
        a=accessor(prim["attributes"][attribute]);b=data[key]
        if len(a)!=len(b):report["issues"].append(name+" "+key+" length")
        delta=max((abs(x-y) for aa,bb in zip(a,b) for x,y in zip(aa,bb)),default=0)
        report["max_delta"]=max(report["max_delta"],delta)
        if delta>1e-5:report["issues"].append(name+" "+key+" mismatch")
        if not all(math.isfinite(x) for row in b for x in row):
            report["issues"].append(name+" "+key+" nonfinite")
    if data["indices"]!=[int(x) for x in accessor(prim["indices"])]:
        report["issues"].append(name+" indices mismatch")
    if not all(type(x) is int for x in data["indices"]):
        report["issues"].append(name+" indices not int")
    if data["triangles"]!=len(data["indices"])//3:
        report["issues"].append(name+" triangle count")
    if abs(data["bounds"]["min"][1])>1e-5:
        report["issues"].append(name+" base not zero")
    if max(abs(math.sqrt(sum(x*x for x in n))-1) for n in data["normals"])>1e-4:
        report["issues"].append(name+" normals not unit")
    if max(abs(math.sqrt(sum(x*x for x in t[:3]))-1) for t in data["tangents"])>1e-4:
        report["issues"].append(name+" tangents not unit")
    report["models"].append({"name":name,"triangles":data["triangles"],
                             "bounds":data["bounds"]})

for image in doc["images"]:
    view=doc["bufferViews"][image["bufferView"]]
    payload=binary[view.get("byteOffset",0):view.get("byteOffset",0)+view["byteLength"]]
    path=OUT/(image["name"]+".png")
    if not path.exists() or hashlib.sha256(payload).digest()!=hashlib.sha256(path.read_bytes()).digest():
        report["issues"].append("atlas hash "+image["name"])
for group in ("sources","outputs"):
    for relative,digest in manifest[group].items():
        path=ROOT/relative
        if not path.exists() or hashlib.sha256(path.read_bytes()).hexdigest()!=digest:
            report["issues"].append("manifest hash "+relative)

(ART/"hero_validation.json").write_text(json.dumps(report,indent=2),encoding="utf-8")
print(json.dumps({"model_count":len(report["models"]),
                  "triangles":sum(x["triangles"] for x in report["models"]),
                  "max_delta":report["max_delta"],"issues":report["issues"]}))
if report["issues"]:raise SystemExit(1)
