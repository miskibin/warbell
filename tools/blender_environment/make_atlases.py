"""Generate Warbell campaign atlases and terrain textures inside Blender MCP.

Execute through mcp_client.py, not blender -b.  The use of Blender's image
datablocks makes the saved .blend a fully editable material source. Meadow
grass and campaign path use preserved task-specific imagegen source PNGs;
forest understory uses the CC0 Poly Haven leafy_grass PBR triplet. A second
CC0 dark path PBR triplet is retained as an optional alternative. Other
materials are procedural.
"""
from pathlib import Path
import bpy
import numpy as np
import json
import math
import hashlib

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "assets/models/blender_environment"
ART = ROOT / "art/blender_environment"
OUT.mkdir(parents=True, exist_ok=True)
ART.mkdir(parents=True, exist_ok=True)
RNG = np.random.default_rng(241008)
N = 1024
S = N // 4

SURFACE = [
    "limestone", "fieldstone", "plaster", "plank",
    "beam", "clay_roof", "slate", "thatch",
    "cobble", "earth", "straw", "iron",
    "bronze", "granite", "burgundy_cloth", "wattle",
]
VEGETATION = [
    "leaf", "grass", "fern", "shrub",
    "flower", "reed", "snow_twig", "crop",
    "oak_bark", "pine_bark", "birch_bark", "snow_needles",
    "wood", "stone", "bone", "mud",
]

def smooth_noise(n, cells, seed):
    rng = np.random.default_rng(seed)
    field = rng.random((cells, cells), dtype=np.float32)
    # Bilinear sampling of a wrapped control grid is inherently tileable.
    q = np.arange(n, dtype=np.float32) * cells / n
    lo = q.astype(np.int32) % cells
    hi = (lo + 1) % cells
    t = q - np.floor(q)
    t = t*t*(3-2*t)
    a = field[lo[:,None],lo[None,:]]
    b = field[hi[:,None],lo[None,:]]
    c = field[lo[:,None],hi[None,:]]
    d = field[hi[:,None],hi[None,:]]
    return ((a*(1-t[:,None])+b*t[:,None])*(1-t[None,:])
            +(c*(1-t[:,None])+d*t[:,None])*t[None,:])

def save_rgba(name, rgba, colorspace="sRGB"):
    image = bpy.data.images.get(name)
    if image: bpy.data.images.remove(image)
    image = bpy.data.images.new(name, width=rgba.shape[1], height=rgba.shape[0],
                                alpha=True, float_buffer=False)
    image.colorspace_settings.name = colorspace
    flat=np.flipud(rgba).astype(np.float32).ravel()
    image.pixels.foreach_set(flat)
    image.update()
    image.filepath_raw = str(OUT / name)
    image.file_format = "PNG"
    image.save()
    image.pack()
    return image

yy, xx = np.mgrid[0:S,0:S].astype(np.float32)
fine = RNG.normal(0,1,(S,S)).astype(np.float32)
large = smooth_noise(S,9,8201)-.5
medium = smooth_noise(S,31,8202)-.5
grain = .065*large+.022*medium+.012*fine
surface = np.ones((N,N,4),dtype=np.float32)
orm = np.zeros_like(surface); orm[...,0]=1; orm[...,3]=1
emissive = np.zeros_like(surface); emissive[...,3]=1

def put_surface(i, rgb, relief=0, rough=.9, metal=0, light=0):
    r,c=divmod(i,4)
    y0,x0=r*S,c*S
    sw = np.clip(np.asarray(rgb,dtype=np.float32)[None,None,:] *
                 (1+grain[...,None]+relief[...,None]),0,1)
    surface[y0:y0+S,x0:x0+S,:3]=sw
    orm[y0:y0+S,x0:x0+S,1]=np.clip(rough+grain*.38,0,1)
    orm[y0:y0+S,x0:x0+S,2]=metal
    orm[y0:y0+S,x0:x0+S,0]=np.clip(1+relief*.23,0.7,1)
    if light: emissive[y0:y0+S,x0:x0+S,:3]=sw*light

def courses(block_w=54,block_h=35,mortar=.08,offset=True):
    row=(yy//block_h).astype(np.int32)
    bx=np.mod(xx+(row%2)*(block_w//2 if offset else 0),block_w)
    by=np.mod(yy,block_h)
    joint=(bx<2)|(by<2)
    edge=(bx<5)|(by<5)
    chip=np.abs(medium)>.26
    return np.where(joint,-mortar,np.where(edge,-.04,0))-.025*chip

put_surface(0,(.61,.60,.53),courses(48,35,.35),.86)
put_surface(1,(.32,.34,.33),courses(40,30,.28)+.13*large,.94)
put_surface(2,(.73,.70,.60),.14*large-.11*(np.abs(medium)>.27),.92)
plank_joint=(np.mod(xx,35)<3)|(np.mod(yy,125)<3)
put_surface(3,(.43,.28,.17),.09*np.sin(xx*.19)+.09*large-.3*plank_joint,.82)
put_surface(4,(.29,.17,.10),.12*np.sin(xx*.28)+.12*large,.84)
roof_joint=(np.mod(yy,28)<3)|(np.mod(xx+(yy//28%2)*22,44)<3)
put_surface(5,(.53,.25,.18),.1*large-.28*roof_joint,.79)
slate_joint=(np.mod(yy,25)<2)|(np.mod(xx+(yy//25%2)*25,50)<2)
put_surface(6,(.30,.33,.36),.08*large-.21*slate_joint,.75)
put_surface(7,(.62,.52,.30),.09*np.sin(xx*.6)+.09*large-.08*(np.mod(yy,31)<3),.98)
stone=((np.sin(xx*.105+large*8)*np.sin(yy*.12+medium*12))>.06)
put_surface(8,(.46,.44,.39),.12*large-.23*(~stone),.95)
put_surface(9,(.40,.31,.23),.14*large+.06*medium,.98)
put_surface(10,(.65,.55,.33),.11*np.sin(xx*.44)+.09*large,.98)
put_surface(11,(.23,.26,.27),.09*large-.04*np.abs(medium),.56,1)
put_surface(12,(.53,.36,.17),.12*large,.43,1)
put_surface(13,(.40,.42,.43),.11*large+.06*fine,.91)
put_surface(14,(.39,.15,.17),.12*large+.05*np.sin(yy*.07),.96)
put_surface(15,(.57,.45,.30),.14*np.sin(xx*.16)*np.sin(yy*.17)+.09*large,.91)

save_rgba("surface_atlas.png",surface)
save_rgba("surface_orm.png",orm,"Non-Color")
save_rgba("surface_emissive.png",emissive)

# A compact hand-designed foliage/crop cutout sheet.  Every cell has a fully
# transparent border and individual lobes/needles rather than opaque rectangles.
vegetation=np.zeros((N,N,4),dtype=np.float32)
for k,name in enumerate(VEGETATION):
    y0,x0=divmod(k,4)[0]*S,divmod(k,4)[1]*S
    X=(xx-S*.5)/(S*.5); Y=(yy-S*.5)/(S*.5)
    alpha=np.zeros((S,S),dtype=np.float32)
    tint=np.zeros((S,S,3),dtype=np.float32)
    if name=="shrub":
        # One card carries a *sprig*: fine stems and numerous distinct leaves.
        # A large leaf silhouette here became cactus-like at game camera scale.
        rng=np.random.default_rng(772)
        tint[:]=(.34,.54,.29)
        for branch in range(9):
            a=branch*2*np.pi/9+.18*rng.uniform(-1,1)
            ca,sa=np.cos(a),np.sin(a)
            along=X*ca+Y*sa
            across=-X*sa+Y*ca
            stem=(along>.02)&(along<.76)&(np.abs(across)<.009)
            alpha[stem]=1
            tint[stem]=(.28,.33,.20)
            for j in range(13):
                t=.12+j*.047
                side=1 if j%2 else -1
                offset=side*(.045+.018*(j%3))
                cx=t*ca-offset*sa
                cy=t*sa+offset*ca
                angle=a+side*(.65+.16*rng.random())
                ll=np.cos(angle);ls=np.sin(angle)
                U=(X-cx)*ll+(Y-cy)*ls
                V=-(X-cx)*ls+(Y-cy)*ll
                leaf=(U/.075)**2+(V/.031)**2<1
                alpha[leaf]=1
                shade=rng.uniform(.78,1.13)
                tint[leaf]=np.clip(np.array((.46,.70,.39))*shade,0,1)
    elif name in ("leaf","snow_twig"):
        # Fractal branch silhouette made of many small, rotated elliptic leaves.
        rng=np.random.default_rng(770+k)
        total={"leaf":55,"snow_twig":27}[name]
        for j in range(total):
            a=j*2.39996+rng.uniform(-.2,.2)
            radius=np.sqrt((j+.3)/(total+3))*.79
            cx,cy=radius*np.cos(a),radius*np.sin(a)
            angle=a+rng.uniform(-.9,.9)
            ca,sa=np.cos(angle),np.sin(angle)
            U=(X-cx)*ca+(Y-cy)*sa
            V=-(X-cx)*sa+(Y-cy)*ca
            leaf=(U/.115)**2+(V/.040)**2<1
            alpha=np.maximum(alpha,leaf.astype(np.float32))
        palette={"leaf":(.71,.79,.54),"snow_twig":(.63,.66,.58)}
        tint[:]=palette[name]
    elif name in ("grass","reed","crop"):
        for j in range(22 if name!="reed" else 12):
            xbase=-.85+j*(1.7/(21 if name!="reed" else 11))
            sway=.23*np.sin(j*2.8)
            height=.60+.32*((j*13)%17)/17
            width=.045 if name!="reed" else .03
            blade=(Y>-.94)&(Y<-.94+height*1.9)&(
                np.abs(X-xbase-sway*(Y+.94)/1.9)<width*(1-(Y+.94)/(height*1.9)))
            alpha=np.maximum(alpha,blade.astype(np.float32))
            if name=="reed":
                head=((X-xbase-sway*.7)/.045)**2+((Y-(-.94+height*1.8))/.12)**2<1
                alpha=np.maximum(alpha,head.astype(np.float32))
        tint[:]=(.50,.72,.35) if name!="reed" else (.51,.62,.38)
    elif name=="fern":
        for j in range(8):
            y=-.88+j*.22
            span=.75*(1-j/10)
            for side in (-1,1):
                cx=side*span*.57
                stripe=np.abs(Y-y-.25*np.abs(X))<.033
                alpha=np.maximum(alpha,((X*side>0)&(np.abs(X)<span)&stripe).astype(np.float32))
        alpha=np.maximum(alpha,((np.abs(X)<.023)&(Y>-.94)&(Y<.88)).astype(np.float32))
        tint[:]=(.44,.71,.36)
    elif name=="flower":
        stem=(np.abs(X)<.025)&(Y<.26)&(Y>-.9)
        alpha[stem]=1
        for a in np.linspace(0,2*np.pi,7,endpoint=False):
            cx,cy=.28*np.cos(a),.48+.28*np.sin(a)
            alpha=np.maximum(alpha,((((X-cx)/.17)**2+((Y-cy)/.12)**2)<1).astype(np.float32))
        tint[:]=(.93,.91,.86)
        tint[stem]=(.48,.67,.32)
    elif name in ("oak_bark","pine_bark","birch_bark","wood","stone","bone","mud"):
        alpha[:]=1
        base={
            "oak_bark":(.30,.22,.17),"pine_bark":(.38,.27,.18),
            "birch_bark":(.75,.74,.66),"wood":(.38,.26,.17),
            "stone":(.43,.43,.39),"bone":(.72,.68,.55),"mud":(.31,.29,.25),
        }[name]
        tint[:]=base
        if "bark" in name or name=="wood":
            tint*=np.clip(1+.15*np.sin(xx*.24)+.08*large,0,1.2)[...,None]
        if name=="birch_bark":
            stripe=(np.sin(yy*.61+medium*12)>.79)&(np.sin(xx*.19)<.15)
            tint[stripe]*=.35
    elif name=="snow_needles":
        for j in range(58):
            a=j*2.39996
            radius=np.sqrt((j+.3)/59)*.80
            cx,cy=radius*np.cos(a),radius*np.sin(a)
            U=(X-cx)*np.cos(a)+(Y-cy)*np.sin(a)
            V=-(X-cx)*np.sin(a)+(Y-cy)*np.cos(a)
            alpha=np.maximum(alpha,(((U/.16)**2+(V/.035)**2)<1).astype(np.float32))
        tint[:]=(.87,.92,.97)
    # Blender/exported cards map their upper edge to the top of the PNG cell.
    # Botanical drawing coordinates above run downward, so orient the tuft
    # base at the bottom and blossoms/reed heads at the top before baking.
    if name in ("grass","reed","crop","fern","flower"):
        alpha=np.flipud(alpha).copy()
        tint=np.flipud(tint).copy()
    var=.75+.25*smooth_noise(S,22,100+k)
    rgb=np.clip(tint*var[...,None],0,1)
    vegetation[y0:y0+S,x0:x0+S,:3]=rgb
    vegetation[y0:y0+S,x0:x0+S,3]=alpha
def photographic_grass_tile():
    """Crop one complete CC0 grass tussock, retaining its photographic alpha."""
    source=ART/"polyhaven/grass_medium_01"
    def pixels(filename):
        image=bpy.data.images.load(str(source/filename),check_existing=False)
        image.colorspace_settings.name="Non-Color"
        width,height=image.size
        values=np.empty((width*height*4,),dtype=np.float32)
        image.pixels.foreach_get(values)
        return np.flipud(values.reshape(height,width,4)).copy()
    diffuse=pixels("grass_medium_01_diff_1k.png")
    opacity=pixels("grass_medium_01_alpha_1k.png")
    x0,y0,x1,y1=193,786,480,915
    colour=diffuse[y0:y1,x0:x1,:3]
    mask=opacity[y0:y1,x0:x1,0]
    # Moderate spring-green grade of the captured olive grass, no invented
    # blade pattern. The alpha source retains the fine irregular blade edges.
    colour=np.clip(colour*np.array([1.14,1.38,1.37],dtype=np.float32)
                   +np.array([.015,.026,.007],dtype=np.float32),0,1)
    h,w=184,222
    xi=np.rint(np.linspace(0,colour.shape[1]-1,w)).astype(np.int32)
    yi=np.rint(np.linspace(0,colour.shape[0]-1,h)).astype(np.int32)
    tile=np.zeros((S,S,4),dtype=np.float32)
    tile[57:57+h,17:17+w,:3]=colour[yi[:,None],xi[None,:]]
    tile[57:57+h,17:17+w,3]=mask[yi[:,None],xi[None,:]]
    return tile

vegetation[0:S,S:2*S]=photographic_grass_tile()
save_rgba("vegetation_atlas.png",vegetation)

GROUND={
    "grass":((.34,.43,.25),.19,.90),
    "forest":((.29,.34,.24),.20,.94),
    "rocky":((.38,.39,.36),.13,.88),
    "swamp":((.27,.31,.25),.17,.95),
    "desert":((.65,.53,.34),.10,.98),
    "snow":((.75,.79,.79),.09,.83),
    "path":((.43,.34,.24),.15,.98),
    "cobble":((.44,.44,.39),.12,.91),
}

PHOTO_NAMES={"grass","path"}
PHOTO_INPUTS={"grass":ART/"imagegen"/"grass-albedo-v2.png",
              "path":ART/"imagegen"/"path-albedo-v2.png"}
missing_photo=[name for name,path in PHOTO_INPUTS.items() if not path.exists()]
if missing_photo:
    raise FileNotFoundError(f"Required imagegen ground sources missing: {missing_photo}")

def photo_ground(name,n,roughness):
    """Import photographic albedo faithfully, with only narrow periodic edge matching."""
    path=PHOTO_INPUTS[name]
    img=bpy.data.images.load(str(path),check_existing=False)
    img.scale(n,n)  # Blender's color-managed downsample, original PNG stays intact in art/.
    rgba=np.empty((n*n*4,),dtype=np.float32)
    img.pixels.foreach_get(rgba)
    img.pack()
    rgb=np.flipud(rgba.reshape(n,n,4))[...,:3].copy()
    # The sources are designed to tile. Make the final opposite-edge pixels exact
    # while changing only a 24-pixel border; no procedural colour/noise overlays.
    strip=24
    for k in range(strip):
        weight=.5*(1-k/strip)
        first=rgb[:,k,:].copy();last=rgb[:,-1-k,:].copy()
        rgb[:,k,:]=first+(last-first)*weight
        rgb[:,-1-k,:]=last+(first-last)*weight
    for k in range(strip):
        weight=.5*(1-k/strip)
        first=rgb[k,:,:].copy();last=rgb[-1-k,:,:].copy()
        rgb[k,:,:]=first+(last-first)*weight
        rgb[-1-k,:,:]=last+(first-last)*weight
    rgba_out=np.ones((n,n,4),dtype=np.float32)
    rgba_out[...,:3]=np.clip(rgb,0,1)
    save_rgba(f"ground_{name}_albedo.png",rgba_out)

    # Color edges are only a weak proxy for relief. Blur before differentiation
    # and keep the normal perturbation subtle to avoid making painted shadows bumpy.
    lum=.2126*rgb[...,0]+.7152*rgb[...,1]+.0722*rgb[...,2]
    height=(lum+np.roll(lum,1,0)+np.roll(lum,-1,0)
            +np.roll(lum,1,1)+np.roll(lum,-1,1))/5
    gx=np.roll(height,-1,1)-np.roll(height,1,1)
    gy=np.roll(height,-1,0)-np.roll(height,1,0)
    normal=np.stack((-gx*1.8,-gy*1.8,np.ones_like(gx)),axis=2)
    normal/=np.linalg.norm(normal,axis=2)[...,None]
    normal_out=np.ones_like(rgba_out)
    normal_out[...,:3]=normal*.5+.5
    save_rgba(f"ground_{name}_normal.png",normal_out,"Non-Color")
    rough_out=np.ones_like(rgba_out)
    rough_out[...,:3]=np.clip(roughness+.08*(lum.mean()-lum),.76,1)[...,None]
    save_rgba(f"ground_{name}_roughness.png",rough_out,"Non-Color")
    return {"source":str(path.relative_to(ROOT)).replace("\\","/"),
            "sha256":hashlib.sha256(path.read_bytes()).hexdigest(),
            "edge_mean_rgb_255":{"x":float(np.abs(rgb[:,0]-rgb[:,-1]).mean()*255),
                                 "y":float(np.abs(rgb[0]-rgb[-1]).mean()*255)}}

POLY_ROOT=ART/"polyhaven"
POLY_DOWNLOADS=json.loads((POLY_ROOT/"downloads.json").read_text(encoding="utf-8"))
POLY_RECORDS={(item["asset"],item["channel"]):item for item in POLY_DOWNLOADS}

def pbr_ground(name,asset,n=512):
    """Blender-imported CC0 diffuse/normal/roughness triplet, preserving PBR channels."""
    channels={"Diffuse":"albedo","nor_gl":"normal","Rough":"roughness"}
    source_info={"asset":asset,"source_page":POLY_RECORDS[(asset,"Diffuse")]["source_page"],
                 "license":"CC0-1.0","channels":{}}
    for channel,suffix in channels.items():
        record=POLY_RECORDS[(asset,channel)]
        source=POLY_ROOT/asset/Path(record["source"]).name
        actual=hashlib.sha256(source.read_bytes()).hexdigest()
        if actual!=record["sha256"]:
            raise ValueError(f"Poly Haven source hash mismatch: {source}")
        img=bpy.data.images.load(str(source),check_existing=False)
        # Read encoded PNG channel values verbatim. Blender's sRGB pixel API
        # otherwise linearises tagged Poly Haven images before our PNG writer.
        img.colorspace_settings.name="Non-Color"
        img.scale(n,n)
        values=np.empty((n*n*4,),dtype=np.float32)
        img.pixels.foreach_get(values)
        img.pack()
        data=np.flipud(values.reshape(n,n,4)).copy()
        # These textures are designed to tile; match only a narrow border so
        # the exported normal/roughness remain aligned with diffuse features.
        for axis in (1,0):
            for k in range(24):
                weight=.5*(1-k/24)
                lo=[slice(None)]*3;hi=[slice(None)]*3
                lo[axis]=k;hi[axis]=-1-k
                first=data[tuple(lo)].copy();last=data[tuple(hi)].copy()
                data[tuple(lo)]=first+(last-first)*weight
                data[tuple(hi)]=last+(first-last)*weight
        if channel=="nor_gl":
            vector=data[...,:3]*2-1
            vector/=np.linalg.norm(vector,axis=2)[...,None]
            data[...,:3]=vector*.5+.5
        elif channel=="Rough":
            # Source is a grayscale roughness map, not luminance from albedo.
            data[...,:3]=data[...,0,None]
        data[...,3]=1
        save_rgba(f"ground_{name}_{suffix}.png",data,
                  "sRGB" if channel=="Diffuse" else "Non-Color")
        source_info["channels"][suffix]={"source":record["source"],"sha256":actual}
    return source_info

photo_sources={}
pbr_sources={}
for ki,(name,(base,amp,rough)) in enumerate(GROUND.items()):
    n=512
    if name=="forest":
        pbr_sources[name]=pbr_ground(name,"leafy_grass",n)
        continue
    if name in PHOTO_NAMES:
        photo_sources[name]=photo_ground(name,n,rough)
        continue
    macro=smooth_noise(n,7,1500+ki)
    medium=smooth_noise(n,16,300+ki)
    fine=smooth_noise(n,51,500+ki)
    grit=smooth_noise(n,121,800+ki)
    detail=(.36*macro+.34*medium+.21*fine+.09*grit-.5)
    v,u=np.mgrid[0:n,0:n]
    color=np.broadcast_to(np.asarray(base,dtype=np.float32),(n,n,3)).copy()
    bright=np.clip((macro-.37)*2.5,0,1)
    dark=np.clip((.63-medium)*2.7,0,1)
    if name=="cobble":
        row=v//32
        ix=((u+(row%2)*16)//32)%16
        iy=row
        stone_value=((ix*73+iy*91+ix*iy*19)%37)/37.0
        color+=((stone_value-.5)*.20)[...,None]
        bx=(u+(row%2)*16)%32;by=v%32
        mortar=(by<3)|(by>=29)|(bx<3)|(bx>=29)|((bx<6)&(by<6))
        color[mortar]=(.20,.20,.18)
        detail-=.35*mortar
    elif name=="path":
        color+=bright[...,None]*np.array((.11,.10,.085))
        color-=dark[...,None]*np.array((.105,.085,.065))
        rut=np.sin(math.tau*6*u/n+macro*2.8)**2
        color-=rut[...,None]*np.array((.035,.033,.027))
        pebble=smooth_noise(n,63,1912)>.64
        color[pebble]+=(.115,.105,.085)
        detail+=.25*pebble-.11*rut
    elif name=="desert":
        detail+=.1*np.sin(math.tau*(8*u+2*v)/n)
    elif name=="forest":
        color+=bright[...,None]*np.array((.085,.12,.065))
        color-=dark[...,None]*np.array((.09,.10,.07))
        leaf=smooth_noise(n,54,1922)>.63
        color[leaf]+=(.09,.055,.012)
        detail-=.19*leaf
    elif name=="grass":
        color+=bright[...,None]*np.array((.075,.125,.045))
        color-=dark[...,None]*np.array((.10,.12,.065))
        blade=(smooth_noise(n,65,1934)>.67)&(
            np.sin(math.tau*(23*u+4*v)/n)>.35)
        color[blade]+=(.075,.13,.035)
        detail+=.20*blade
    elif name=="rocky":
        detail+=.10*np.sin(math.tau*(7*u+8*v)/n+smooth_noise(n,8,1935)*6)
    elif name=="snow":
        detail+=.09*np.sin(math.tau*(4*u+2*v)/n+smooth_noise(n,7,1936)*5)
    image=np.ones((n,n,4),dtype=np.float32)
    color+=amp*.20*detail[...,None]
    image[...,:3]=np.clip(color,0,1)
    save_rgba(f"ground_{name}_albedo.png",image)
    gx=np.roll(detail,-1,1)-np.roll(detail,1,1)
    gy=np.roll(detail,-1,0)-np.roll(detail,1,0)
    nrm=np.stack((-gx*3.8,-gy*3.8,np.ones_like(gx)),axis=2)
    nrm/=np.linalg.norm(nrm,axis=2)[...,None]
    normal=np.ones_like(image); normal[...,:3]=nrm*.5+.5
    save_rgba(f"ground_{name}_normal.png",normal,"Non-Color")
    rmap=np.ones_like(image); rmap[...,:3]=np.clip(rough+.08*detail[...,None],0,1)
    save_rgba(f"ground_{name}_roughness.png",rmap,"Non-Color")

# A second complete CC0 triplet is retained as an optional dark gravel/soil
# material. It does not replace the light campaign path in the main scene.
pbr_sources["stony_dirt_path_alternate"]=pbr_ground("stony_dirt_path","stony_dirt_path")

(ART/"texture_spec.json").write_text(json.dumps({
    "schema":"warbell.blender_environment_textures.v1",
    "surface_atlas":{"size":[N,N],"grid":[4,4],"cells":SURFACE,
                     "uv_origin":"top-left","orm_channels":{"R":"AO","G":"roughness","B":"metallic"},
                     "emissive":"restrained"},
    "vegetation_atlas":{"size":[N,N],"grid":[4,4],"cells":VEGETATION,
                         "alpha_cutoff":0.5},
    "ground":{"size":[512,512],"tileable":True,"tile_world_units":2.5,
              "names":list(GROUND),"normal":"OpenGL +Y",
              "imagegen_sources":photo_sources,"polyhaven_sources":pbr_sources}
},indent=2),encoding="utf-8")
print("WARBELL_ENV_TEXTURES_DONE",json.dumps({"surface":SURFACE,"vegetation":VEGETATION,
                                               "ground":list(GROUND)}))
