"""Turnaround proportions v2 -- occlusion-aware.

BACK view  -> Y-bands + widths (clean: no shield, no sword-in-front)
SIDE view  -> depth (Z) per band
FRONT view -> shield only
Output: overlay on back+side + JSON {part: y, h, width_back, depth_side, %HH}.
"""
import cv2, numpy as np, json, os
SRC = r"C:\Users\skibi\Downloads\Gemini_Generated_Image_4znaze4znaze4zna (1).png"
OUT = r"D:\tileworld-bevy-forest\model_proportions"
PAN = {"front":(28,331),"left":(366,508),"back":(587,824),"right":(900,1042)}
img = cv2.imread(SRC); H,W = img.shape[:2]
rgb = cv2.cvtColor(img,cv2.COLOR_BGR2RGB).astype(np.int32)
R,G,B = rgb[...,0],rgb[...,1],rgb[...,2]
bright = rgb.max(2)
fg = (bright<235)|(rgb.max(2)-rgb.min(2)>18)
brown = (R>G+8)&(G>=B)&(R>60)&(R<210)&fg

def panel(name): x0,x1=PAN[name]; return x0,x1
def smooth(a,k=11): k|=1; return np.convolve(a,np.ones(k)/k,mode="same")

# ---------- BACK: bands + widths ----------
bx0,bx1 = panel("back")
pf = fg[:,bx0:bx1]; pb = brown[:,bx0:bx1]
rows=np.where(pf.any(1))[0]; top,bot=rows[0],rows[-1]; HH=bot-top
w=np.zeros(H);
for y in range(top,bot+1):
    xs=np.where(pf[y])[0]
    if len(xs): w[y]=xs[-1]-xs[0]+1
ws=smooth(w.astype(float))
def bmin(a,b): a=max(a,top); b=min(b,bot); return a+int(np.argmin(ws[a:b])) if b>a else a

# brown belt = widest brown row in mid; hem = lowest brown row
br_rowsum = pb.sum(1)
br_rows = np.where(br_rowsum>3)[0]
hem = int(br_rows[-1]) if len(br_rows) else top+int(.6*HH)
# belt: local min of width within brown vertical extent
belt = bmin(top+int(.42*HH), top+int(.60*HH))
neck = bmin(top+int(.10*HH), top+int(.30*HH))
knee = bmin(hem+int(.04*HH), bot-int(.16*HH))
ankle= bmin(bot-int(.18*HH), bot-int(.05*HH))

def outer_width(ya,yb):
    spans=[]
    for y in range(ya,yb+1):
        xs=np.where(pf[y])[0]
        if len(xs): spans.append(xs[-1]-xs[0]+1)
    return int(np.median(spans)) if spans else 0
def limb_width(ya,yb,side):
    """median width of one leg (split at central gap)"""
    vals=[]
    cx=(bx1-bx0)//2
    for y in range(ya,yb+1):
        xs=np.where(pf[y])[0]
        if len(xs)<2: continue
        half = xs[xs<cx] if side=="L" else xs[xs>=cx]
        if len(half): vals.append(half[-1]-half[0]+1)
    return int(np.median(vals)) if vals else 0

bands=[("helmet",top,neck),("torso",neck,belt),("skirt",belt,hem),
       ("thigh",hem,knee),("shin",knee,ankle),("foot",ankle,bot)]

# ---------- SIDE (left): depth per Y ----------
sx0,sx1=panel("left"); sfp=fg[:,sx0:sx1]
def depth(ya,yb):
    vals=[]
    for y in range(ya,yb+1):
        xs=np.where(sfp[y])[0]
        if len(xs): vals.append(xs[-1]-xs[0]+1)
    return int(np.median(vals)) if vals else 0

report={"HH_px":int(HH),"bands":{}}
for name,ya,yb in bands:
    wfull=outer_width(ya,yb)
    wlimb=limb_width(ya,yb,"L") if name in("thigh","shin","foot") else wfull
    report["bands"][name]=dict(y=int(ya),h=int(yb-ya),pct_h=round(100*(yb-ya)/HH,1),
        width=wlimb,depth=depth(ya,yb))

# ---------- FRONT: shield ----------
fx0,fx1=panel("front")
sh = brown[:,fx0:(fx0+fx1)//2]
ys,xs=np.where(sh)
if len(xs)>50:
    report["shield"]=dict(x=int(fx0+xs.min()),y=int(ys.min()),
        w=int(xs.max()-xs.min()),h=int(ys.max()-ys.min()))

# ---------- draw overlay (back + side) ----------
ov=img.copy()
COL=dict(helmet=(0,200,255),torso=(0,255,0),skirt=(255,180,0),
         thigh=(255,0,180),shin=(180,0,255),foot=(0,128,255))
for name,ya,yb in bands:
    c=COL[name]; p=report["bands"][name]
    # back: width box centred on back centre
    cxb=(bx0+bx1)//2; hw=p["width"]//2
    if name in("thigh","shin","foot"):  # one leg, offset left of centre
        cxb = bx0 + (bx1-bx0)//2 - p["width"]//2 - 6
        cv2.rectangle(ov,(cxb,ya),(cxb+p["width"],yb),c,2)
    else:
        cv2.rectangle(ov,(cxb-hw,ya),(cxb+hw,yb),c,2)
    cv2.putText(ov,f"{name} {p['pct_h']}% w{p['width']} d{p['depth']}",(bx0,ya+12),
                cv2.FONT_HERSHEY_SIMPLEX,0.34,c,1,cv2.LINE_AA)
    # side: depth box
    cxs=(sx0+sx1)//2; hd=p["depth"]//2
    cv2.rectangle(ov,(cxs-hd,ya),(cxs+hd,yb),c,1)
for yj in (neck,belt,hem,knee,ankle):
    cv2.line(ov,(bx0,yj),(sx1,yj),(110,110,110),1)
if "shield" in report:
    s=report["shield"]; cv2.rectangle(ov,(s["x"],s["y"]),(s["x"]+s["w"],s["y"]+s["h"]),(0,0,255),2)
cv2.imwrite(os.path.join(OUT,"overlay_v2.png"),ov)
with open(os.path.join(OUT,"proportions_v2.json"),"w") as f: json.dump(report,f,indent=2)
print(json.dumps(report,indent=2))
