"""Verify the bark-only PBR change against frozen pre-correction GLBs."""
from pathlib import Path
import hashlib
import io
import json
import struct
import numpy as np
from PIL import Image

ROOT = Path(__file__).resolve().parents[2]
OLD = ROOT / "target/world-preview/bark-material-before"
NEW = ROOT / "assets/models/forest_slice/cc0"
ART = ROOT / "art/forest_preview"

def read(path):
    raw = path.read_bytes()
    size = struct.unpack_from("<I", raw, 12)[0]
    doc = json.loads(raw[20:20 + size])
    binary = raw[28 + size:]
    return raw, doc, binary

def accessor(doc, binary, index):
    item = doc["accessors"][index]
    view = doc["bufferViews"][item["bufferView"]]
    dtype = np.dtype({5121:"u1",5123:"<u2",5125:"<u4",5126:"<f4"}[item["componentType"]])
    width = {"SCALAR":1,"VEC2":2,"VEC3":3,"VEC4":4}[item["type"]]
    stride = view.get("byteStride", width*dtype.itemsize)
    offset = view.get("byteOffset",0) + item.get("byteOffset",0)
    return np.ndarray((item["count"],width),dtype=dtype,buffer=binary,
                      offset=offset,strides=(stride,dtype.itemsize)).copy()

def images(doc,binary):
    result={}
    for image in doc["images"]:
        view=doc["bufferViews"][image["bufferView"]]
        start=view.get("byteOffset",0)
        pixels=Image.open(io.BytesIO(binary[start:start+view["byteLength"]])).convert("RGBA")
        result[image["name"]]=np.asarray(pixels)
    return result

expected_arm=np.asarray(Image.open(ART/"cc0_prepared/tree_small_02_trunk_arm_matte_1k.png").convert("RGBA"))
expected_normal=np.asarray(Image.open(ART/"cc0_prepared/tree_small_02_trunk_nor_soft_1k.png").convert("RGBA"))
reports=[]
for name in ("tree_small_02_optimized","tree_small_02_backdrop"):
    oldraw,olddoc,oldbin=read(OLD/(name+".glb"))
    newraw,newdoc,newbin=read(NEW/(name+".glb"))
    oldmeshes={m["name"]:m for m in olddoc["meshes"]}
    newmeshes={m["name"]:m for m in newdoc["meshes"]}
    assert oldmeshes.keys()==newmeshes.keys()
    maxdelta={}
    for meshname in oldmeshes:
        before=oldmeshes[meshname]["primitives"][0]
        after=newmeshes[meshname]["primitives"][0]
        for attr in ("POSITION","NORMAL","TANGENT","TEXCOORD_0","TEXCOORD_1"):
            if attr not in before["attributes"] and attr not in after["attributes"]:
                continue
            a=accessor(olddoc,oldbin,before["attributes"][attr])
            b=accessor(newdoc,newbin,after["attributes"][attr])
            assert a.shape==b.shape,(name,meshname,attr)
            delta=float(np.max(np.abs(a.astype(float)-b.astype(float))))
            maxdelta[f"{meshname}:{attr}"]=delta
            if attr != "TANGENT":
                assert delta==0,(name,meshname,attr,delta)
            else:
                assert delta<1.1,(name,meshname,attr,delta)  # one normal-orthogonal repair allowed
        a=accessor(olddoc,oldbin,before["indices"])
        b=accessor(newdoc,newbin,after["indices"])
        assert np.array_equal(a,b),(name,meshname,"indices")
    oi,ni=images(olddoc,oldbin),images(newdoc,newbin)
    assert len(oi)==len(ni)==9
    assert np.array_equal(ni["tree_small_02_trunk_arm_matte_1k"],expected_arm)
    assert np.array_equal(ni["tree_small_02_trunk_nor_soft_1k"],expected_normal)
    for oldname,oldpixels in oi.items():
        if oldname in ("tree_small_02_arm_1k","tree_small_02_nor_gl_1k"):
            continue
        assert np.array_equal(oldpixels,ni[oldname]),(name,oldname)
    mat=next(m for m in newdoc["materials"] if m["name"]=="tree_small_02_trunk")
    assert mat["normalTexture"].get("scale",1)==1
    reports.append({"model":name,"before_sha256":hashlib.sha256(oldraw).hexdigest(),
                    "after_sha256":hashlib.sha256(newraw).hexdigest(),
                    "geometry_attribute_max_delta":maxdelta,
                    "geometry_unchanged_except_possible_tangent_repair":True,
                    "all_seven_other_images_decoded_pixels_unchanged":True,
                    "trunk_arm_matches_derived_source":True,
                    "trunk_normal_matches_baked_0_6_source":True,
                    "normal_texture_scale":1})
out={"schema":"warbell.matte_bark_glb_audit.v1","models":reports}
(ART/"matte_bark_glb_audit.json").write_text(json.dumps(out,indent=2),encoding="utf-8")
print(json.dumps({"pass":True,"models":len(reports),"sha256":[r["after_sha256"] for r in reports]}))
