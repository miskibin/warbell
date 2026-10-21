"""Bake the Mixamo walk / run / orc / dive-roll clips into src/player/anim/mixamo_clips.rs.

    python -P tools/mixamo_retarget/bake.py <dir with Walking.fbx "Knight Running.fbx" "Orc Walk.fbx" "Mutant Run.fbx" "Dive Roll.fbx"> \
        src/player/anim/mixamo_clips.rs

Frame layout: see tools/ual_retarget/bake.py (53 floats: pelvis, chest, head, arms, boots, in leg
lengths / world deltas from the Mixamo T-pose, mirrored onto our body frame).

- WALK: one in-place loop, rotated so frame 0 is the LEFT heel strike (the game keys footstep
  sfx / dust on the gait phase crossing 0 and 0.5), plus the speed the stance boot sweeps back at
  (leg lengths / s) — the player plays the loop at ground speed / that, so the boots never skate.
- RUN: Mixamo's "Fast Run" (default settings) downloaded on the knight's own Mixamo auto-rig (`Knight Fast Run.fbx`), kept
  exactly as Mixamo plays it: `in_place` takes the forward travel off, nothing else is touched
  ([`sliders`] / `symmetric` / `even_stance` stay for clips cut from other characters).
- ORC_WALK / ORC_RUN: Mixamo's "Orc Walk" (male, a heavy rolling waddle) and "Mutant Run" for the
  orks' patrol and charge — same treatment as the hero's run (one symmetric, evenly retimed loop from
  the left heel strike). The ork animator (`player/anim/ork.rs`) takes the legs, pelvis and trunk
  from them; its own arms keep the club and shield.
- ROLL: the dive-roll's body shape in the pelvis's own frame (the pelvis turn + travel removed): the
  game's root does the somersault, the clip supplies the folding limbs.

Needs numpy + ufbx (pip install ufbx). NB iterating ufbx scene lists twice crashes: see mx.py.
"""
import os
import sys

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import mx

src_dir, out_path = sys.argv[1], sys.argv[2]



def load(name):
    c = mx.Clip(os.path.join(src_dir, name + '.fbx'))
    b = mx.Baker(c)
    ts, P = c.sample()
    return b, b.frames(ts, P)


def qrot(q, v):
    u = q[..., :3]
    w = q[..., 3:4]
    t = 2 * np.cross(u, v)
    return v + w * t + np.cross(u, t)


def stance_runs(y, thr):
    """Cyclic contiguous runs of frames below `thr`: [(start, length)]."""
    n = len(y)
    low = y < thr
    runs = []
    for i in range(n):
        if low[i] and not low[i - 1]:
            L = 0
            while low[(i + L) % n] and L < n:
                L += 1
            runs.append((i, L))
    return runs


def cycle(F):
    """Drop the duplicated end frame, rotate to the left heel strike; -> frames, native speed (legs/s),
    duty factor (the fraction of the cycle a boot is planted)."""
    F = F[:-1]
    n = len(F)
    yl, yr = F[:, 34], F[:, 44]
    thr = min(yl.min(), yr.min()) + 0.035
    s0, ln = max(stance_runs(yl, thr), key=lambda r: r[1])
    F = np.roll(F, -s0, axis=0)
    sp = []
    for col in (35, 45):
        y = F[:, col - 1]
        runs = stance_runs(y, thr)
        s, L = max(runs, key=lambda r: r[1])
        idx = (s + np.arange(L)) % n
        t = np.arange(L) / mx.FPS
        sp.append(-np.polyfit(t, F[idx, col], 1)[0])
    print(f'  cycle {n} frames, left stance {ln}, stance sweep speed L/R {sp[0]:.2f}/{sp[1]:.2f} legs/s')
    return F, float(np.mean(sp)), ln / n


def in_place(F):
    """A clip with root travel (Mixamo's "In Place" unticked) -> in place: the pelvis's straight-line
    forward drift taken off the pelvis and both ankles (its bob about that line kept)."""
    F = F.copy()
    t = np.arange(len(F))
    k, c = np.polyfit(t, F[:, 2], 1)
    for col in (2, 35, 45):
        F[:, col] -= k * t
    return F


def stride_cut(F):
    """A clip holding several strides -> one: left heel strike .. the next (the middle stride, so
    neither end is the clip's own start / stop), with the closing frame kept for `cycle` to drop."""
    yl = F[:, 34]
    thr = min(yl.min(), F[:, 44].min()) + 0.035
    starts = sorted(s for s, _ in stance_runs(yl, thr))
    if len(starts) < 2:
        return F
    i = max(0, len(starts) // 2 - 1)
    a, b = starts[i], starts[i + 1]
    return F[a:b + 1]


def mirror_rows(F):
    """Left <-> right: swap the limb slots and mirror across X (positions / directions x -> -x,
    rotations (x, y, z, w) -> (x, -y, -z, w))."""
    M = F.copy()
    q = np.array([1, -1, -1, 1.0])
    v = np.array([-1.0, 1.0, 1.0])
    M[:, 0:3] = F[:, 0:3] * v
    for s in (3, 7, 11):
        M[:, s:s + 4] = F[:, s:s + 4] * q
    for a, b in ((15, 27), (18, 30), (33, 43), (36, 46)):  # right <-> left (dirs / ankles / poles)
        M[:, a:a + 3] = F[:, b:b + 3] * v
        M[:, b:b + 3] = F[:, a:a + 3] * v
    M[:, 21:24] = F[:, 21:24] * v
    M[:, 24:27] = F[:, 24:27] * v
    M[:, 39:43] = F[:, 49:53] * q
    M[:, 49:53] = F[:, 39:43] * q
    return M


def symmetric(F):
    """A loop whose second step is the mirror of its first (the clip's two legs differ a little, which
    would show as skating on the weaker side): left strike .. right strike, then that mirrored."""
    h = len(F) // 2
    return np.concatenate([F[:h], mirror_rows(F[:h])])


def even_stance(F, ln):
    """Retime the left stance (frames 0..ln) so the planted ankle sweeps back at a constant speed: the
    clip's boot brakes after the strike and surges before toe-off, which the game (one playback rate
    for the whole stance) would show as the boot creeping on the ground. Every column is resampled
    at the times where the ankle reaches evenly spaced points of its sweep."""
    F = F.copy()
    z = np.minimum.accumulate(F[:ln + 1, 35])  # monotonic (backward) sweep
    if z[0] - z[-1] < 1e-4:
        return F
    want = np.linspace(z[0], z[-1], ln + 1)
    t = np.interp(-want, -z, np.arange(ln + 1))
    i0 = np.clip(np.floor(t).astype(int), 0, ln - 1)
    a = (t - i0)[:, None]
    seg = F[i0] * (1 - a) + F[i0 + 1] * a
    for q in (3, 7, 11, 39, 49):  # quaternions: renormalise the lerp
        seg[:, q:q + 4] /= np.linalg.norm(seg[:, q:q + 4], axis=1, keepdims=True)
    for d in (15, 18, 21, 24, 27, 30, 36, 46):  # directions
        seg[:, d:d + 3] /= np.linalg.norm(seg[:, d:d + 3], axis=1, keepdims=True)
    F[:ln + 1] = seg
    return F


def rot_z(v, a):
    c, s = np.cos(a)[:, None], np.sin(a)[:, None]
    x, y = v[:, 0:1], v[:, 1:2]
    return np.concatenate([x * c - y * s, x * s + y * c, v[:, 2:3]], 1)


def rot_x(v, a):
    c, s = np.cos(a)[:, None], np.sin(a)[:, None]
    y, z = v[:, 1:2], v[:, 2:3]
    return np.concatenate([v[:, 0:1], y * c - z * s, y * s + z * c], 1)


def sliders(F, T, stride=1.0, lift=1.0, arm_space=0.0, arm_min=6.0, arm_swing=1.0):
    """Mixamo-page style edits on a loop (frame 0 = left strike):
    - stride: the boots' fore-aft travel (and the pelvis's) about their mean, x this;
    - lift:   each boot's height above its contact height, x this (the knee follows through the IK);
    - arm_space: degrees each upper arm is swung in toward the body about the forward axis (the
      forearm turns with it, so the elbow keeps its bend), never closer than `arm_min` of spread;
    - arm_swing: the upper arms' fore-aft pump (pitch off hanging) x this, forearms turning along."""
    F = F.copy()
    for zc in (35, 45):
        m = F[:, zc].mean()
        F[:, zc] = m + (F[:, zc] - m) * stride
    F[:, 2] = F[:, 2].mean() + (F[:, 2] - F[:, 2].mean()) * stride
    for yc in (34, 44):
        c = F[:, yc].min()
        F[:, yc] = c + (F[:, yc] - c) * lift
    if arm_space:
        for u, f in ((15, 18), (27, 30)):
            out = np.sign(T[u])  # the T-pose arm points straight out: its x sign is "outward"
            abd = np.degrees(np.arctan2(out * F[:, u], -F[:, u + 1]))
            turn = np.radians(np.clip(np.minimum(arm_space, abd - arm_min), 0.0, None))
            # A +Z turn swings a hanging arm toward +x: inward is the sign opposite to `out`.
            a = -out * turn
            F[:, u:u + 3] = rot_z(F[:, u:u + 3], a)
            F[:, f:f + 3] = rot_z(F[:, f:f + 3], a)
    if arm_swing != 1.0:
        for u, f in ((15, 18), (27, 30)):
            pitch = np.arctan2(F[:, u + 2], -F[:, u + 1])
            a = -(arm_swing - 1.0) * pitch  # a +X turn lowers the pitch
            F[:, u:u + 3] = rot_x(F[:, u:u + 3], a)
            F[:, f:f + 3] = rot_x(F[:, f:f + 3], a)
    return F


def air(F, lo, hi):
    """Frames lo..=hi with the clip's own pelvis rise taken off (pelvis + both ankles)."""
    F = F[lo:hi + 1].copy()
    rise = F[:, 1] - F[0, 1]
    for c in (1, 34, 44):
        F[:, c] -= rise
    return F


def body_frame(F, tp_pelvis_y):
    """Express every frame in its own pelvis frame (pelvis upright at the T-pose height)."""
    F = F.copy()
    out = np.zeros_like(F)
    for i, r in enumerate(F):
        qp = r[3:7]
        qi = mx.qinv(qp)
        pel = r[0:3]
        out[i, 0:3] = np.array([0.0, tp_pelvis_y, 0.0])
        out[i, 3:7] = [0, 0, 0, 1]
        for s in (7, 11, 39, 49):  # chest, head, boots: world delta -> delta in the pelvis frame
            out[i, s:s + 4] = mx.qmul(qi, r[s:s + 4])
        for s in (15, 18, 21, 24, 27, 30, 36, 46):  # directions
            out[i, s:s + 3] = qrot(qi, r[s:s + 3])
        for s in (33, 43):  # ankles, relative to the pelvis
            out[i, s:s + 3] = qrot(qi, r[s:s + 3] - pel) + np.array([0.0, tp_pelvis_y, 0.0])
    return out


def fmt(F):
    return '\n'.join('    [' + ', '.join(f'{v:.4f}' for v in row) + '],' for row in F)


lines = [
    "//! Generated by tools/mixamo_retarget/bake.py from Mixamo's Walking / Running / Orc Walk / Mutant Run /",
    "//! Dive Roll",
    "//! (X Bot, in place, 30 fps; free for use in games under Adobe's Mixamo terms). Layout: see",
    "//! `swordplay::Frame`. Do not edit by hand.",
    '',
    '#![allow(clippy::excessive_precision)]',
    '',
    'pub(super) const FPS: f32 = 30.0;',
]

b, W = load('Walking')
lines.append('/// The Mixamo T-pose (the reference the clips are measured from).')
lines.append('pub(super) const TPOSE: [f32; 53] = [' + ', '.join(f'{v:.4f}' for v in b.tpose_frame()) + '];')
lines.append('')
print('Walking')
Wc, wsp, wd = cycle(W)
lines += [f'/// One walk loop ({len(Wc)} frames), frame 0 = left heel strike.',
          f'pub(super) const WALK_SPEED: f32 = {wsp:.4f}; // legs / s',
          f'pub(super) const WALK_DUTY: f32 = {wd:.4f};',
          f'pub(super) const WALK: [[f32; 53]; {len(Wc)}] = [', fmt(Wc), '];', '']
b, R = load('Knight Fast Run')
print('Running')
Rc, rsp, rd = cycle(in_place(R))
# Downloaded on the knight's own rig, so it is kept exactly as Mixamo plays it: no sliders, no
# mirroring, no retiming — only the travel taken out (the game moves the root).
lines += [f'/// One run loop ({len(Rc)} frames), frame 0 = left heel strike: Mixamo "Fast Run" on the',
          '/// knight auto-rig, untouched.',
          f'pub(super) const RUN_SPEED: f32 = {rsp:.4f}; // legs / s',
          f'pub(super) const RUN_DUTY: f32 = {rd:.4f};',
          f'pub(super) const RUN: [[f32; 53]; {len(Rc)}] = [', fmt(Rc), '];', '']
for name, tag in (('Orc Walk', 'ORC_WALK'), ('Mutant Run', 'ORC_RUN')):
    b, O = load(name)
    print(name)
    Oc, osp, od = cycle(in_place(O))
    Oc = symmetric(even_stance(Oc, round(od * len(Oc))))
    lines += [f'/// Mixamo "{name}" for the orks: one loop ({len(Oc)} frames), frame 0 = left heel strike.',
              f'pub(super) const {tag}_SPEED: f32 = {osp:.4f}; // legs / s',
              f'pub(super) const {tag}_DUTY: f32 = {od:.4f};',
              f'pub(super) const {tag}: [[f32; 53]; {len(Oc)}] = [', fmt(Oc), '];', '']
b, D = load('Dive Roll')
DB = body_frame(D, b.tp_pelvis_y)[33:73]
lines += [f'/// The dive-roll\'s body shape ({len(DB)} frames: the dive, the tuck, the landing), in the pelvis frame.',
          f'pub(super) const ROLL: [[f32; 53]; {len(DB)}] = [', fmt(DB), '];', '']
open(out_path, 'w', encoding='utf-8', newline='\n').write('\n'.join(lines))
print('wrote', out_path)
