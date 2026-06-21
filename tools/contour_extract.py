"""Per-part SHAPE extraction: contour of each exploded part + the whole-knight
silhouette, simplified to low-poly polygons (approxPolyDP). Gives the actual
outline to extrude/loft procedurally -- boxes only gave size, this gives shape.
Outputs polygon overlay + contours.json (verts in head-units, origin = part centre)."""
import cv2, numpy as np, json, os
OUT=r"D:\tileworld-bevy-forest\model_proportions"
PARTS=r"C:\Users\skibi\Downloads\Gemini_Generated_Image_cuvefjcuvefjcuve.png"
TURN =r"C:\Users\skibi\Downloads\Gemini_Generated_Image_4znaze4znaze4zna (1).png"

def mask_of(img):
    rgb=cv2.cvtColor(img,cv2.COLOR_BGR2RGB).astype(np.int32)
    b=rgb.max(2)
    m=((b<238)|(rgb.max(2)-rgb.min(2)>16)).astype(np.uint8)
    return cv2.morphologyEx(m,cv2.MORPH_CLOSE,np.ones((7,7),np.uint8))

def poly(mask,eps_frac=0.010):
    cs,_=cv2.findContours(mask,cv2.RETR_EXTERNAL,cv2.CHAIN_APPROX_SIMPLE)
    if not cs: return None
    c=max(cs,key=cv2.contourArea)
    ap=cv2.approxPolyDP(c,eps_frac*cv2.arcLength(c,True),True)
    return ap.reshape(-1,2)

# ---- parts sheet ----
img=cv2.imread(PARTS); m=mask_of(img)
n,lbl,stats,cent=cv2.connectedComponentsWithStats(m,8)
LBL={0:"helm",1:"cuirass",2:"pauldron",4:"upper_arm",6:"lower_arm",
     11:"gauntlet",9:"tasset_skirt",12:"leg",14:"boot",7:"shield",8:"sword"}
# map verified ids -> need same ordering as parts_raw; rebuild id by matching bbox centre
raw={c["id"]:c for c in json.load(open(os.path.join(OUT,"parts_raw.json")))}
U=raw[0]["h"]                      # head unit px
ov=img.copy(); out={"unit":"head-height","head_px":int(U),"parts":{}}
COLS=[(0,0,255),(0,200,0),(255,120,0),(0,200,255),(255,0,200),(180,0,255),(0,128,255)]
def emit_poly(name,sub,x,y,w,h,col,eps=0.008):
    p=poly(sub,eps_frac=eps)
    if p is None: return
    p=p+[x,y]
    cv2.polylines(ov,[p.reshape(-1,1,2)],True,col,2)
    for vx,vy in p: cv2.circle(ov,(int(vx),int(vy)),3,col,-1)
    cv2.putText(ov,f"{name}({len(p)})",(x,y-4),cv2.FONT_HERSHEY_SIMPLEX,0.5,col,1,cv2.LINE_AA)
    cxp,cyp=x+w/2,y+h/2
    out["parts"][name]=dict(n=len(p),verts=[[round((vx-cxp)/U,3),round((cyp-vy)/U,3)] for vx,vy in p])

for k,(rid,name) in enumerate(LBL.items()):
    c=raw[rid]; x,y,w,h=c["x"],c["y"],c["w"],c["h"]; col=COLS[k%len(COLS)]
    sub=m[y:y+h,x:x+w]
    if name=="leg":                # split fused thigh+shin at knee (46.6%) -> two polys
        ky=int(h*0.466)
        emit_poly("thigh",sub[:ky],x,y,w,ky,(255,0,200),eps=0.012)
        emit_poly("shin", sub[ky:],x,y+ky,w,h-ky,(180,0,255),eps=0.012)
    else:
        emit_poly(name,sub,x,y,w,h,col,eps=0.008)
cv2.imwrite(os.path.join(OUT,"contours_parts.png"),ov)

# ---- whole knight silhouette (turnaround front panel) ----
t=cv2.imread(TURN); tm=mask_of(t)
fx0,fx1=28,331
sil=np.zeros_like(tm); sil[:,fx0:fx1]=tm[:,fx0:fx1]
p=poly(sil,eps_frac=0.006)
tov=t.copy()
if p is not None:
    cv2.polylines(tov,[p.reshape(-1,1,2)],True,(0,0,255),2)
    for vx,vy in p: cv2.circle(tov,(int(vx),int(vy)),3,(0,200,255),-1)
    out["whole_front_silhouette"]=dict(n=len(p),verts=[[int(a),int(b)] for a,b in p])
cv2.imwrite(os.path.join(OUT,"contour_whole.png"),tov)
json.dump(out,open(os.path.join(OUT,"contours.json"),"w"),indent=2)
print("parts polys:",{k:v["n"] for k,v in out["parts"].items()})
print("whole knight verts:",out.get("whole_front_silhouette",{}).get("n"))
