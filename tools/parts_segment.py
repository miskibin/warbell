"""Segment the EXPLODED parts sheet via connected components (parts separated by
white gaps -> each blob = one part). Tight bbox + colour per part. Front view =>
clean intrinsic W,H per part, zero occlusion. Numbered overlay for labelling."""
import cv2, numpy as np, json, os
SRC=r"C:\Users\skibi\Downloads\Gemini_Generated_Image_cuvefjcuvefjcuve.png"
OUT=r"D:\tileworld-bevy-forest\model_proportions"
img=cv2.imread(SRC); H,W=img.shape[:2]
rgb=cv2.cvtColor(img,cv2.COLOR_BGR2RGB).astype(np.int32)
bright=rgb.max(2); fg=((bright<238)|(rgb.max(2)-rgb.min(2)>16)).astype(np.uint8)
fg=cv2.morphologyEx(fg,cv2.MORPH_CLOSE,np.ones((7,7),np.uint8))  # seal facet seams
n,lbl,stats,cent=cv2.connectedComponentsWithStats(fg,connectivity=8)
comps=[]
for i in range(1,n):
    x,y,w,h,a=stats[i]
    if a<400: continue
    m=lbl[y:y+h,x:x+w]==i
    px=rgb[y:y+h,x:x+w][m]
    comps.append(dict(id=len(comps),x=int(x),y=int(y),w=int(w),h=int(h),area=int(a),
        cx=float(cent[i][0]),cy=float(cent[i][1]),
        rgb=[int(px[:,0].mean()),int(px[:,1].mean()),int(px[:,2].mean())]))
comps.sort(key=lambda c:(round(c["cy"]/40),c["cx"]))   # rough top->bottom, left->right
ov=img.copy()
print(f"image {W}x{H}, {len(comps)} parts")
print(f"{'id':>2} {'x':>4} {'y':>4} {'w':>4} {'h':>4} {'cx':>5} {'cy':>5} {'area':>6}  rgb")
for c in comps:
    cv2.rectangle(ov,(c["x"],c["y"]),(c["x"]+c["w"],c["y"]+c["h"]),(0,0,255),2)
    cv2.putText(ov,str(c["id"]),(c["x"]+2,c["y"]+20),cv2.FONT_HERSHEY_SIMPLEX,0.7,(255,0,0),2)
    print(f"{c['id']:>2} {c['x']:>4} {c['y']:>4} {c['w']:>4} {c['h']:>4} {c['cx']:>5.0f} {c['cy']:>5.0f} {c['area']:>6}  {c['rgb']}")
cv2.imwrite(os.path.join(OUT,"parts_numbered.png"),ov)
json.dump(comps,open(os.path.join(OUT,"parts_raw.json"),"w"),indent=2)
print("wrote parts_numbered.png")
