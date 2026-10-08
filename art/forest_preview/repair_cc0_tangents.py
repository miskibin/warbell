"""Replace degenerate Blender GLB tangents with a normal-orthogonal basis.

The GLB is authored/exported in Blender MCP. A tiny fraction of decimated
branch vertices can receive zero MikkTSpace tangents on thin slivers. This
script repairs only those 16-byte TANGENT elements in place; geometry, UVs,
indices, materials, textures, and GLB structure stay byte-identical.
"""
from pathlib import Path
import hashlib
import json
import struct
import sys
import numpy as np

ROOT=Path(__file__).resolve().parents[2]
NAME=sys.argv[1] if len(sys.argv)>1 else "island_tree_01_optimized"
FILE=ROOT/f"assets/models/forest_slice/cc0/{NAME}.glb"
raw=bytearray(FILE.read_bytes())
if struct.unpack_from("<4sI",raw)!=(b"glTF",2):raise ValueError("Not GLB 2.0")
json_size=struct.unpack_from("<I",raw,12)[0]
doc=json.loads(raw[20:20+json_size])
binary_start=20+json_size+8

def accessor(ix):
    a=doc["accessors"][ix];view=doc["bufferViews"][a["bufferView"]]
    if a["componentType"]!=5126 or a["type"] not in ("VEC3","VEC4"):
        raise ValueError("Expected float normal/tangent")
    width=3 if a["type"]=="VEC3" else 4
    stride=view.get("byteStride",width*4)
    start=binary_start+view.get("byteOffset",0)+a.get("byteOffset",0)
    array=np.ndarray((a["count"],width),dtype="<f4",buffer=raw,
                     offset=start,strides=(stride,4))
    return array

before=hashlib.sha256(raw).hexdigest()
details=[]
for mesh in doc["meshes"]:
    for primitive in mesh["primitives"]:
        attrs=primitive["attributes"]
        if "TANGENT" not in attrs:continue
        tangent=accessor(attrs["TANGENT"]);normal=accessor(attrs["NORMAL"])
        invalid=np.where(~np.isfinite(tangent).all(axis=1) |
                         (np.linalg.norm(tangent[:,:3],axis=1)<.5))[0]
        for index in invalid:
            n=normal[index].astype(np.float64)
            n/=np.linalg.norm(n)
            axis=np.array([0.,1.,0.]) if abs(n[1])<.9 else np.array([1.,0.,0.])
            t=np.cross(axis,n);t/=np.linalg.norm(t)
            handed=-1. if tangent[index,3]<0 else 1.
            tangent[index]=[float(t[0]),float(t[1]),float(t[2]),handed]
        if len(invalid):
            details.append({"mesh":mesh.get("name"),"repaired":int(len(invalid))})

after=hashlib.sha256(raw).hexdigest()
FILE.write_bytes(raw)
print(json.dumps({"file":str(FILE.relative_to(ROOT)),"before_sha256":before,
                  "after_sha256":after,"repairs":details}))
