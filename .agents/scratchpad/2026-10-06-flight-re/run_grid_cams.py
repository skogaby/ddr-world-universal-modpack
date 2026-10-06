import sys, os, glob; sys.path.insert(0, os.path.dirname(__file__)); sys.path.insert(0, 'scripts')
import render as R
from render import *
def sample_clamp(tex, uv):
    H, W = tex.shape[:2]; x = np.clip(uv[:, 0], 0, 1) * W - 0.5; y = np.clip(uv[:, 1], 0, 1) * H - 0.5
    x0 = np.clip(np.floor(x).astype(int), 0, W - 1); y0 = np.clip(np.floor(y).astype(int), 0, H - 1); x1 = np.clip(x0 + 1, 0, W - 1); y1 = np.clip(y0 + 1, 0, H - 1)
    fx = np.clip(x - x0, 0, 1)[:, None]; fy = np.clip(y - y0, 0, 1)[:, None]
    return (tex[y0, x0] * (1 - fx) + tex[y0, x1] * fx) * (1 - fy) + (tex[y1, x0] * (1 - fx) + tex[y1, x1] * fx) * fy
wrap = R.sample
grid = [m for m in load('fly_ble', 240) if m['mesh'] == 0]
# a camera inside the tube on the axis looking down +z, like the cabinet shot
rows = []
for mode, fn in (('wrap', wrap), ('clamp', sample_clamp)):
    R.sample = fn
    rows.append(render(grid, eye=(0, 0, -20), fwd=(0, 0.15, 1), up=(0, 1, 0), fov=70, Wd=640, Hd=360))
Image.fromarray(np.concatenate(rows, 1)).save(OUT + 'grid_axis_wrap_vs_clamp.png')
