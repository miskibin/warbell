"""List literal runtime environment references absent from the Blender kit."""
from pathlib import Path
import json
import re

root=Path(__file__).resolve().parents[2]
models={entry["name"] for entry in json.loads(
    (root/"art/blender_environment/manifest.json").read_text(encoding="utf-8"))["models"]}
refs=set()
for source in (root/"src").glob("*.rs"):
    code=source.read_text(encoding="utf-8")
    refs.update(re.findall(r'\.model\("([^"]+)"\)',code))
# The biome mapper selects a name through match arms/arrays before calling
# model(name), so the direct-call search above cannot see those references.
biome_code=(root/"src/blenderground.rs").read_text(encoding="utf-8")
biome_refs=set(re.findall(r'"([a-z][a-z0-9_]*_[a-z0-9_]+)"',biome_code))
refs.update(biome_refs)
missing=sorted(refs-models)
print(json.dumps({"literal_references":len(refs),"biome_references":len(biome_refs),
                  "missing":missing,"unreferenced_models":len(models-refs)},indent=2))
if missing: raise SystemExit(1)
