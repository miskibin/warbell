"""Crop each turnaround panel, upscale, overlay a labelled pixel ruler grid
(original-image coords) so part bboxes can be read by eye accurately."""
import cv2, numpy as np, os
SRC = r"C:\Users\skibi\Downloads\Gemini_Generated_Image_4znaze4znaze4zna (1).png"
OUT = r"D:\tileworld-bevy-forest\model_proportions"
os.makedirs(OUT, exist_ok=True)
img = cv2.imread(SRC)
PAN = {"front":(28,331),"left":(366,508),"back":(587,824),"right":(900,1042)}
Y0, Y1 = 170, 800          # vertical crop around figure
S = 3                       # upscale
STEP = 20
for name,(x0,x1) in PAN.items():
    crop = img[Y0:Y1, x0:x1]
    up = cv2.resize(crop, None, fx=S, fy=S, interpolation=cv2.INTER_NEAREST)
    h,w = up.shape[:2]
    # vertical grid lines (orig x)
    for ox in range(((x0)//STEP+1)*STEP, x1, STEP):
        gx = (ox-x0)*S
        cv2.line(up,(gx,0),(gx,h),(60,60,60),1)
        cv2.putText(up,str(ox),(gx+1,12),cv2.FONT_HERSHEY_SIMPLEX,0.3,(0,255,255),1)
    for oy in range(((Y0)//STEP+1)*STEP, Y1, STEP):
        gy=(oy-Y0)*S
        cv2.line(up,(0,gy),(w,gy),(60,60,60),1)
        cv2.putText(up,str(oy),(2,gy-2),cv2.FONT_HERSHEY_SIMPLEX,0.3,(0,255,255),1)
    cv2.imwrite(os.path.join(OUT,f"ruler_{name}.png"), up)
    print("wrote", name, "crop", x0,x1)
