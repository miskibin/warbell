"""Segment the master model sheet: top band = assembled turnaround (front/side/back),
lower grid = each part in front+side pairs. Connected components -> numbered overlay
so blobs can be labelled + paired front/side."""
import cv2, numpy as np, json, os
SRC=r"D:\tileworld-bevy-forest\model_proportions\master_sheet.png"
OUT=r"D:\tileworld-bevy-forest\model_proportions"
img=cv2.imread(SRC); H,W=img.shape[:2]
rgb=cv2.cvtColor(img,cv2.COLOR_BGR2RGB).astype(np.int32)
bright=rgb.max(2)
fg=((bright<238)|(rgb.max(2)-rgb.min(2)>16)).astype(np.uint8)
fg=cv2.morphologyEx(fg,cv2.MORPH_CLOSE,np.ones((5,5),np.uint8))
n,lbl,stats,cent=cv2.connectedComponentsWithStats(fg,8)
comps=[]
for i in range(1,n):
    x,y,w,h,a=stats[i]
    if a<500: continue
    m=lbl[y:y+h,x:x+w]==i
    px=rgb[y:y+h,x:x+w][m]
    comps.append(dict(id=0,x=int(x),y=int(y),w=int(w),h=int(h),area=int(a),
        cx=float(cent[i][0]),cy=float(cent[i][1]),
        rgb=[int(px[:,0].mean()),int(px[:,1].mean()),int(px[:,2].mean())]))
comps.sort(key=lambda c:(round(c["cy"]/60),c["cx"]))
for k,c in enumerate(comps): c["id"]=k
ov=img.copy()
print(f"image {W}x{H}, {len(comps)} blobs")
print(f"{'id':>3} {'x':>4} {'y':>4} {'w':>4} {'h':>4} {'cx':>5} {'cy':>5} {'area':>7}  rgb")
for c in comps:
    cv2.rectangle(ov,(c["x"],c["y"]),(c["x"]+c["w"],c["y"]+c["h"]),(0,0,255),2)
    cv2.putText(ov,str(c["id"]),(c["x"]+3,c["y"]+26),cv2.FONT_HERSHEY_SIMPLEX,0.9,(255,0,0),3)
    print(f"{c['id']:>3} {c['x']:>4} {c['y']:>4} {c['w']:>4} {c['h']:>4} {c['cx']:>5.0f} {c['cy']:>5.0f} {c['area']:>7}  {c['rgb']}")
cv2.imwrite(os.path.join(OUT,"master_numbered.png"),ov)
json.dump(comps,open(os.path.join(OUT,"master_raw.json"),"w"),indent=2)
print("wrote master_numbered.png")
