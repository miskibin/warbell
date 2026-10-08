"""Author the single 48x48 Warbell forest approval scene in Blender MCP."""
from pathlib import Path
import bpy
import math
import random
import json
import hashlib
from mathutils import Vector

ROOT=Path(__file__).resolve().parents[2]
ART=ROOT/"art/forest_preview"
OUT=ROOT/"assets/models/forest_slice"
ENV=ROOT/"assets/models/blender_environment"
TREES=ROOT/"assets/models/blender_trees"
OUT.mkdir(parents=True,exist_ok=True)
ART.mkdir(parents=True,exist_ok=True)
rng=random.Random(24100832)
path=[(9,24),(6,15),(1,6),(-2,-4),(-1,-14),(-7,-24)]

def height(x,z):
    return .06*math.sin(.22*x)*math.cos(.15*z)+.03*math.sin(.63*x+.40*z)

def distance_to_path(x,z):
    best=1e9
    for (ax,az),(bx,bz) in zip(path,path[1:]):
        dx,dz=bx-ax,bz-az
        t=max(0,min(1,((x-ax)*dx+(z-az)*dz)/(dx*dx+dz*dz)))
        best=min(best,math.hypot(x-(ax+t*dx),z-(az+t*dz)))
    return best

instances=[]
trees=[]
def place(model,x,z,scale=1,angle=None,dy=0):
    y=height(x,z)+dy
    if isinstance(scale,(float,int)): scale=[float(scale)]*3
    item={"model":model,"position":[round(x,5),round(y,5),round(z,5)],
          "rotation_y":round(rng.uniform(0,math.tau) if angle is None else angle,6),
          "scale":[round(float(v),5) for v in scale]}
    instances.append(item)
    if model.startswith("tree:"): trees.append((x,z))
    return item

tree_names=["tree:oak_a","tree:oak_b","tree:birch_a","tree:birch_b","tree:pine_a"]
# Foreground frame and deliberately asymmetric focal crowns.
focal=[("tree:pine_a",-11,11,2.80),("tree:birch_a",14,6,2.45),
       ("tree:oak_a",-16,-3,2.95),("tree:oak_b",13,-8,2.75),
       ("tree:oak_a",-8,-13,3.10),("tree:birch_b",20,15,2.25)]
for model,x,z,s in focal: place(model,x,z,s)
# Crown overlap along the rear horizon hides the finite terrain boundary.
for row,zbase in enumerate((-22.1,-18.5)):
    for i in range(13 if row==0 else 11):
        x=-23.0+i*(46/(12 if row==0 else 10))+rng.uniform(-1.1,1.1)
        z=zbase+rng.uniform(-1.0,1.0)
        if distance_to_path(x,z)<2.6: continue
        model=rng.choices(tree_names,weights=[4,3,2,2,2])[0]
        place(model,x,z,rng.uniform(2.0,3.05))
for i in range(46):
    for _ in range(120):
        x,z=rng.uniform(-22,22),rng.uniform(-19,18)
        if distance_to_path(x,z)<3.45: continue
        if min((math.hypot(x-tx,z-tz) for tx,tz in trees),default=100)<3.3: continue
        break
    else: continue
    model=rng.choices(tree_names,weights=[5,4,2,2,2])[0]
    place(model,x,z,rng.uniform(1.85,2.85))

# Three inexpensive frozen-tree ranks extend the forest beyond the detailed
# 48u composition and obscure the otherwise open 120u terrain horizon.
for row,zbase in enumerate((-27.5,-39.0,-51.0)):
    for i in range(20 if row==0 else 17):
        x=-55+i*(110/(19 if row==0 else 16))+rng.uniform(-1.5,1.5)
        z=zbase+rng.uniform(-2,2)
        model=rng.choices(tree_names,weights=[5,4,2,2,3])[0]
        place(model,x,z,rng.uniform(2.0,2.95))

# Only a handful of foreground trunks use the new 13k-triangle sculpted oaks.
# Distant trees remain the earlier shared study meshes for measured efficiency.
near_candidates=[i for i in instances if i["model"] in ("tree:oak_a","tree:oak_b")
                 and i["position"][2]>6]
near_candidates.sort(key=lambda i:math.hypot(i["position"][0]-10.5,
                                             i["position"][2]-19.3))
for j,item in enumerate(near_candidates[:5]):
    item["model"]="slice:hero_oak_a" if j%2==0 else "slice:hero_oak_b"
    item["scale"]=[1.02+.05*(j%2)]*3
for j,item in enumerate(i for i in instances
                        if i["model"] in ("tree:oak_a","tree:oak_b")
                        and i["position"][2]>-6
                        and math.hypot(i["position"][0]-10.5,
                                       i["position"][2]-19.3)<25):
    item["model"]="slice:hero_oak_b" if j%2 else "slice:hero_oak_a"
    item["scale"]=[rng.uniform(.84,1.02)]*3

def scatter(names,count,scale_lo,scale_hi,path_clear,seed_bias=0):
    for i in range(count):
        for _ in range(80):
            x,z=rng.uniform(-22.6,22.6),rng.uniform(-22.6,22.6)
            if distance_to_path(x,z)<path_clear: continue
            if seed_bias and rng.random()>.58 and z<-4: continue
            break
        else: continue
        place(names[i%len(names)],x,z,rng.uniform(scale_lo,scale_hi))

# Woody understory, where one third of plants carry a different form or tint.
scatter(["slice:hero_shrub_a","slice:hero_shrub_b"],154,.75,1.10,2.0)
scatter(["fern_a","fern_c","fern_b"],90,.85,1.35,1.8)
scatter(["rock_a","rock_b","rock_c"],48,.8,1.55,1.95)
scatter(["stump_a","swamp_log_a","dead_tree_a"],15,.65,1.3,2.0)
scatter(["mushroom_a","mushroom_b","mushroom_c","mushroom_d"],28,.75,1.35,1.7)
# The approval camera sees a continuous, uneven meadow carpet. Most clumps
# occupy the visible half, while a lighter background layer keeps depth.
for i in range(2450):
    for _ in range(80):
        x=rng.uniform(-22.3,22.3)
        z=rng.uniform(-6,22.2) if i<2050 else rng.uniform(-22,-6)
        if distance_to_path(x,z)<1.56: continue
        break
    else: continue
    model="slice:hero_grass_a" if i%2==0 else "slice:hero_grass_b"
    place(model,x,z,rng.uniform(.47,.66))

flower_ids=["slice:hero_flower_yellow","slice:hero_flower_white",
            "slice:hero_flower_white","slice:hero_flower_purple"]
for i in range(240):
    for _ in range(80):
        x=rng.uniform(-20,20)
        z=rng.uniform(-5,20) if i<210 else rng.uniform(-19,-5)
        distance=distance_to_path(x,z)
        if not 1.65<distance<9.5: continue
        break
    else: continue
    place(flower_ids[i%len(flower_ids)],x,z,rng.uniform(.78,1.04))

# Low authored gravel breaks up the photographic road surface and its verges.
for i in range(53):
    segment=rng.randrange(5)
    ax,az=path[segment];bx,bz=path[segment+1]
    t=rng.random()
    dx,dz=bx-ax,bz-az
    length=math.hypot(dx,dz)
    side=(rng.choice((-1,1))*rng.uniform(.78,1.42)
          if rng.random()<.67 else rng.uniform(-.78,.78))
    x=ax+t*dx-side*dz/length
    z=az+t*dz+side*dx/length
    place(f"slice:path_pebbles_{'abc'[i%3]}",x,z,
          rng.uniform(.40,.78),dy=-rng.uniform(.015,.045))

# A close inspection composition: small stones, fallen wood and fungi beneath
# a birch/pine canopy, with a few distinct blossoms around the fern edges.
for model,x,z,scale in (
    ("rock_a",-5.1,6.0,1.55),("rock_b",-6.15,5.45,.93),
    ("rock_c",-4.15,6.5,.75),("stump_a",-4.4,4.4,1.12),
    ("swamp_log_a",-6.65,7.15,.95),("fern_a",-6.0,6.35,1.3),
    ("fern_c",-3.9,5.65,1.14),("fern_b",-5.4,4.8,1.16),
    ("shrub_a",-7.15,4.5,1.1),("shrub_c",-7.35,8.2,1.05),
    ("mushroom_a",-5.65,6.8,1.1),("mushroom_c",-4.0,4.95,1.0),
    ("slice:hero_flower_yellow",-6.8,4.9,.94),
    ("slice:hero_flower_white",-3.8,7.1,.88),
    ("slice:hero_flower_purple",-6.1,8.4,.88),
    ("slice:hero_shrub_a",-7.8,6.7,1.08),
    ("slice:hero_flower_white",-5.8,7.9,1.0),
    ("slice:hero_flower_purple",-7.1,7.7,1.0),
    ("slice:hero_grass_a",-4.8,6.1,1.1),
    ("slice:path_pebbles_b",-2.6,6.1,.75),
):
    place(model,x,z,scale)

# Overlap natural crowns across the distant path terminus, rather than
# presenting a straight empty corridor and the terrain sheet horizon.
for model,x,z,scale in (("tree:oak_b",-10.7,-21.5,2.35),
                        ("tree:birch_b",-4.1,-21.8,2.15),
                        ("shrub_a",-8.7,-21.0,1.3),
                        ("shrub_b",-5.6,-20.5,1.2)):
    place(model,x,z,scale)
camera={"eye":[10.5,3.05,19.3],"target":[-3.5,.95,-8.5]}
closeup={"eye":[1.45,1.69,11.45],"target":[-5.35,.62,5.65]}
layout={"schema":"warbell.forest_slice.v1","origin":[500,0,500],
        "bounds":[120,120],"camera":camera,"closeup_camera":closeup,
        "path_centerline":[[x,z] for x,z in path],"path_width":3.1,
        "instances":instances}
(OUT/"layout.json").write_text(json.dumps(layout,indent=2),encoding="utf-8")

# Build an editable Blender source using the same authored mesh datablocks.
env_names={i["model"] for i in instances
           if not i["model"].startswith(("tree:","slice:"))}
mesh_sources={name:bpy.data.objects[name].data for name in env_names}
bpy.ops.object.select_all(action="SELECT")
bpy.ops.object.delete(use_global=False)
tree_sources={}
forest_tree_atlas=bpy.data.images.load(str(OUT/"tree_atlas_forest.png"),check_existing=True)
forest_tree_atlas.pack()
for name in sorted({i["model"].split(":",1)[1] for i in instances if i["model"].startswith("tree:")}):
    before=set(bpy.data.objects)
    bpy.ops.import_scene.gltf(filepath=str(TREES/(name+".glb")))
    imported=[o for o in bpy.data.objects if o not in before and o.type=="MESH"]
    if len(imported)!=1: raise ValueError(f"Tree import {name} yielded {len(imported)} meshes")
    obj=imported[0]
    mesh=obj.data.copy()
    mesh.transform(obj.matrix_world)
    for mat in mesh.materials:
        if mat and mat.use_nodes:
            for node in mat.node_tree.nodes:
                if node.type=="TEX_IMAGE":
                    node.image=forest_tree_atlas
    tree_sources["tree:"+name]=mesh
    for o in [o for o in bpy.data.objects if o not in before]: bpy.data.objects.remove(o,do_unlink=True)

# The new foreground kit is an independent Blender-authored GLB with one
# shared 2048 PBR atlas; import its meshes into this editable composition.
slice_sources={}
before=set(bpy.data.objects)
bpy.ops.import_scene.gltf(filepath=str(OUT/"hero_forest_kit.glb"))
for obj in [o for o in bpy.data.objects if o not in before and o.type=="MESH"]:
    key="slice:"+obj.name.split(".")[0]
    mesh=obj.data.copy()
    mesh.transform(obj.matrix_world)
    slice_sources[key]=mesh
for obj in [o for o in bpy.data.objects if o not in before]:
    bpy.data.objects.remove(obj,do_unlink=True)
required_slice={i["model"] for i in instances if i["model"].startswith("slice:")}
if not required_slice.issubset(slice_sources):
    raise ValueError(f"Hero GLB missing {sorted(required_slice-set(slice_sources))}")

def textured_material(name,file):
    mat=bpy.data.materials.new(name)
    mat.use_nodes=True
    nodes=mat.node_tree.nodes
    bsdf=nodes.get("Principled BSDF")
    tex=nodes.new("ShaderNodeTexImage")
    tex.image=bpy.data.images.load(str(file),check_existing=True)
    tex.image.pack()
    mat.node_tree.links.new(tex.outputs["Color"],bsdf.inputs["Base Color"])
    return mat

terrain_mat=textured_material("Forest Slice Grass",ENV/"ground_grass_albedo.png")
path_mat=textured_material("Forest Slice Warm Path",OUT/"ground_path_albedo.png")
grid=120
verts=[];uvs=[];faces=[]
for j in range(grid+1):
    z=-60+j
    for i in range(grid+1):
        x=-60+i
        verts.append((x,-z,height(x,z)))
        uvs.append((i/2.5,j/2.5))
for j in range(grid):
    for i in range(grid):
        a=j*(grid+1)+i
        faces.append((a,a+1,a+grid+2,a+grid+1))
terrain=bpy.data.meshes.new("Forest 120u terrain")
terrain.from_pydata(verts,[],faces)
terrain.update()
terrain.uv_layers.new()
for poly in terrain.polygons:
    for li in poly.loop_indices:
        vi=terrain.loops[li].vertex_index
        terrain.uv_layers.active.data[li].uv=uvs[vi]
terrain.materials.append(terrain_mat)
ground_obj=bpy.data.objects.new("Forest terrain | editable",terrain)
bpy.context.collection.objects.link(ground_obj)

# Source-scene path ribbon follows the exact ground-agent centreline.
pverts=[];pfaces=[];puv=[];acc=0
for k,(x,z) in enumerate(path):
    prev=path[max(0,k-1)];nxt=path[min(len(path)-1,k+1)]
    dx,dz=nxt[0]-prev[0],nxt[1]-prev[1]
    length=math.hypot(dx,dz)
    px,pz=-dz/length,dx/length
    if k: acc+=math.hypot(x-path[k-1][0],z-path[k-1][1])
    for side in (-1,1):
        xx,zz=x+side*1.55*px,z+side*1.55*pz
        pverts.append((xx,-zz,height(xx,zz)+.012))
        puv.append((0 if side<0 else 1,acc/7.6))
    if k: pfaces.append((2*k-2,2*k-1,2*k+1,2*k))
path_mesh=bpy.data.meshes.new("Path centreline warm dirt")
path_mesh.from_pydata(pverts,[],pfaces)
path_mesh.update();path_mesh.uv_layers.new()
for poly in path_mesh.polygons:
    for li in poly.loop_indices:
        vi=path_mesh.loops[li].vertex_index
        path_mesh.uv_layers.active.data[li].uv=puv[vi]
path_mesh.materials.append(path_mat)
bpy.context.collection.objects.link(bpy.data.objects.new("Curving path | editable",path_mesh))

for idx,item in enumerate(instances):
    sources=(tree_sources if item["model"].startswith("tree:") else
             slice_sources if item["model"].startswith("slice:") else mesh_sources)
    data=sources[item["model"]]
    obj=bpy.data.objects.new(f"{idx:04d} {item['model']}",data)
    x,y,z=item["position"]
    obj.location=(x,-z,y)
    obj.rotation_euler[2]=item["rotation_y"]
    obj.scale=item["scale"]
    bpy.context.collection.objects.link(obj)

cam_data=bpy.data.cameras.new("Approval camera")
cam=bpy.data.objects.new("Approval camera",cam_data)
bpy.context.collection.objects.link(cam)
eye=Vector((camera["eye"][0],-camera["eye"][2],camera["eye"][1]))
target=Vector((camera["target"][0],-camera["target"][2],camera["target"][1]))
cam.location=eye
cam.rotation_euler=(target-eye).to_track_quat("-Z","Y").to_euler()
cam_data.lens=32
bpy.context.scene.camera=cam
sun_data=bpy.data.lights.new("Warm afternoon sun","SUN")
sun=bpy.data.objects.new("Warm afternoon sun",sun_data)
bpy.context.collection.objects.link(sun)
sun.rotation_euler=(math.radians(32),math.radians(-27),math.radians(-17))
sun_data.energy=2.4
bpy.context.scene.world.color=(.45,.64,.85)
bpy.context.scene.render.resolution_x=1600
bpy.context.scene.render.resolution_y=900
bpy.context.preferences.filepaths.save_version=0
bpy.ops.wm.save_as_mainfile(filepath=str(ART/"forest_scene.blend"))

report={"schema":"warbell.forest_slice_source.v1",
        "layout_sha256":hashlib.sha256((OUT/"layout.json").read_bytes()).hexdigest(),
        "scene_sha256":hashlib.sha256((ART/"forest_scene.blend").read_bytes()).hexdigest(),
        "instances":len(instances),
        "by_family":{kind:sum(i["model"].startswith(kind) for i in instances)
                     for kind in ("tree:","shrub_","meadow_grass_","meadow_flowers_","rock_")},
        "camera":camera,"closeup_camera":closeup,"path_centerline":path}
(ART/"scene_report.json").write_text(json.dumps(report,indent=2),encoding="utf-8")
print("WARBELL_FOREST_SLICE_DONE",json.dumps(report))
