"""Bake talking faces into an exported townsfolk GLB (in place): MakeHuman's own visemes as morph targets,
plus teeth, a tongue and a dark mouth interior, so a speaker's lips really part on each sound.

    blender -b npc_cast.blend --python add_face_morphs.py -- <npc> <npc_<npc>.glb> <mh_visemes dir> <mpfb user data dir>

Morph targets, in this order (`src/lipsync.rs` reads them by index):
  0      jaw_open                       faceunits01 `jawOpen` (ARKit face unit)
  1..14  viseme_PP FF TH DD kk CH SS nn RR aa E I O U   visemes02 (Meta/Oculus set, Mika Suominen)
  15,16  blink_l, blink_r               faceunits01 `eyeBlinkLeft` / `eyeBlinkRight`
All CC0 MakeHuman asset packs (static.makehumancommunity.org/assets/assetpacks: visemes02, faceunits01). A
`.target` file is a list of base-mesh vertex offsets (MakeHuman units, Y up), the same mesh every look in the
cast blend is built on, so each is applied to THIS look's body (its macro shape keys mixed in), then carried onto
the exported (decimated, re-atlased) GLB vertices by nearest body surface in the rest pose — the same transfer
`add_mouth_morph.py` used for its hand-made jaw (which this replaces). The nearest face must face the same way
as the GLB vertex, so a lower-lip vertex can't take the upper lip's motion across a closed slit; eyeball vertices
(nearer the eye proxy than the skin) never move with a blink.

The targets also move MakeHuman's helper teeth / tongue vertices, so the teeth (`teeth_base`) and tongue
(`tongue01`, both CC0 MakeHuman proxies) are fitted to this body through their `.mhclo` (three base vertices +
offset each, as MPFB fits clothes) and follow every target exactly. They go into one extra `npc_mouth`
primitive skinned to the head joint, coloured per vertex from their own textures (no new image), together with
a MOUTH BAG: a very dark sphere round a point behind the lips, each vertex pushed out along its ray to 4 mm
short of the head's own skin, faced inward (from outside only its far wall draws). The skin is single-sided, so
without it an open mouth showed the culled back faces, i.e. straight through the head; each bag vertex takes the
morph offsets of the skin point nearest it, so the bag opens with the jaw and lips.

Idempotent: any morph targets already on the mesh are replaced, a previous `npc_mouth` primitive is dropped, and
the buffer is compacted so no orphaned data piles up across re-runs.
"""
import bpy, bmesh, sys, os, json, struct
import numpy as np
from mathutils import Vector
from mathutils.bvhtree import BVHTree
from mathutils.kdtree import KDTree

argv = sys.argv[sys.argv.index("--") + 1:]
NPC, GLB, PACKS, MPFB = argv[0], argv[1], argv[2], argv[3]

VISEMES = ["PP", "FF", "TH", "DD", "kk", "CH", "SS", "nn", "RR", "aa", "E", "I", "O", "U"]
TARGETS = ([("jaw_open", "faceunits01/targets/faceunits/jawOpen")]
           + [(f"viseme_{v}", f"visemes02/targets/visemes/viseme_{v}") for v in VISEMES]
           + [("blink_l", "faceunits01/targets/faceunits/eyeBlinkLeft"),
              ("blink_r", "faceunits01/targets/faceunits/eyeBlinkRight")])
BAG_GAP = 0.004        # m the mouth bag stays inside the skin
BAG_MAX = 0.09         # m furthest the bag reaches (straight back: the throat)
BAG_RGB = (0.022, 0.006, 0.005)
TEETH_TRIS = 520       # per arch after the decimate


def log(*a):
    print("[face]", *a, flush=True)


# ---------------------------------------------------------------- the look's full body, rest pose
arm = next(o for o in bpy.data.objects if o.type == 'ARMATURE' and o.get("npc") == NPC)
arm.location = (0.0, 0.0, 0.0)
arm.rotation_euler = (0.0, 0.0, 0.0)
arm.data.pose_position = 'REST'
kids = [c for c in bpy.data.objects if c.parent == arm and c.type == 'MESH']
body = next(o for o in kids if o.name == NPC)
for m in body.modifiers:
    if m.type != 'ARMATURE':
        m.show_viewport = False
bpy.context.view_layer.update()
dg = bpy.context.evaluated_depsgraph_get()
me = body.evaluated_get(dg).to_mesh()
mw = body.matrix_world
co = np.array([tuple(mw @ v.co) for v in me.vertices])
polys = [tuple(p.vertices) for p in me.polygons]
SCALE = float(body.get("MPFB_GEN_scale_factor", 0.1))
NB = len(co)
log(f"{NPC}: body {NB} verts, scale {SCALE}")


def group(name, thr=0.5):
    gi = body.vertex_groups[name].index
    return [v.index for v in body.data.vertices if any(g.group == gi and g.weight > thr for g in v.groups)]


helper = np.zeros(NB, bool)
helper[group("HelperGeometry", 0.0)] = True
skin_polys = [p for p in polys if not any(helper[i] for i in p)]
skin_bvh = BVHTree.FromPolygons([Vector(c) for c in co], skin_polys)
skin_normals = []
for p in skin_polys:
    a, b, c = (Vector(co[p[0]]), Vector(co[p[1]]), Vector(co[p[2]]))
    skin_normals.append((b - a).cross(c - a).normalized())

# eye proxy: GLB eyeball vertices sit on it, and must not ride the eyelid's blink
eye = next(o for o in kids if "high-poly" in o.name)
eme = eye.evaluated_get(dg).to_mesh()
eye_bvh = BVHTree.FromPolygons([eye.matrix_world @ v.co for v in eme.vertices], [tuple(p.vertices) for p in eme.polygons])


def read_target(rel):
    """MakeHuman .target -> (NB, 3) offsets in the blend frame (MPFB: x, -z, y, x scale factor)."""
    D = np.zeros((NB, 3))
    for line in open(os.path.join(PACKS, rel + ".target")):
        p = line.split()
        if len(p) == 4 and not line.startswith("#"):
            i = int(p[0])
            if i < NB:
                D[i] = (float(p[1]), -float(p[3]), float(p[2]))
    return D * SCALE


T = np.stack([read_target(rel) for _, rel in TARGETS])  # (nT, NB, 3)
NT = len(TARGETS)
log("targets", [n for n, _ in TARGETS], "max offset mm", [round(float(np.linalg.norm(t, axis=1).max()) * 1000, 1) for t in T])

# ---------------------------------------------------------------- the GLB
raw = open(GLB, "rb").read()
off, js, bin_ = 12, None, b""
while off < len(raw):
    ln, typ = struct.unpack_from("<II", raw, off)
    chunk = raw[off + 8: off + 8 + ln]
    if typ == 0x4E4F534A:
        js = json.loads(chunk)
    else:
        bin_ = bytearray(chunk)
    off += 8 + ln

FMT = {5120: "b", 5121: "B", 5122: "h", 5123: "H", 5125: "I", 5126: "f"}
NCOMP = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4, "MAT4": 16}


def read_acc(i):
    a = js["accessors"][i]
    bv = js["bufferViews"][a["bufferView"]]
    start = bv.get("byteOffset", 0) + a.get("byteOffset", 0)
    n, ct = NCOMP[a["type"]], a["componentType"]
    dt = np.dtype("<" + FMT[ct])
    stride = bv.get("byteStride", 0) or n * dt.itemsize
    out = np.empty((a["count"], n), dt)
    for k in range(a["count"]):
        out[k] = np.frombuffer(bin_, dt, n, start + k * stride)
    if a.get("normalized") and ct != 5126:
        out = out.astype(np.float32) / np.iinfo(dt).max
    return out


def add_acc(arr, ctype, typ, target=None, minmax=False):
    while len(bin_) % 4:
        bin_.append(0)
    start = len(bin_)
    data = np.ascontiguousarray(arr).astype("<" + FMT[ctype])
    bin_.extend(data.tobytes())
    bv = {"buffer": 0, "byteOffset": start, "byteLength": data.nbytes}
    if target:
        bv["target"] = target
    js["bufferViews"].append(bv)
    a = {"bufferView": len(js["bufferViews"]) - 1, "componentType": ctype, "count": len(arr), "type": typ}
    if minmax:
        a["min"] = data.reshape(len(arr), -1).min(0).tolist()
        a["max"] = data.reshape(len(arr), -1).max(0).tolist()
    js["accessors"].append(a)
    return len(js["accessors"]) - 1


def add_sparse(arr):
    """A VEC3 morph accessor stored sparse: only the vertices that move (most of a body doesn't)."""
    nz = np.where(np.abs(arr).max(1) > 1e-7)[0]
    if len(nz) == 0:
        nz = np.array([0])
    def view(data):
        while len(bin_) % 4:
            bin_.append(0)
        start = len(bin_)
        bin_.extend(data.tobytes())
        js["bufferViews"].append({"buffer": 0, "byteOffset": start, "byteLength": data.nbytes})
        return len(js["bufferViews"]) - 1
    iv = view(nz.astype("<u4"))
    vv = view(np.ascontiguousarray(arr[nz]).astype("<f4"))
    js["accessors"].append({"componentType": 5126, "count": len(arr), "type": "VEC3",
                            "min": arr.min(0).tolist(), "max": arr.max(0).tolist(),
                            "sparse": {"count": len(nz), "indices": {"bufferView": iv, "componentType": 5125},
                                       "values": {"bufferView": vv}}})
    return len(js["accessors"]) - 1


mesh = next(m for m in js["meshes"] if any("JOINTS_0" in p["attributes"] for p in m["primitives"]))
mat_names = [m.get("name", "") for m in js["materials"]]
if "npc_mouth" in mat_names:
    mi = mat_names.index("npc_mouth")
    mesh["primitives"] = [p for p in mesh["primitives"] if p.get("material") != mi]
else:
    js["materials"].append({"name": "npc_mouth", "pbrMetallicRoughness": {"metallicFactor": 0.0, "roughnessFactor": 0.45}})
    mi = len(js["materials"]) - 1

# register the skin onto the cast body by the head joint (the export re-roots / re-rests the rig)
skin = js["skins"][0]
joint_names = [js["nodes"][j]["name"] for j in skin["joints"]]
HI = joint_names.index("head")
ibm = read_acc(skin["inverseBindMatrices"])[HI].reshape(4, 4).T
hg = np.linalg.inv(ibm)[:3, 3]
hb = arm.matrix_world @ arm.data.bones["head"].head_local
shift = np.array(hb) - np.array((hg[0], -hg[2], hg[1]))
log(f"glb -> blend shift {np.round(shift, 4)}")


def to_blend(P):
    return np.stack([P[:, 0], -P[:, 2], P[:, 1]], 1) + shift


def to_glb(B, delta=False):
    G = B if delta else B - shift
    return np.stack([G[:, 0], G[:, 2], -G[:, 1]], 1)


def transfer(Pb, Nb):
    """Per GLB vertex (blend frame + normal): the targets' offsets (nT, n, 3) off the nearest skin face that
    faces its way."""
    out = np.zeros((NT, len(Pb), 3))
    for k in range(len(Pb)):
        b = Vector(Pb[k])
        best, bd = None, 1e9
        for loc, _n, fi, d in skin_bvh.find_nearest_range(b, 0.012):
            facing = Nb is None or skin_normals[fi].dot(Vector(Nb[k])) > 0.2
            score = d + (0.0 if facing else 0.02)
            if score < bd:
                best, bd = (loc, fi, d), score
        if best is None:
            loc, _n, fi, d = skin_bvh.find_nearest(b, 0.05)
            if loc is None:
                continue
            best = (loc, fi, d)
        loc, fi, d = best
        el = eye_bvh.find_nearest(b, 0.006)
        if el[0] is not None and el[3] < d:
            continue  # an eyeball vertex
        f = skin_polys[fi]
        ws = np.array([1.0 / max((Vector(co[j]) - loc).length, 1e-5) for j in f])
        ws /= ws.sum()
        out[:, k] = np.einsum("j,tjc->tc", ws, T[:, list(f)])
    return out


# ---------------------------------------------------------------- every existing primitive
body_prim = None
deltas = []
for p in mesh["primitives"]:
    P = read_acc(p["attributes"]["POSITION"]).astype(np.float64)
    N = read_acc(p["attributes"]["NORMAL"]).astype(np.float64) if "NORMAL" in p["attributes"] else None
    Nb = None if N is None else np.stack([N[:, 0], -N[:, 2], N[:, 1]], 1)
    D = transfer(to_blend(P), Nb)
    deltas.append(D)
    if mat_names[p.get("material", 0)] == "npc_body":
        body_prim = (p, P, N, D)
    log(f"prim {mat_names[p.get('material', 0)]}: {len(P)} verts, {int((np.abs(D).sum((0, 2)) > 1e-6).sum())} move")

# ---------------------------------------------------------------- mouth: liner + teeth + tongue
mouth_pts = [Vector(co[i]) for i in group("joint-mouth")]
pivot = np.array(sum(mouth_pts, Vector()) / len(mouth_pts))
centre_b = pivot + np.array((0.0, 0.03, -0.01))      # behind the lips (faces -Y), a touch under the nose
centre_g = to_glb(centre_b[None])[0]

p, P, N, D = body_prim
idx = read_acc(p["indices"]).reshape(-1).astype(np.int64)
J = read_acc(p["attributes"]["JOINTS_0"])
W = read_acc(p["attributes"]["WEIGHTS_0"]).astype(np.float32)
tris = idx.reshape(-1, 3)
own_bvh = BVHTree.FromPolygons([Vector(v) for v in P], [tuple(t) for t in tris])
# the bag: a lat-long sphere round a point behind the lips, each vertex pushed out along its ray until it
# is BAG_GAP short of the head's own skin (so it hugs the inside of the lips, cheeks, chin, throat)
NLAT, NLON = 14, 24
dirs = []
for i in range(1, NLAT):
    th = np.pi * i / NLAT
    for j in range(NLON):
        ph = 2 * np.pi * j / NLON
        dirs.append((np.sin(th) * np.cos(ph), np.cos(th), np.sin(th) * np.sin(ph)))
dirs = np.array([(0, 1, 0)] + dirs + [(0, -1, 0)])
rad = np.full(len(dirs), np.nan)
for k, d in enumerate(dirs):
    hit = own_bvh.ray_cast(Vector(centre_g), Vector(d), BAG_MAX)
    if hit[0] is not None:
        rad[k] = max(hit[3] - BAG_GAP, 0.004)
    else:
        rad[k] = BAG_MAX if d[2] < -0.5 else np.nan   # only straight back may run out to the cap
B_idx = []
ring = lambda i, j: 1 + (i - 1) * NLON + (j % NLON)
last = len(dirs) - 1
for j in range(NLON):
    B_idx.append((0, ring(1, j + 1), ring(1, j)))
    B_idx.append((last, ring(NLAT - 1, j), ring(NLAT - 1, j + 1)))
    for i in range(1, NLAT - 1):
        a0, a1, b0, b1 = ring(i, j), ring(i, j + 1), ring(i + 1, j), ring(i + 1, j + 1)
        B_idx += [(a0, a1, b1), (a0, b1, b0)]
B_idx = np.array(B_idx)
cr = np.cross(dirs[B_idx[:, 1]] - dirs[B_idx[:, 0]], dirs[B_idx[:, 2]] - dirs[B_idx[:, 0]])
out_ = (cr * dirs[B_idx].sum(1)).sum(1) > 0
B_idx[out_] = B_idx[out_][:, ::-1]                        # wound to face inward
nbr = [set() for _ in dirs]
for t in B_idx:
    for x in t:
        nbr[x].update(t)
for _ in range(20):                                       # a ray out through a gap (the slit) takes its neighbours'
    miss = np.isnan(rad)
    if not miss.any():
        break
    for k in np.where(miss)[0]:
        r = [rad[n] for n in nbr[k] if not np.isnan(rad[n])]
        if r:
            rad[k] = min(r)
rad = np.nan_to_num(rad, nan=0.01)
for _ in range(3):                                        # no spikes into a nostril / socket
    rad = np.array([min(rad[k], np.mean([rad[n] for n in nbr[k]]) + 0.006) for k in range(len(rad))])
B_pos = centre_g + dirs * rad[:, None]
B_D = np.zeros((NT, len(B_pos), 3))
B_J = np.zeros((len(B_pos), 4), J.dtype)
B_W = np.zeros((len(B_pos), 4), np.float32)
for k, q in enumerate(B_pos):
    loc, _n, fi, _d = own_bvh.find_nearest(Vector(q))
    t = tris[fi]
    a_, b_, c_ = (P[t[0]], P[t[1]], P[t[2]])
    v0, v1, v2 = b_ - a_, c_ - a_, np.array(loc) - a_
    d00, d01, d11, d20, d21 = v0 @ v0, v0 @ v1, v1 @ v1, v2 @ v0, v2 @ v1
    den = max(d00 * d11 - d01 * d01, 1e-12)
    w1 = (d11 * d20 - d01 * d21) / den
    w2 = (d00 * d21 - d01 * d20) / den
    bw = np.clip(np.array((1 - w1 - w2, w1, w2)), 0, 1)
    bw /= bw.sum()
    B_D[:, k] = np.einsum("j,tjc->tc", bw, D[:, t])
    B_J[k], B_W[k] = J[t[np.argmax(bw)]], W[t[np.argmax(bw)]]
B_D = np.stack([to_glb(d, True) for d in B_D])
B_nrm = -dirs                                             # faces inward: from outside only its far wall shows
B_col = np.tile(np.array(BAG_RGB + (1.0,)), (len(B_pos), 1))
log(f"bag: {len(B_idx)} tris, radius {rad.min():.3f}..{rad.max():.3f}")


def read_obj(path):
    vs, vts, faces = [], [], []
    for line in open(path):
        s = line.split()
        if not s:
            continue
        if s[0] == "v":
            vs.append(tuple(map(float, s[1:4])))
        elif s[0] == "vt":
            vts.append(tuple(map(float, s[1:3])))
        elif s[0] == "f":
            faces.append([(int(c.split("/")[0]) - 1, int(c.split("/")[1]) - 1 if "/" in c and c.split("/")[1] else -1) for c in s[1:]])
    return vs, vts, faces


def read_mhclo(path):
    refs, sc, mode = [], {}, None
    for line in open(path):
        s = line.split()
        if not s or s[0].startswith("#"):
            continue
        if s[0] in ("x_scale", "y_scale", "z_scale"):
            sc[s[0]] = (int(s[1]), int(s[2]), float(s[3]))
        elif s[0] == "verts":
            mode = "v"
        elif mode == "v" and s[0].lstrip("-").replace(".", "").isdigit():
            if len(s) == 1:
                refs.append(((int(s[0]),) * 3, (1.0, 0.0, 0.0), (0.0, 0.0, 0.0)))
            else:
                refs.append((tuple(map(int, s[:3])), tuple(map(float, s[3:6])), (float(s[6]), -float(s[8]), float(s[7]))))
        else:
            mode = None
    return refs, sc


def fit(asset):
    """Fitted positions + target offsets of a MakeHuman proxy on this body (MPFB's clothes fit)."""
    folder = os.path.join(MPFB, asset)
    name = os.path.basename(asset)
    refs, sc = read_mhclo(os.path.join(folder, name + ".mhclo"))
    xs, ys, zs = sc["x_scale"], sc["y_scale"], sc["z_scale"]
    xsz = abs(co[xs[0]][0] - co[xs[1]][0]) / xs[2]
    ysz = abs(co[ys[0]][2] - co[ys[1]][2]) / ys[2]
    zsz = abs(co[zs[0]][1] - co[zs[1]][1]) / zs[2]
    V = np.array([r[0] for r in refs])
    Wt = np.array([r[1] for r in refs])
    O = np.array([r[2] for r in refs]) * np.array((xsz, zsz, ysz))
    pos = np.einsum("nj,njc->nc", Wt, co[V]) + O
    dl = np.einsum("nj,tnjc->tnc", Wt, T[:, V])
    vs, vts, faces = read_obj(os.path.join(folder, name + ".obj"))
    assert len(vs) == len(refs), (asset, len(vs), len(refs))
    return pos, dl, vts, faces, V


def texture(path):
    img = bpy.data.images.load(path)
    w, h = img.size
    px = np.array(img.pixels[:]).reshape(h, w, img.channels)[:, :, :3]
    if img.colorspace_settings.name == "sRGB":
        px = np.where(px <= 0.04045, px / 12.92, ((px + 0.055) / 1.055) ** 2.4)
    return lambda u, v: px[min(int(v * h), h - 1) % h, min(int(u * w), w - 1) % w]


def proxy_part(pos, dl, vts, faces, tex, keep_faces, tris_goal, gain):
    """One proxy piece as a decimated triangle mesh: (pos, normals, idx, deltas, colours) in the GLB frame."""
    bm = bmesh.new()
    verts = [bm.verts.new(tuple(p)) for p in pos]
    uv = bm.loops.layers.uv.new()
    for f in [f for i, f in enumerate(faces) if keep_faces[i]]:
        try:
            face = bm.faces.new([verts[c[0]] for c in f])
        except ValueError:
            continue
        for loop, c in zip(face.loops, f):
            loop[uv].uv = vts[c[1]] if c[1] >= 0 else (0.5, 0.5)
    for v in [v for v in bm.verts if not v.link_faces]:
        bm.verts.remove(v)
    m = bpy.data.meshes.new("proxy")
    bm.to_mesh(m)
    bm.free()
    o = bpy.data.objects.new("proxy", m)
    bpy.context.scene.collection.objects.link(o)
    n_tris = sum(len(pl.vertices) - 2 for pl in m.polygons)
    if n_tris > tris_goal:
        d = o.modifiers.new("dec", 'DECIMATE')
        d.ratio = tris_goal / n_tris
    o.modifiers.new("tri", 'TRIANGULATE')
    ev = o.evaluated_get(bpy.context.evaluated_depsgraph_get())
    em = ev.to_mesh()
    em.calc_loop_triangles()
    P2 = np.array([tuple(v.co) for v in em.vertices])
    N2 = np.array([tuple(v.normal) for v in em.vertices])
    I2 = np.array([tuple(t.vertices) for t in em.loop_triangles])
    col = np.zeros((len(P2), 3))
    cnt = np.zeros(len(P2))
    uvl = em.uv_layers.active.data
    for t in em.loop_triangles:
        for li, vi in zip(t.loops, t.vertices):
            u, v = uvl[li].uv
            col[vi] += tex(u, v)
            cnt[vi] += 1
    col = col / np.maximum(cnt, 1)[:, None] * gain
    # the decimate moved vertices: each takes the offsets of the nearest original one of this piece
    kd = KDTree(len(pos))
    for i, p in enumerate(pos):
        kd.insert(p, i)
    kd.balance()
    near_i = [kd.find(Vector(p))[1] for p in P2]
    D2 = dl[:, near_i]
    bpy.data.objects.remove(o)
    return (to_glb(P2), to_glb(N2, True), I2, np.stack([to_glb(d, True) for d in D2]),
            np.concatenate([col, np.ones((len(P2), 1))], 1))


upper = set(group("helper-upper-teeth", 0.0))
pieces = []
tpos, tdl, tvts, tfaces, tV = fit("teeth/teeth_base")
tex_t = texture(os.path.join(MPFB, "teeth/teeth_base/teeth.png"))
is_up = np.array([any(v in upper for v in tV[i]) for i in range(len(tV))])
for want in (True, False):
    keep = [all(is_up[c[0]] == want for c in f) for f in tfaces]
    pieces.append(proxy_part(tpos, tdl, tvts, tfaces, tex_t, keep, TEETH_TRIS, 0.9))
gpos, gdl, gvts, gfaces, _ = fit("tongue/tongue01")
tex_g = texture(os.path.join(MPFB, "tongue/tongue01/tongue01_diffuse.png"))
pieces.append(proxy_part(gpos, gdl, gvts, gfaces, tex_g, [True] * len(gfaces), 600, 0.75))
log("teeth/tongue tris", [len(pc[2]) for pc in pieces])

# assemble the mouth primitive
M_pos, M_nrm, M_idx, M_D, M_col, M_J, M_W = [B_pos], [B_nrm], [B_idx], [B_D], [B_col], [B_J], [B_W]
base = len(B_pos)
for pos_, nrm_, idx_, D_, col_ in pieces:
    M_pos.append(pos_); M_nrm.append(nrm_); M_idx.append(idx_ + base); M_D.append(D_); M_col.append(col_)
    jj = np.zeros((len(pos_), 4), J.dtype); jj[:, 0] = HI
    ww = np.zeros((len(pos_), 4), np.float32); ww[:, 0] = 1.0
    M_J.append(jj); M_W.append(ww)
    base += len(pos_)
M_pos, M_nrm, M_idx = np.concatenate(M_pos), np.concatenate(M_nrm), np.concatenate(M_idx)
M_D, M_col, M_J, M_W = np.concatenate(M_D, 1), np.concatenate(M_col), np.concatenate(M_J), np.concatenate(M_W)
M_nrm /= np.maximum(np.linalg.norm(M_nrm, axis=1), 1e-9)[:, None]

jct = js["accessors"][p["attributes"]["JOINTS_0"]]["componentType"]
mouth = {"attributes": {
    "POSITION": add_acc(M_pos, 5126, "VEC3", 34962, True),
    "NORMAL": add_acc(M_nrm, 5126, "VEC3", 34962),
    "COLOR_0": add_acc(M_col, 5126, "VEC4", 34962),
    "JOINTS_0": add_acc(M_J, jct, "VEC4", 34962),
    "WEIGHTS_0": add_acc(M_W, 5126, "VEC4", 34962),
}, "indices": add_acc(M_idx.reshape(-1), 5125, "SCALAR", 34963), "material": mi}
mesh["primitives"].append(mouth)
deltas.append(M_D)

# ---------------------------------------------------------------- targets on every primitive
for prim, D in zip(mesh["primitives"], deltas):
    prim["targets"] = [{"POSITION": add_sparse(to_glb(D[t], True) if prim is not mouth else D[t])} for t in range(NT)]
mesh["weights"] = [0.0] * NT
ex = mesh.setdefault("extras", {})
ex.pop("jaw_open_full", None)
ex["targetNames"] = [n for n, _ in TARGETS]

# ---------------------------------------------------------------- compact: drop orphaned accessors / views
used_acc = set()
for m in js["meshes"]:
    for pr in m["primitives"]:
        used_acc.update(pr["attributes"].values())
        if "indices" in pr:
            used_acc.add(pr["indices"])
        for t in pr.get("targets", []):
            used_acc.update(t.values())
for s in js.get("skins", []):
    if "inverseBindMatrices" in s:
        used_acc.add(s["inverseBindMatrices"])
for an in js.get("animations", []):
    for sm in an["samplers"]:
        used_acc.update((sm["input"], sm["output"]))
amap = {old: new for new, old in enumerate(sorted(used_acc))}
js["accessors"] = [js["accessors"][i] for i in sorted(used_acc)]
for m in js["meshes"]:
    for pr in m["primitives"]:
        pr["attributes"] = {k: amap[v] for k, v in pr["attributes"].items()}
        if "indices" in pr:
            pr["indices"] = amap[pr["indices"]]
        pr["targets"] = [{k: amap[v] for k, v in t.items()} for t in pr.get("targets", [])]
        if not pr["targets"]:
            del pr["targets"]
for s in js.get("skins", []):
    if "inverseBindMatrices" in s:
        s["inverseBindMatrices"] = amap[s["inverseBindMatrices"]]
for an in js.get("animations", []):
    for sm in an["samplers"]:
        sm["input"], sm["output"] = amap[sm["input"]], amap[sm["output"]]
used_bv = {a["bufferView"] for a in js["accessors"] if "bufferView" in a} | {im["bufferView"] for im in js.get("images", []) if "bufferView" in im}
for a in js["accessors"]:
    if "sparse" in a:
        used_bv |= {a["sparse"]["indices"]["bufferView"], a["sparse"]["values"]["bufferView"]}
vmap, nb = {}, bytearray()
views = []
for i in sorted(used_bv):
    bv = dict(js["bufferViews"][i])
    while len(nb) % 4:
        nb.append(0)
    s0 = bv.get("byteOffset", 0)
    nb.extend(bin_[s0:s0 + bv["byteLength"]])
    bv["byteOffset"] = len(nb) - bv["byteLength"]
    vmap[i] = len(views)
    views.append(bv)
js["bufferViews"] = views
for a in js["accessors"]:
    if "bufferView" in a:
        a["bufferView"] = vmap[a["bufferView"]]
    if "sparse" in a:
        for k in ("indices", "values"):
            a["sparse"][k]["bufferView"] = vmap[a["sparse"][k]["bufferView"]]
for im in js.get("images", []):
    if "bufferView" in im:
        im["bufferView"] = vmap[im["bufferView"]]
bin_ = nb
js["buffers"][0]["byteLength"] = len(bin_)

jb = json.dumps(js, separators=(",", ":")).encode()
jb += b" " * (-len(jb) % 4)
while len(bin_) % 4:
    bin_.append(0)
out = bytearray(b"glTF") + struct.pack("<II", 2, 12 + 8 + len(jb) + 8 + len(bin_))
out += struct.pack("<II", len(jb), 0x4E4F534A) + jb + struct.pack("<II", len(bin_), 0x004E4942) + bin_
open(GLB, "wb").write(out)
log(f"DONE {GLB}: {NT} targets, mouth {len(M_pos)} verts / {len(M_idx)} tris, {len(raw)} -> {len(out)} bytes")
