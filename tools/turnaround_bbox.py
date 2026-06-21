"""Measure knight turnaround proportions from an ortho 4-view render.

Step 1 of model-from-image pipeline: get REAL pixel bboxes per body part,
overlay them so proportions can be verified by eye before any 3D is built.

Approach (hybrid, NOT naive color-seg -- armor is uniform grey so colour can't
split helmet/torso/limbs):
  1. foreground mask = non-white background
  2. split into 4 panels by all-background column gaps
  3. per panel: silhouette width profile w(y) -> detect joints (neck/waist/hem/
     knee/ankle) as narrowings; brown colour mask isolates tunic/skirt; shield
     = big brown+dark blob on figure's left (front panel only)
  4. draw labelled bboxes + dump JSON (px and %HH, HH = helm-top..heel)
"""
import cv2, numpy as np, json, sys, os

SRC = sys.argv[1] if len(sys.argv) > 1 else r"C:\Users\skibi\Downloads\Gemini_Generated_Image_4znaze4znaze4zna (1).png"
OUT = r"D:\tileworld-bevy-forest\model_proportions"
os.makedirs(OUT, exist_ok=True)

img = cv2.imread(SRC)                      # BGR
assert img is not None, f"cannot read {SRC}"
H, W = img.shape[:2]
rgb = cv2.cvtColor(img, cv2.COLOR_BGR2RGB).astype(np.int32)
R, G, B = rgb[..., 0], rgb[..., 1], rgb[..., 2]

# --- foreground: anything not near-white ---
bright = rgb.max(2)
fg = (bright < 235) | (rgb.max(2) - rgb.min(2) > 18)   # not white AND/OR coloured

# --- colour classes ---
brown = (R > G + 8) & (G >= B) & (R > 60) & (R < 210) & fg   # warm leather
grey  = (np.abs(R - G) < 22) & (np.abs(G - B) < 22) & fg & ~brown

def runs(mask_cols, min_w=20):
    """contiguous column runs where mask_cols True"""
    out, s = [], None
    for x, v in enumerate(mask_cols):
        if v and s is None: s = x
        if not v and s is not None:
            if x - s >= min_w: out.append((s, x)); s = None
            else: s = None
    if s is not None and len(mask_cols) - s >= min_w: out.append((s, len(mask_cols)))
    return out

# --- split into 4 panels by column occupancy ---
col_has = fg.sum(0) > 4
panels = runs(col_has, min_w=W // 12)
# merge tiny gaps: keep 4 biggest
panels = sorted(panels, key=lambda p: p[1]-p[0], reverse=True)[:4]
panels = sorted(panels)
print("panels(x0,x1):", panels)

VIEWS = ["front", "left", "back", "right"]
overlay = img.copy()
report = {}

def smooth(a, k=9):
    k = k | 1
    return np.convolve(a, np.ones(k)/k, mode="same")

for i, (x0, x1) in enumerate(panels[:4]):
    view = VIEWS[i] if i < 4 else f"p{i}"
    pf = fg[:, x0:x1]
    rows = np.where(pf.any(1))[0]
    if len(rows) == 0: continue
    top, bot = rows[0], rows[-1]
    HH = bot - top                       # heel..helm-top reference

    # width profile (max-min x span per row, in panel coords)
    w = np.zeros(H)
    lo = np.full(H, -1); hi = np.full(H, -1)
    for y in range(top, bot + 1):
        xs = np.where(pf[y])[0]
        if len(xs): lo[y], hi[y], w[y] = xs[0], xs[-1], xs[-1]-xs[0]+1
    ws = smooth(w.astype(float), 11)

    def band_minimum(a, b):
        seg = ws[a:b].copy()
        return a + int(np.argmin(seg)) if b > a else a

    # neck: narrowest row in top 12-28% band
    neck = band_minimum(top + int(.10*HH), top + int(.30*HH))
    # waist/belt: narrowest in 42-60% band
    waist = band_minimum(top + int(.40*HH), top + int(.62*HH))

    # brown (tunic+skirt) bbox inside panel
    pb = brown[:, x0:x1]
    br_rows = np.where(pb.sum(1) > 3)[0]
    skirt_hem = br_rows[-1] if len(br_rows) else waist + int(.12*HH)

    # knee: narrowest between hem and bottom-15%
    knee = band_minimum(skirt_hem + int(.03*HH), bot - int(.18*HH))
    # ankle: narrowest in bottom 12-22%
    ankle = band_minimum(bot - int(.20*HH), bot - int(.06*HH))

    def span(a, b):
        seg_lo = [lo[y] for y in range(a, b+1) if lo[y] >= 0]
        seg_hi = [hi[y] for y in range(a, b+1) if hi[y] >= 0]
        if not seg_lo: return (x0, x0+1)
        return (x0 + min(seg_lo), x0 + max(seg_hi) + 1)

    parts = {}
    def add(name, ya, yb):
        sx0, sx1 = span(ya, yb)
        parts[name] = dict(x=int(sx0), y=int(ya), w=int(sx1-sx0), h=int(yb-ya),
                           pct_h=round(100*(yb-ya)/HH, 1))

    add("helmet", top, neck)
    add("torso",  neck, waist)
    add("skirt",  waist, skirt_hem)
    add("thigh",  skirt_hem, knee)
    add("shin",   knee, ankle)
    add("foot",   ankle, bot)

    # shield (front only): brown/dark blob LEFT of body centre
    if view == "front":
        cx = (x0 + x1)//2
        sh = (brown[:, x0:cx] | ((bright[:, x0:cx] < 90) & fg[:, x0:cx]))
        ys, xs = np.where(sh)
        if len(xs) > 50:
            # keep leftmost cluster (shield juts out past torso)
            parts["shield"] = dict(x=int(x0+xs.min()), y=int(ys.min()),
                                   w=int(xs.max()-xs.min()), h=int(ys.max()-ys.min()),
                                   pct_h=round(100*(ys.max()-ys.min())/HH,1))

    report[view] = dict(panel=[int(x0), int(x1)], HH=int(HH), parts=parts)

    # draw
    COL = dict(helmet=(0,200,255), torso=(0,255,0), skirt=(255,180,0),
               thigh=(255,0,180), shin=(180,0,255), foot=(0,128,255),
               shield=(0,0,255))
    for name, p in parts.items():
        c = COL.get(name,(255,255,255))
        cv2.rectangle(overlay,(p["x"],p["y"]),(p["x"]+p["w"],p["y"]+p["h"]),c,2)
        cv2.putText(overlay,f"{name} {p['pct_h']}%",(p["x"],max(p["y"]-3,10)),
                    cv2.FONT_HERSHEY_SIMPLEX,0.32,c,1,cv2.LINE_AA)
    # joint lines
    for yj,lbl in [(neck,"neck"),(waist,"waist"),(skirt_hem,"hem"),(knee,"knee"),(ankle,"ankle")]:
        cv2.line(overlay,(x0,yj),(x1,yj),(120,120,120),1)

cv2.imwrite(os.path.join(OUT,"overlay.png"), overlay)
with open(os.path.join(OUT,"proportions.json"),"w") as f:
    json.dump(report,f,indent=2)
print("wrote", os.path.join(OUT,"overlay.png"))
print(json.dumps(report, indent=2))
