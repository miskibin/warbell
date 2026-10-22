"""Delete the black cornea shell from exported townsfolk GLBs, in place (no Blender needed).

    python -I tools/npc_mpfb/drop_cornea_glb.py assets/npc/npc_guard.glb [more.glb ...]

MakeHuman eyes are two shells: the eyeball and a cornea shell textured with the eye map's transparent area.
The atlas bake is opaque, so that area baked BLACK and the cornea covered the iris: every look stared out of
black sockets (`export_game.py`'s `drop_cornea` option fixes it at export; these looks were exported without).
Here the body primitive's triangles whose albedo (the look's `_albedo.dds` next to the GLB) is near-black and
that sit in the eye band (just above the head joint, at the front of the face) and on the OUTER shell (the
pupil, also dark, is on the eyeball under it) are dropped from its index
list; the vertices stay, so morph targets and skinning are untouched. Idempotent. Run `add_face_morphs.py`
afterwards (or before: it compacts the buffer either way).

Needs numpy + texture2ddecoder (BC7).
"""
import json, os, struct, sys

import numpy as np
import texture2ddecoder as t2d

FMT = {5121: "B", 5123: "H", 5125: "I", 5126: "f"}
NC = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4, "MAT4": 16}
DARK = 0.06  # mean albedo of a cornea triangle (the iris / sclera are far brighter)

for glb in sys.argv[1:]:
    raw = open(glb, "rb").read()
    off, js, bin_ = 12, None, b""
    while off < len(raw):
        ln, typ = struct.unpack_from("<II", raw, off)
        ch = raw[off + 8: off + 8 + ln]
        if typ == 0x4E4F534A:
            js = json.loads(ch)
        else:
            bin_ = bytearray(ch)
        off += 8 + ln

    def acc(i):
        a = js["accessors"][i]
        bv = js["bufferViews"][a["bufferView"]]
        dt = np.dtype("<" + FMT[a["componentType"]])
        n = NC[a["type"]]
        st = bv.get("byteStride", 0) or n * dt.itemsize
        s0 = bv.get("byteOffset", 0) + a.get("byteOffset", 0)
        return np.array([np.frombuffer(bin_, dt, n, s0 + k * st) for k in range(a["count"])])

    dds = glb[:-4] + "_albedo.dds"
    d = open(dds, "rb").read()
    h, w = struct.unpack_from("<II", d, 12)
    o = 148 if d[84:88] == b"DX10" else 128
    px = np.frombuffer(t2d.decode_bc7(d[o:o + w * h], w, h), np.uint8).reshape(h, w, 4)[:, :, :3]
    skin = js["skins"][0]
    names = [js["nodes"][j]["name"] for j in skin["joints"]]
    ibm = acc(skin["inverseBindMatrices"])[names.index("head")].reshape(4, 4).T
    head = np.linalg.inv(ibm)[:3, 3]
    mats = [m.get("name", "") for m in js["materials"]]
    for p in js["meshes"][0]["primitives"]:
        if mats[p.get("material", 0)] != "npc_body":
            continue
        P = acc(p["attributes"]["POSITION"])
        UV = acc(p["attributes"]["TEXCOORD_0"])
        I = acc(p["indices"]).reshape(-1, 3)
        c = UV[I].mean(1)
        lum = px[np.clip((c[:, 1] * h).astype(int), 0, h - 1), np.clip((c[:, 0] * w).astype(int), 0, w - 1)].mean(1) / 255
        cen = P[I].mean(1) - head
        eye_band = (cen[:, 1] > -0.01) & (cen[:, 1] < 0.09) & (np.abs(cen[:, 0]) < 0.07) & (cen[:, 2] > 0.03)
        dark = (lum < DARK) & eye_band
        # Per eye, the dark set is the cornea cap plus the pupil on the eyeball under it: fit a sphere to
        # it, and only the outer shell goes (the pupil sits ~2.5 mm further in on the guard).
        drop = np.zeros(len(I), bool)
        for side in (1.0, -1.0):
            mine = dark & (np.sign(cen[:, 0]) == side)
            if mine.sum() < 300:
                continue  # no cornea left (exported with drop_cornea, or already run): only the pupil
            V = P[np.unique(I[mine])]
            sol = np.linalg.lstsq(np.c_[2 * V, np.ones(len(V))], (V ** 2).sum(1), rcond=None)[0]
            r = np.linalg.norm(P[I].mean(1) - sol[:3], axis=1)
            cut = (np.percentile(r[mine], 10) + np.percentile(r[mine], 50)) / 2
            drop |= mine & (r > cut)
        keep = I[~drop].reshape(-1).astype("<u4")
        while len(bin_) % 4:
            bin_.append(0)
        s0 = len(bin_)
        bin_ += keep.tobytes()
        js["bufferViews"].append({"buffer": 0, "byteOffset": s0, "byteLength": keep.nbytes, "target": 34963})
        js["accessors"].append({"bufferView": len(js["bufferViews"]) - 1, "componentType": 5125, "count": len(keep), "type": "SCALAR"})
        p["indices"] = len(js["accessors"]) - 1
        print(f"{os.path.basename(glb)}: dropped {int(drop.sum())} cornea triangles of {len(I)}")
    js["buffers"][0]["byteLength"] = len(bin_)
    jb = json.dumps(js, separators=(",", ":")).encode()
    jb += b" " * (-len(jb) % 4)
    while len(bin_) % 4:
        bin_.append(0)
    out = bytearray(b"glTF") + struct.pack("<II", 2, 12 + 8 + len(jb) + 8 + len(bin_))
    out += struct.pack("<II", len(jb), 0x4E4F534A) + jb + struct.pack("<II", len(bin_), 0x004E4942) + bin_
    open(glb, "wb").write(out)
