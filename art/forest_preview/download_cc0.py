"""Download exactly the Poly Haven 1k glTF files from the preserved API manifest."""
from pathlib import Path
from urllib.request import Request,urlopen
from concurrent.futures import ThreadPoolExecutor,as_completed
import hashlib
import json
import sys

ROOT=Path(__file__).resolve().parents[2]
ASSET=sys.argv[1] if len(sys.argv)>1 else "tree_small_02"
SOURCE=ROOT/f"art/forest_preview/polyhaven_api/{ASSET}-files.json"
OUT=ROOT/f"target/world-preview/cc0-sources/{ASSET}"
ART=ROOT/"art/forest_preview"
UA="Warbell-Forest-Art-Study/1.0 (https://github.com/miskibin/warbell)"
manifest=json.loads(SOURCE.read_text(encoding="utf-8"))
entry=manifest["gltf"]["1k"]["gltf"]
files={f"{ASSET}_1k.gltf":{"url":entry["url"],"md5":entry["md5"],"size":entry["size"]}}
files.update(entry["include"])
files[f"textures/{ASSET}_leaves_alpha_1k.png"]=manifest["leaves_alpha"]["1k"]["png"]

def obtain(item):
    rel,meta=item
    dest=OUT/rel
    dest.parent.mkdir(parents=True,exist_ok=True)
    if not dest.exists() or hashlib.md5(dest.read_bytes()).hexdigest()!=meta["md5"]:
        temp=dest.with_suffix(dest.suffix+".part")
        request=Request(meta["url"],headers={"User-Agent":UA})
        with urlopen(request,timeout=120) as response,temp.open("wb") as handle:
            while chunk:=response.read(1024*1024):handle.write(chunk)
        if temp.stat().st_size!=meta["size"]:
            raise ValueError(f"size mismatch: {rel}")
        if hashlib.md5(temp.read_bytes()).hexdigest()!=meta["md5"]:
            raise ValueError(f"md5 mismatch: {rel}")
        temp.replace(dest)
    return {"path":str(dest.relative_to(ROOT)),"url":meta["url"],
            "bytes":dest.stat().st_size,"md5":meta["md5"],
            "sha256":hashlib.sha256(dest.read_bytes()).hexdigest()}

with ThreadPoolExecutor(max_workers=4) as pool:
    results=[f.result() for f in as_completed([pool.submit(obtain,item)
                                                for item in files.items()])]
report={"schema":"warbell.forest_cc0_source.v1","asset":ASSET,
        "page":f"https://polyhaven.com/a/{ASSET}",
        "license":"CC0-1.0","request_user_agent":UA,
        "files":sorted(results,key=lambda x:x["path"])}
(ART/f"cc0_{ASSET}_downloads.json").write_text(json.dumps(report,indent=2),encoding="utf-8")
print(json.dumps({"files":len(results),"bytes":sum(r["bytes"] for r in results),
                  "manifest":f"art/forest_preview/cc0_{ASSET}_downloads.json"}))
