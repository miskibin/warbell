"""Independent stdlib GLB -> runtime JSON and texture/manifest parity audit."""
from pathlib import Path
import hashlib
import json
import math
import struct

ROOT=Path(__file__).resolve().parents[2]
ASSET=ROOT/"assets/models/blender_environment"
ART=ROOT/"art/blender_environment"

def glb():
    data=(ASSET/"environment_kit.glb").read_bytes()
    assert data[:4]==b"glTF"
    jlen,jtag=struct.unpack_from("<I4s",data,12)
    assert jtag==b"JSON"
    doc=json.loads(data[20:20+jlen])
    off=20+jlen
    n,tag=struct.unpack_from("<I4s",data,off)
    assert tag==b"BIN\x00"
    return doc,data[off+8:off+8+n]

def accessor(doc,binary,idx):
    acc=doc["accessors"][idx];view=doc["bufferViews"][acc["bufferView"]]
    fmt={5126:"f",5121:"B",5123:"H",5125:"I"}[acc["componentType"]]
    k={"SCALAR":1,"VEC2":2,"VEC3":3,"VEC4":4}[acc["type"]]
    size=struct.calcsize("<"+fmt)
    stride=view.get("byteStride",k*size)
    off=view.get("byteOffset",0)+acc.get("byteOffset",0)
    norm={5121:255,5123:65535}.get(acc["componentType"],1) if acc.get("normalized") else 1
    result=[[x/norm for x in struct.unpack_from("<"+fmt*k,binary,off+i*stride)]
            for i in range(acc["count"])]
    return [x[0] for x in result] if k==1 else result

def delta(a,b):
    assert len(a)==len(b),(len(a),len(b))
    return max((abs(x-y) for aa,bb in zip(a,b)
                for x,y in zip(aa,bb)),default=0)

def main():
    doc,binary=glb()
    node_meshes={node["name"]:doc["meshes"][node["mesh"]]["primitives"][0]
                 for node in doc["nodes"] if "mesh" in node and "name" in node}
    manifest=json.loads((ART/"manifest.json").read_text(encoding="utf-8"))
    report={"schema":"warbell.blender_environment_validation.v1",
            "model_count":0,"max_attribute_delta":0,"max_normal_length_error":0,
            "total_triangles":0,"images":[],"flower_petals":{},"issues":[]}
    for entry in manifest["models"]:
        name=entry["name"]
        data=json.loads((ASSET/(name+".json")).read_text(encoding="utf-8"))
        prim=node_meshes[name]
        for key,attribute in (("positions","POSITION"),("normals","NORMAL"),
                              ("uvs","TEXCOORD_0"),("colors","COLOR_0")):
            actual=accessor(doc,binary,prim["attributes"][attribute])
            error=delta(data[key],actual)
            report["max_attribute_delta"]=max(report["max_attribute_delta"],error)
            if error>1e-5: report["issues"].append(f"{name} {attribute} delta {error}")
        indices=accessor(doc,binary,prim["indices"])
        if not all(type(i) is int for i in data["indices"]):
            report["issues"].append(f"{name} index JSON type is not integer")
        if indices!=data["indices"]: report["issues"].append(f"{name} indices differ")
        if len(indices)//3!=data["triangles"]: report["issues"].append(f"{name} triangle count")
        if min(p[1] for p in data["positions"]) < -1e-5:
            report["issues"].append(f"{name} base below zero")
        for n in data["normals"]:
            error=abs(math.sqrt(sum(x*x for x in n))-1)
            report["max_normal_length_error"]=max(report["max_normal_length_error"],error)
        report["model_count"]+=1
        report["total_triangles"]+=data["triangles"]
        roof_cells={"hut":7,"cottage":7,"townhouse":5,"longhouse":6,
                    "keep_core":6,"tower":6}
        if name in roof_cells:
            cell=roof_cells[name]
            roof_normals=[n for uv,n in zip(data["uvs"],data["normals"])
                          if min(3,int(uv[0]*4))+4*min(3,int(uv[1]*4))==cell]
            if not roof_normals or min(n[1] for n in roof_normals)<=.05:
                report["issues"].append(f"{name} roof normal faces downward")
        if name in ("meadow_flowers_yellow","meadow_flowers_white",
                    "meadow_flowers_purple"):
            # The flower atlas cell is column 0, row 1. A GLB exporter may
            # silently substitute fake white COLOR_0 while JSON↔GLB parity
            # still passes, so verify the actual authored petal palette.
            petals=[color for uv,color in zip(data["uvs"],data["colors"])
                    if 0<uv[0]<.25 and .25<uv[1]<.5]
            if len(petals)<40:
                report["issues"].append(f"{name} flower UVs missing")
            else:
                report["flower_petals"][name]=[
                    sum(color[channel] for color in petals)/len(petals)
                    for channel in range(3)]
    palettes=report["flower_petals"]
    for name,limits in {
        "meadow_flowers_yellow":lambda c:c[0]>.85 and c[2]<.55,
        "meadow_flowers_white":lambda c:min(c)>.80,
        "meadow_flowers_purple":lambda c:c[1]<.75 and c[2]>.75,
    }.items():
        if name not in palettes or not limits(palettes[name]):
            report["issues"].append(f"{name} COLOR_0 petal palette lost")
    for image in doc.get("images",[]):
        view=doc["bufferViews"][image["bufferView"]]
        payload=binary[view.get("byteOffset",0):view.get("byteOffset",0)+view["byteLength"]]
        digest=hashlib.sha256(payload).hexdigest()
        report["images"].append({"name":image.get("name"),"sha256":digest,"size":len(payload)})
        path=ASSET/(image.get("name","")+".png")
        if path.exists() and hashlib.sha256(path.read_bytes()).hexdigest()!=digest:
            report["issues"].append(f"embedded PNG hash differs: {path.name}")
    for rel,digest in manifest.get("sha256",{}).items():
        path=(ART if rel.endswith(".blend") else ASSET)/rel
        if path.exists() and hashlib.sha256(path.read_bytes()).hexdigest()!=digest:
            report["issues"].append(f"manifest SHA differs: {rel}")
    for rel,digest in manifest.get("source_sha256",{}).items():
        if rel=="texture_spec.json": path=ART/rel
        elif rel.startswith(("imagegen/","polyhaven/")): path=ART/rel
        else: path=ROOT/"tools/blender_environment"/rel
        if not path.exists() or hashlib.sha256(path.read_bytes()).hexdigest()!=digest:
            report["issues"].append(f"source SHA differs: {rel}")
    (ART/"validation.json").write_text(json.dumps(report,indent=2),encoding="utf-8")
    print(json.dumps(report,indent=2))
    if report["issues"]: raise SystemExit(1)

if __name__=="__main__": main()
