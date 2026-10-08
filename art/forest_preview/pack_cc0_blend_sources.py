"""Pack original CC0 1K maps into each editable optimized Blender library."""
from pathlib import Path
import bpy,json,hashlib

ROOT=Path(__file__).resolve().parents[2]
ART=ROOT/"art/forest_preview"
results=[]
for name in ("tree_small_02_optimized","island_tree_01_optimized"):
    file=ART/f"{name}.blend"
    bpy.ops.wm.open_mainfile(filepath=str(file))
    bpy.data.orphans_purge(do_recursive=True)
    bpy.ops.file.pack_all()
    missing=[i.name for i in bpy.data.images if i.source=="FILE" and i.users>0 and not i.packed_file]
    if missing:raise ValueError(f"Unpacked used images in {name}: {missing}")
    bpy.context.preferences.filepaths.use_file_compression=True
    bpy.context.preferences.filepaths.save_version=0
    bpy.ops.wm.save_as_mainfile(filepath=str(file),compress=True)
    result={"name":name,"bytes":file.stat().st_size,
            "sha256":hashlib.sha256(file.read_bytes()).hexdigest(),
            "packed_images":sum(bool(i.packed_file) for i in bpy.data.images if i.source=="FILE")}
    if result["bytes"]>=100_000_000:raise ValueError(f"Source exceeds 100 MB: {result}")
    results.append(result)
print("WARBELL_CC0_PACKED_SOURCES",json.dumps(results))
