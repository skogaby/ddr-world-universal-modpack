#!/usr/bin/env python3
"""Write `_non` cut-away shots for a stage camera set that has only main (`_st`) shots.

The Background Dancers director (src/mods/background_dancers/schedule.rs, A3's CameraActor rule)
hard-cuts to the next `_non` shot AT every dance-clip change and holds it 1-2 s, so the dancer's
clip switch is hidden by the camera cut. A set without `_non` shots keeps its main shot running
through the cut and the switch shows (Griffin House and the stages borrowing its set, 2026-10-26).

Each generated shot is derived from one main shot, so it stays inside whatever room that shot
was authored for: the eye starts on the segment from the dancer's chest to the main shot's eye
(NEAR_CM from the chest, at most 95 % of the way out; the main eye sampled at PHASES of the shot)
and pushes in slowly while looking at the chest. Positions are the camanm's centimetres (game space, Y up; the dancer
stands at the origin).

    gen_cutaway_shots.py <camera dir> [--prefix KEY] [--scale S] [--far M]
      --scale  multiply every main shot's eye by S first (a stage ported at a different size
               than the room the shots were authored for) and write the scaled main shots too
      --far    far clip plane in metres (camanm slot 4) for every shot, main ones rewritten too
               (Griffin House's 200 m hid Wall Eyes' ~760 m sky dome)
"""
import argparse
import glob
import math
import os
import re
import sys

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import anm_dump as A  # noqa: E402

FOCUS = np.array([0.0, 115.0, 0.0])   # cm: the dancer's chest
FOV = 46.0                            # camanm slot 2 (the stock shots use 38-44)
LENGTH = 180                          # frames at 60 Hz; the director holds a `_non` 1-2 s
NEAR_CM = 220.0                       # a cut-away starts this far from the chest (a full-figure
PUSH = 0.88                           # medium shot) -- never past the main eye -- and pushes in to
                                      # PUSH of it
PHASES = (0.0, 0.5)                   # where in each main shot the eye is taken


def look_quat(eye, target):
    """(x, y, z, w) of a camera at `eye` looking at `target` (local -Z forward, +Y up; the
    row-vector convention of anm_dump.quat_to_rowmat)."""
    fwd = target - eye
    fwd = fwd / np.linalg.norm(fwd)
    right = np.cross(fwd, [0.0, 1.0, 0.0])
    right /= np.linalg.norm(right)
    up = np.cross(right, fwd)
    m = np.array([right, up, -fwd]).T            # columns = local axes (the transposed rows)
    t = m[0, 0] + m[1, 1] + m[2, 2]
    if t > 0:
        s = 2.0 * math.sqrt(t + 1.0)
        q = ((m[2, 1] - m[1, 2]) / s, (m[0, 2] - m[2, 0]) / s, (m[1, 0] - m[0, 1]) / s, 0.25 * s)
    elif m[0, 0] > m[1, 1] and m[0, 0] > m[2, 2]:
        s = 2.0 * math.sqrt(1.0 + m[0, 0] - m[1, 1] - m[2, 2])
        q = (0.25 * s, (m[0, 1] + m[1, 0]) / s, (m[0, 2] + m[2, 0]) / s, (m[2, 1] - m[1, 2]) / s)
    elif m[1, 1] > m[2, 2]:
        s = 2.0 * math.sqrt(1.0 + m[1, 1] - m[0, 0] - m[2, 2])
        q = ((m[0, 1] + m[1, 0]) / s, 0.25 * s, (m[1, 2] + m[2, 1]) / s, (m[0, 2] - m[2, 0]) / s)
    else:
        s = 2.0 * math.sqrt(1.0 + m[2, 2] - m[0, 0] - m[1, 1])
        q = ((m[0, 2] + m[2, 0]) / s, (m[1, 2] + m[2, 1]) / s, 0.25 * s, (m[1, 0] - m[0, 1]) / s)
    q = np.array(q)
    return tuple(float(v) for v in q / np.linalg.norm(q))


def camera_tracks(data):
    parsed = A.parse_anm(data)
    return parsed, next(c for c in parsed['chunks'] if c['type'] == 4)['tracks']


def scaled_shot(data, scale, far=None):
    """A main shot with every eye position multiplied by `scale` (orientation and lens kept) and,
    given, its far plane (slot 4) replaced."""
    parsed, tracks = camera_tracks(data)
    spec = A.anm_to_spec(parsed)
    for slot, t in enumerate(spec['camera']):
        if t is None:
            continue
        if slot == 1:
            t['keys'] = [tuple(c * scale for c in k) for k in t['keys']]
        elif slot == 4 and far is not None:
            t['keys'] = [(float(far),) for _k in t['keys']]
    return A.write_anm(spec)


def cutaway(eye0, far=200.0):
    """A LENGTH-frame push-in from `eye0`'s direction at the dancer's chest."""
    d = eye0 - FOCUS
    dist = float(np.linalg.norm(d))
    k0 = min(0.95, NEAR_CM / dist)
    times, quats, pos, prev = [], [], [], None
    for f in range(0, LENGTH + 1, 2):
        k = k0 * (1.0 + (PUSH - 1.0) * (f / LENGTH))
        e = FOCUS + d * k
        q = look_quat(e, FOCUS)
        if prev is not None and sum(a * b for a, b in zip(prev, q)) < 0:
            q = tuple(-v for v in q)
        prev = q
        times.append(f)
        quats.append(q)
        pos.append(tuple(float(v) for v in e))

    def const(slot, v):
        return dict(kind=8, target=slot, times=[0], keys=[(float(v),)])
    camera = [dict(kind=1, target=0, times=times, keys=quats), dict(kind=4, target=1, times=times, keys=pos),
              const(2, FOV), const(3, 0.05), const(4, far), const(5, 4.0 / 3.0)]
    return A.write_anm(dict(frame_count=LENGTH, flag=0, fps=60, camera=camera))


def check(data, eye0):
    """Worst view-direction error of the written shot (anm_dump.sample_track)."""
    _parsed, tracks = camera_tracks(data)
    worst = 0.0
    for f in (0.0, LENGTH / 2.0, float(LENGTH)):
        q = A.sample_track(data, tracks[0], f)
        p = np.array(A.sample_track(data, tracks[1], f))
        fwd = -np.array(A.quat_to_rowmat(q)[2])
        want = (FOCUS - p) / np.linalg.norm(FOCUS - p)
        worst = max(worst, float(np.abs(fwd - want).max()))
    return worst


def main(argv):
    ap = argparse.ArgumentParser()
    ap.add_argument('camera_dir')
    ap.add_argument('--prefix')
    ap.add_argument('--scale', type=float, default=1.0)
    ap.add_argument('--far', type=float)
    args = ap.parse_args(argv)
    mains = sorted(f for f in glob.glob(os.path.join(args.camera_dir, '*')) if re.search(r'_st\d+(\.camanm)?$', f))
    if not mains:
        print('no _stNN shots in %s' % args.camera_dir)
        return 1
    prefix = args.prefix or re.sub(r'_st\d+(\.camanm)?$', '', os.path.basename(mains[0]))
    ext = '.camanm' if mains[0].endswith('.camanm') else ''
    for f in glob.glob(os.path.join(args.camera_dir, prefix + '_non*')):
        os.remove(f)
    n = 0
    for path in mains:
        data = open(path, 'rb').read()
        if abs(args.scale - 1.0) > 1e-9 or args.far is not None:
            data = scaled_shot(data, args.scale, args.far)
            open(path, 'wb').write(data)
        parsed, tracks = camera_tracks(data)
        length = parsed['header']['frame_count']
        for ph in PHASES:
            eye = np.array(A.sample_track(data, tracks[1], ph * length))
            n += 1
            out = cutaway(eye, args.far if args.far is not None else 200.0)
            err = check(out, eye)
            assert err < 1e-3, (path, ph, err)
            name = '%s_non%02d%s' % (prefix, n, ext)
            open(os.path.join(args.camera_dir, name), 'wb').write(out)
            k0 = min(0.95, NEAR_CM / float(np.linalg.norm(eye - FOCUS)))
            print('%s <- %s @ %.0f%%: eye (%.0f, %.0f, %.0f), %.0f cm from the chest' % (
                name, os.path.basename(path), 100 * ph, *(FOCUS + (eye - FOCUS) * k0), np.linalg.norm((eye - FOCUS) * k0)))
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
