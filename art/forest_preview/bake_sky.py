"""Project a licensed radiance panorama into the game's fixed cube-face convention."""
from pathlib import Path
import hashlib
import json
import math
import cv2
import numpy as np

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / 'art/forest_preview/polyhaven/kloofendal_48d_partly_cloudy_puresky_2k.hdr'
OUT = ROOT / 'assets/models/forest_slice/sky_cube_rgba16f.bin'
FACE = 512
hdr = cv2.imread(str(SOURCE), cv2.IMREAD_UNCHANGED)[..., ::-1].copy()
assert hdr.dtype == np.float32 and np.isfinite(hdr).all()
h, w, _ = hdr.shape
lum = hdr @ np.array([0.2126, 0.7152, 0.0722], dtype=np.float32)
sun_y, sun_x = np.unravel_index(lum.argmax(), lum.shape)
source_phi = ((sun_x + .5) / w - .5) * math.tau
theta = (sun_y + .5) / h * math.pi
target_phi = math.atan2(-.24, -.97)
yaw = source_phi - target_phi
# Preserve natural cloud contrast; a bounded sun avoids fp16 overflow and excessive bloom.
gain = .85 / float(np.percentile(lum[:h//2], 95))
q = (np.arange(FACE, dtype=np.float32) + .5) / FACE * 2 - 1
u, v = np.meshgrid(q, q)
one = np.ones_like(u)
faces = [(one,-v,-u),(-one,-v,u),(u,one,v),(u,-one,-v),(u,-v,one),(-u,-v,-one)]
payload = bytearray()
for xyz in faces:
    vec = np.stack(xyz, axis=-1)
    vec /= np.linalg.norm(vec, axis=-1, keepdims=True)
    phi = np.arctan2(vec[...,2], vec[...,0]) + yaw
    map_x = (((phi / math.tau + .5) % 1) * w - .5).astype(np.float32)
    map_y = (np.arccos(vec[...,1]) / math.pi * h - .5).astype(np.float32)
    rgb = cv2.remap(hdr, map_x, map_y, cv2.INTER_LINEAR, borderMode=cv2.BORDER_WRAP)
    rgba = np.concatenate([np.clip(rgb * gain, 0, 8), one[...,None]], axis=-1)
    payload.extend(rgba.astype('<f2').tobytes())
OUT.write_bytes(payload)
direction = [math.sin(theta)*math.cos(target_phi), math.cos(theta), math.sin(theta)*math.sin(target_phi)]
report = {'schema':'warbell.forest_slice_sky.v1','source_page':'https://polyhaven.com/a/kloofendal_48d_partly_cloudy_puresky',
          'license':'CC0-1.0','authors':['Greg Zaal','Jarod Guest'],'source_sha256':hashlib.sha256(SOURCE.read_bytes()).hexdigest(),
          'output_sha256':hashlib.sha256(payload).hexdigest(),'format':'RGBA16Float little endian, +X -X +Y -Y +Z -Z',
          'face_size':FACE,'bytes':len(payload),'radiance_gain':gain,'radiance_clamp':8,
          'skybox_brightness':1800,'source_panorama_dimensions':[w,h],'world_sun_direction':direction,
          'method':'Bilinear equirectangular-to-cubemap projection, yaw aligned to upper-left key light; upper-hemisphere luminance p95 normalized to 0.85.'}
(ROOT/'art/forest_preview/sky_report.json').write_text(json.dumps(report,indent=2))
print(json.dumps(report,indent=2))
