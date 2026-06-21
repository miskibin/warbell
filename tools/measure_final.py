"""Final turnaround proportions -- joints READ from measured back-view profile
(profile_dump.py), widths computed occlusion-aware (torso core = middle run,
legs = single-limb run), depth from side view. Draws clean overlay + JSON."""
import cv2, numpy as np, json, os
SRC=r"C:\Users\skibi\Downloads\Gemini_Generated_Image_4znaze4znaze4zna (1).png"
OUT=r"D:\tileworld-bevy-forest\model_proportions"
PAN={"front":(28,331),"left":(366,508),"back":(587,824),"right":(900,1042)}
img=cv2.imread(SRC); rgb=cv2.cvtColor(img,cv2.COLOR_BGR2RGB).astype(np.int32)
R,G,B=rgb[...,0],rgb[...,1],rgb[...,2]; bright=rgb.max(2)
fg=(bright<235)|(rgb.max(2)-rgb.min(2)>18)
brown=(R>G+8)&(G>=B)&(R>60)&(R<210)&fg

TOP,HEEL=183,785; HH=HEEL-TOP
# joints (y px) read from measured profile
J={"helm_top":183,"neck":283,"belt_top":466,"hem":528,"knee":633,"ankle":753,"heel":785}
BANDS=[("helmet","helm_top","neck"),("torso","neck","belt_top"),
       ("pelvis","belt_top","hem"),("thigh","hem","knee"),
       ("shin","knee","ankle"),("foot","ankle","heel")]
LIMB={"thigh","shin","foot"}

bx0,bx1=PAN["back"]; pf=fg[:,bx0:bx1]
sx0,sx1=PAN["left"]; sp=fg[:,sx0:sx1]

def runs_of(row):
    xs=np.where(row)[0]
    if len(xs)==0: return []
    cut=np.where(np.diff(xs)>4)[0]
    segs=np.split(xs,cut+1)
    return [(s[0],s[-1]) for s in segs]

def width_core(ya,yb):
    """median width of the CENTRAL run (torso, excludes arms)"""
    vals=[]
    for y in range(ya,yb):
        rs=runs_of(pf[y])
        if not rs: continue
        cx=(bx1-bx0)/2
        mid=min(rs,key=lambda r:abs((r[0]+r[1])/2-cx))
        vals.append(mid[1]-mid[0]+1)
    return int(np.median(vals)) if vals else 0
def width_outer(ya,yb):
    vals=[]
    for y in range(ya,yb):
        xs=np.where(pf[y])[0]
        if len(xs): vals.append(xs[-1]-xs[0]+1)
    return int(np.median(vals)) if vals else 0
def width_limb(ya,yb):
    """median width of one leg (largest run on left half)"""
    vals=[];cx=(bx1-bx0)/2
    for y in range(ya,yb):
        rs=[r for r in runs_of(pf[y]) if (r[0]+r[1])/2<cx]
        if rs: r=max(rs,key=lambda r:r[1]-r[0]); vals.append(r[1]-r[0]+1)
    return int(np.median(vals)) if vals else 0
def depth(ya,yb):
    vals=[]
    for y in range(ya,yb):
        xs=np.where(sp[y])[0]
        if len(xs): vals.append(xs[-1]-xs[0]+1)
    return int(np.median(vals)) if vals else 0

rep={"HH_px":HH,"px_per_HH":1.0,"bands":{}}
for name,a,b in BANDS:
    ya,yb=J[a],J[b]
    w = width_limb(ya,yb) if name in LIMB else width_core(ya,yb)
    rep["bands"][name]=dict(y0=ya,y1=yb,h=yb-ya,pct_h=round(100*(yb-ya)/HH,1),
        width=w,depth=depth(ya,yb))
# extra: shoulder span (max outer torso) + foot len from side
rep["shoulder_span"]=width_outer(J["neck"],J["neck"]+60)
rep["stance_outer"]=width_outer(J["hem"]+20,J["ankle"]-20)
# shield from front
fx0,fx1=PAN["front"]; sh=brown[:,fx0:(fx0+fx1)//2]
ys,xs=np.where(sh)
if len(xs)>50:
    rep["shield"]=dict(x=int(fx0+xs.min()),y=int(ys.min()),w=int(xs.max()-xs.min()),h=int(ys.max()-ys.min()))

# ---- draw ----
ov=img.copy()
COL=dict(helmet=(0,200,255),torso=(0,255,0),pelvis=(255,180,0),
         thigh=(255,0,180),shin=(180,0,255),foot=(0,128,255))
cxb=(bx0+bx1)//2; cxs=(sx0+sx1)//2
for name,a,b in BANDS:
    ya,yb=J[a],J[b]; c=COL[name]; p=rep["bands"][name]
    if name in LIMB:
        x=bx0+(bx1-bx0)//2-p["width"]-6
        cv2.rectangle(ov,(x,ya),(x+p["width"],yb),c,2)
    else:
        cv2.rectangle(ov,(cxb-p["width"]//2,ya),(cxb+p["width"]//2,yb),c,2)
    cv2.putText(ov,f"{name} {p['pct_h']}% w{p['width']} d{p['depth']}",
                (bx0-2,ya+13),cv2.FONT_HERSHEY_SIMPLEX,0.36,c,1,cv2.LINE_AA)
    hd=p["depth"]//2
    cv2.rectangle(ov,(cxs-hd,ya),(cxs+hd,yb),c,1)
for k in("neck","belt_top","hem","knee","ankle"):
    cv2.line(ov,(bx0-10,J[k]),(sx1+10,J[k]),(110,110,110),1)
if "shield" in rep:
    s=rep["shield"]; cv2.rectangle(ov,(s["x"],s["y"]),(s["x"]+s["w"],s["y"]+s["h"]),(0,0,255),2)
    cv2.putText(ov,"shield",(s["x"],s["y"]-3),cv2.FONT_HERSHEY_SIMPLEX,0.4,(0,0,255),1)
cv2.imwrite(os.path.join(OUT,"overlay_final.png"),ov)
json.dump(rep,open(os.path.join(OUT,"proportions_final.json"),"w"),indent=2)
print(json.dumps(rep,indent=2))
