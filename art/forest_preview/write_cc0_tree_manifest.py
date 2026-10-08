"""Freeze source, optimization, runtime and Blender-scene provenance hashes."""
from pathlib import Path
from collections import Counter
import hashlib,json

ROOT=Path(__file__).resolve().parents[2]

ART=ROOT/"art/forest_preview"
ASSETS=ROOT/"assets/models/forest_slice"
validation_report=json.loads((ART/"cc0_validation.json").read_text(encoding="utf-8"))
if not validation_report["pass"]:raise ValueError("Independent CC0 validation did not pass")
source_audit=json.loads((ART/"final_scene_audit.json").read_text(encoding="utf-8"))
if not source_audit["pass"]:raise ValueError("Blender scene audit did not pass")
validated={row["file"]:row for row in validation_report["models"]}
def sha(file):return hashlib.sha256(file.read_bytes()).hexdigest()
def item(file):return {"path":str(file.relative_to(ROOT)).replace("\\","/"),
                       "bytes":file.stat().st_size,"sha256":sha(file)}

models=[]
for name in ("tree_small_02","island_tree_01"):
    source_doc=ART/"polyhaven_api"/(name+"-files.json")
    source_gltf=ROOT/f"target/world-preview/cc0-sources/{name}/{name}_1k.gltf"
    source=json.loads(source_gltf.read_text(encoding="utf-8"))
    original_triangles=sum(source["accessors"][p["indices"]]["count"]//3
        for mesh in source["meshes"] for p in mesh["primitives"])
    output_name=name+"_optimized"
    glb=ASSETS/"cc0"/(output_name+".glb")
    validation=validated[str(glb.relative_to(ROOT)).replace("\\","/")]
    if validation["sha256"]!=sha(glb):
        raise ValueError(f"CC0 validation stale for {glb.name}")
    if any(im["name"] is None for im in validation["images"]):
        raise ValueError(f"Unnamed embedded image in {output_name}")
    if len(validation["images"])!=9:
        raise ValueError(f"Expected nine original PBR maps in {output_name}")
    if any(im["dimensions"]!=[1024,1024] for im in validation["images"]):
        raise ValueError(f"Expected original 1K maps in {output_name}")
    if any(im["name"] is None for im in validation["images"]):
        raise ValueError(f"Missing original map name in {output_name}")
    mins=[min(mesh["min"][axis] for mesh in validation["meshes"]) for axis in range(3)]
    maxs=[max(mesh["max"][axis] for mesh in validation["meshes"]) for axis in range(3)]
    download=json.loads((ART/f"cc0_{name}_downloads.json").read_text(encoding="utf-8"))
    if download["license"]!="CC0-1.0":raise ValueError(name)
    models.append({"id":output_name,"source_page":download["page"],
        "license":download["license"],"source_api_manifest":item(source_doc),
        "source_gltf":item(source_gltf),
        "source_download_manifest":item(ART/f"cc0_{name}_downloads.json"),
        "prepared_leaf_rgba":item(ART/"cc0_prepared"/(name+"_leaves_rgba_1k.png")),
        "original_triangles":original_triangles,
        "output_triangles":validation["triangles"],
        "output_bounds_y_up":{"min":mins,"max":maxs},
        "runtime_glb":item(glb),
        "editable_blend":item(ART/(output_name+".blend")),
        "material_modes":[m["name"]+":"+m["alpha_mode"] for m in validation["materials"]],
        "embedded_images":validation["images"],
        "post_export_tangent_repairs":404 if name=="island_tree_01" else 0})

layout=json.loads((ASSETS/"layout.json").read_text(encoding="utf-8"))
counts=Counter(row["model"] for row in layout["instances"] if row["model"].startswith("gltf:"))
scripts=[ART/(name+".py") for name in (
    "download_cc0","prepare_cc0_leaf_alpha","preview_cc0","separate_cc0",
    "optimize_cc0_tree","preview_cc0_island","optimize_cc0_island",
    "repair_cc0_tangents","pack_cc0_blend_sources","apply_cc0_layout",
    "apply_cc0_understory_layout","apply_final_forest_tune",
    "sync_cc0_tree_scene","sync_cc0_understory_scene",
    "audit_final_scene","refresh_scene_report")]
manifest={"schema":"warbell.forest_cc0_optimization.v1",
    "models":models,"layout":item(ASSETS/"layout.json"),
    "layout_v5_input_snapshot":item(ART/"layout_v5_before_final_tune.json"),
    "forest_scene_blend":item(ART/"forest_scene.blend"),
    "scene_report":item(ART/"scene_report.json"),
    "blender_source_audit":item(ART/"final_scene_audit.json"),
    "png_runtime_repack":item(ART/"cc0_png_repack.json"),
    "png_runtime_repack_script":item(ROOT/"tools/transcode_forest_glb_textures.py"),
    "independent_glb_validation":item(ART/"cc0_validation.json"),
    "runtime_gltf_instance_counts":dict(sorted(counts.items())),
    "authoring_scripts":[item(file) for file in scripts],
    "validation":"tools/validate_forest_cc0.py PASS for every layout-referenced GLB"}
if source_audit["layout_sha256"]!=manifest["layout"]["sha256"] or \
   source_audit["scene_sha256"]!=manifest["forest_scene_blend"]["sha256"]:
    raise ValueError("Blender source audit is stale")
out=ART/"cc0_tree_optimization.json"
out.write_text(json.dumps(manifest,indent=2),encoding="utf-8")
print(json.dumps({"manifest":str(out.relative_to(ROOT)),"models":len(models),
                  "instances":sum(counts.values()),"sha256":sha(out)}))
