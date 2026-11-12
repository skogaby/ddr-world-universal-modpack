import sys, os, glob; sys.path.insert(0, os.path.dirname(__file__)); sys.path.insert(0, 'scripts')
import render as R
from render import *
wrap_sample = R.sample
def sample_clamp(tex, uv):
    H, W = tex.shape[:2]; x = np.clip(uv[:, 0], 0, 1) * W - 0.5; y = np.clip(uv[:, 1], 0, 1) * H - 0.5
    x0 = np.clip(np.floor(x).astype(int), 0, W - 1); y0 = np.clip(np.floor(y).astype(int), 0, H - 1); x1 = np.clip(x0 + 1, 0, W - 1); y1 = np.clip(y0 + 1, 0, H - 1)
    fx = np.clip(x - x0, 0, 1)[:, None]; fy = np.clip(y - y0, 0, 1)[:, None]
    return (tex[y0, x0] * (1 - fx) + tex[y0, x1] * fx) * (1 - fy) + (tex[y1, x0] * (1 - fx) + tex[y1, x1] * fx) * fy
MODE = os.environ.get('MODE', 'all')  # all | add | none
R.sample = sample_clamp if MODE == 'all' else (wrap_sample if MODE == 'none' else None)
if MODE == 'add':
    CL = set()
    R.sample = lambda t, uv: sample_clamp(t, uv) if id(t) in CL else wrap_sample(t, uv)
ms = load('fly_bg', 240) + load('fly_add', 240) + load('fly_ble', 240)
if MODE == 'add':
    for M in ms:
        if M['part'] == 'fly_add' and M['tex'] is not None: CL.add(id(M['tex']))
c = sorted(glob.glob(D + 'camera/*_st0*.camanm'))
imgs = [render_cam(ms, c[i], fr, 640, 360) for i, fr in ((0, 60), (4, 60), (1, 90), (3, 30))]
Image.fromarray(np.concatenate([np.concatenate(imgs[:2], 1), np.concatenate(imgs[2:], 1)], 0)).save(OUT + 'clamp_%s.png' % MODE)
