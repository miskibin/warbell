"""Final part dims. W,H = clean intrinsic from the EXPLODED parts sheet (no
occlusion); D = depth grafted from the turnaround side view. Normalised to
HEAD HEIGHT (helm h) -> scale-free, standard char-proportion unit.
Outputs labelled overlay + final_spec.json (head-units, ready for procedural build)."""
import cv2, json, os
OUT=r"D:\tileworld-bevy-forest\model_proportions"
SRC=r"C:\Users\skibi\Downloads\Gemini_Generated_Image_cuvefjcuvefjcuve.png"
raw={c["id"]:c for c in json.load(open(os.path.join(OUT,"parts_raw.json")))}

# verified id -> part name (right-side arm pieces = clean split)
LBL={0:"helm",1:"cuirass",2:"pauldron",4:"upper_arm",6:"lower_arm",
     11:"gauntlet",9:"tasset_skirt",12:"leg",14:"boot",7:"shield",8:"sword"}
U=raw[0]["h"]            # head unit = helm height px (=119)

# depth in head-units, from turnaround (d_frac * 602px / U); arms/legs ~cylindrical (=width)
DEPTH={"helm":0.66,"cuirass":0.72,"tasset_skirt":0.78,"boot":0.78,
       "shield":0.12,"sword":0.10}   # flat/own; others default to width below

parts={}
for i,name in LBL.items():
    c=raw[i]; w=round(c["w"]/U,2); h=round(c["h"]/U,2)
    d=DEPTH.get(name, w)             # default depth = width (limbs roughly round)
    parts[name]=dict(w=w,h=h,d=d,w_px=c["w"],h_px=c["h"],rgb=c["rgb"])

# split fused leg into thigh/shin by turnaround ratio (thigh .466 / shin .534)
leg=parts.pop("leg")
parts["thigh"]=dict(w=leg["w"],h=round(leg["h"]*0.466,2),d=leg["w"],rgb=leg["rgb"])
parts["shin"] =dict(w=leg["w"],h=round(leg["h"]*0.534,2),d=leg["w"],rgb=leg["rgb"])

spec=dict(unit="head-height", head_px=int(U),
          note="W,H from exploded parts sheet; D from turnaround side; limbs D=W",
          parts=parts)
json.dump(spec,open(os.path.join(OUT,"final_spec.json"),"w"),indent=2)

# labelled overlay
img=cv2.imread(SRC); ov=img.copy()
for i,name in LBL.items():
    c=raw[i]
    cv2.rectangle(ov,(c["x"],c["y"]),(c["x"]+c["w"],c["y"]+c["h"]),(0,0,255),2)
    cv2.putText(ov,name,(c["x"],c["y"]-4),cv2.FONT_HERSHEY_SIMPLEX,0.55,(0,80,255),2,cv2.LINE_AA)
cv2.imwrite(os.path.join(OUT,"parts_labeled.png"),ov)

print(f"head unit = {int(U)} px")
for k,p in spec["parts"].items():
    print(f"{k:14} w{p['w']:.2f} h{p['h']:.2f} d{p['d']:.2f} heads  rgb{p.get('rgb')}")
tot=parts['helm']['h']+parts['cuirass']['h']+parts['thigh']['h']+parts['shin']['h']+parts['boot']['h']
print(f"~standing height (helm+torso+leg+boot): {tot:.1f} heads")
