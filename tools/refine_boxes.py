"""Occlusion-aware proportion boxes (fixes: arms-in-chest, sword-in-leg).
Back view: split each row into runs -> torso = CENTRAL run, arms = OUTER runs,
legs = per-leg run. Side view: depth only where sword doesn't cross (head/torso/
pelvis/foot); leg depth := leg width (cylindrical). Output overlay + procedural spec."""
import cv2, numpy as np, json, os
SRC=r"C:\Users\skibi\Downloads\Gemini_Generated_Image_4znaze4znaze4zna (1).png"
OUT=r"D:\tileworld-bevy-forest\model_proportions"
PAN={"front":(28,331),"left":(366,508),"back":(587,824),"right":(900,1042)}
img=cv2.imread(SRC); rgb=cv2.cvtColor(img,cv2.COLOR_BGR2RGB).astype(np.int32)
bright=rgb.max(2); fg=(bright<235)|(rgb.max(2)-rgb.min(2)>18)
bx0,bx1=PAN["back"]; pf=fg[:,bx0:bx1]
sx0,sx1=PAN["left"]; sp=fg[:,sx0:sx1]
TOP,HEEL=183,785; HH=HEEL-TOP

def runs_of(row,gap=5):
    xs=np.where(row)[0]
    if len(xs)==0: return []
    cut=np.where(np.diff(xs)>gap)[0]
    return [(int(s[0]),int(s[-1])) for s in np.split(xs,cut+1)]

def central_run(ya,yb):
    """median width of the run nearest panel centre (torso/belt, excludes arms)"""
    cx=(bx1-bx0)/2; los,his=[],[]
    for y in range(ya,yb):
        rs=runs_of(pf[y])
        if not rs: continue
        r=min(rs,key=lambda r:abs((r[0]+r[1])/2-cx)); los.append(r[0]);his.append(r[1])
    if not los: return None
    return dict(x=bx0+int(np.median(los)),w=int(np.median(his))-int(np.median(los))+1)

def outer_run(ya,yb,side,min_runs=1):
    """one arm/leg: extreme run on given half. min_runs=3 -> only rows where the
    silhouette is cleanly armL|body|armR (avoids fused rows inflating arm width)."""
    cx=(bx1-bx0)/2; los,his=[],[]
    for y in range(ya,yb):
        allr=runs_of(pf[y])
        if len(allr)<min_runs: continue
        rs=[r for r in allr if ((r[0]+r[1])/2<cx)==(side=='L')]
        if not rs: continue
        r=(min(rs,key=lambda r:r[0]) if side=='L' else max(rs,key=lambda r:r[1]))
        los.append(r[0]);his.append(r[1])
    if not los: return None
    return dict(x=bx0+int(np.median(los)),w=int(np.median(his))-int(np.median(los))+1)

def side_depth(ya,yb):
    los,his=[],[]
    for y in range(ya,yb):
        xs=np.where(sp[y])[0]
        if len(xs): los.append(xs[0]);his.append(xs[-1])
    if not los: return None
    return dict(z=sx0+int(np.median(los)),d=int(np.median(his))-int(np.median(los))+1)

# bands (y px). arms 388 = where back silhouette splits torso|arms
B={"helm":(183,283),"pauldron":(283,388),"cuirass":(388,466),
   "pelvis":(466,528),"thigh":(528,633),"shin":(633,753),"foot":(753,785)}
ARM=(388,520)

ov=img.copy(); spec={"HH_px":HH,"parts":{}}
def emit(name,ya,yb,bb,depth,c,draw_side=True):
    if not bb: return
    cv2.rectangle(ov,(bb["x"],ya),(bb["x"]+bb["w"],yb),c,2)
    cv2.putText(ov,name,(bb["x"],ya+11),cv2.FONT_HERSHEY_SIMPLEX,0.32,c,1,cv2.LINE_AA)
    if depth and draw_side:
        cv2.rectangle(ov,(depth["z"],ya),(depth["z"]+depth["d"],yb),c,1)
    spec["parts"][name]=dict(y=round((ya-TOP)/HH,3),h=round((yb-ya)/HH,3),
        w=round(bb["w"]/HH,3),d=round((depth["d"]/HH) if depth else round(bb["w"]/HH,3),3))

C=dict(helm=(0,200,255),pauldron=(0,255,120),cuirass=(0,255,0),arm=(255,140,0),
       pelvis=(255,180,0),thigh=(255,0,180),shin=(180,0,255),foot=(0,128,255))
# torso column (central, clean of arms)
emit("helm",*B["helm"],central_run(*B["helm"]),side_depth(*B["helm"]),C["helm"])
emit("pauldron",*B["pauldron"],central_run(*B["pauldron"]),side_depth(*B["pauldron"]),C["pauldron"])
emit("cuirass",*B["cuirass"],central_run(*B["cuirass"]),side_depth(*B["cuirass"]),C["cuirass"])
emit("pelvis",*B["pelvis"],central_run(*B["pelvis"]),side_depth(*B["pelvis"]),C["pelvis"])
# arms (outer runs, both shown, one in spec)
aL=outer_run(*ARM,'L',min_runs=3); aR=outer_run(*ARM,'R',min_runs=3)
for a in (aL,aR):
    if a: cv2.rectangle(ov,(a["x"],ARM[0]),(a["x"]+a["w"],ARM[1]),C["arm"],2)
if aL: spec["parts"]["arm"]=dict(y=round((ARM[0]-TOP)/HH,3),h=round((ARM[1]-ARM[0])/HH,3),
        w=round(aL["w"]/HH,3),d=round(aL["w"]/HH,3))
# legs (per-leg width; depth := width since sword occludes side; foot depth real)
for nm in ("thigh","shin","foot"):
    ya,yb=B[nm]
    lw=outer_run(ya,yb,'L'); rw=outer_run(ya,yb,'R')
    dep = side_depth(ya,yb) if nm=="foot" else None   # foot clear of sword
    for r in (lw,rw):
        if r: cv2.rectangle(ov,(r["x"],ya),(r["x"]+r["w"],yb),C[nm],2)
    if lw: spec["parts"][nm]=dict(y=round((ya-TOP)/HH,3),h=round((yb-ya)/HH,3),
            w=round(lw["w"]/HH,3),d=round((dep["d"]/HH) if dep else round(lw["w"]/HH,3),3))
    if nm=="foot" and dep:  # draw foot depth on side
        cv2.rectangle(ov,(dep["z"],ya),(dep["z"]+dep["d"],yb),C[nm],1)

for yj in (283,388,466,528,633,753): cv2.line(ov,(bx0-8,yj),(sx1+8,yj),(110,110,110),1)
cv2.imwrite(os.path.join(OUT,"overlay_refine.png"),ov)
json.dump(spec,open(os.path.join(OUT,"spec.json"),"w"),indent=2)
print(json.dumps(spec,indent=2))
