"""Refresh the final forest scene report from saved runtime/source artifacts."""
from pathlib import Path
from collections import Counter
import json,hashlib

ROOT=Path(__file__).resolve().parents[2]
ART=ROOT/"art/forest_preview"
LAYOUT=ROOT/"assets/models/forest_slice/layout.json"
SCENE=ART/"forest_scene.blend"
layout=json.loads(LAYOUT.read_text(encoding="utf-8"))
counts=Counter(row["model"] for row in layout["instances"])
report={"schema":"warbell.forest_slice_source.v2",
        "layout_sha256":hashlib.sha256(LAYOUT.read_bytes()).hexdigest(),
        "scene_sha256":hashlib.sha256(SCENE.read_bytes()).hexdigest(),
        "scene_bytes":SCENE.stat().st_size,
        "instances":len(layout["instances"]),
        "by_model":dict(sorted(counts.items())),
        "camera":layout["camera"],
        "closeup_camera":layout["closeup_camera"],
        "path_centerline":layout["path_centerline"],
        "linked_tree_blends":["tree_small_02_optimized.blend",
                              "island_tree_01_optimized.blend",
                              "tree_small_02_backdrop.blend"]}
(ART/"scene_report.json").write_text(json.dumps(report,indent=2),encoding="utf-8")
print({"instances":report["instances"],"gltf":sum(v for k,v in counts.items() if k.startswith("gltf:")),
       "scene_bytes":report["scene_bytes"],"layout_sha256":report["layout_sha256"]})
