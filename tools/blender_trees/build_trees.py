"""MCP-executed Blender authoring script for Warbell's studio tree experiment.

This file is sent to Blender through execute_blender_code, not run with `blender -P`.
Geometry is modeled in Blender Z-up and exported both to GLB and Bevy Y-up JSON.
All five trees are original procedural structures made here; leaf photo cutouts are
existing MIT-licensed Warbell textures regraded by make_atlas.py.
"""
from pathlib import Path
import bpy, math, random, json, hashlib, struct
from mathutils import Vector

ROOT = Path(__file__).resolve().parents[2]
ASSET = ROOT / "assets/models/blender_trees"
ART = ROOT / "art/blender_trees"
ASSET.mkdir(parents=True, exist_ok=True)
ART.mkdir(parents=True, exist_ok=True)

bpy.ops.object.select_all(action="SELECT")
bpy.ops.object.delete(use_global=False)

ATLAS = {"oak": (0.008, 0.008, 0.492, 0.492),
         "birch": (0.508, 0.008, 0.992, 0.492),
         "pine": (0.008, 0.508, 0.492, 0.992)}
BARK = {"oak": (0.508, 0.508, 0.992, 0.664),
        "birch": (0.508, 0.672, 0.992, 0.824),
        "pine": (0.508, 0.836, 0.992, 0.992)}

class Builder:
    def __init__(self, name, species, seed):
        self.name, self.species, self.rng = name, species, random.Random(seed)
        self.verts, self.normals, self.uvs, self.cols, self.faces = [], [], [], [], []

    def vert(self, p, n, uv, c):
        self.verts.append(tuple(p))
        self.normals.append(tuple(n))
        self.uvs.append(tuple(uv))
        self.cols.append(tuple(c))
        return len(self.verts)-1

    def tube(self, a, b, ra, rb, sides=6, tint=(1,1,1,1)):
        a, b = Vector(a), Vector(b)
        axis = (b-a).normalized()
        helpv = Vector((0,1,0)) if abs(axis.y)<.88 else Vector((1,0,0))
        u = axis.cross(helpv).normalized()
        v = axis.cross(u).normalized()
        uvrect = BARK[self.species]
        ring = []
        for end, centre, radius in ((0,a,ra),(1,b,rb)):
            row=[]
            for i in range(sides+1):
                ang = i/sides * math.tau
                n = u*math.cos(ang)+v*math.sin(ang)
                p = centre+n*radius
                # Bark repeats across both the circumference and tree height inside its band.
                tu = uvrect[0]+(uvrect[2]-uvrect[0])*(i/sides)
                tv = uvrect[1]+(uvrect[3]-uvrect[1])*(end*.88+.06)
                row.append(self.vert(p,n,(tu,tv),tint))
            ring.append(row)
        for i in range(sides):
            a0,a1,b0,b1 = ring[0][i],ring[0][i+1],ring[1][i],ring[1][i+1]
            self.faces.extend(((a0,a1,b0),(a1,b1,b0)))

    def card(self, stem, direction, width, length, angle, tint):
        stem, up = Vector(stem), Vector(direction).normalized()
        h = Vector((0,0,1)) if abs(up.z)<.85 else Vector((0,1,0))
        side = up.cross(h).normalized()
        side = side*math.cos(angle) + up.cross(side)*math.sin(angle)
        side.normalize()
        normal = side.cross(up).normalized()
        tip = stem + up*length
        half = side*(width*.5)
        rect = ATLAS[self.species]
        ps = (stem-half,stem+half,tip+half,tip-half)
        uv = ((rect[0],rect[3]),(rect[2],rect[3]),(rect[2],rect[1]),(rect[0],rect[1]))
        ids=[self.vert(p,normal,t,c) for p,t,c in zip(ps,uv,[tint]*4)]
        self.faces.extend(((ids[0],ids[1],ids[2]),(ids[0],ids[2],ids[3])))

def jitter(rng, v, spread): return v*(1+rng.uniform(-spread,spread))

def oak(name, seed):
    m=Builder(name,"oak",seed); r=m.rng
    bend=r.uniform(-.055,.055)
    m.tube((0,0,0),(bend*.3,0,.48),.102,.079,9)
    m.tube((bend*.3,0,.48),(bend,0,1.02),.080,.048,8)
    m.tube((bend,0,1.02),(bend+.035,.01,1.49),.051,.013,6)
    for k in range(5):
        ang=k*math.tau/5+.27+r.uniform(-.22,.22)
        m.tube((0,0,.055),(.12*math.cos(ang),.12*math.sin(ang),.0),.045,.008,5)
    for i in range(9):
        a=i*math.tau/9+r.uniform(-.15,.15)
        z=.71+(i%4)*.11+r.uniform(-.045,.045)
        reach=jitter(r,.48+(.05 if i%3==0 else 0),.17)
        root=(bend*.7,0,z)
        mid=(.55*reach*math.cos(a),.55*reach*math.sin(a),z+.23)
        tip=(reach*math.cos(a),reach*math.sin(a),z+.34+r.uniform(-.08,.09))
        m.tube(root,mid,.032,.017,5)
        m.tube(mid,tip,.018,.004,4)
        for j in range(5):
            t=.31+j*.15
            c=Vector(mid).lerp(Vector(tip),t)+Vector((r.uniform(-.10,.10),r.uniform(-.10,.10),r.uniform(-.10,.10)))
            outward=Vector((math.cos(a),math.sin(a),r.uniform(.22,.75))).normalized()
            twig=c+outward*r.uniform(.09,.17)
            m.tube(c,twig,.007,.002,3)
            for q in range(4):
                d=outward+Vector((r.uniform(-.45,.45),r.uniform(-.45,.45),r.uniform(-.15,.35)))
                green=r.choice(((.57,.90,.39,1),(.68,.98,.46,1),(.49,.79,.36,1),(.75,.99,.49,1)))
                m.card(twig+outward*r.uniform(-.035,.035),d,jitter(r,.26,.18),jitter(r,.31,.16),q*.76+r.random()*.35,green)
    # A few high crown sprigs soften the leader while retaining visible branch gaps.
    for i in range(8):
        a=r.random()*math.tau; rad=r.uniform(.06,.30)
        s=(rad*math.cos(a),rad*math.sin(a),r.uniform(1.31,1.52))
        m.card(s,(math.cos(a)*.45,math.sin(a)*.45,1),.19,.22,r.random()*math.pi,(.69,.97,.45,1))
    # Interior sprigs make a continuous, asymmetric crown around the grown branch
    # skeleton. They are distributed through volume rather than stacked as a blob.
    for i in range(245):
        z=r.uniform(1.02,1.60)
        taper=math.sqrt(max(.06,1-((z-1.33)/.42)**2))
        a=r.random()*math.tau
        rad=math.sqrt(r.random())*.53*taper
        c=Vector((rad*math.cos(a)+.045,rad*math.sin(a),z))
        d=Vector((math.cos(a)*r.uniform(.12,.65),math.sin(a)*r.uniform(.12,.65),r.uniform(.45,1)))
        green=r.choice(((.56,.88,.36,1),(.67,.97,.43,1),(.47,.76,.33,1),(.74,.98,.46,1)))
        m.card(c,d,jitter(r,.225,.2),jitter(r,.255,.2),r.random()*math.pi,green)
    return m

def birch(name,seed):
    m=Builder(name,"birch",seed); r=m.rng
    drift=r.uniform(-.08,.08)
    m.tube((0,0,0),(drift*.35,0,.73),.071,.047,8)
    m.tube((drift*.35,0,.73),(drift,.025,1.41),.049,.010,7)
    for k in range(3):
        a=k*math.tau/3+.4
        m.tube((0,0,.045),(.085*math.cos(a),.085*math.sin(a),.005),.026,.007,4)
    for i in range(8):
        a=i*math.tau/8+r.uniform(-.17,.17)
        z=.68+i*.067
        reach=r.uniform(.34,.54)
        root=(drift*.6,0,z)
        tip=(drift+reach*math.cos(a),reach*math.sin(a),z+.10)
        m.tube(root,tip,.017,.003,4)
        for j in range(5):
            t=.36+j*.13
            attach=Vector(root).lerp(Vector(tip),t)
            hang=attach+Vector((math.cos(a)*r.uniform(.05,.12),math.sin(a)*r.uniform(.05,.12),-r.uniform(.07,.17)))
            m.tube(attach,hang,.004,.0015,3)
            for q in range(4):
                d=(r.uniform(-.4,.4),r.uniform(-.4,.4),r.uniform(-.08,.62))
                green=r.choice(((.68,.94,.47,1),(.77,1.0,.54,1),(.63,.89,.45,1)))
                m.card(hang+Vector((r.uniform(-.025,.025),r.uniform(-.025,.025),r.uniform(-.025,.025))),d,jitter(r,.205,.15),jitter(r,.26,.15),q*.76+r.random()*.35,green)
    for i in range(6):
        a=r.random()*math.tau
        s=(drift+r.uniform(.03,.16)*math.cos(a),r.uniform(.03,.16)*math.sin(a),r.uniform(1.28,1.44))
        m.card(s,(math.cos(a)*.3,math.sin(a)*.3,.8),.13,.17,r.random()*math.pi,(.70,.99,.49,1))
    # Fine, airy foliage throughout the upper bole; the old branch-tip-only
    # placement formed dark hanging bouquets below an empty crown.
    for i in range(240):
        z=r.uniform(.87,1.49)
        taper=math.sqrt(max(.08,1-((z-1.20)/.42)**2))
        a=r.random()*math.tau
        rad=math.sqrt(r.random())*.43*taper
        c=Vector((drift+rad*math.cos(a),rad*math.sin(a),z))
        d=Vector((math.cos(a)*r.uniform(.05,.5),math.sin(a)*r.uniform(.05,.5),r.uniform(-.2,.8)))
        if d.length<.1: d.z=.4
        green=r.choice(((.68,.94,.44,1),(.75,1.0,.50,1),(.59,.86,.40,1)))
        m.card(c,d,jitter(r,.17,.2),jitter(r,.23,.2),r.random()*math.pi,green)
    return m

def pine(name,seed):
    m=Builder(name,"pine",seed); r=m.rng
    m.tube((0,0,0),(.008,-.012,.75),.112,.078,8)
    m.tube((.008,-.012,.75),(.025,-.022,1.55),.079,.036,7)
    m.tube((.025,-.022,1.55),(.025,-.025,2.18),.038,.002,6)
    for k in range(4):
        a=k*math.tau/4+.18
        m.tube((0,0,.035),(.12*math.cos(a),.12*math.sin(a),.002),.04,.009,4)
    whorls=[(.44,.73,8),(.68,.68,8),(.92,.60,8),(1.16,.51,7),(1.39,.42,7),(1.62,.32,6),(1.83,.23,6),(2.02,.13,5)]
    for row,(z,reach,count) in enumerate(whorls):
        phase=r.uniform(-.3,.3)+row*.22
        for i in range(count):
            a=i*math.tau/count+phase+r.uniform(-.11,.11)
            length=jitter(r,reach,.10)
            start=(.018,0,z)
            mid=(length*.55*math.cos(a),length*.55*math.sin(a),z-.07)
            tip=(length*math.cos(a),length*math.sin(a),z-.14+r.uniform(-.035,.03))
            m.tube(start,mid,.014,.008,4)
            m.tube(mid,tip,.008,.001,3)
            # A sprig plane wraps each branch, one face near the tip and one rotated.
            d=Vector(tip)-Vector(mid)
            for q,t in enumerate((.12,.28,.45,.61,.76,.86)):
                s=Vector(start).lerp(Vector(tip),t)
                green=r.choice(((.48,.76,.43,1),(.57,.83,.47,1),(.42,.70,.40,1)))
                m.card(s,d+Vector((0,0,r.uniform(.08,.20))),jitter(r,.25,.15),jitter(r,.38,.12),q*.65+r.random()*.25,green)
    # Short inner shoots hide the bare central ladder without flattening the
    # serrated whorl silhouette that distinguishes this conifer from a cone.
    for i in range(82):
        z=r.uniform(.70,1.94)
        a=r.random()*math.tau
        reach=.42*(2.2-z)/1.5
        s=Vector((r.uniform(.05,reach)*math.cos(a),r.uniform(.05,reach)*math.sin(a),z))
        d=Vector((math.cos(a)*.8,math.sin(a)*.8,r.uniform(-.3,.15)))
        m.card(s,d,jitter(r,.20,.14),jitter(r,.28,.15),r.random()*math.pi,(.49,.77,.43,1))
    return m

image=bpy.data.images.load(str(ASSET / "tree_atlas.png"), check_existing=True)
image.reload()
mat=bpy.data.materials.new("Warbell | shared cutout atlas")
mat.use_nodes=True
bsdf=mat.node_tree.nodes.get("Principled BSDF")
bsdf.inputs["Roughness"].default_value=.91
tex=mat.node_tree.nodes.new("ShaderNodeTexImage")
tex.image=image
uvmap=mat.node_tree.nodes.new("ShaderNodeUVMap")
uvmap.uv_map="UVMap"
mat.node_tree.links.new(uvmap.outputs["UV"],tex.inputs["Vector"])
vcol=mat.node_tree.nodes.new("ShaderNodeVertexColor")
vcol.layer_name="Color"
mix=mat.node_tree.nodes.new("ShaderNodeMixRGB")
mix.blend_type="MULTIPLY"
mix.inputs[0].default_value=1.0
mat.node_tree.links.new(tex.outputs["Color"],mix.inputs[1])
mat.node_tree.links.new(vcol.outputs["Color"],mix.inputs[2])
mat.node_tree.links.new(mix.outputs["Color"],bsdf.inputs["Base Color"])
clip=mat.node_tree.nodes.new("ShaderNodeMath")
clip.operation="ROUND"  # glTF exporter recognizes this as alphaMode: MASK, cutoff .5.
mat.node_tree.links.new(tex.outputs["Alpha"],clip.inputs[0])
mat.node_tree.links.new(clip.outputs[0],bsdf.inputs["Alpha"])
mat.surface_render_method="DITHERED"
mat.use_transparency_overlap=False
mat.use_backface_culling=False

models=[oak("oak_a",5142),oak("oak_b",84021),birch("birch_a",7001),birch("birch_b",18220),pine("pine_a",9981)]
manifest={"schema":"warbell.blender_tree_manifest.v1","atlas":"tree_atlas.png","atlas_size":[1024,1024],"source":"Blender 4.5.9 via official mcp-for-blender execute_blender_code","leaf_source":"dgreenheck/ez-tree MIT; original files already shipped at assets/textures/leaves/{oakL,aspenL,pineL}.png","models":[]}

def glb_attribute(path, name):
    """Read the exported shading values back so the JSON and GLB are identical."""
    data=path.read_bytes()
    jlen,jtag=struct.unpack_from("<I4s",data,12)
    assert jtag==b"JSON"
    doc=json.loads(data[20:20+jlen])
    binary_off=20+jlen
    binlen,bintag=struct.unpack_from("<I4s",data,binary_off)
    assert bintag==b"BIN\x00"
    binary=data[binary_off+8:binary_off+8+binlen]
    primitive=doc["meshes"][0]["primitives"][0]
    accessor=doc["accessors"][primitive["attributes"][name]]
    view=doc["bufferViews"][accessor["bufferView"]]
    fmt={5126:"f",5121:"B",5123:"H"}[accessor["componentType"]]
    count={"VEC2":2,"VEC3":3,"VEC4":4}[accessor["type"]]
    scalar_size=struct.calcsize("<"+fmt)
    stride=view.get("byteStride",count*scalar_size)
    off=view.get("byteOffset",0)+accessor.get("byteOffset",0)
    normalizer={5121:255.0,5123:65535.0}.get(accessor["componentType"],1.0) if accessor.get("normalized") else 1.0
    return [[x/normalizer for x in struct.unpack_from("<"+fmt*count,binary,off+i*stride)] for i in range(accessor["count"])]

for index,m in enumerate(models):
    # Root flares are sculpted around the origin; shift their lowest tip onto the ground.
    min_z=min(v[2] for v in m.verts)
    m.verts=[(v[0],v[1],v[2]-min_z) for v in m.verts]
    mesh=bpy.data.meshes.new(m.name)
    mesh.from_pydata(m.verts,[],m.faces)
    mesh.update()
    mesh.uv_layers.new(name="UVMap")
    mesh.color_attributes.new(name="Color",type="FLOAT_COLOR",domain="CORNER")
    uv_values=[]
    color_values=[]
    for poly in mesh.polygons:
        poly.use_smooth=True
        for li in poly.loop_indices:
            vi=mesh.loops[li].vertex_index
            # Blender's bottom-left UV origin; GLB exporter flips to the PNG convention.
            uv_values.extend((m.uvs[vi][0],1-m.uvs[vi][1]))
            color_values.extend(m.cols[vi])
    # Reacquire both after creating attributes. Blender's Python proxy to the
    # UV layer becomes stale when the colour layer is added and aliases colour
    # memory; writing through it silently makes striped atlas sampling.
    mesh.color_attributes["Color"].data.foreach_set("color",color_values)
    mesh.uv_layers["UVMap"].data.foreach_set("uv",uv_values)
    mesh.update()
    if index==0:
        print("WARBELL_UV_BEFORE_EXPORT",[(mesh.loops[li].vertex_index,tuple(mesh.uv_layers["UVMap"].data[li].uv)) for li in range(4)],m.uvs[:4])
    obj=bpy.data.objects.new(m.name,mesh)
    bpy.context.collection.objects.link(obj)
    mesh.materials.append(mat)
    obj.location.x=(index-2)*2.2
    # Per-object export uses local mesh origin, with origin at tree base.
    for o in bpy.context.selected_objects: o.select_set(False)
    obj.select_set(True); bpy.context.view_layer.objects.active=obj
    oldx=obj.location.x; obj.location.x=0
    bpy.ops.export_scene.gltf(filepath=str(ASSET/(m.name+".glb")),export_format="GLB",use_selection=True,export_apply=False,export_yup=True,export_attributes=False)
    obj.location.x=oldx
    # Blender Z-up -> Bevy Y-up is a proper rotation, determinant +1.
    positions=[[round(x,6),round(z,6),round(-y,6)] for x,y,z in m.verts]
    glb_path=ASSET/(m.name+".glb")
    glb_positions=glb_attribute(glb_path,"POSITION")
    assert len(glb_positions)==len(positions)
    assert max(abs(a-b) for pa,pb in zip(glb_positions,positions) for a,b in zip(pa,pb))<2e-5
    normals=glb_attribute(glb_path,"NORMAL")
    colors=glb_attribute(glb_path,"COLOR_0")
    indices=[v for tri in m.faces for v in tri]
    mins=[min(p[i] for p in positions) for i in range(3)]
    maxs=[max(p[i] for p in positions) for i in range(3)]
    data={"schema":"warbell.blender_tree_mesh.v1","name":m.name,"positions":positions,"normals":normals,"uvs":m.uvs,"colors":colors,"indices":indices,"bounds":{"min":mins,"max":maxs},"triangles":len(m.faces)}
    (ASSET/(m.name+".json")).write_text(json.dumps(data,separators=(",",":")),encoding="utf-8")
    manifest["models"].append({"name":m.name,"species":m.species,"vertices":len(m.verts),"triangles":len(m.faces),"bounds":{"min":mins,"max":maxs}})

bpy.ops.wm.save_as_mainfile(filepath=str(ART/"warbell_trees.blend"))
for p in [ASSET/"tree_atlas.png",*(ASSET/(m.name+ext) for m in models for ext in (".glb",".json")),ART/"warbell_trees.blend"]:
    manifest.setdefault("sha256",{})[p.name]=hashlib.sha256(p.read_bytes()).hexdigest()
(ART/"manifest.json").write_text(json.dumps(manifest,indent=2),encoding="utf-8")
print("WARBELL_TREES_DONE",json.dumps({"models":manifest["models"],"blend":str(ART/"warbell_trees.blend")}))
