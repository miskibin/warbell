"""Dump measured silhouette profile of the clean BACK view, row by row, so
anatomy joints can be read from real pixels (width minima = neck/waist/knee/
ankle; run-count 1->2 = legs separating; brown flag = tunic/belt extent)."""
import cv2, numpy as np
SRC=r"C:\Users\skibi\Downloads\Gemini_Generated_Image_4znaze4znaze4zna (1).png"
img=cv2.imread(SRC); rgb=cv2.cvtColor(img,cv2.COLOR_BGR2RGB).astype(np.int32)
R,G,B=rgb[...,0],rgb[...,1],rgb[...,2]; bright=rgb.max(2)
fg=(bright<235)|(rgb.max(2)-rgb.min(2)>18)
brown=(R>G+8)&(G>=B)&(R>60)&(R<210)&fg
bx0,bx1=587,824; pf=fg[:,bx0:bx1]; pb=brown[:,bx0:bx1]
rows=np.where(pf.any(1))[0]; top,bot=rows[0],rows[-1]; HH=bot-top
def nruns(row):
    xs=np.where(row)[0]
    if len(xs)==0: return 0,0
    splits=np.where(np.diff(xs)>4)[0]
    return len(splits)+1, xs[-1]-xs[0]+1
print(f"BACK panel x[{bx0}:{bx1}] top={top} bot={bot} HH={HH}")
print(f"{'y':>4} {'%HH':>5} {'width':>5} {'runs':>4} {'brown':>5}")
for y in range(top,bot+1,5):
    n,wd=nruns(pf[y]); br=int(pb[y].sum())
    bar="#"*(wd//4)
    print(f"{y:>4} {100*(y-top)/HH:5.1f} {wd:>5} {n:>4} {br:>5}  {bar}")
