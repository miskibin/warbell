"""Geometry and export primitives for the original Warbell Blender environment kit.

Authoring coordinates are Blender Z-up.  Faces and UVs are explicit so the
runtime JSON can exactly mirror the exported glTF, including split normals.
"""
from pathlib import Path
import bpy
import math
import json
import struct
import hashlib
from mathutils import Vector

ROOT = Path(__file__).resolve().parents[2]
ASSET = ROOT / "assets/models/blender_environment"
ART = ROOT / "art/blender_environment"
ASSET.mkdir(parents=True, exist_ok=True)
ART.mkdir(parents=True, exist_ok=True)

SURFACE = {
    "limestone":0,"fieldstone":1,"plaster":2,"plank":3,
    "beam":4,"clay_roof":5,"slate":6,"thatch":7,
    "cobble":8,"earth":9,"straw":10,"iron":11,
    "bronze":12,"granite":13,"burgundy_cloth":14,"wattle":15,
    "bone":2,"mud":9,
}
VEGETATION = {
    "leaf":0,"grass":1,"fern":2,"shrub":3,
    "flower":4,"reed":5,"snow_twig":6,"crop":7,
    "oak_bark":8,"pine_bark":9,"birch_bark":10,"snow_needles":11,
    "wood":12,"stone":13,"bone":14,"mud":15,
}

def uv_rect(cell, pad=.018):
    row,col=divmod(cell,4)
    return ((col+pad)/4,(row+pad)/4,(col+1-pad)/4,(row+1-pad)/4)

def tint(rgb=(1,1,1),alpha=1):
    return tuple(rgb)+(alpha,)

class Model:
    def __init__(self,name,material="opaque",description=""):
        self.name=name
        self.material=material
        self.description=description
        self.vertices=[]
        self.uvs=[]
        self.colors=[]
        self.faces=[]
        self.card_normals={}

    def vertex(self,p,uv,color):
        self.vertices.append(tuple(float(v) for v in p))
        self.uvs.append(tuple(float(v) for v in uv))
        self.colors.append(tuple(float(v) for v in color))
        return len(self.vertices)-1

    def quad(self,a,b,c,d,cell,colors=(1,1,1,1),uv=None):
        if isinstance(cell,str):
            cell=(SURFACE if self.material=="opaque" else VEGETATION)[cell]
        rect=uv_rect(cell)
        if uv is None:
            u0,v0,u1,v1=rect
            uv=((u0,v1),(u1,v1),(u1,v0),(u0,v0))
        ids=[self.vertex(p,t,colors) for p,t in zip((a,b,c,d),uv)]
        self.faces.extend(((ids[0],ids[1],ids[2]),(ids[0],ids[2],ids[3])))

    def box(self,center,size,cell,colors=(1,1,1,1),top=None):
        x,y,z=center; w,d,h=size
        x0,x1=x-w/2,x+w/2; y0,y1=y-d/2,y+d/2; z0,z1=z-h/2,z+h/2
        # All surfaces have their own explicit atlas UVs and outward winding.
        def q(a,b,c,d,face_cell):
            self.quad(d,c,b,a,face_cell,colors)
        q((x0,y0,z0),(x0,y0,z1),(x1,y0,z1),(x1,y0,z0),cell)
        q((x1,y1,z0),(x1,y1,z1),(x0,y1,z1),(x0,y1,z0),cell)
        q((x0,y1,z0),(x0,y1,z1),(x0,y0,z1),(x0,y0,z0),cell)
        q((x1,y0,z0),(x1,y0,z1),(x1,y1,z1),(x1,y1,z0),cell)
        q((x0,y0,z1),(x0,y1,z1),(x1,y1,z1),(x1,y0,z1),top or cell)
        q((x0,y1,z0),(x0,y0,z0),(x1,y0,z0),(x1,y1,z0),cell)

    def beam(self,a,b,width,depth,cell,colors=(1,1,1,1)):
        a,b=Vector(a),Vector(b)
        axis=(b-a).normalized()
        guide=Vector((0,0,1)) if abs(axis.z)<.87 else Vector((0,1,0))
        u=axis.cross(guide).normalized()*width*.5
        v=axis.cross(u).normalized()*depth*.5
        near=(a-u-v,a+u-v,a+u+v,a-u+v)
        far=(b-u-v,b+u-v,b+u+v,b-u+v)
        for i in range(4):
            j=(i+1)%4
            self.quad(near[i],far[i],far[j],near[j],cell,colors)
        self.quad(near[3],near[2],near[1],near[0],cell,colors)
        self.quad(far[0],far[1],far[2],far[3],cell,colors)

    def cylinder(self,center,radius,height,cell,sides=10,top_radius=None,colors=(1,1,1,1)):
        x,y,z=center; rb=radius; rt=radius if top_radius is None else top_radius
        z0,z1=z-height*.5,z+height*.5
        for i in range(sides):
            a=math.tau*i/sides;b=math.tau*(i+1)/sides
            self.quad((x+rb*math.cos(a),y+rb*math.sin(a),z0),
                      (x+rb*math.cos(b),y+rb*math.sin(b),z0),
                      (x+rt*math.cos(b),y+rt*math.sin(b),z1),
                      (x+rt*math.cos(a),y+rt*math.sin(a),z1),cell,colors)
        for i in range(1,sides-1):
            a,b=math.tau*i/sides,math.tau*(i+1)/sides
            uv=uv_rect((SURFACE if self.material=="opaque" else VEGETATION)[cell])
            # Triangular caps via degenerate quads; texture spans the same cell.
            self.quad((x,y,z1),(x+rt*math.cos(a),y+rt*math.sin(a),z1),
                      (x+rt*math.cos(b),y+rt*math.sin(b),z1),(x,y,z1),cell,colors)
            self.quad((x,y,z0),(x+rb*math.cos(b),y+rb*math.sin(b),z0),
                      (x+rb*math.cos(a),y+rb*math.sin(a),z0),(x,y,z0),cell,colors)

    def card(self,center,width,height,cell,angle=0,colors=(1,1,1,1)):
        x,y,z=center
        dx=math.cos(angle)*width*.5;dy=math.sin(angle)*width*.5
        first=len(self.vertices)
        self.quad((x-dx,y-dy,z),(x+dx,y+dy,z),
                  (x+dx,y+dy,z+height),(x-dx,y-dy,z+height),cell,colors)
        # Foliage is a thin vertical plane, but its blades/leaves point mostly
        # skyward. Explicit split normals keep one side from becoming a dark
        # silhouette under Bevy's directional sun.
        normal=(.18*math.sin(angle),-.18*math.cos(angle),math.sqrt(1-.18*.18))
        for vertex in range(first,first+4):
            self.card_normals[vertex]=normal

    def boulder(self,center,radius,cell="granite",seed=0,colors=(1,1,1,1)):
        # Eight-sided, sculpted low-poly stone with a broad foot and uneven crown.
        x,y,z=center
        rings=[]
        for j,(level,rr) in enumerate(((0,.75),(.34,1),(.78,.80),(1.18,.30))):
            ring=[]
            for i in range(9):
                a=i*math.tau/9
                wobble=1+.13*math.sin(seed*1.79+i*2.31+j*.73)
                h=.08*math.sin(seed+i*3.2+j)
                ring.append((x+radius*rr*wobble*math.cos(a),
                             y+radius*rr*wobble*math.sin(a),
                             z+radius*(level+h)))
            rings.append(ring)
        for j in range(3):
            for i in range(9):
                k=(i+1)%9
                col=tuple(min(1,max(0,c*(.88+.10*math.sin(seed+i+j)))) for c in colors[:3])+(colors[3],)
                self.quad(rings[j][i],rings[j][k],rings[j+1][k],rings[j+1][i],cell,col)
        for i in range(9):
            self.quad(rings[-1][i],rings[-1][(i+1)%9],
                      (x,y,z+radius*1.18),(x,y,z+radius*1.18),cell,colors)

    def mesh_object(self,materials):
        mesh=bpy.data.meshes.new(self.name)
        mesh.from_pydata(self.vertices,[],self.faces)
        mesh.update()
        mesh.uv_layers.new(name="UVMap")
        mesh.color_attributes.new(name="Color",type="FLOAT_COLOR",domain="CORNER")
        uv_values=[];color_values=[];split_normals=[]
        for poly in mesh.polygons:
            poly.use_smooth=all(mesh.loops[li].vertex_index in self.card_normals
                                for li in poly.loop_indices)
            for li in poly.loop_indices:
                vi=mesh.loops[li].vertex_index
                uv_values.extend((self.uvs[vi][0],1-self.uvs[vi][1]))
                color_values.extend(self.colors[vi])
                split_normals.append(self.card_normals.get(vi,(0,0,0)))
        mesh.color_attributes["Color"].data.foreach_set("color",color_values)
        mesh.uv_layers["UVMap"].data.foreach_set("uv",uv_values)
        if self.card_normals:
            mesh.normals_split_custom_set(split_normals)
        mesh.update()
        mesh.materials.append(materials[self.material])
        obj=bpy.data.objects.new(self.name,mesh)
        bpy.context.collection.objects.link(obj)
        return obj

def material(name,image,orm=None,emissive=None,mask=False):
    existing=bpy.data.materials.get(name)
    if existing: bpy.data.materials.remove(existing)
    mat=bpy.data.materials.new(name)
    mat.use_nodes=True
    nodes=mat.node_tree.nodes;links=mat.node_tree.links
    bsdf=nodes.get("Principled BSDF")
    bsdf.inputs["Roughness"].default_value=.9
    uv=nodes.new("ShaderNodeUVMap");uv.uv_map="UVMap"
    color=nodes.new("ShaderNodeTexImage");color.image=image
    links.new(uv.outputs["UV"],color.inputs["Vector"])
    vcol=nodes.new("ShaderNodeVertexColor");vcol.layer_name="Color"
    mul=nodes.new("ShaderNodeMixRGB");mul.blend_type="MULTIPLY";mul.inputs[0].default_value=1
    links.new(color.outputs["Color"],mul.inputs[1])
    links.new(vcol.outputs["Color"],mul.inputs[2])
    links.new(mul.outputs["Color"],bsdf.inputs["Base Color"])
    if mask:
        clip=nodes.new("ShaderNodeMath");clip.operation="ROUND"
        links.new(color.outputs["Alpha"],clip.inputs[0])
        links.new(clip.outputs[0],bsdf.inputs["Alpha"])
        mat.surface_render_method="DITHERED"
        mat.use_backface_culling=False
    if orm is not None:
        tex=nodes.new("ShaderNodeTexImage");tex.image=orm
        links.new(uv.outputs["UV"],tex.inputs["Vector"])
        split=nodes.new("ShaderNodeSeparateColor");links.new(tex.outputs["Color"],split.inputs["Color"])
        links.new(split.outputs["Green"],bsdf.inputs["Roughness"])
        links.new(split.outputs["Blue"],bsdf.inputs["Metallic"])
    if emissive is not None:
        tex=nodes.new("ShaderNodeTexImage");tex.image=emissive
        links.new(uv.outputs["UV"],tex.inputs["Vector"])
        links.new(tex.outputs["Color"],bsdf.inputs["Emission Color"])
        bsdf.inputs["Emission Strength"].default_value=.35
    return mat

def load_materials():
    images={}
    for name in ("surface_atlas.png","surface_orm.png","surface_emissive.png",
                 "vegetation_atlas.png"):
        images[name]=bpy.data.images.load(str(ASSET/name),check_existing=True)
        images[name].reload()
    return {
        "opaque":material("Warbell Environment | opaque atlas",images["surface_atlas.png"],
                          images["surface_orm.png"],images["surface_emissive.png"]),
        "cutout":material("Warbell Environment | cutout vegetation",images["vegetation_atlas.png"],
                          mask=True),
    }

def glb_doc(path):
    data=path.read_bytes()
    jlen,jtag=struct.unpack_from("<I4s",data,12)
    assert jtag==b"JSON"
    doc=json.loads(data[20:20+jlen])
    binary_off=20+jlen
    binlen,bintag=struct.unpack_from("<I4s",data,binary_off)
    assert bintag==b"BIN\x00"
    return doc,data[binary_off+8:binary_off+8+binlen]

def accessor(doc,binary,idx):
    acc=doc["accessors"][idx]
    view=doc["bufferViews"][acc["bufferView"]]
    fmt={5126:"f",5121:"B",5123:"H",5125:"I"}[acc["componentType"]]
    count={"SCALAR":1,"VEC2":2,"VEC3":3,"VEC4":4}[acc["type"]]
    scalar_size=struct.calcsize("<"+fmt)
    stride=view.get("byteStride",count*scalar_size)
    off=view.get("byteOffset",0)+acc.get("byteOffset",0)
    norm={5121:255.0,5123:65535.0}.get(acc["componentType"],1.0) if acc.get("normalized") else 1.0
    values=[[v/norm for v in struct.unpack_from("<"+fmt*count,binary,off+i*stride)]
            for i in range(acc["count"])]
    return [v[0] for v in values] if count==1 else values

def export(models):
    materials=load_materials()
    for m in models:
        bottom=min(v[2] for v in m.vertices)
        if abs(bottom)>1e-7:
            m.vertices=[(x,y,z-bottom) for x,y,z in m.vertices]
    objects=[m.mesh_object(materials) for m in models]
    bpy.ops.object.select_all(action="DESELECT")
    for obj in objects: obj.select_set(True)
    bpy.context.view_layer.objects.active=objects[0]
    glb=ASSET/"environment_kit.glb"
    bpy.ops.export_scene.gltf(filepath=str(glb),export_format="GLB",
                              use_selection=True,export_apply=False,export_yup=True,
                              export_attributes=False,
                              export_vertex_color="NAME",export_vertex_color_name="Color",
                              export_all_vertex_colors=False)
    doc,binary=glb_doc(glb)
    meshes={x["name"]:doc["meshes"][x["mesh"]]["primitives"][0]
            for x in doc["nodes"] if "mesh" in x and "name" in x}
    manifest={"schema":"warbell.blender_environment_manifest.v1",
              "source":"Blender 4.5.9 MCP-authored meshes/atlases; imagegen meadow grass/path; CC0 Poly Haven forest PBR and optional dark path PBR",
              "license":"Project artwork plus CC0 Poly Haven textures; source and author credits in art/blender_environment/README.md",
              "kit_glb":glb.name,"models":[],"sha256":{},"source_sha256":{}}
    for m in models:
        prim=meshes[m.name]
        attrs=prim["attributes"]
        positions=accessor(doc,binary,attrs["POSITION"])
        normals=accessor(doc,binary,attrs["NORMAL"])
        uvs=accessor(doc,binary,attrs["TEXCOORD_0"])
        colors=accessor(doc,binary,attrs["COLOR_0"])
        indices=[int(i) for i in accessor(doc,binary,prim["indices"])]
        assert len(positions)==len(normals)==len(uvs)==len(colors)
        assert len(indices)%3==0
        assert abs(min(p[1] for p in positions))<1e-4,(m.name,min(p[1] for p in positions))
        bounds={"min":[min(p[i] for p in positions) for i in range(3)],
                "max":[max(p[i] for p in positions) for i in range(3)]}
        data={"schema":"warbell.blender_environment_mesh.v1","name":m.name,
              "atlas":"surface_atlas.png" if m.material=="opaque" else "vegetation_atlas.png",
              "material":m.material,"positions":positions,"normals":normals,
              "uvs":uvs,"colors":colors,"indices":indices,"bounds":bounds,
              "triangles":len(indices)//3}
        if m.material=="opaque":
            data["orm_atlas"]="surface_orm.png"
            data["emissive_atlas"]="surface_emissive.png"
        path=ASSET/(m.name+".json")
        path.write_text(json.dumps(data,separators=(",",":")),encoding="utf-8")
        manifest["models"].append({"name":m.name,"material":m.material,
                                   "description":m.description,"vertices":len(positions),
                                   "triangles":len(indices)//3,"bounds":bounds})
    # Studio layout is a copy of the kit meshes, offset only after export.
    for i,obj in enumerate(objects):
        obj.location=((i%9-4)*5.5,(i//9)*5.5,0)
    bpy.context.preferences.filepaths.save_version=0
    bpy.ops.wm.save_as_mainfile(filepath=str(ART/"warbell_environment.blend"))
    for path in list(ASSET.glob("*.png"))+list(ASSET.glob("*.json"))+[glb,ART/"warbell_environment.blend"]:
        manifest["sha256"][path.name]=hashlib.sha256(path.read_bytes()).hexdigest()
    for path in sorted((ROOT/"tools/blender_environment").glob("*.py")):
        manifest["source_sha256"][path.name]=hashlib.sha256(path.read_bytes()).hexdigest()
    manifest["source_sha256"]["texture_spec.json"]=hashlib.sha256(
        (ART/"texture_spec.json").read_bytes()).hexdigest()
    for path in sorted((ART/"imagegen").glob("*.png")):
        manifest["source_sha256"]["imagegen/"+path.name]=hashlib.sha256(path.read_bytes()).hexdigest()
    for path in sorted((ART/"polyhaven").rglob("*.png")):
        manifest["source_sha256"][str(path.relative_to(ART)).replace("\\","/")]=hashlib.sha256(path.read_bytes()).hexdigest()
    manifest["source_sha256"]["polyhaven/downloads.json"]=hashlib.sha256(
        (ART/"polyhaven/downloads.json").read_bytes()).hexdigest()
    (ART/"manifest.json").write_text(json.dumps(manifest,indent=2),encoding="utf-8")
    print("WARBELL_ENV_EXPORT_DONE",json.dumps({"models":len(models),
          "triangles":sum(x["triangles"] for x in manifest["models"]),
          "kit_mb":round(glb.stat().st_size/1048576,2)}))
