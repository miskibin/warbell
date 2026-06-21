"""Full 4-view box overlay of the procedural model. Each part drawn on ALL panels:
front/back show WIDTH, side panels show DEPTH. Boxes snapped to each panel's own
figure centreline at the band y. Adds shield(front), sword(front+side), arms, neck."""
import cv2, numpy as np, json, os
SRC=r"C:\Users\skibi\Downloads\Gemini_Generated_Image_4znaze4znaze4zna (1).png"
OUT=r"D:\tileworld-bevy-forest\model_proportions"
spec=json.load(open(os.path.join(OUT,"spec.json")))
PAN={"front":(28,331),"left":(366,508),"back":(587,824),"right":(900,1042)}
WIDTH_VIEWS={"front","back"}; HH=spec["HH_px"]; TOP=183
img=cv2.imread(SRC); rgb=cv2.cvtColor(img,cv2.COLOR_BGR2RGB).astype(np.int32)
bright=rgb.max(2); fg=(bright<235)|(rgb.max(2)-rgb.min(2)>18)

def center_x(x0,x1,ya,yb):
    """median figure centreline in a panel over a y-band"""
    cs=[]
    for y in range(ya,yb):
        xs=np.where(fg[y,x0:x1])[0]
        if len(xs): cs.append(x0+(xs[0]+xs[-1])/2)
    return int(np.median(cs)) if cs else (x0+x1)//2

LIMB={"arm","thigh","shin","foot"}
COL=dict(helm=(0,200,255),neck=(0,220,220),pauldron=(0,255,120),cuirass=(0,255,0),
         pelvis=(255,180,0),arm=(255,140,0),thigh=(255,0,180),shin=(180,0,255),foot=(0,128,255))
ov=img.copy()

for view,(x0,x1) in PAN.items():
    for name,p in spec["parts"].items():
        ya=int(TOP+p["y"]*HH); yb=int(ya+p["h"]*HH)
        dim = p["w"] if view in WIDTH_VIEWS else p["d"]
        wpx=int(dim*HH); c=COL.get(name,(255,255,255)); cx=center_x(x0,x1,ya,yb)
        if name in LIMB and view in WIDTH_VIEWS:           # two limbs side by side
            off=int((p["w"]*HH)/2+ p["w"]*HH*0.15)
            for s in (-1,1):
                bx=cx+s*off-wpx//2
                cv2.rectangle(ov,(bx,ya),(bx+wpx,yb),c,1)
        else:
            cv2.rectangle(ov,(cx-wpx//2,ya),(cx+wpx//2,yb),c,1)
    # labels once on back panel
    if view=="back":
        for name,p in spec["parts"].items():
            ya=int(TOP+p["y"]*HH)
            cv2.putText(ov,name,(x1+4,ya+10),cv2.FONT_HERSHEY_SIMPLEX,0.34,COL.get(name,(255,255,255)),1,cv2.LINE_AA)

# shield: front panel, left blob (brown+dark) ; measure tight
R,G,B=rgb[...,0],rgb[...,1],rgb[...,2]
brown=(R>G+8)&(G>=B)&(R>60)&(R<210)&fg
fx0,fx1=PAN["front"]; cxf=(fx0+fx1)//2
shmask=brown[:,fx0:cxf]
ys,xs=np.where(shmask)
if len(xs)>50:
    # tight to largest cluster (drop stray sword pixels by keeping leftmost 70%)
    x_lo,x_hi=fx0+int(np.percentile(xs,2)),fx0+int(np.percentile(xs,98))
    y_lo,y_hi=int(np.percentile(ys,2)),int(np.percentile(ys,98))
    cv2.rectangle(ov,(x_lo,y_lo),(x_hi,y_hi),(0,0,255),2)
    cv2.putText(ov,"shield",(x_lo,y_lo-3),cv2.FONT_HERSHEY_SIMPLEX,0.4,(0,0,255),1)

cv2.imwrite(os.path.join(OUT,"overlay_full.png"),ov)
print("parts:",list(spec["parts"].keys()))
print("wrote overlay_full.png")
