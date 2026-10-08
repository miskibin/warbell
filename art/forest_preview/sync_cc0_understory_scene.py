"""Sync scanned CC0 understory and grass scales into forest_scene.blend via MCP."""
from pathlib import Path
import bpy,json,hashlib

ROOT=Path(__file__).resolve().parents[2]
ART=ROOT/"art/forest_preview"
SCENE=ART/"forest_scene.blend"
ASSETS=ROOT/"assets/models/forest_slice"
rows=json.loads((ASSETS/"layout.json").read_text(encoding="utf-8"))["instances"]
ids=sorted({r["model"][5:] for r in rows if r["model"].startswith("gltf:cc0_")})
sources={}
for name in ids:
    before=set(bpy.data.objects)
    bpy.ops.import_scene.gltf(filepath=str(ASSETS/"cc0"/(name+".glb")))
    imported=[o for o in bpy.data.objects if o not in before]
    parts=[]
    for obj in imported:
        if obj.type!="MESH":continue
        mesh=obj.data.copy()
        mesh.transform(obj.matrix_world)
        mesh.name=f"source {name} | {obj.name}"
        parts.append(mesh)
    if not parts:raise ValueError(f"No meshes in {name}")
    sources[name]=parts
    for obj in imported:bpy.data.objects.remove(obj,do_unlink=True)

# Later art passes append a handful of already-authored flower instances to
# layout.json. Reuse their source mesh datablocks rather than recreating mesh
# geometry or depending on the pre-pass scene having those new row numbers.
flower_sources={}
for model in sorted({r["model"] for r in rows
                     if r["model"].startswith("slice:hero_flower_")}):
    exemplar=next((o for o in bpy.data.objects
                   if o.type=="MESH" and f" {model}" in o.name),None)
    if exemplar is None:raise ValueError(f"Missing authored flower source {model}")
    flower_sources[model]=exemplar.data

replaced=0
for index,row in enumerate(rows):
    prefix=f"{index:04d} "
    model=row["model"]
    if model.startswith("gltf:cc0_"):
        for old in [o for o in bpy.data.objects if o.name.startswith(prefix)]:
            bpy.data.objects.remove(old,do_unlink=True)
        meshes=sources[model[5:]]
        for part,mesh in enumerate(meshes):
            obj=bpy.data.objects.new(f"{index:04d} {model} | {part}",mesh)
            bpy.context.scene.collection.objects.link(obj)
        replaced+=1
    instances=[o for o in bpy.data.objects if o.name.startswith(prefix)]
    if not instances and model in flower_sources:
        obj=bpy.data.objects.new(f"{index:04d} {model}",flower_sources[model])
        bpy.context.scene.collection.objects.link(obj)
        instances=[obj]
    if not instances:raise ValueError(f"Missing scene object at layout index {index}: {model}")
    x,y,z=row["position"];sx,sy,sz=row["scale"]
    for obj in instances:
        obj.location=(x,-z,y)
        obj.rotation_euler[2]=row["rotation_y"]
        obj.scale=(sx,sz,sy)

bpy.ops.file.pack_all()
bpy.ops.file.make_paths_relative()
bpy.data.orphans_purge(do_recursive=True)
bpy.context.preferences.filepaths.save_version=0
bpy.ops.wm.save_as_mainfile(filepath=str(SCENE))
print("WARBELL_CC0_UNDERSTORY_SCENE_SYNC",json.dumps({
    "models":ids,"replaced_instances":replaced,"all_instances":len(rows),
    "scene_bytes":SCENE.stat().st_size,
    "scene_sha256":hashlib.sha256(SCENE.read_bytes()).hexdigest()}))
