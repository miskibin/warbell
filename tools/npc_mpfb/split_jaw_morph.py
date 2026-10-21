"""Make the baked `jaw_open` morph target open the mouth, in place (no Blender needed).

    python tools/npc_mpfb/split_jaw_morph.py assets/npc/npc_guard.glb [assets/npc/npc_archer.glb ...]

`add_mouth_morph.py` weights the whole lower face to the jaw, and its smoothing passes leave the UPPER lip at
~0.85 of the lower lip's weight, so the morph swings both lips down together and a closed mouth only stretches.
This keeps the deltas below the lip slit and fades them out through it, so the upper lip stays put and the lower
lip + chin drop: the lips part.

The slit height is read off the head joint (the skins share proportions): `SLIT_BELOW_HEAD` below it. Found
by sampling the albedo along the midline: the lower lip's lining is red up to ~1.687 and the upper lip's red
starts ~1.703 on the guard (head joint 1.748); the archer's pair sits the same distance under its own head joint.
Idempotent: the original deltas are kept in the `extras.jaw_open_full` list on the first run.
"""
import json
import struct
import sys

import numpy as np

SLIT_BELOW_HEAD = 0.034
FADE_BELOW = 0.007  # full weight this far under the slit
FADE_ABOVE = 0.004  # zero this far over it


def smooth(x):
    x = np.clip(x, 0.0, 1.0)
    return x * x * (3.0 - 2.0 * x)


def patch(path):
    b = open(path, "rb").read()
    jl = struct.unpack("<I", b[12:16])[0]
    j = json.loads(b[20:20 + jl])
    bl = struct.unpack("<I", b[20 + jl:24 + jl])[0]
    off0 = 28 + jl
    binc = bytearray(b[off0:off0 + bl])
    tail = b[off0 + bl:]

    def view(i, dtype, comp):
        a = j["accessors"][i]
        bv = j["bufferViews"][a["bufferView"]]
        o = bv.get("byteOffset", 0) + a.get("byteOffset", 0)
        return np.frombuffer(binc, dtype=dtype, count=a["count"] * comp, offset=o).reshape(a["count"], comp)

    sk = j["skins"][0]
    names = [j["nodes"][n].get("name") for n in sk["joints"]]
    ib = view(sk["inverseBindMatrices"], np.float32, 16).reshape(-1, 4, 4)
    head_y = float(-ib[names.index("head")][3, 1])
    slit = head_y - SLIT_BELOW_HEAD
    done = 0
    for m in j["meshes"]:
        for pr in m["primitives"]:
            if not pr.get("targets"):
                continue
            pos = view(pr["attributes"]["POSITION"], np.float32, 3)
            acc_i = pr["targets"][0]["POSITION"]
            d = view(acc_i, np.float32, 3)
            if np.abs(d).max() < 1e-4:
                continue
            full = pr.setdefault("extras", {}).setdefault("jaw_open_full", d.reshape(-1).tolist())
            d[:] = np.asarray(full, np.float32).reshape(-1, 3)
            f = smooth((slit + FADE_ABOVE - pos[:, 1]) / (FADE_ABOVE + FADE_BELOW)).astype(np.float32)
            d *= f[:, None]
            a = j["accessors"][acc_i]
            a["min"] = d.min(0).tolist()
            a["max"] = d.max(0).tolist()
            done += 1
    js = json.dumps(j, separators=(",", ":")).encode()
    js += b" " * ((4 - len(js) % 4) % 4)
    out = b"glTF" + struct.pack("<II", 2, 12 + 8 + len(js) + 8 + len(binc) + len(tail)) + struct.pack("<I", len(js)) + b"JSON" + js
    out += struct.pack("<I", len(binc)) + b"BIN\x00" + bytes(binc) + tail
    open(path, "wb").write(out)
    print(f"{path}: head joint y {head_y:.3f}, slit {slit:.3f}, {done} morph target(s) split")


if __name__ == "__main__":
    for p in sys.argv[1:]:
        patch(p)
