"""Blender MCP authoring/export of detailed, slice-only forest geometry."""
from pathlib import Path
import sys
import bpy
import math
import random
import json
import hashlib
from mathutils import Vector

ROOT=Path(__file__).resolve().parents[2]
sys.path.insert(0,str(ROOT/"tools/blender_environment"))
from envkit import Model,glb_doc,accessor,uv_rect

OUT=ROOT/"assets/models/forest_slice"
ART=ROOT/"art/forest_preview"
OUT.mkdir(parents=True,exist_ok=True)
ATLAS=OUT/"hero_tree_atlas.png"
NORMAL=OUT/"hero_tree_normal.png"
ORM=OUT/"hero_tree_orm.png"

def vertex(m,p,uv,col=(1,1,1,1),normal=None):
    i=m.vertex(tuple(p),uv,col)
    if normal is not None: m.card_normals[i]=tuple(Vector(normal).normalized())
    return i

def tube(m,points,radii,sides=10,cell=0,col=(1,1,1,1),bark_full=False):
    """Curved tapered branch with smoothly sampled radial normals and bark UVs."""
    points=[Vector(p) for p in points]
    rings=[]
    lengths=[0.]
    for a,b in zip(points,points[1:]): lengths.append(lengths[-1]+(b-a).length)
    for j,p in enumerate(points):
        direction=(points[min(j+1,len(points)-1)]-points[max(j-1,0)]).normalized()
        guide=Vector((0,0,1)) if abs(direction.z)<.93 else Vector((0,1,0))
        right=direction.cross(guide).normalized()
        up=direction.cross(right).normalized()
        ring=[]
        for k in range(sides+1):
            a=2*math.pi*k/sides
            radial=math.cos(a)*right+math.sin(a)*up
            wobble=1+.06*math.sin(k*2.3+j*.87)
            q=p+radial*radii[j]*wobble
            if bark_full:
                uv=(.006+k/sides*.238,.006+j/(len(points)-1)*.488)
            else:
                u0,v0,u1,v1=uv_rect(cell,.024)
                uv=(u0+k/sides*(u1-u0),v0+lengths[j]/max(lengths[-1],.01)*(v1-v0))
            ring.append(vertex(m,q,uv,col,radial))
        rings.append(ring)
    for j in range(len(rings)-1):
        for k in range(sides):
            a,b,c,d=rings[j][k],rings[j][k+1],rings[j+1][k+1],rings[j+1][k]
            m.faces.extend(((a,b,c),(a,c,d)))

def spray(m,center,width,height,cell,rng,col=(1,1,1,1)):
    """A small photographic leaf spray on an independently aimed 3D plane."""
    az=rng.uniform(0,math.tau)
    nz=rng.uniform(.26,.91)
    nr=math.sqrt(1-nz*nz)
    normal=Vector((math.cos(az)*nr,math.sin(az)*nr,nz))
    right=Vector((-math.sin(az),math.cos(az),0))
    vertical=normal.cross(right).normalized()
    twist=rng.uniform(-.8,.8)
    right,vertical=(right*math.cos(twist)+vertical*math.sin(twist),
                    vertical*math.cos(twist)-right*math.sin(twist))
    center=Vector(center)
    r=right*width*.5;v=vertical*height*.5
    corners=(center-r-v,center+r-v,center+r+v,center-r+v)
    first=len(m.vertices)
    m.quad(*corners,cell,col)
    lit=(normal*.47+Vector((0,0,1))*.53).normalized()
    for index in range(first,first+4): m.card_normals[index]=tuple(lit)

def oak(name,seed,lean=0):
    rng=random.Random(seed)
    m=Model(name,"cutout","Sculpted oak with scanned lichen bark, tapered limbs, flared roots and thousands of independent leaf sprays")
    height=5.55+rng.uniform(-.17,.18)
    trunk=[];radii=[]
    for j,(z,r) in enumerate(((0,.49),(.10,.54),(.32,.45),(.7,.37),(1.2,.31),
                              (1.9,.265),(2.7,.22),(3.5,.17),(4.3,.10),(5.0,.028))):
        trunk.append((lean*z*.10+.025*math.sin(j*.82),
                      .08*math.sin(j*.54+.2),z))
        radii.append(r*(1+rng.uniform(-.045,.045)))
    tube(m,trunk,radii,18,bark_full=True)
    # Buttress roots flare out of the bark cylinder and visibly meet the soil.
    for k in range(8):
        a=k*math.tau/8+rng.uniform(-.1,.1)
        ca,sa=math.cos(a),math.sin(a)
        rr=rng.uniform(.70,1.09)
        tube(m,[(ca*.26,sa*.26,.56),(ca*.50,sa*.50,.25),
                (ca*rr,sa*rr,.035)],
             [.19,.125,.02],8,bark_full=True)
    endpoints=[]
    for i in range(15):
        a=i*2.399963+rng.uniform(-.23,.23)
        z0=1.95+i*.142+rng.uniform(-.10,.11)
        x0=lean*z0*.10;y0=.08*math.sin(z0*.5)
        reach=rng.uniform(1.48,2.46)*(1-.018*i)
        rise=rng.uniform(.48,1.15)
        d=Vector((math.cos(a),math.sin(a),0))
        s=Vector((x0,y0,z0))
        mid=s+d*reach*.46+Vector((0,0,rise*.47))
        end=s+d*reach+Vector((0,0,rise))
        bend=Vector((-d.y,d.x,0))*rng.uniform(-.17,.17)
        mid+=bend
        tube(m,[s,mid,end],[.135-.003*i,.087-.002*i,.033],9,bark_full=True)
        for side in (-1,1):
            turn=a+side*rng.uniform(.37,.77)
            extent=rng.uniform(.57,.96)
            q=end+Vector((math.cos(turn)*extent,math.sin(turn)*extent,
                          rng.uniform(.15,.48)))
            tube(m,[mid*.26+end*.74,q],[.047,.012],7,bark_full=True)
            endpoints.append((q,rng.uniform(.57,.83)))
        endpoints.append((end,rng.uniform(.48,.7)))
    # Airy overlapping lobes; the source crop contains only 1–4 leaves, so
    # there are no giant flat fan sheets. The canopy is uneven by branch.
    target=5900 if name.endswith("a") else 5550
    for j in range(target):
        tip,radius=endpoints[j%len(endpoints)]
        theta=rng.uniform(0,math.tau)
        radial=math.sqrt(rng.random())*radius
        zoff=rng.uniform(-.48,.48)*radius
        center=tip+Vector((radial*math.cos(theta),radial*math.sin(theta),zoff))
        center.z=min(height+.38,max(2.05,center.z))
        w=rng.uniform(.17,.31)
        h=w*rng.uniform(.74,1.22)
        cell=rng.choice((1,1,1,2,5))
        brightness=rng.uniform(.78,1.08)
        green=rng.uniform(.88,1.04)
        colour=(brightness*.91,brightness*green,brightness*.75,1)
        spray(m,center,w,h,cell,rng,colour)
    return m

def grass_patch(name,seed):
    rng=random.Random(seed)
    m=Model(name,"cutout","Dense irregular 3D blade clump, not one rectangular card")
    for i in range(112 if name.endswith("a") else 96):
        a=i*2.399963+rng.uniform(-.22,.22)
        rr=math.sqrt((i+.3)/112)*rng.uniform(.06,.45)
        x,y=rr*math.cos(a),rr*math.sin(a)
        h=rng.uniform(.22,.54)
        bend=rng.uniform(.035,.115)
        width=rng.uniform(.009,.022)
        tangent=Vector((-math.sin(a),math.cos(a),0))
        direction=Vector((math.cos(a),math.sin(a),0))
        base=Vector((x,y,0));mid=base+direction*bend*.4+Vector((0,0,h*.54))
        tip=base+direction*bend+Vector((0,0,h))
        color=(rng.uniform(.70,.97),rng.uniform(.86,1),rng.uniform(.61,.84),1)
        u0,v0,u1,v1=uv_rect(6)
        left=vertex(m,base-tangent*width,(u0,v1),color)
        right=vertex(m,base+tangent*width,(u1,v1),color)
        shoulder=vertex(m,mid+tangent*width*.55,(u1,v0+.27*(v1-v0)),color)
        shoulder_l=vertex(m,mid-tangent*width*.55,(u0,v0+.27*(v1-v0)),color)
        apex=vertex(m,tip,((u0+u1)/2,v0),color)
        m.faces.extend(((left,right,shoulder),(left,shoulder,shoulder_l),
                        (shoulder_l,shoulder,apex)))
    return m

def shrub(name,seed):
    rng=random.Random(seed)
    m=Model(name,"cutout","Branching small-leaf shrub with layered photographic sprays")
    tube(m,[(0,0,0),(0,0,.26),(0,0,.46)],[.06,.04,.015],8,cell=8)
    for j in range(21):
        a=j*2.399963+rng.uniform(-.18,.18)
        rr=rng.uniform(.24,.56)
        tip=Vector((rr*math.cos(a),rr*math.sin(a),rng.uniform(.30,.82)))
        tube(m,[(0,0,.20),tip*.54+Vector((0,0,.12)),tip],[.035,.021,.007],6,cell=8)
        for k in range(18):
            center=tip+Vector((rng.uniform(-.16,.16),rng.uniform(-.16,.16),
                               rng.uniform(-.15,.13)))
            spray(m,center,rng.uniform(.075,.17),rng.uniform(.08,.19),
                  rng.choice((1,2,5,13)),rng,
                  (rng.uniform(.72,.96),rng.uniform(.81,1),rng.uniform(.68,.91),1))
    return m

def petal(m,center,along,across,length,width,cell,col=(1,1,1,1)):
    center=Vector(center);along=Vector(along).normalized();across=Vector(across).normalized()
    base=center;tip=center+along*length
    mid=base+along*length*.56
    a,b,c,d=(base,mid-across*width*.5,tip,mid+across*width*.5)
    u0,v0,u1,v1=uv_rect(cell)
    petal_uv=(((u0+u1)/2,v1),(u0,(v0+v1)/2),
              ((u0+u1)/2,v0),(u1,(v0+v1)/2))
    ids=[vertex(m,p,uv,col) for p,uv in zip((a,b,c,d),petal_uv)]
    m.faces.extend(((ids[0],ids[1],ids[2]),(ids[0],ids[2],ids[3])))

def flower_patch(name,seed):
    rng=random.Random(seed)
    m=Model(name,"cutout","Separate stems and modelled wildflower petals")
    variant=name.rsplit("_",1)[-1]
    cell={"white":9,"yellow":10,"purple":11}[variant]
    n=8 if variant=="purple" else 6
    for i in range(n):
        a=i*2.399963+rng.uniform(-.22,.22)
        r=rng.uniform(.06,.39)
        x,y=r*math.cos(a),r*math.sin(a)
        h=rng.uniform(.28,.58)
        tube(m,[(x,y,0),(x+.018,y,h*.56),(x+.027,y+.01,h)],
             [.012,.008,.004],5,cell=6,col=(.68,.84,.60,1))
        top=Vector((x+.027,y+.01,h))
        if variant=="purple":
            for ring in range(4):
                c=top-Vector((0,0,ring*.055))
                for k in range(5):
                    b=k*math.tau/5+ring*.45
                    petal(m,c,Vector((math.cos(b),math.sin(b),.18)),
                          Vector((-math.sin(b),math.cos(b),0)),.053,.025,cell)
        else:
            for k in range(7 if variant=="white" else 6):
                b=k*math.tau/(7 if variant=="white" else 6)
                petal(m,top,Vector((math.cos(b),math.sin(b),.14)),
                      Vector((-math.sin(b),math.cos(b),0)),
                      .065 if variant=="white" else .058,.027,cell)
            tube(m,[top,top+Vector((0,0,.025))],[.022,.019],7,
                 cell=10, col=(.96,.83,.33,1))
    return m

def pebble_patch(name,seed):
    rng=random.Random(seed)
    m=Model(name,"opaque","Low irregular gravel and pebble cluster for path contact shadows")
    for i in range(11 if name.endswith("a") else 8):
        a=i*2.399963+rng.uniform(-.28,.28)
        rr=math.sqrt((i+.3)/11)*rng.uniform(.06,.41)
        x,y=rr*math.cos(a),rr*math.sin(a)
        r=rng.uniform(.035,.105)
        m.boulder((x,y,.008),r,cell=7,seed=seed+i,
                  colors=(rng.uniform(.83,1),rng.uniform(.79,.96),rng.uniform(.72,.91),1))
    return m

models=[oak("hero_oak_a",401),oak("hero_oak_b",402,.22),
        grass_patch("hero_grass_a",501),grass_patch("hero_grass_b",502),
        shrub("hero_shrub_a",601),shrub("hero_shrub_b",602),
        flower_patch("hero_flower_white",701),
        flower_patch("hero_flower_yellow",702),
        flower_patch("hero_flower_purple",703),
        pebble_patch("path_pebbles_a",801),
        pebble_patch("path_pebbles_b",802),
        pebble_patch("path_pebbles_c",803)]
for model in models:
    bottom=min(v[2] for v in model.vertices)
    if abs(bottom)>1e-8:
        model.vertices=[(x,y,z-bottom) for x,y,z in model.vertices]

bpy.ops.object.select_all(action="SELECT")
bpy.ops.object.delete(use_global=False)

def material(name,masked):
    mat=bpy.data.materials.new(name);mat.use_nodes=True
    nodes=mat.node_tree.nodes;links=mat.node_tree.links
    bsdf=nodes.get("Principled BSDF")
    uv=nodes.new("ShaderNodeUVMap");uv.uv_map="UVMap"
    tex=nodes.new("ShaderNodeTexImage");tex.image=bpy.data.images.load(str(ATLAS),check_existing=True)
    links.new(uv.outputs["UV"],tex.inputs["Vector"])
    vc=nodes.new("ShaderNodeVertexColor");vc.layer_name="Color"
    mul=nodes.new("ShaderNodeMixRGB");mul.blend_type="MULTIPLY";mul.inputs[0].default_value=1
    links.new(tex.outputs["Color"],mul.inputs[1]);links.new(vc.outputs["Color"],mul.inputs[2])
    links.new(mul.outputs["Color"],bsdf.inputs["Base Color"])
    ntex=nodes.new("ShaderNodeTexImage");ntex.image=bpy.data.images.load(str(NORMAL),check_existing=True)
    ntex.image.colorspace_settings.name="Non-Color"
    links.new(uv.outputs["UV"],ntex.inputs["Vector"])
    nmap=nodes.new("ShaderNodeNormalMap");nmap.inputs["Strength"].default_value=.7
    links.new(ntex.outputs["Color"],nmap.inputs["Color"])
    links.new(nmap.outputs["Normal"],bsdf.inputs["Normal"])
    otex=nodes.new("ShaderNodeTexImage");otex.image=bpy.data.images.load(str(ORM),check_existing=True)
    otex.image.colorspace_settings.name="Non-Color"
    links.new(uv.outputs["UV"],otex.inputs["Vector"])
    split=nodes.new("ShaderNodeSeparateColor");links.new(otex.outputs["Color"],split.inputs["Color"])
    links.new(split.outputs["Green"],bsdf.inputs["Roughness"])
    links.new(split.outputs["Blue"],bsdf.inputs["Metallic"])
    if masked:
        links.new(tex.outputs["Alpha"],bsdf.inputs["Alpha"])
        mat.surface_render_method="DITHERED"
        mat.use_backface_culling=False
    return mat

materials={"cutout":material("Forest Hero | leaves and flora",True),
           "opaque":material("Forest Hero | path stones",False)}
objects=[m.mesh_object(materials) for m in models]
bpy.ops.object.select_all(action="DESELECT")
for o in objects:o.select_set(True)
bpy.context.view_layer.objects.active=objects[0]
kit=OUT/"hero_forest_kit.glb"
bpy.ops.export_scene.gltf(filepath=str(kit),export_format="GLB",use_selection=True,
                          export_yup=True,export_apply=False,export_tangents=True,
                          export_vertex_color="NAME",export_vertex_color_name="Color",
                          export_all_vertex_colors=False)
doc,binary=glb_doc(kit)
prims={n["name"]:doc["meshes"][n["mesh"]]["primitives"][0]
       for n in doc["nodes"] if "mesh" in n and "name" in n}
manifest={"schema":"warbell.forest_slice_hero_manifest.v1","kit_glb":kit.name,
          "models":[],"sources":{},"outputs":{}}
for m in models:
    p=prims[m.name];attrs=p["attributes"]
    for required in ("POSITION","NORMAL","TEXCOORD_0","COLOR_0","TANGENT"):
        if required not in attrs:raise RuntimeError(f"{m.name} lacks {required}")
    positions=accessor(doc,binary,attrs["POSITION"])
    normals=accessor(doc,binary,attrs["NORMAL"])
    uvs=accessor(doc,binary,attrs["TEXCOORD_0"])
    colors=accessor(doc,binary,attrs["COLOR_0"])
    tangents=accessor(doc,binary,attrs["TANGENT"])
    indices=[int(v) for v in accessor(doc,binary,p["indices"])]
    bounds={"min":[min(v[k] for v in positions) for k in range(3)],
            "max":[max(v[k] for v in positions) for k in range(3)]}
    data={"schema":"warbell.forest_slice_tree.v1","name":m.name,
          "material":m.material,"atlas":"hero_tree_atlas.png",
          "normal_atlas":"hero_tree_normal.png","orm_atlas":"hero_tree_orm.png",
          "positions":positions,"normals":normals,"uvs":uvs,
          "colors":colors,"tangents":tangents,"indices":indices,
          "bounds":bounds,"triangles":len(indices)//3}
    (OUT/(m.name+".json")).write_text(json.dumps(data,separators=(",",":")),encoding="utf-8")
    manifest["models"].append({"name":m.name,"material":m.material,
                               "triangles":data["triangles"],"bounds":bounds})

for image in doc["images"]:
    view=doc["bufferViews"][image["bufferView"]]
    payload=binary[view.get("byteOffset",0):view.get("byteOffset",0)+view["byteLength"]]
    expected=OUT/(image["name"]+".png")
    if expected.exists() and hashlib.sha256(payload).digest()!=hashlib.sha256(expected.read_bytes()).digest():
        raise RuntimeError(f"GLB embedded atlas differs: {image['name']}")

bpy.context.preferences.filepaths.save_version=0
bpy.ops.wm.save_as_mainfile(filepath=str(ART/"hero_models.blend"))
for p in [ATLAS,NORMAL,ORM,kit,ART/"hero_models.blend"]+[OUT/(m.name+".json") for m in models]:
    manifest["outputs"][str(p.relative_to(ROOT))]=hashlib.sha256(p.read_bytes()).hexdigest()
for p in [Path(__file__),ART/"bake_hero_atlas.py",
          ROOT/"assets/textures/leaves/oakL.png"]:
    manifest["sources"][str(p.relative_to(ROOT))]=hashlib.sha256(p.read_bytes()).hexdigest()
(ART/"hero_manifest.json").write_text(json.dumps(manifest,indent=2),encoding="utf-8")
print("WARBELL_FOREST_HERO_KIT",json.dumps({"models":len(models),
     "triangles":sum(x["triangles"] for x in manifest["models"]),
     "oaks":[x for x in manifest["models"] if x["name"].startswith("hero_oak")]}))
