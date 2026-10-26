"""Mixamo FBX sampler (ufbx) -> per-frame 53-float body frames in the layout of
tools/ual_retarget/bake.py (consumed by src/player/anim/mixamo.rs).

Needs: numpy, ufbx (pip). Run with `python -P` so the cwd is not put on sys.path.
"""
import numpy as np
import ufbx

FPS = 30.0
M = np.array([-1.0, 1.0, 1.0])  # our body frame mirrors X (see ual_retarget/bake.py)

BONES = {
    'pelvis': 'Hips', 'spine_03': 'Spine2', 'Head': 'Head',
    'thigh_l': 'LeftUpLeg', 'calf_l': 'LeftLeg', 'foot_l': 'LeftFoot', 'toe_l': 'LeftToeBase',
    'thigh_r': 'RightUpLeg', 'calf_r': 'RightLeg', 'foot_r': 'RightFoot', 'toe_r': 'RightToeBase',
    'upperarm_l': 'LeftArm', 'lowerarm_l': 'LeftForeArm', 'hand_l': 'LeftHand',
    'upperarm_r': 'RightArm', 'lowerarm_r': 'RightForeArm', 'hand_r': 'RightHand',
    'index_r': 'RightHandIndex1', 'pinky_r': 'RightHandPinky1', 'middle_r': 'RightHandMiddle1',
}
# A character auto-rigged on the Mixamo page can come back with fewer finger chains (the knight's
# gauntlets got only the index): without them the hand frame is the hand bone's own turn from bind.
FINGERS = ('index_r', 'pinky_r', 'middle_r')
# The right hand's bind-pose (index - pinky, middle - wrist) in the file frame (Y-up, facing +Z,
# right arm along -X, palm down) — checked against clips that do carry the fingers.
BIND_BLADE, BIND_EDGE = np.array([0.0, 0.0, 1.0]), np.array([-1.0, 0.0, 0.0])


def qmul(a, b):
    w1, x1, y1, z1 = a[..., 3], a[..., 0], a[..., 1], a[..., 2]
    w2, x2, y2, z2 = b[..., 3], b[..., 0], b[..., 1], b[..., 2]
    return np.stack([w1 * x2 + x1 * w2 + y1 * z2 - z1 * y2,
                     w1 * y2 - x1 * z2 + y1 * w2 + z1 * x2,
                     w1 * z2 + x1 * y2 - y1 * x2 + z1 * w2,
                     w1 * w2 - x1 * x2 - y1 * y2 - z1 * z2], -1)


def qinv(q):
    return q * np.array([-1, -1, -1, 1.0])


def mq(q):  # mirror a world rotation across X
    return q * np.array([1, -1, -1, 1.0])


def qrot(q, v):
    u, w = q[..., :3], q[..., 3:]
    t = 2 * np.cross(u, v)
    return v + w * t + np.cross(u, t)


def unit(v):
    return v / np.linalg.norm(v, axis=-1, keepdims=True)


def mat_to_quat(R):
    """Rotation matrix (3x3, columns = basis) -> (x, y, z, w)."""
    t = np.trace(R)
    if t > 0:
        s = np.sqrt(t + 1) * 2
        return np.array([(R[2, 1] - R[1, 2]) / s, (R[0, 2] - R[2, 0]) / s, (R[1, 0] - R[0, 1]) / s, s / 4])
    i = int(np.argmax(np.diag(R)))
    j, k = (i + 1) % 3, (i + 2) % 3
    s = np.sqrt(1 + R[i, i] - R[j, j] - R[k, k]) * 2
    q = np.zeros(4)
    q[i] = s / 4
    q[3] = (R[k, j] - R[j, k]) / s
    q[j] = (R[j, i] + R[i, j]) / s
    q[k] = (R[k, i] + R[i, k]) / s
    return q


def world(node):
    m = node.node_to_world
    A = np.array([[m.c0.x, m.c1.x, m.c2.x, m.c3.x], [m.c0.y, m.c1.y, m.c2.y, m.c3.y], [m.c0.z, m.c1.z, m.c2.z, m.c3.z]])
    R = A[:, :3] / np.linalg.norm(A[:, :3], axis=0)
    q = mat_to_quat(R)
    return A[:, 3], q / np.linalg.norm(q)


class Clip:
    def __init__(self, path):
        self.scene = ufbx.load_file(path)
        self.stacks = {}
        for st in self.scene.anim_stacks:
            self.stacks[st.name] = st
        nodes = list(self.scene.nodes)  # NB: iterating scene.nodes a second time crashes ufbx-python
        self.nodes = {n.name.replace('mixamorig:', ''): n for n in nodes}
        self.bones = {k: v for k, v in BONES.items() if v in self.nodes or k not in FINGERS}
        if len(self.bones) < len(BONES):
            self.bones = {k: v for k, v in self.bones.items() if k not in FINGERS}
        # The T-pose (bind) reference = the unevaluated default pose.
        self.tpose = {k: world(self.nodes[v]) for k, v in self.bones.items()}
        # Mixamo names its take 'mixamo.com'; a pose exported from Blender (Riding.fbx) has one take of its own name
        self.stack = self.stacks.get('mixamo.com') or next(iter(self.stacks.values()))

    def sample(self, fps=FPS):
        """-> times, {bone: (pos (n,3) cm, quat (n,4) xyzw)} in the file's own (Y-up, +Z-forward) frame."""
        dur = self.stack.time_end - self.stack.time_begin
        n = int(round(dur * fps)) + 1
        ts = np.arange(n) / fps
        out = {k: ([], []) for k in self.bones}
        for t in ts:
            ev = ufbx.evaluate_scene(self.scene, self.stack.anim, float(t))
            en = {x.name.replace('mixamorig:', ''): x for x in list(ev.nodes)}
            for k, v in self.bones.items():
                p, q = world(en[v])
                out[k][0].append(p)
                out[k][1].append(q)
        return ts, {k: (np.array(a), np.array(b)) for k, (a, b) in out.items()}


# Bones whose rotations a clip cut on the knight's own Mixamo auto-rig hands straight to the game's
# joints (`mixamo::direct`): Hips, Torso, Head, the legs (thigh, shin, boot) and the arms (upper,
# fore), left then right.
DIRECT = ('pelvis', 'spine_03', 'Head', 'thigh_l', 'calf_l', 'foot_l', 'thigh_r', 'calf_r', 'foot_r',
          'upperarm_l', 'lowerarm_l', 'upperarm_r', 'lowerarm_r')


class Baker:
    """Bake one Mixamo clip into the 53-float frame layout (see ual_retarget/bake.py)."""

    def __init__(self, clip):
        self.c = clip
        T = clip.tpose
        self.ref = {n: T[n][1] for n in ('pelvis', 'spine_03', 'Head', 'foot_l', 'foot_r', 'hand_r')}
        self.legL = np.linalg.norm(T['calf_l'][0] - T['thigh_l'][0]) + np.linalg.norm(T['foot_l'][0] - T['calf_l'][0])
        self.tp_pelvis_y = T['pelvis'][0][1] / self.legL

    def frames(self, ts, P):
        legL, ref = self.legL, self.ref
        pos = lambda n: P[n][0] * M / legL
        rot = lambda n: mq(qmul(P[n][1], qinv(np.broadcast_to(ref[n], P[n][1].shape))))
        dirn = lambda a, b: unit((P[b][0] - P[a][0]) * M)

        def pole(sd):
            h, k, a = P['thigh_' + sd][0], P['calf_' + sd][0], P['foot_' + sd][0]
            d = unit(a - h)
            v = k - h
            return unit((v - np.sum(v * d, -1, keepdims=True) * d) * M)
        if 'pinky_r' in P:
            blade = unit((P['index_r'][0] - P['pinky_r'][0]) * M)
            edge = (P['middle_r'][0] - P['hand_r'][0]) * M
        else:
            turn = qmul(P['hand_r'][1], qinv(np.broadcast_to(ref['hand_r'], P['hand_r'][1].shape)))
            blade = unit(qrot(turn, np.broadcast_to(BIND_BLADE, turn[..., :3].shape)) * M)
            edge = qrot(turn, np.broadcast_to(BIND_EDGE, turn[..., :3].shape)) * M
        edge = unit(edge - np.sum(edge * blade, -1, keepdims=True) * blade)
        cols = [pos('pelvis'), rot('pelvis'), rot('spine_03'), rot('Head'),
                dirn('upperarm_r', 'lowerarm_r'), dirn('lowerarm_r', 'hand_r'), blade, edge,
                dirn('upperarm_l', 'lowerarm_l'), dirn('lowerarm_l', 'hand_l'),
                pos('foot_l'), pole('l'), rot('foot_l'),
                pos('foot_r'), pole('r'), rot('foot_r')]
        F = np.concatenate(cols, -1)
        assert F.shape[1] == 53
        return F

    def direct(self, P):
        """Each DIRECT bone's world turn from its bind pose (x, y, z, w; mirrored into our body
        frame), 4 floats a bone: on a rig whose bind pose IS the knight's T-pose these are exactly the
        turns to put on the matching joints."""
        T = self.c.tpose
        cols = [mq(qmul(P[n][1], qinv(np.broadcast_to(T[n][1], P[n][1].shape)))) for n in DIRECT]
        return np.concatenate(cols, -1)

    def tpose_frame(self):
        T = self.c.tpose
        P = {k: (v[0][None], v[1][None]) for k, v in T.items()}
        return self.frames(np.zeros(1), P)[0]
