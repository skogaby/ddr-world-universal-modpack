# Q4: 60 Hz emulation of the MUSIC FIT grid tube (fly_ble = DRAW_B03_grid01) with the Wii's exact
# per-frame texture-matrix translation (m13 += 0.125 per 60 Hz frame, wrapped) from a stage camera.
# Writes a frame strip + a 1-D space-time diagram of the ring pattern along the tube axis.
import sys, os, glob, numpy as np
sys.path.insert(0, os.path.dirname(__file__)); sys.path.insert(0, 'scripts')
import render as R
from render import *
from PIL import Image
OUT = R.OUT
frames = int(os.environ.get('FRAMES', '8'))
cam = sorted(glob.glob(D + 'camera/*_st0*.camanm'))[int(os.environ.get('CAM', '0'))]
imgs = []
for n in range(frames):
    # the DLL samples the 8-frame .sanm at frame n (offV = n/8 at 60 Hz) -- identical to the Wii's
    # m13 = frac(0.125 * n)
    ms = load('fly_ble', n)
    imgs.append(render_cam([m for m in ms if m['mesh'] == 0], cam, 60, 480, 270))
Image.fromarray(np.concatenate(imgs, 1)).save(OUT + 'emu_grid_strip.png')
# 1-D space-time: alpha of the grid texture along v (the tube axis), per frame, 1.3 units / px
import zan_dump as z
b = open(os.path.expanduser('~/Desktop/DDR Wii ISOs/Dance Dance Revolution - Music Fit (Japan)/stage/STG201.bin'), 'rb').read()
tex = np.asarray(z.tpl_images(b[0x1b080:0x1b080 + 139936])[14])
col = tex[:, 64, 3].astype(float) / 255  # alpha along v at mid-u (no longitudinal line)
rows = []
for n in range(6):
    shift = int(round(128 * 0.125 * n))
    rows.append(np.roll(col, -shift))  # t' = t + m13: the pattern moves toward -v
rows = np.array(rows)
print('alpha peaks (v px) per 60 Hz frame; the head ring (alpha 1.0) and its three trails:')
for n, r in enumerate(rows):
    peaks = [i for i in range(128) if r[i] > 0.2 and r[i] >= r[(i - 1) % 128] and r[i] >= r[(i + 1) % 128]]
    print(' f%d' % n, [(p, round(float(r[p]), 2)) for p in peaks])
