"""Locate tangent/UV degeneracy in an exported standalone CC0 tree GLB."""
from pathlib import Path
import json,struct
import numpy as np
import sys

ROOT=Path(__file__).resolve().parents[2]
name=sys.argv[1] if len(sys.argv)>1 else "tree_small_02_optimized"
raw=(ROOT/f"assets/models/forest_slice/cc0/{name}.glb").read_bytes()
jsonlen=struct.unpack_from("<I",raw,12)[0]
doc=json.loads(raw[20:20+jsonlen])
binbase=20+jsonlen+8
def arr(ix):
    a=doc["accessors"][ix]
    v=doc["bufferViews"][a["bufferView"]]
    dtype=np.dtype({5123:"<u2",5125:"<u4",5126:"<f4"}[a["componentType"]])
    width={"SCALAR":1,"VEC2":2,"VEC3":3,"VEC4":4}[a["type"]]
    stride=v.get("byteStride",width*dtype.itemsize)
    return np.ndarray((a["count"],width),dtype=dtype,buffer=raw,
        offset=binbase+v.get("byteOffset",0)+a.get("byteOffset",0),
        strides=(stride,dtype.itemsize))
for mesh in doc["meshes"]:
    for p in mesh["primitives"]:
        attr=p["attributes"]
        tangent=arr(attr["TANGENT"])
        bad=np.where(np.linalg.norm(tangent[:,:3],axis=1)<.5)[0]
        if not len(bad):continue
        print("BAD_COUNT",mesh["name"],len(bad))
        pos=arr(attr["POSITION"]);uv=arr(attr["TEXCOORD_0"]);normal=arr(attr["NORMAL"])
        inds=arr(p["indices"]).reshape(-1,3)
        for index in bad[:12]:
            faces=inds[np.any(inds==index,axis=1)]
            print("BAD",mesh["name"],int(index),"position",pos[index].tolist(),
                  "normal",normal[index].tolist(),"uv",uv[index].tolist(),
                  "faces",len(faces))
            for face in faces[:8]:
                tex=uv[face]
                signed_area=float(np.linalg.det(np.array([tex[1]-tex[0],tex[2]-tex[0]])))
                geo=pos[face]
                area3=float(np.linalg.norm(np.cross(geo[1]-geo[0],geo[2]-geo[0])))
                print("FACE",face.tolist(),"uv",tex.tolist(),"area2",signed_area,
                      "position",geo.tolist(),"area3",area3)
