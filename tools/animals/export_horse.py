"""Export Henry S's "Horse" (CC-BY, credits in `assets/CREDITS.md`) as the rideable mount GLB.

    blender -b --python export_horse.py -- <Horse.fbx> <out.glb>

The source FBX (Sketchfab "source" download, with `../textures/`) is already game-shaped: a clean rest
pose facing -Y, hooves on the ground, saddle + bridle + horseshoes skinned to the one skeleton and 28
clips. The Sketchfab glTF conversion breaks the mane's skin binding, so this works from the FBX. We:

* keep the clips `src/horse.rs` plays (renamed), drop the rest;
* hook the mane / tail texture's alpha up (the FBX leaves it opaque grey cards) — patched to
  alphaMode MASK in the written GLB;
* downscale textures (4K body -> 2K, the rest -> 1K);
* scale decimetres -> metres and measure each gait's native ground speed from the planted hooves (the
  clips are in place), printed for `horse.rs`'s gait table.
"""
import bpy, json, math, os, struct, sys
from mathutils import Vector

argv = sys.argv[sys.argv.index("--") + 1:]
SRC, OUT = argv[0], argv[1]
FPS = 30
KEEP = {  # source action suffix -> clip name in the GLB
    "1 Ilde": "Idle", "2 Ilde": "Idle2", "3 Ilde": "Idle3", "6 Eat": "Eat",
    "Walk": "Walk", "Walk_L": "WalkL", "Walk_R": "WalkR",
    "Gallop": "Gallop", "Gallop_L": "GallopL", "Gallop_R": "GallopR",
    "Run_Stop": "Stop", "Buck_1": "Rear",
}
TEX = {"Horse": 2048}  # per material; everything else 1024


def log(*a):
    print("[horse]", *a, flush=True)


bpy.ops.wm.read_factory_settings(use_empty=True)
scene = bpy.context.scene
scene.render.fps = FPS
bpy.ops.import_scene.fbx(filepath=SRC, use_anim=True, ignore_leaf_bones=False)
texdir = os.path.join(os.path.dirname(os.path.dirname(SRC)), "textures")
for img in bpy.data.images:
    if img.filepath and not os.path.exists(bpy.path.abspath(img.filepath)):
        c = os.path.join(texdir, os.path.basename(img.filepath.replace("\\", "/")))
        if os.path.exists(c):
            img.filepath = c
            img.reload()
arm = next(o for o in scene.objects if o.type == 'ARMATURE')
meshes = [o for o in scene.objects if o.type == 'MESH']
for o in list(scene.objects):
    if o.type == 'MESH' and o.name.startswith("Icosphere"):
        bpy.data.objects.remove(o)

# mane / tail cards: alpha from the albedo
for m in bpy.data.materials:
    if not m.use_nodes or "hair" not in m.name.lower():
        continue
    nt = m.node_tree
    bs = next(n for n in nt.nodes if n.type == 'BSDF_PRINCIPLED')
    for l in list(nt.links):
        if l.to_node == bs and l.to_socket.name == "Base Color" and l.from_node.type == 'TEX_IMAGE':
            nt.links.new(l.from_node.outputs["Alpha"], bs.inputs["Alpha"])
# textures
for m in bpy.data.materials:
    if not m.use_nodes:
        continue
    cap = TEX.get(m.name, 1024)
    for n in m.node_tree.nodes:
        if n.type == 'TEX_IMAGE' and n.image and n.image.size[0] > cap:
            img = n.image
            k = cap / max(img.size)
            img.scale(max(1, int(img.size[0] * k)), max(1, int(img.size[1] * k)))
            img.pack()
            log(f"downscaled {img.name} -> {tuple(img.size)}")

# clips
acts = {}
for a in list(bpy.data.actions):
    suffix = a.name.split("|")[-1]
    if suffix in KEEP:
        a.name = KEEP[suffix]
        acts[a.name] = a
    else:
        bpy.data.actions.remove(a)
log("clips", sorted(acts))

# native gait speeds (m/s) from the planted hooves
pb = arm.pose.bones
hooves = [b.name for b in arm.data.bones if b.name.endswith("_0_end") and "Toe" in b.name]
ad = arm.animation_data or arm.animation_data_create()
UNIT = arm.scale.x  # FBX decimetre rig: the armature object carries x0.1
for clip in ("Walk", "Gallop"):
    a = acts[clip]
    ad.action = a
    if hasattr(ad, "action_slot") and a.slots:
        ad.action_slot = a.slots[0]
    lo, hi = int(a.frame_range[0]), int(a.frame_range[1])
    vs = []
    for h in hooves:
        tr = []
        for f in range(lo, hi + 1):
            scene.frame_set(f)
            tr.append(arm.matrix_world @ pb[h].head)
        zmin = min(p.z for p in tr)
        vs += [(tr[i + 1].y - tr[i].y) * FPS for i in range(len(tr) - 1) if tr[i].z < zmin + 0.4 and tr[i + 1].z < zmin + 0.4]
    vs.sort()
    log(f"{clip}: {(hi - lo) / FPS:.3f} s cycle, native speed {vs[len(vs) // 2] * 0.1:.3f} m/s")
ad.action = None

# the FBX imports in decimetres (the armature's own x0.1 turns its centimetre bones into dm): x0.1 more -> metres
tops = [o for o in scene.objects if o.parent is None]
for t in tops:
    t.scale = t.scale * 0.1
    t.location = t.location * 0.1
bpy.context.view_layer.update()
zmin = min((o.matrix_world @ Vector(c)).z for o in meshes if o.name in bpy.data.objects for c in o.bound_box)
for t in tops:
    t.location.z -= zmin
bpy.context.view_layer.update()
hi = max((o.matrix_world @ Vector(c)).z for o in meshes if o.name in bpy.data.objects for c in o.bound_box)
log(f"height (ears) {hi - 0:.2f} m")

for tr in list(ad.nla_tracks):
    ad.nla_tracks.remove(tr)
for clip, a in acts.items():
    tr = ad.nla_tracks.new()
    tr.name = clip
    st = tr.strips.new(clip, int(a.frame_range[0]), a)
    if hasattr(st, "action_slot") and len(getattr(a, "slots", [])):
        st.action_slot = a.slots[0]
    tr.mute = True
for x in scene.objects:
    x.select_set(True)
bpy.ops.export_scene.gltf(
    filepath=OUT, export_format='GLB', use_selection=True,
    export_apply=False, export_materials='EXPORT', export_image_format='AUTO',
    export_skins=True, export_def_bones=False, export_rest_position_armature=True,
    export_animations=True, export_animation_mode='NLA_TRACKS', export_force_sampling=True,
    export_optimize_animation_size=True, export_anim_slide_to_zero=True, export_reset_pose_bones=True,
    export_frame_range=False, export_yup=True, export_cameras=False, export_lights=False,
)

# patch: mane / tail cards alpha-tested
with open(OUT, "rb") as f:
    data = f.read()
jlen = struct.unpack_from("<I", data, 12)[0]
js = json.loads(data[20:20 + jlen])
for m in js.get("materials", []):
    if "hair" in m.get("name", "").lower():
        m["alphaMode"] = "MASK"
        m["alphaCutoff"] = 0.4
        m["doubleSided"] = True
    elif m.get("alphaMode") == "BLEND":
        m.pop("alphaMode")
raw = json.dumps(js, separators=(",", ":")).encode()
raw += b" " * ((4 - len(raw) % 4) % 4)
rest = data[20 + jlen:]
out = struct.pack("<III", 0x46546C67, 2, 12 + 8 + len(raw) + len(rest)) + struct.pack("<II", len(raw), 0x4E4F534A) + raw + rest
with open(OUT, "wb") as f:
    f.write(out)
log(f"DONE {OUT}: {len(out) / 1e6:.1f} MB, materials {[ (m['name'], m.get('alphaMode', 'OPAQUE')) for m in js['materials']]}")
