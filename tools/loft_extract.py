"""Extract loft profiles from the master sheet: for each part, FRONT blob -> per-height
half-WIDTH, SIDE blob -> per-height half-DEPTH. Resampled to N slices, normalised to a
head unit. The JS builder sweeps an elliptical cross-section scaled by (halfW,halfD) up
the height -> a real shaped low-poly part (helm dome, boot toe, shield slab) from two
silhouettes. Colour sampled from the front blob."""
import cv2, numpy as np, json, os
OUT=r"D:\tileworld-bevy-forest\model_proportions"
img=cv2.imread(os.path.join(OUT,"master_sheet.png"))
raw={c["id"]:c for c in json.load(open(os.path.join(OUT,"master_raw.json")))}
N=16

# part -> (front_id, side_id); mirror=symmetric L/R limb
PAIR={
 "helm":(3,4,False),"cuirass":(5,6,False),"tabard":(21,22,False),
 "pauldron":(9,10,True),"arm":(13,14,True),"gauntlet":(17,18,True),
 "thigh":(23,24,True),"shin":(27,30,True),"boot":(33,34,True),
 "shield":(19,20,False),"sword":(28,25,False),
}
def submask(cid):
    c=raw[cid]; x,y,w,h=c["x"],c["y"],c["w"],c["h"]
    sub=img[y:y+h,x:x+w]
    rgb=cv2.cvtColor(sub,cv2.COLOR_BGR2RGB).astype(np.int32)
    m=((rgb.max(2)<238)|(rgb.max(2)-rgb.min(2)>16))
    return m,(w,h)
def half_profile(cid):
    """per-height half-extent (px), resampled to N (top->bottom)"""
    m,(w,h)=submask(cid)
    hp=np.zeros(h)
    for r in range(h):
        xs=np.where(m[r])[0]
        hp[r]=(xs[-1]-xs[0]+1)/2 if len(xs) else 0
    idx=np.linspace(0,h-1,N).astype(int)
    return hp[idx], h

# head unit: turnaround front figure (id0) height / 6.3 heads
U=raw[0]["h"]/6.3
parts={}
for name,(fid,sid,mir) in PAIR.items():
    fw,fh=half_profile(fid)      # half-width per slice
    sd,sh=half_profile(sid)      # half-depth per slice
    h=max(fh,sh)
    c=raw[fid]
    parts[name]=dict(
        h=round(h/U,3),
        halfW=[round(v/U,3) for v in fw],
        halfD=[round(v/U,3) for v in sd],
        rgb=c["rgb"], mirror=mir)
spec=dict(unit="head", U_px=round(U,1), N=N, parts=parts)
json.dump(spec,open(os.path.join(OUT,"loft_spec.json"),"w"),indent=2)
for k,p in parts.items():
    print(f"{k:9} h{p['h']:.2f}  Wmax{max(p['halfW'])*2:.2f} Dmax{max(p['halfD'])*2:.2f}  rgb{p['rgb']} {'(L/R)' if p['mirror'] else ''}")
print("head unit px:",round(U,1))
