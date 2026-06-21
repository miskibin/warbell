"""Pull a gentle per-part FRONT-BULGE from the Depth Anything map and merge into
loft_spec.json. Per front blob: mask depth, remove a fitted plane (kills the scene
background gradient + tilt), normalise the residual, take per-slice mean -> bulge[N]
(0..1, how much that height bulges toward the viewer). The loft pushes the front face
out by this -> rounded chest/dome instead of a flat octagon front."""
import numpy as np, json, os
OUT=r"D:\tileworld-bevy-forest\model_proportions"
D=np.load(os.path.join(OUT,"depth_master_sheet.npy"))
raw={c["id"]:c for c in json.load(open(os.path.join(OUT,"master_raw.json")))}
spec=json.load(open(os.path.join(OUT,"loft_spec.json")))
N=spec["N"]
FRONT={"helm":3,"cuirass":5,"tabard":21,"pauldron":9,"arm":13,"gauntlet":17,
       "thigh":23,"shin":27,"boot":33,"shield":19,"sword":28}

import cv2
img=cv2.imread(os.path.join(OUT,"master_sheet.png"))
rgbf=cv2.cvtColor(img,cv2.COLOR_BGR2RGB).astype(np.int32)

def bulge(cid):
    c=raw[cid]; x,y,w,h=c["x"],c["y"],c["w"],c["h"]
    sub=D[y:y+h,x:x+w]
    rg=rgbf[y:y+h,x:x+w]
    m=((rg.max(2)<238)|(rg.max(2)-rg.min(2)>16))
    ys,xs=np.where(m)
    if len(xs)<30: return [0.0]*N
    z=sub[ys,xs]
    A=np.c_[xs,ys,np.ones_like(xs)]                 # fit plane z=ax+by+c
    coef,*_=np.linalg.lstsq(A,z,rcond=None)
    res=z-A@coef                                    # relief residual
    res=(res-res.min())/(np.ptp(res)+1e-6)
    out=[]
    for i in range(N):
        r0,r1=int(i*h/N),int((i+1)*h/N)
        sel=(ys>=r0)&(ys<r1)
        out.append(round(float(res[sel].mean()) if sel.any() else 0.0,3))
    return out

for name,cid in FRONT.items():
    spec["parts"][name]["bulge"]=bulge(cid)
json.dump(spec,open(os.path.join(OUT,"loft_spec.json"),"w"),indent=2)
for name in FRONT:
    b=spec["parts"][name]["bulge"]
    print(f"{name:9} bulge avg {np.mean(b):.2f} range {min(b):.2f}-{max(b):.2f}")
