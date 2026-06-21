"""Snapped proportion boxes: per anatomical band, box = ACTUAL foreground pixel
extent (not synthetic centered). Back panel = XY, side panel = Z(depth).
Legs split into 2 runs, arms boxed separately. Joints read from measured profile.
Output: overlay that hugs the figure + parametric JSON (fractions of HH) for a
procedural box builder."""
import cv2, numpy as np, json, os
SRC=r"C:\Users\skibi\Downloads\Gemini_Generated_Image_4znaze4znaze4zna (1).png"
OUT=r"D:\tileworld-bevy-forest\model_proportions"
PAN={"front":(28,331),"left":(366,508),"back":(587,824),"right":(900,1042)}
img=cv2.imread(SRC); rgb=cv2.cvtColor(img,cv2.COLOR_BGR2RGB).astype(np.int32)
bright=rgb.max(2); fg=(bright<235)|(rgb.max(2)-rgb.min(2)>18)
bx0,bx1=PAN["back"]; pf=fg[:,bx0:bx1]
sx0,sx1=PAN["left"]; sp=fg[:,sx0:sx1]
TOP,HEEL=183,785; HH=HEEL-TOP
J={"helm_top":183,"neck":283,"belt_top":466,"hem":528,"knee":633,"ankle":753,"heel":785}

def runs_of(row,gap=5):
    xs=np.where(row)[0]
    if len(xs)==0: return []
    cut=np.where(np.diff(xs)>gap)[0]
    return [(s[0],s[-1]) for s in np.split(xs,cut+1)]

def extent_x(mask,ya,yb,sel=None):
    """min/max x over band; sel='L'/'R' picks one leg run; 'arms' picks side runs"""
    lo,hi=1e9,-1
    for y in range(ya,yb):
        rs=runs_of(mask[y])
        if not rs: continue
        cx=mask.shape[1]/2
        if sel=='L': rs=[r for r in rs if (r[0]+r[1])/2<cx]
        elif sel=='R': rs=[r for r in rs if (r[0]+r[1])/2>=cx]
        if not rs: continue
        lo=min(lo,min(r[0] for r in rs)); hi=max(hi,max(r[1] for r in rs))
    return (None if hi<0 else (int(lo),int(hi)))

def box_back(ya,yb,sel=None):
    e=extent_x(pf,ya,yb,sel)
    return None if not e else dict(x=bx0+e[0],w=e[1]-e[0]+1)
def depth_side(ya,yb):
    e=extent_x(sp,ya,yb)
    return None if not e else dict(z=sx0+e[0],d=e[1]-e[0]+1)

# parts: (name, y0, y1, selector)
P=[("helmet","helm_top","neck",None),
   ("chest","neck","belt_top",None),     # full shoulder+arm span (upper body region)
   ("pelvis","belt_top","hem",None),
   ("thighL","hem","knee","L"),("thighR","hem","knee","R"),
   ("shinL","knee","ankle","L"),("shinR","knee","ankle","R"),
   ("footL","ankle","heel","L"),("footR","ankle","heel","R")]

ov=img.copy()
COL=dict(helmet=(0,200,255),chest=(0,255,0),pelvis=(255,180,0),
         thighL=(255,0,180),thighR=(255,0,180),shinL=(180,0,255),shinR=(180,0,255),
         footL=(0,128,255),footR=(0,128,255))
spec={"HH_px":HH,"parts":{}}
for name,a,b,sel in P:
    ya,yb=J[a],J[b]; bb=box_back(ya,yb,sel); dz=depth_side(ya,yb)
    if not bb: continue
    c=COL[name]
    cv2.rectangle(ov,(bb["x"],ya),(bb["x"]+bb["w"],yb),c,2)
    if dz: cv2.rectangle(ov,(dz["z"],ya),(dz["z"]+dz["d"],yb),c,1)
    base=name.rstrip("LR")
    if not name.endswith("R"):
        spec["parts"][base]=dict(y_frac=round((ya-TOP)/HH,3),h_frac=round((yb-ya)/HH,3),
            w_frac=round(bb["w"]/HH,3),d_frac=round((dz["d"]/HH) if dz else 0,3),
            w_px=bb["w"],h_px=yb-ya,d_px=(dz["d"] if dz else 0))
    cv2.putText(ov,base,(bb["x"],ya+12),cv2.FONT_HERSHEY_SIMPLEX,0.34,c,1,cv2.LINE_AA)
for k in("neck","belt_top","hem","knee","ankle"):
    cv2.line(ov,(bx0-8,J[k]),(sx1+8,J[k]),(110,110,110),1)
cv2.imwrite(os.path.join(OUT,"overlay_snap.png"),ov)
json.dump(spec,open(os.path.join(OUT,"spec.json"),"w"),indent=2)
print(json.dumps(spec,indent=2))
