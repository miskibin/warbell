"""Sync runtime CC0 tree instances into the editable forest Blender scene.

Each optimized tree is a linked mesh library. The scene can show 29 trees
without duplicating their geometry or packed 1K PBR textures into its .blend.
Run through Blender MCP with forest_scene.blend loaded.
"""
from pathlib import Path
import bpy,json,hashlib

ROOT=Path(__file__).resolve().parents[2]
ART=ROOT/"art/forest_preview"
LAYOUT=ROOT/"assets/models/forest_slice/layout.json"
SCENE=ART/"forest_scene.blend"
instances=json.loads(LAYOUT.read_text(encoding="utf-8"))["instances"]
names=sorted({r["model"][5:] for r in instances if r["model"].startswith("gltf:")
              and r["model"][5:] in {"tree_small_02_optimized","island_tree_01_optimized"}})
sources={}
for name in names:
    file=ART/f"{name}.blend"
    with bpy.data.libraries.load(str(file),link=True) as (available,loaded):
        loaded.meshes=list(available.meshes)
    sources[name]=[m for m in loaded.meshes if m and m.materials]
    assert len(sources[name])==3,(name,sources[name])

replaced=0
for index,row in enumerate(instances):
    model=row["model"]
    if not model.startswith("gltf:") or model[5:] not in sources:continue
    prefix=f"{index:04d} "
    for old in [o for o in bpy.data.objects if o.name.startswith(prefix)]:
        bpy.data.objects.remove(old,do_unlink=True)
    x,y,z=row["position"];sx,sy,sz=row["scale"]
    for mesh in sources[model[5:]]:
        part=mesh.materials[0].name
        obj=bpy.data.objects.new(f"{index:04d} {model} | {part}",mesh)
        bpy.context.scene.collection.objects.link(obj)
        obj.location=(x,-z,y)
        obj.rotation_euler[2]=row["rotation_y"]
        obj.scale=(sx,sz,sy)
    replaced+=1

bpy.ops.file.make_paths_relative()
bpy.context.preferences.filepaths.save_version=0
bpy.ops.wm.save_as_mainfile(filepath=str(SCENE))
print("WARBELL_CC0_TREE_SCENE_SYNC",json.dumps({
    "trees":replaced,"meshes_per_tree":3,"scene_bytes":SCENE.stat().st_size,
    "scene_sha256":hashlib.sha256(SCENE.read_bytes()).hexdigest(),
    "linked_libraries":[m.library.filepath for parts in sources.values() for m in parts[:1]]}))
