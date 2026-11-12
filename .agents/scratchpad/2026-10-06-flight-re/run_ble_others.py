import sys, os, glob; sys.path.insert(0, os.path.dirname(__file__)); sys.path.insert(0, 'scripts')
import render as R
from render import *
def sample_clamp(tex, uv):
    H, W = tex.shape[:2]; x = np.clip(uv[:, 0], 0, 1) * W - 0.5; y = np.clip(uv[:, 1], 0, 1) * H - 0.5
    x0 = np.clip(np.floor(x).astype(int), 0, W - 1); y0 = np.clip(np.floor(y).astype(int), 0, H - 1); x1 = np.clip(x0 + 1, 0, W - 1); y1 = np.clip(y0 + 1, 0, H - 1)
    fx = np.clip(x - x0, 0, 1)[:, None]; fy = np.clip(y - y0, 0, 1)[:, None]
    return (tex[y0, x0] * (1 - fx) + tex[y0, x1] * fx) * (1 - fy) + (tex[y1, x0] * (1 - fx) + tex[y1, x1] * fx) * fy
R.sample = sample_clamp
ble = load('fly_ble', 240)
c = sorted(glob.glob(D + 'camera/*_st0*.camanm'))
a = render_cam([m for m in ble if m['mesh'] != 0], c[1], 90, 640, 360)
b = render_cam(ble, c[1], 90, 640, 360)
Image.fromarray(np.concatenate([a, b], 1)).save(OUT + 'ble_others_vs_all_clamp.png')
for m in ble: print(m['mesh'], m['blend'], len(m['P']), 'z', m['P'][:,2].min().round(), m['P'][:,2].max().round(), 'r', np.hypot(m['P'][:,0],m['P'][:,1]).mean().round(1))
