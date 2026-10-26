"""Horse-riding look-dev rig (Blender) — and the source of the hero's riding seat.

    blender -b --python ride_lookdev.py -- <Horse.fbx> "<mixamo_knight>/Sword And Shield Idle.fbx" \
        <horseman_mitro model.glb> <out dir> rear|side|both [still frames,comma]

Builds Henry S's horse (FBX) on an NLA timeline (idle -> walk -> gallop -> turn -> stop) with root motion
from the measured hoof slide, seats the knight's Mixamo rig on it (base pose aim-retargeted from Mitro123's
CC-BY "Horseman battle Animations" rider, hands IK'd to the reins on the neck, feet to the stirrup irons),
runs the procedural seat (spring hips, torso lean on speed / accel, steadied head) and films a chase cam.
`POSE_OUT=<Riding.fbx>` instead exports the seated knight at native scale at `POSE_FRAME` (default 45) for
`tools/mixamo_retarget/bake.py` (the `RIDE` clip). Tunables via env: see the `E(...)` calls.
"""
import bpy, sys, os, re, math, random
from mathutils import Matrix, Vector, Quaternion, Euler
a = sys.argv[sys.argv.index('--')+1:]
horse_p, knight_p, rider_p, outdir, cam_mode = a[:5]
stills = [int(x) for x in a[5].split(',')] if len(a) > 5 else None
E = lambda k, d: float(os.environ.get(k, d))
bpy.ops.wm.read_factory_settings(use_empty=True)
sc = bpy.context.scene; sc.render.fps = 30
upd = bpy.context.view_layer.update
UP = Vector((0, 0, 1))

def new_objs(fn):
    before = set(bpy.data.objects); fn(); return [o for o in bpy.data.objects if o not in before]
H = new_objs(lambda: bpy.ops.import_scene.fbx(filepath=horse_p, use_anim=True, ignore_leaf_bones=False))
texdir = os.path.join(os.path.dirname(os.path.dirname(horse_p)), 'textures')
for img in bpy.data.images:
    if img.filepath and not os.path.exists(bpy.path.abspath(img.filepath)):
        c = os.path.join(texdir, os.path.basename(img.filepath.replace(chr(92), '/')))
        if os.path.exists(c): img.filepath = c
harm = [o for o in H if o.type == 'ARMATURE'][0]
for m in bpy.data.materials:
    if m.use_nodes and 'hair' in m.name.lower():
        nt = m.node_tree; bs = [n for n in nt.nodes if n.type == 'BSDF_PRINCIPLED'][0]
        for l in list(nt.links):
            if l.to_node == bs and l.to_socket.name == 'Base Color' and l.from_node.type == 'TEX_IMAGE':
                nt.links.new(l.from_node.outputs['Alpha'], bs.inputs['Alpha'])
K = new_objs(lambda: bpy.ops.import_scene.fbx(filepath=knight_p, use_anim=False))
karm = [o for o in K if o.type == 'ARMATURE'][0]
if karm.animation_data: karm.animation_data.action = None
R = new_objs(lambda: bpy.ops.import_scene.gltf(filepath=rider_p))
rarm = [o for o in R if o.type == 'ARMATURE'][0]
for o in R:
    if o.type == 'MESH': o.hide_render = True; o.hide_viewport = True
for o in list(bpy.data.objects):
    if o.type == 'MESH' and o.name.startswith('Icosphere'): bpy.data.objects.remove(o)
steel = bpy.data.materials.new('Steel'); steel.use_nodes = True
bsd = steel.node_tree.nodes['Principled BSDF']
bsd.inputs['Base Color'].default_value = (0.42, 0.43, 0.45, 1); bsd.inputs['Metallic'].default_value = 0.85; bsd.inputs['Roughness'].default_value = 0.38
for o in K:
    if o.type == 'MESH':
        o.data.materials.clear(); o.data.materials.append(steel)

# ---------------- horse timeline (NLA) + root motion ----------------
P = 'Skeleton|Skeleton|'
FPS = 30
# speeds in world units / frame, measured from hoof slide during stance (clips are in place)
V_WALK, V_GALLOP = 0.395, 1.732
SEGS = [  # (clip, seconds, speed fn(t01) in u/frame, yaw rate rad/frame)
    ('1 Ilde', 2.0, lambda t: 0.0, 0.0),
    ('Walk', 3.5, lambda t: V_WALK, 0.0),
    ('Gallop', 3.0, lambda t: V_GALLOP, 0.0),
    ('Gallop_L', 1.8, lambda t: V_GALLOP, math.radians(38) / FPS),
    ('Gallop', 2.2, lambda t: V_GALLOP, 0.0),
    ('Run_Stop', 48 / FPS, lambda t: V_GALLOP * max(0.0, 1 - t / 0.75) ** 1.6, 0.0),
    ('1 Ilde', 2.0, lambda t: 0.0, 0.0),
]
OV = 10  # crossfade frames
starts = [1]
for s in SEGS: starts.append(starts[-1] + round(s[1] * FPS))
F_END = starts[-1] - 1
harm.animation_data_create(); harm.animation_data.action = None
tracks = [harm.animation_data.nla_tracks.new(), harm.animation_data.nla_tracks.new()]
for i, (clip, secs, _, _) in enumerate(SEGS):
    act = bpy.data.actions[P + clip]
    s0 = starts[i] - (OV // 2 if i > 0 else 0)
    s1 = starts[i + 1] + (OV // 2 if i < len(SEGS) - 1 else 0)
    st = tracks[i % 2].strips.new('%d_%s' % (i, clip), s0, act)
    if hasattr(st, 'action_slot') and act.slots: st.action_slot = act.slots[0]
    alen = act.frame_range[1] - act.frame_range[0]
    st.repeat = max(0.05, (s1 - s0) / alen)
    # only the upper track crossfades; the lower one stays at full weight so nothing blends toward the (10x-scale) FBX rest pose
    upper = (i % 2 == 1)
    st.blend_in = OV if upper else 0
    st.blend_out = OV if (upper and i < len(SEGS) - 1) else 0
    st.extrapolation = 'NOTHING'
def seg_at(f):
    for i in range(len(SEGS)):
        if f < starts[i + 1]: return i
    return len(SEGS) - 1
def motion(f):
    i = seg_at(f); t = (f - starts[i]) / max(1, starts[i + 1] - starts[i])
    v, w = SEGS[i][2](t), SEGS[i][3]
    # crossfade into the next segment's speed over the overlap
    if i + 1 < len(SEGS) and f > starts[i + 1] - OV / 2:
        k = (f - (starts[i + 1] - OV / 2)) / OV
        v = v * (1 - k) + SEGS[i + 1][2](0.0) * k; w = w * (1 - k) + SEGS[i + 1][3] * k
    return v, w
pos = Vector((0, 0, 0)); yaw = 0.0
fwd_of = lambda y: Vector((math.sin(y), -math.cos(y), 0))  # rest forward is -Y; +yaw = turn left
HPOS, HYAW, HSPD = {}, {}, {}
vs = 0.0
for f in range(1, F_END + 1):
    v, w = motion(f); vs += (v - vs) * 0.35
    yaw += w; pos = pos + fwd_of(yaw) * vs
    HPOS[f], HYAW[f], HSPD[f] = pos.copy(), yaw, vs
    harm.location = pos; harm.rotation_euler = (0, 0, yaw)
    harm.keyframe_insert('location', frame=f); harm.keyframe_insert('rotation_euler', frame=f)

# ---------------- rider base pose (aim-retarget from the Mitro horseman) ----------------
strip = lambda n: re.sub(r'_\d+$', '', n)
rmap = {strip(b.name): b.name for b in rarm.data.bones}
rarm.animation_data.action = rarm.animation_data.action  # keep
sc.frame_set(60); upd()
sW = lambda n: rarm.matrix_world @ rarm.pose.bones[rmap[n]].head
tW = lambda n: karm.matrix_world @ karm.pose.bones[n].head
def rotate_bone(pb, q, arm=None):
    arm = arm or karm
    cur = arm.matrix_world @ pb.matrix
    t, r, s_ = cur.decompose()
    pb.matrix = arm.matrix_world.inverted() @ Matrix.LocRotScale(t, q @ r, s_)
    upd()
def basis(up, side):
    up = up.normalized(); f = up.cross(side).normalized(); side = f.cross(up)
    return Matrix((side, f, up)).transposed().to_quaternion()
for pb in karm.pose.bones: pb.rotation_mode = 'QUATERNION'
hips = karm.pose.bones['mixamorig:Hips']
Bs = basis(sW('mixamorig:Spine2') - sW('mixamorig:Hips'), sW('mixamorig:RightUpLeg') - sW('mixamorig:LeftUpLeg'))
Bt = basis(tW('mixamorig:Spine2') - tW('mixamorig:Hips'), tW('mixamorig:RightUpLeg') - tW('mixamorig:LeftUpLeg'))
rotate_bone(hips, Bs @ Bt.inverted())
def child_of(n):
    kids = [c.name for c in karm.pose.bones[n].children if c.name in rmap and not c.name.endswith('_End')]
    pri = [c for c in kids if any(k in c for k in ('Spine', 'Neck', 'Head', 'Arm', 'Leg', 'Foot', 'Toe', 'Middle1'))]
    return (pri or kids or [None])[0]
for pb in karm.pose.bones:
    if pb.name == 'mixamorig:Hips' or pb.name not in rmap: continue
    c = child_of(pb.name)
    if not c: continue
    sd = sW(c) - sW(pb.name); td = tW(c) - tW(pb.name)
    if sd.length < 1e-6 or td.length < 1e-6: continue
    rotate_bone(pb, td.normalized().rotation_difference(sd.normalized()))
BASE = {pb.name: pb.rotation_quaternion.copy() for pb in karm.pose.bones}

# ---------------- scale + seat ----------------
sc.frame_set(1); upd()
saddle = [o for o in H if o.type == 'MESH' and o.name.startswith('Saddle1')]
dg = bpy.context.evaluated_depsgraph_get()
pts = []
for o in saddle:
    e = o.evaluated_get(dg); me = e.to_mesh(); pts += [o.matrix_world @ v.co for v in me.vertices]; e.to_mesh_clear()
cx = sum(p.x for p in pts) / len(pts); cy = sum(p.y for p in pts) / len(pts)
span = max(p.y for p in pts) - min(p.y for p in pts)
near = [p for p in pts if (p.x - cx) ** 2 + (p.y - cy) ** 2 < span ** 2 * 0.02]
seat0 = Vector((cx, cy, max(p.z for p in near)))
seat0 = seat0 + Vector((0, E('SEAT_BACK', 1.3) * seat0.z / 15.0, 0))
ground_z = 0.0
seat_h = seat0.z - ground_z
kh = max((karm.matrix_world @ b.head_local).z for b in karm.data.bones) - min((karm.matrix_world @ b.head_local).z for b in karm.data.bones)
KSCALE0 = karm.scale.copy(); KLOC0 = karm.location.copy()
karm.scale = karm.scale * (10.0 if os.environ.get('POSE_OUT') else seat_h * 1.12 / kh); upd()
SPINE = 'BN_Spine_02_04'
hbm = lambda n: harm.matrix_world @ harm.pose.bones[n].matrix
seat_local = hbm(SPINE).inverted() @ seat0
U = seat_h / 15.0   # ~0.1 m
# knight facing -> horse forward (yaw only, at rest)
kf = tW('mixamorig:LeftToeBase') - tW('mixamorig:LeftFoot'); kf.z = 0; kf.normalize()
KYAW0 = math.atan2(-1, 0) - math.atan2(kf.y, kf.x)   # rotate knight forward onto -Y
Q0 = karm.matrix_world.to_quaternion().copy()
karm.rotation_mode = 'QUATERNION'; karm.rotation_quaternion = Q0

# ---------------- IK: hands on the reins (follow the neck), feet in the stirrups ----------------
def empty(name, parent=None, bone=None):
    e = bpy.data.objects.new(name, None); sc.collection.objects.link(e); e.empty_display_size = 0.5
    if parent:
        e.parent = parent
        if bone: e.parent_type = 'BONE'; e.parent_bone = bone
    return e
sc.frame_set(1); upd()
fwd0, side0 = Vector((0, -1, 0)), Vector((1, 0, 0))   # horse at frame 1: forward -Y; +X = horse's left
reins = {}
for s, sgn in (('Left', 1), ('Right', -1)):
    e = empty('rein_' + s, harm, 'BN_Neck_00_06'); upd()
    e.matrix_world = Matrix.Translation(seat0 + fwd0 * (E('REIN_FWD', 3.8) * U) + UP * (E('REIN_UP', 1.6) * U) + side0 * (sgn * 0.9 * U))
    reins[s] = e
    st = 'BN_L_Stirrup_048' if s == 'Left' else 'BN_R_Stirrup_049'
    f = empty('stir_' + s, harm, st); upd()
    side_pts = [q for q in pts if q.x * sgn > 1.5 * U]
    sp = min(side_pts, key=lambda q: q.z)
    print('RES stirrup', s, tuple(round(x, 1) for x in sp))
    f.matrix_world = Matrix.Translation(sp + UP * (E('STIR_UP', 0.9) * U) + fwd0 * (0.3 * U))
    reins['stir' + s] = f
upd()
for s, sgn in (('Left', 1), ('Right', -1)):
    # which knight side is the horse's left? decide by world position at rest
    pass
# knight's own left/right in world after yaw: compute once the knight is placed (below)
poles = {}
def add_ik(bone, target, pole, pole_ang, chain=2):
    c = karm.pose.bones[bone].constraints.new('IK')
    c.target = target; c.pole_target = pole; c.pole_angle = math.radians(pole_ang); c.chain_count = chain
    return c
for s in ('Left', 'Right'):
    poles['arm' + s] = empty('pole_arm_' + s, karm)
    poles['leg' + s] = empty('pole_leg_' + s, karm)

# ---------------- per-frame rider sim ----------------
def place_rider(hpos, q):
    karm.rotation_quaternion = q; upd()
    hw = tW('mixamorig:Hips'); karm.location = karm.location + (hpos - hw); upd()
SPINE_B = ['mixamorig:Spine', 'mixamorig:Spine1', 'mixamorig:Spine2']
dt = 1.0 / FPS
p = None; v = Vector((0, 0, 0))
th, thv = 0.0, 0.0       # torso pitch spring (rad), + = lean forward
pr = Vector((0, 0)); prv = Vector((0, 0))  # rider pitch/roll follow
prev_hv = None
iks_made = False
KEYS = {}
for f in range(1, F_END + 1):
    sc.frame_set(f); upd()
    M = hbm(SPINE)
    T = M @ seat_local + UP * (E('HIP_UP', 0.9) * U)
    yaw = HYAW[f]; hq = Quaternion(UP, yaw)
    # saddle pitch/roll in the horse frame
    dq = hq.inverted() @ M.to_quaternion() @ hbm.__call__(SPINE).to_quaternion().inverted() if False else None
    sdir = (M.to_quaternion() @ Vector((0, 1, 0)))
    loc_dir = hq.inverted() @ sdir
    sp_pitch = math.atan2(loc_dir.z, (loc_dir.x ** 2 + loc_dir.y ** 2) ** 0.5)
    # hips spring: tight horizontally, softer vertically (the rider's seat absorbs the bounce)
    if p is None: p = T.copy()
    for _ in range(4):
        h = dt / 4
        kxy, kz = E('K_XY', 1600), E('K_Z', 260)
        acc = Vector(((T.x - p.x) * kxy, (T.y - p.y) * kxy, (T.z - p.z) * kz))
        acc -= Vector((v.x * 2 * kxy ** 0.5, v.y * 2 * kxy ** 0.5, v.z * 2 * 0.55 * kz ** 0.5))
        v += acc * h; p += v * h
    p.z = min(max(p.z, T.z - 0.35 * U), T.z + 1.4 * U)
    # rider body pitch/roll partially follows the saddle
    tgt = Vector((sp_pitch * 0.45, 0.0))
    prv += ((tgt - pr) * 120 - prv * 2 * 0.8 * 120 ** 0.5) * dt; pr += prv * dt
    rq = hq @ Quaternion(Vector((1, 0, 0)), -pr.x) @ Quaternion(UP, KYAW0) @ Q0
    # torso: lean with speed, sway back on acceleration / forward when braking, nod with vertical bounce
    hv = HPOS[f] - HPOS[f - 1] if f > 1 else Vector((0, 0, 0))
    fa = (hv.length - (prev_hv.length if prev_hv is not None else hv.length)) / U  # forward accel (0.1 m/frame^2)
    prev_hv = hv
    spd = HSPD[f] / V_GALLOP
    th_t = math.radians(E('LEAN_BASE', -6) + E('LEAN_GALLOP', 14) * spd)
    kt = 90.0
    tha = (th_t - th) * kt - thv * 2 * 0.45 * kt ** 0.5 - fa * E('ACC_SWAY', 2.2) + v.z / U * E('BOUNCE_NOD', 0.05)
    thv += tha * dt; th += thv * dt
    # apply
    for n in SPINE_B + ['mixamorig:Neck', 'mixamorig:Head']:
        karm.pose.bones[n].rotation_quaternion = BASE[n]
    upd()
    place_rider(p, rq)
    if not iks_made:
        # side mapping + IK constraints once the knight is in place
        kl = tW('mixamorig:LeftUpLeg') - tW('mixamorig:RightUpLeg')
        hl = (Quaternion(UP, yaw) @ side0)
        kL_is_hL = kl.dot(hl) > 0
        m = {'Left': 'Left' if kL_is_hL else 'Right', 'Right': 'Right' if kL_is_hL else 'Left'}
        kinv = karm.matrix_world.inverted()
        for s, sgn in (('Left', 1), ('Right', -1)):
            hs = m[s]
            side_w = (tW('mixamorig:%sUpLeg' % s) - tW('mixamorig:Hips')).normalized()
            fwd_w = Quaternion(UP, yaw) @ fwd0
            poles['arm' + s].matrix_world = Matrix.Translation(tW('mixamorig:%sArm' % s) + side_w * 6 * U - fwd_w * 6 * U - UP * 4 * U)
            poles['leg' + s].matrix_world = Matrix.Translation(tW('mixamorig:%sLeg' % s) + fwd_w * 8 * U + side_w * 2 * U)
            add_ik('mixamorig:%sForeArm' % s, reins[hs], poles['arm' + s], E('ARM_POLE_' + s[0], 0 if s == 'Left' else 180))
            add_ik('mixamorig:%sLeg' % s, reins['stir' + hs], poles['leg' + s], E('LEG_POLE_' + s[0], 135 if s == 'Left' else 45))
        iks_made = True; upd()
    # torso pitch about the rider's side axis, spread over the spine
    side_axis = (tW('mixamorig:LeftUpLeg') - tW('mixamorig:RightUpLeg')).normalized()
    for n in SPINE_B:
        rotate_bone(karm.pose.bones[n], Quaternion(side_axis, th / 3))
    # head stabiliser: keep the gaze level-ish whatever the torso does
    hd = karm.pose.bones['mixamorig:Head']
    hdir = (karm.matrix_world @ hd.matrix).to_quaternion() @ Vector((0, 0, 1))  # bone local Z ~ face forward
    fw = Quaternion(UP, yaw) @ fwd0
    want = (fw + UP * math.tan(math.radians(E('GAZE', -6)))).normalized()
    corr = hdir.rotation_difference(want)
    ang = corr.angle
    if ang > math.pi / 2: corr = Quaternion()
    half = Quaternion(corr.axis, corr.angle * 0.5) if corr.angle > 1e-4 else Quaternion()
    rotate_bone(karm.pose.bones['mixamorig:Neck'], half)
    rotate_bone(hd, half)
    if f in (30, 240):
        upd(); r3 = lambda v: tuple(round(x, 1) for x in v)
        print('RES dbg f', f, 'seatT', r3(T), 'hips', r3(tW('mixamorig:Hips')), 'horse', r3(HPOS[f]))
        for s_ in ('Left', 'Right'):
            print('RES dbg', s_, 'upleg', r3(tW('mixamorig:%sUpLeg' % s_)), 'knee', r3(tW('mixamorig:%sLeg' % s_)), 'ankle', r3(tW('mixamorig:%sFoot' % s_)),
                  'stirT', r3(reins['stir' + s_].matrix_world.translation), 'elbow', r3(tW('mixamorig:%sForeArm' % s_)), 'shoulder', r3(tW('mixamorig:%sArm' % s_)), 'hand', r3(tW('mixamorig:%sHand' % s_)), 'reinT', r3(reins[s_].matrix_world.translation),
                  'legpole', r3(poles['leg' + s_].matrix_world.translation))
        print('RES dbg stirrup bone heads', [r3(harm.matrix_world @ harm.pose.bones[b].head) for b in ('BN_L_Stirrup_048', 'BN_L_Stirrup_048_end', 'BN_R_Stirrup_049', 'BN_R_Stirrup_049_end')])
    if os.environ.get('POSE_OUT') and f == int(E('POSE_FRAME', 45)):
        upd()
        cap = {pb.name: pb.matrix.copy() for pb in karm.pose.bones}
        for pb in karm.pose.bones:
            for c in list(pb.constraints): pb.constraints.remove(c)
        karm.animation_data_clear() if karm.animation_data else None
        karm.location = KLOC0; karm.rotation_quaternion = Q0; karm.scale = KSCALE0; upd()
        for pb in karm.pose.bones:  # parents first
            pb.matrix = cap[pb.name]; upd()
        act = bpy.data.actions.new('Riding'); karm.animation_data_create(); karm.animation_data.action = act
        for fr in (1, 30):
            for pb in karm.pose.bones:
                pb.keyframe_insert('rotation_quaternion', frame=fr); pb.keyframe_insert('location', frame=fr)
        sc.frame_start, sc.frame_end = 1, 30
        bpy.ops.object.select_all(action='DESELECT')
        for o in K: o.select_set(True)
        bpy.context.view_layer.objects.active = karm
        bpy.ops.export_scene.fbx(filepath=os.environ['POSE_OUT'], use_selection=True, bake_anim=True, bake_anim_use_all_actions=False,
                                 bake_anim_use_nla_strips=False, add_leaf_bones=False, armature_nodetype='NULL')
        print('RES pose exported', os.environ['POSE_OUT'], 'hips', tW('mixamorig:Hips'))
        sys.exit(0)
    karm.keyframe_insert('location', frame=f); karm.keyframe_insert('rotation_quaternion', frame=f)
    for n in SPINE_B + ['mixamorig:Neck', 'mixamorig:Head']:
        karm.pose.bones[n].keyframe_insert('rotation_quaternion', frame=f)

# ---------------- set dressing ----------------
sc.render.engine = 'BLENDER_EEVEE_NEXT'
sc.render.resolution_x, sc.render.resolution_y = 1280, 720
sc.view_settings.view_transform = 'AgX'
w = bpy.data.worlds.new('W'); sc.world = w; w.use_nodes = True
bg = w.node_tree.nodes['Background']; bg.inputs[0].default_value = (0.58, 0.68, 0.82, 1); bg.inputs[1].default_value = 1.0
sun = bpy.data.objects.new('Sun', bpy.data.lights.new('Sun', 'SUN')); sc.collection.objects.link(sun)
sun.data.energy = 4.0; sun.data.angle = math.radians(2)
sun.rotation_euler = Vector((-0.35, -0.55, -0.75)).to_track_quat('-Z', 'Y').to_euler()
bpy.ops.mesh.primitive_plane_add(size=6000, location=(0, 0, 0))
g = bpy.context.active_object
gm = bpy.data.materials.new('Ground'); gm.use_nodes = True; nt = gm.node_tree
bsdf = nt.nodes['Principled BSDF']; bsdf.inputs['Roughness'].default_value = 1.0
tc = nt.nodes.new('ShaderNodeTexCoord')
n1 = nt.nodes.new('ShaderNodeTexNoise'); n1.inputs['Scale'].default_value = 0.02; n1.inputs['Detail'].default_value = 8
n2 = nt.nodes.new('ShaderNodeTexNoise'); n2.inputs['Scale'].default_value = 0.35; n2.inputs['Detail'].default_value = 4
for n in (n1, n2): nt.links.new(tc.outputs['Object'], n.inputs['Vector'])
mix = nt.nodes.new('ShaderNodeMix'); mix.data_type = 'RGBA'
ramp = nt.nodes.new('ShaderNodeValToRGB')
ramp.color_ramp.elements[0].color = (0.14, 0.19, 0.07, 1); ramp.color_ramp.elements[1].color = (0.30, 0.33, 0.13, 1)
ramp2 = nt.nodes.new('ShaderNodeValToRGB')
ramp2.color_ramp.elements[0].position = 0.45; ramp2.color_ramp.elements[1].position = 0.62
ramp2.color_ramp.elements[0].color = (0.0, 0.0, 0.0, 1); ramp2.color_ramp.elements[1].color = (1, 1, 1, 1)
nt.links.new(n2.outputs['Fac'], ramp.inputs['Fac']); nt.links.new(n1.outputs['Fac'], ramp2.inputs['Fac'])
mix.inputs['A'].default_value = (0.0, 0.0, 0.0, 1)
nt.links.new(ramp.outputs['Color'], mix.inputs['A'])
mix.inputs['B'].default_value = (0.33, 0.27, 0.17, 1)   # dry dirt patches
nt.links.new(ramp2.outputs['Color'], mix.inputs['Factor'])
nt.links.new(mix.outputs['Result'], bsdf.inputs['Base Color'])
g.data.materials.append(gm)
# scattered bushes / stones so speed reads
random.seed(7)
bpy.ops.mesh.primitive_ico_sphere_add(subdivisions=2, radius=1)
bush = bpy.context.active_object; bush.name = 'BushProto'
bm = bpy.data.materials.new('Bush'); bm.use_nodes = True
bm.node_tree.nodes['Principled BSDF'].inputs['Base Color'].default_value = (0.08, 0.14, 0.05, 1)
bush.data.materials.append(bm)
st = bpy.data.materials.new('Stone'); st.use_nodes = True
st.node_tree.nodes['Principled BSDF'].inputs['Base Color'].default_value = (0.16, 0.155, 0.15, 1)
path = [HPOS[f] for f in range(1, F_END + 1, 5)]
for i in range(420):
    c = random.choice(path)
    off = Vector((random.uniform(-220, 220), random.uniform(-220, 220), 0))
    if any((c + off - q).length < 22 for q in path[::3]): continue
    o = bpy.data.objects.new('b%d' % i, bush.data.copy() if i % 5 == 0 else bush.data); sc.collection.objects.link(o)
    s = random.uniform(1.5, 6); o.scale = (s * random.uniform(0.8, 1.4), s * random.uniform(0.8, 1.4), s * random.uniform(0.4, 0.9))
    o.location = c + off
    if i % 5 == 0: o.data.materials[0] = st; o.scale = (s * 0.5, s * 0.6, s * 0.35)
bush.hide_render = True

# ---------------- camera ----------------
cam = bpy.data.objects.new('Cam', bpy.data.cameras.new('Cam')); sc.collection.objects.link(cam); sc.camera = cam
cam.data.lens = E('LENS', 30); cam.data.clip_start = 0.5; cam.data.clip_end = 8000
def film(mode, frames, out):
    cp, cv, lp = None, Vector((0, 0, 0)), None
    for f in range(1, F_END + 1):
        fw = fwd_of(HYAW[f]); sd = Vector((-fw.y, fw.x, 0))
        if mode == 'rear':
            want = HPOS[f] - fw * E('CAM_BACK', 40) * U + UP * E('CAM_UP', 23) * U + sd * E('CAM_SIDE', -3) * U
            look = HPOS[f] + fw * 30 * U + UP * 19 * U
        else:
            want = HPOS[f] + sd * 50 * U + fw * 8 * U + UP * 14 * U
            look = HPOS[f] + UP * 13 * U
        if cp is None: cp, lp = want.copy(), look.copy()
        k = E('CAM_K', 110.0)
        cv += ((want - cp) * k - cv * 2 * 0.9 * k ** 0.5) * dt; cp = cp + cv * dt
        lp = lp.lerp(look, 0.3)
        cam.location = cp; cam.rotation_euler = (lp - cp).to_track_quat('-Z', 'Y').to_euler()
        cam.keyframe_insert('location', frame=f); cam.keyframe_insert('rotation_euler', frame=f)
    sc.render.image_settings.file_format = 'PNG'
    for f in frames:
        sc.frame_set(f); sc.render.filepath = os.path.join(out, 'f_%05d.png' % f)
        bpy.ops.render.render(write_still=True)
    print('RES filmed', mode, len(frames))
print('RES F_END', F_END, 'starts', starts)
modes = ['rear', 'side'] if cam_mode == 'both' else [cam_mode]
for m in modes:
    if cam.animation_data: cam.animation_data_clear()
    (None if os.environ.get('NORENDER') else film(m, stills or list(range(1, F_END + 1)), os.path.join(outdir, m)))
