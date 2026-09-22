#!/usr/bin/env python3
"""Generate the Background Dancers' MOVIE camera set — the `.camanm` stage-mode
clips used instead of the stage's own cameras while Background Movies =
FULLSCREEN (NO STAGE) has a movie as the backdrop (DDR 5th Mix look).

Why a separate set: the stock stage cameras frame the dancers inside a room
(most of them 5-17 m away, the dancer 15-40 % of the frame height) and the
Griffin House set spends most of its time behind the dancers. With the movie
as the whole background the dancer IS the picture, so these shots stay close
(full body at ~60-75 % of the frame height, a few medium shots), favour the
FRONT (every main shot within +-80 degrees of the dancers' facing, +Z) and
never swing the view off the dancers.

Every shot is a smooth parametric move around the dancer group, described by
keys over the clip's normalised time u in [0, 1] (smootherstep between keys):

    az    azimuth in degrees (0 = straight in front, + = the dancers' left /
          screen right as seen from the front camera... i.e. world +X)
    el    elevation of the eye above the look target, degrees
    vis   visible frame HEIGHT at the look target, metres (sets the distance)
    ly    look-target height, metres
    lx    look-target lateral offset, metres (default 0)
    hfov  in-game horizontal FOV, degrees

The file FOV is derived through the inverse of the game's 16:9 re-projection
(`core::anm::camera::half_tangent`): `t' = tan(hfov/2)`, file vertical FOV =
`2·atan((1 − t'²) / (2·t') / aspect_file)`.

Two variants per shot: `_1p` (one dancer at x = 0) and `_2p` (two dancers at
x = ±0.8 m — no medium shots, no deep side angles where one dancer hides the
other). The mod uses the `_1p` clips with one dancer and the `_2p` clips
with two; `_non` clips are the A3 cut-aways shown for 1-2 s at every dance
change (only their first ~2 s are ever visible). Names without a player tag
are used for both.

    ./scripts/gen_movie_cameras.py                         # write the set
    ./scripts/gen_movie_cameras.py --check <unpacked data> # + framing report

`--check` needs the stock choreography + two bodies unpacked with
`scripts/unpack_arc.py` (`pl_emi00`, `pl_afro00`, `mc_female`, `mc_male` under
`<root>/data/chara/`): every camera frame is tested against the key points
(head top, hands, feet, hips) of poses sampled every 15 frames from every A3
dance clip, placed like the game places the dancers, and the report gives how
often something leaves the HUD-safe frame and how tall the dancer reads.
"""
import argparse
import glob
import math
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import anm_dump as ad  # noqa: E402

REPO = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
DEFAULT_OUT = os.path.join(REPO, "data_mods", "background_dancers", "movie_camera")

FPS = 60
MAIN_FRAMES = 450   # 7.5 s of dance time, the stock stage-clip length
NON_FRAMES = 150    # cut-aways are held 1-2 s from frame 0
ASPECT_FILE = 4.0 / 3.0
OUTPUT_ASPECT = 16.0 / 9.0
NEAR = 0.1
FAR = 32768.0
DUO_PITCH = 1.6     # A3 placement: x = (i - (n-1)/2) * 1.6

# ---------------------------------------------------------------------------
# Shot table
# ---------------------------------------------------------------------------
# (name, frames, description, keys for ONE dancer, keys for TWO dancers);
# `None` = no clip for that count. The framing values (`vis`, `ly`) were
# fitted with `--check <data> --fit` against the stock choreography: a FULL
# shot (vis >= 1.9 m) keeps the whole figure — head top, hands, feet — inside
# the HUD-safe frame for ~96 % of the poses (the dancers travel ±0.6 m
# sideways and ±1 m in depth, so the figure reads ~55-60 % of the frame
# height); a MEDIUM shot (vis < 1.9 m) keeps head and hips in and lets the
# feet go (the figure fills the frame). Every main shot faces the dancers'
# FRONT (|az| <= 80°); nothing looks at their backs, nothing tilts away.
MAIN = MAIN_FRAMES
NON = NON_FRAMES


def K(u, az, el, vis, ly, hfov=50.0, lx=0.0):
    return (u, dict(az=az, el=el, vis=vis, ly=ly, lx=lx, hfov=hfov))


SHOTS = [
    # --- main list (cycled) --------------------------------------------------
    ("st01", MAIN, "front push-in",
     [K(0.0, 0, 9, 2.75, 0.80), K(1.0, 6, 5, 2.50, 0.80)],
     [K(0.0, 0, 9, 2.90, 0.72), K(1.0, 6, 5, 2.66, 0.74)]),
    ("st02", MAIN, "slow orbit, front-left to front-right",
     [K(0.0, -40, 11, 2.60, 0.80), K(1.0, 28, 9, 2.56, 0.78)],
     [K(0.0, -40, 11, 2.86, 0.70), K(1.0, 28, 9, 2.80, 0.70)]),
    ("st03", MAIN, "low hero angle from the left, rising",
     [K(0.0, -32, -10, 2.40, 0.86, 48), K(1.0, -18, -4, 2.42, 0.82, 48)],
     [K(0.0, -32, -10, 2.62, 0.92, 48), K(1.0, -18, -4, 2.60, 0.86, 48)]),
    ("st04", MAIN, "crane down from above the front",
     [K(0.0, 12, 42, 2.95, 0.84), K(1.0, 4, 12, 2.56, 0.78)],
     [K(0.0, 12, 42, 3.32, 0.74), K(1.0, 4, 12, 2.80, 0.70)]),
    ("st05", MAIN, "medium shot, front-right, drifting in",
     [K(0.0, 34, 6, 1.72, 1.05, 46), K(1.0, 20, 4, 1.67, 1.05, 46)],
     None),
    ("st06", MAIN, "wide orbit through the front, right to left",
     [K(0.0, 62, 14, 2.66, 0.72), K(1.0, -14, 12, 2.60, 0.76)],
     [K(0.0, 44, 14, 3.00, 0.68), K(1.0, -14, 12, 2.82, 0.70)]),
    ("st07", MAIN, "pull back from the upper body to the full figure",
     [K(0.0, -8, 4, 1.70, 1.06, 46), K(0.3, -8, 4, 1.72, 1.05, 46), K(1.0, -4, 7, 2.56, 0.80, 46)],
     None),
    ("st08", MAIN, "high three-quarter from the left, easing in",
     [K(0.0, -46, 28, 2.68, 0.76), K(1.0, -30, 22, 2.60, 0.76)],
     [K(0.0, -44, 28, 3.30, 0.64), K(1.0, -30, 22, 3.00, 0.66)]),
    ("st09", MAIN, "side profile opening to the front three-quarter",
     [K(0.0, 80, 5, 2.48, 0.78), K(1.0, 42, 7, 2.52, 0.80)],
     None),
    ("st10", MAIN, "straight front, gentle rise and drift",
     [K(0.0, -6, 0, 2.52, 0.82, 54), K(1.0, 4, 10, 2.56, 0.78, 54)],
     [K(0.0, -6, 0, 2.72, 0.80, 54), K(1.0, 4, 10, 2.76, 0.74, 54)]),
    ("st11", MAIN, "low front push between the pair",
     None,
     [K(0.0, 0, -6, 2.85, 0.86, 52), K(1.0, 0, -2, 2.58, 0.80, 52)]),
    ("st12", MAIN, "orbit in from the right dancer's side",
     None,
     [K(0.0, 40, 12, 2.95, 0.64), K(1.0, 12, 10, 2.72, 0.72)]),
    ("st13", MAIN, "medium low angle from the front-left",
     [K(0.0, -26, -6, 1.78, 1.02, 46), K(1.0, -12, -2, 1.72, 1.02, 46)],
     None),
    ("st14", MAIN, "medium orbit across the front",
     [K(0.0, 22, 8, 1.72, 1.05, 46), K(1.0, -20, 8, 1.72, 1.05, 46)],
     None),
    # --- cut-aways (only their first 1-2 s are shown, at a dance change) -----
    ("non01", NON, "low front push, looking up",
     [K(0.0, 18, -16, 2.48, 0.86), K(1.0, 10, -12, 2.32, 0.88)],
     [K(0.0, 18, -16, 2.68, 0.92), K(1.0, 10, -12, 2.52, 0.92)]),
    ("non02", NON, "high, steep from the front",
     [K(0.0, 6, 48, 2.70, 1.00), K(1.0, -6, 44, 2.70, 0.96)],
     [K(0.0, 6, 48, 3.10, 0.90), K(1.0, -6, 44, 3.00, 0.88)]),
    ("non03", NON, "close profile slide from the left",
     [K(0.0, -80, 3, 1.68, 1.06, 46), K(1.0, -62, 3, 1.66, 1.06, 46)],
     None),
    ("non04", NON, "quick whip orbit, front-right to front",
     [K(0.0, 52, 12, 2.62, 0.74), K(0.7, 8, 10, 2.54, 0.78), K(1.0, 4, 10, 2.54, 0.78)],
     [K(0.0, 44, 12, 2.92, 0.66), K(0.7, 8, 10, 2.72, 0.72), K(1.0, 4, 10, 2.72, 0.72)]),
    ("non05", NON, "from above the right shoulder line",
     None,
     [K(0.0, 28, 34, 3.40, 0.62), K(1.0, 16, 30, 3.12, 0.66)]),
    ("non06", NON, "medium low push from the front-left",
     [K(0.0, -20, -8, 1.84, 1.02, 46), K(1.0, -12, -5, 1.74, 1.02, 46)],
     None),
]


# ---------------------------------------------------------------------------
# Maths
# ---------------------------------------------------------------------------
def smootherstep(x):
    x = max(0.0, min(1.0, x))
    return x * x * x * (x * (6 * x - 15) + 10)


def params_at(keys, u):
    if u <= keys[0][0]:
        return dict(keys[0][1])
    for (u0, a), (u1, b) in zip(keys, keys[1:]):
        if u <= u1:
            s = smootherstep((u - u0) / (u1 - u0) if u1 > u0 else 1.0)
            return {k: a[k] + (b[k] - a[k]) * s for k in a}
    return dict(keys[-1][1])


def sub(a, b):
    return [a[i] - b[i] for i in range(3)]


def cross(a, b):
    return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]


def dot(a, b):
    return sum(a[i] * b[i] for i in range(3))


def norm(a):
    n = math.sqrt(dot(a, a)) or 1.0
    return [c / n for c in a]


def v_tan(hfov):
    """Vertical half-tangent the game renders with for an in-game hFOV."""
    return math.tan(math.radians(hfov) / 2.0) / OUTPUT_ASPECT


def camera_at(p):
    """(eye, target, hfov) for one parameter set (dancer-group space, metres)."""
    d = p["vis"] / (2.0 * v_tan(p["hfov"]))
    el, az = math.radians(p["el"]), math.radians(p["az"])
    target = [p["lx"], p["ly"], 0.0]
    eye = [target[0] + d * math.cos(el) * math.sin(az),
           target[1] + d * math.sin(el),
           target[2] + d * math.cos(el) * math.cos(az)]
    return eye, target, p["hfov"]


def file_fov(hfov):
    """The .camanm vertical FOV (degrees) that the game turns into `hfov`."""
    t = math.tan(math.radians(hfov) / 2.0)
    h = (1.0 - t * t) / (2.0 * t)
    return math.degrees(2.0 * math.atan(h / ASPECT_FILE))


def game_hfov(fov_file, aspect_file=ASPECT_FILE):
    h = math.tan(math.radians(fov_file) / 2.0) * aspect_file
    return math.degrees(2.0 * 0.5 * math.atan2(2.0, 2.0 * h))


def look_quat(eye, target):
    """Quaternion (x, y, z, w) whose row matrix (anm_dump.quat_to_rowmat) has
    row2 = back (eye - target), row1 = up, row0 = right — the game camera
    looks down its local -Z with local +Y up."""
    zb = norm(sub(eye, target))
    xr = norm(cross([0.0, 1.0, 0.0], zb))
    yu = cross(zb, xr)
    R = [xr, yu, zb]                       # rows = local axes in world
    m = [[R[j][i] for j in range(3)] for i in range(3)]   # column convention
    tr = m[0][0] + m[1][1] + m[2][2]
    if tr > 0:
        s = math.sqrt(tr + 1.0) * 2
        w, x, y, z = 0.25 * s, (m[2][1] - m[1][2]) / s, (m[0][2] - m[2][0]) / s, (m[1][0] - m[0][1]) / s
    elif m[0][0] > m[1][1] and m[0][0] > m[2][2]:
        s = math.sqrt(1.0 + m[0][0] - m[1][1] - m[2][2]) * 2
        w, x, y, z = (m[2][1] - m[1][2]) / s, 0.25 * s, (m[0][1] + m[1][0]) / s, (m[0][2] + m[2][0]) / s
    elif m[1][1] > m[2][2]:
        s = math.sqrt(1.0 + m[1][1] - m[0][0] - m[2][2]) * 2
        w, x, y, z = (m[0][2] - m[2][0]) / s, (m[0][1] + m[1][0]) / s, 0.25 * s, (m[1][2] + m[2][1]) / s
    else:
        s = math.sqrt(1.0 + m[2][2] - m[0][0] - m[1][1]) * 2
        w, x, y, z = (m[1][0] - m[0][1]) / s, (m[0][2] + m[2][0]) / s, (m[1][2] + m[2][1]) / s, 0.25 * s
    q = (x, y, z, w)
    Rq = ad.quat_to_rowmat(q)
    err = max(abs(Rq[i][j] - R[i][j]) for i in range(3) for j in range(3))
    if err > 1e-6:
        raise AssertionError("quaternion round trip %.2e" % err)
    return q


# ---------------------------------------------------------------------------
# Clip building
# ---------------------------------------------------------------------------
def variants():
    """[(clip name, frames, keys, desc, dancers)] for every variant of every shot."""
    out = []
    for name, frames, desc, solo, pair in SHOTS:
        kind = "non" if name.startswith("non") else "st"
        stem = "movie_%s%s" % (kind, name[len(kind):])
        if solo:
            out.append(("%s_1p" % stem, frames, solo, desc, 1))
        if pair:
            out.append(("%s_2p" % stem, frames, pair, desc, 2))
    return out


def clip_frames(keys, frames):
    """Per-frame (eye, target, hfov)."""
    return [camera_at(params_at(keys, f / float(frames))) for f in range(frames + 1)]


def build_spec(cams, frames):
    quats, poss, fovs = [], [], []
    prev = None
    for eye, target, hfov in cams:
        q = look_quat(eye, target)
        if prev is not None and sum(a * b for a, b in zip(q, prev)) < 0:
            q = tuple(-c for c in q)          # keep the key stream hemisphere-continuous
        prev = q
        quats.append(q)
        poss.append(tuple(c * 100.0 for c in eye))     # metres -> centimetres
        fovs.append((file_fov(hfov),))
    fov_const = max(abs(f[0] - fovs[0][0]) for f in fovs) < 1e-4
    slots = [
        dict(kind=1, target=0, keys=quats),
        dict(kind=4, target=1, keys=poss),
        dict(kind=8, target=2, times=[0] if fov_const else None, keys=[fovs[0]] if fov_const else fovs),
        dict(kind=8, target=3, times=[0], keys=[(NEAR,)]),
        dict(kind=8, target=4, times=[0], keys=[(FAR,)]),
        dict(kind=8, target=5, times=[0], keys=[(ASPECT_FILE,)]),
    ]
    return dict(frame_count=frames, flag=0, fps=FPS, camera=slots)


def game_camera(parsed, frame):
    """The game recipe (`core::anm::camera::camera_from_slots`) at a frame."""
    cam = next(c for c in parsed["chunks"] if c["type"] == 4)
    T = cam["tracks"]
    d = parsed["data"]
    q = ad.sample_track(d, T[0], frame)
    pos = ad.sample_track(d, T[1], frame)
    fov = ad.sample_track(d, T[2], frame)[0]
    asp = ad.sample_track(d, T[5], frame)[0]
    R = ad.quat_to_rowmat(q)
    eye = [c * 0.01 for c in pos]
    fwd = [-R[2][i] for i in range(3)]
    up = norm(R[1])
    return eye, fwd, up, game_hfov(fov, asp)


def verify(out_bytes, cams):
    parsed = ad.parse_anm(out_bytes)
    worst_eye = worst_dir = worst_fov = 0.0
    for f in range(0, len(cams), 7):
        eye_w, target_w, hfov_w = cams[f]
        eye, fwd, up, hfov = game_camera(parsed, f)
        want = norm(sub(target_w, eye_w))
        worst_eye = max(worst_eye, max(abs(a - b) for a, b in zip(eye, eye_w)))
        worst_dir = max(worst_dir, math.degrees(math.acos(max(-1.0, min(1.0, dot(fwd, want))))))
        worst_fov = max(worst_fov, abs(hfov - hfov_w))
        if up[1] < 0.5:
            raise AssertionError("camera rolled over (up.y = %.2f)" % up[1])
    return worst_eye, worst_dir, worst_fov


# ---------------------------------------------------------------------------
# Framing check
# ---------------------------------------------------------------------------
# (bone, offset along the bone's local axes): extremities from the stock AABBs.
POINTS = [("Head", (0.24, 0, 0)), ("Head", (0, 0, 0)), ("Hips", (0, 0, 0)),
          ("LeftHand", (0.18, 0, 0)), ("RightHand", (-0.18, 0, 0)),
          ("LeftToeBase", (0.08, 0, 0)), ("RightToeBase", (-0.08, 0, 0)),
          ("LeftFoot", (0, -0.1, 0)), ("RightFoot", (0, -0.1, 0))]
BODIES = {"female": ("pl_emi00", 0.9), "male": ("pl_afro00", 1.0)}
# HUD-safe frame in NDC (the lifebar / banner at the top, the score strip at
# the bottom): anything outside counts as "cut".
SAFE_X = 0.94
SAFE_TOP = 0.80
SAFE_BOTTOM = -0.80


def pose_cloud(root, step=15):
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    import ktmdl_dump as kd  # noqa: E402

    def xf(p, M):
        return [p[0] * M[0][j] + p[1] * M[1][j] + p[2] * M[2][j] + M[3][j] for j in range(3)]
    samples = []
    for sex, (key, scale) in BODIES.items():
        base = os.path.join(root, "data", "chara")
        names = dict(kd.parse_b2it(open(os.path.join(base, key, key + ".b2it"), "rb").read()))
        for path in sorted(glob.glob(os.path.join(base, "mc_" + sex, "*.anm"))):
            if "_tu01" in path:
                continue
            a = ad.parse_anm(open(path, "rb").read())
            hier = next(c for c in a["chunks"] if c["type"] == 1)
            parents = [(-1 if p == 0xFF else p) for _, p in hier["pairs"]]
            for fr in range(0, a["header"]["frame_count"], step):
                pose = ad.evaluate_pose(a, fr, parents)
                samples.append([[c * scale for c in xf(off, pose[names[b]]["world"])] for b, off in POINTS])
    return samples


def basis(cam):
    """Precomputed projection basis for a (eye, fwd, up, hfov) camera."""
    eye, fwd, up, hfov = cam
    right = norm(cross(fwd, up))
    upv = cross(right, fwd)
    tx = math.tan(math.radians(hfov) / 2.0)
    return (eye, fwd, right, upv, tx, tx / OUTPUT_ASPECT)


def project(p, b):
    eye, fwd, right, upv, tx, ty = b
    vx, vy, vz = p[0] - eye[0], p[1] - eye[1], p[2] - eye[2]
    z = vx * fwd[0] + vy * fwd[1] + vz * fwd[2]
    if z <= 0.05:
        return None
    return ((vx * right[0] + vy * right[1] + vz * right[2]) / (z * tx),
            (vx * upv[0] + vy * upv[1] + vz * upv[2]) / (z * ty))


# A frame narrower than this cannot hold a whole dancer: it is judged as a
# MEDIUM shot (head, head top and hips must stay in the safe frame; hands and
# feet may leave it). Wider frames are FULL shots (every key point counts).
MEDIUM_VIS = 1.9
MEDIUM_POINTS = (0, 1, 2)


def pose_cut(pose_pts, b, medium):
    """(any counted point cut, head cut, (y min, y max) of the projected points)
    for one placed pose against a `basis()` camera."""
    any_cut = head_cut = False
    ys = []
    for pi, p in enumerate(pose_pts):
        s = project(p, b)
        if s is None:
            any_cut = True
            continue
        sx, sy = s
        ys.append(sy)
        inside = abs(sx) <= SAFE_X and SAFE_BOTTOM <= sy <= SAFE_TOP
        if not inside:
            if not medium or (pi % len(POINTS)) in MEDIUM_POINTS:
                any_cut = True
            if (pi % len(POINTS)) in (0, 1):
                head_cut = True
    return any_cut, head_cut, (min(ys), max(ys)) if ys else None


def placed(cloud, dancers, si, salt):
    """One pose per dancer, placed at the A3 x offsets (independent poses)."""
    n = len(cloud)
    pts = []
    for d in range(dancers):
        x0 = (d - (dancers - 1) * 0.5) * DUO_PITCH
        pose = cloud[(si + 997 * d + 131 * salt) % n]
        pts.extend([p[0] + x0, p[1], p[2]] for p in pose)
    return pts


def check(parsed, frames, dancers, cloud, vis_at):
    """(cut %, head-cut %, median height, p10 height, class) over camera frames x poses."""
    cut = head_cut = total = 0
    heights = []
    medium_frames = 0
    for fi, f in enumerate(range(0, frames + 1, 30)):
        cam = basis(game_camera(parsed, f))
        medium = vis_at(f) < MEDIUM_VIS
        medium_frames += medium
        for si in range(0, len(cloud), 3):
            total += 1
            c, h, span = pose_cut(placed(cloud, dancers, si, fi), cam, medium)
            cut += c
            head_cut += h
            if span:
                heights.append((span[1] - span[0]) / 2.0)
    heights.sort()
    cls = "medium" if medium_frames * 2 > (frames // 30 + 1) else "full"
    return (100.0 * cut / total, 100.0 * head_cut / total,
            heights[len(heights) // 2], heights[len(heights) // 10], cls)


FIT_POSE_STEP = 8        # every 8th sampled pose (~290 of ~2350)
FIT_BUDGET = 4.0         # % of poses allowed to cut


def static_rate(p, dancers, cloud, medium):
    """Cut rate (%) of a STATIC camera at parameter set `p`."""
    eye, target, hfov = camera_at(p)
    fwd = norm(sub(target, eye))
    right = norm(cross(fwd, [0.0, 1.0, 0.0]))
    b = basis((eye, fwd, cross(right, fwd), hfov))
    idx = range(0, len(cloud), FIT_POSE_STEP)
    bad = sum(pose_cut(placed(cloud, dancers, si, 0), b, medium)[0] for si in idx)
    return 100.0 * bad / len(idx)


def min_vis(p, dancers, cloud, medium, lo, hi):
    """Smallest vis in [lo, hi] meeting FIT_BUDGET (bisection — the cut rate
    falls as the frame grows), or None when even `hi` cuts too often."""
    if static_rate(dict(p, vis=hi), dancers, cloud, medium) > FIT_BUDGET:
        return None
    for _ in range(7):                       # ~1 cm resolution over a 2 m range
        mid = 0.5 * (lo + hi)
        if static_rate(dict(p, vis=mid), dancers, cloud, medium) <= FIT_BUDGET:
            hi = mid
        else:
            lo = mid
    return hi


def fit(keys, dancers, cloud):
    """For every key: the smallest `vis` (and the `ly` it needs) keeping the
    cut rate at or under FIT_BUDGET % for a STATIC camera at that key's
    az / el / hfov / lx — the tuning aid behind the table (`--fit`). `ly` is
    searched coarse (0.10 m) then fine (0.02 m) around the best coarse value."""
    out = []
    for u, p in keys:
        medium = p["vis"] < MEDIUM_VIS
        lo, hi = (1.0, MEDIUM_VIS - 0.01) if medium else (MEDIUM_VIS, 3.8)

        def best_over(lys):
            best = None
            for ly in lys:
                v = min_vis(dict(p, ly=ly), dancers, cloud, medium, lo, hi)
                if v is not None and (best is None or v < best[0]):
                    best = (v, ly)
            return best
        coarse = best_over([x / 100.0 for x in range(60, 131, 10)])
        if coarse is None:
            out.append((u, p, None))
            continue
        c = int(round(coarse[1] * 100))
        fine = best_over([x / 100.0 for x in range(max(60, c - 8), min(130, c + 8) + 1, 2)]) or coarse
        rate = static_rate(dict(p, vis=fine[0], ly=fine[1]), dancers, cloud, medium)
        out.append((u, p, (fine[0], fine[1], rate)))
    return out


# ---------------------------------------------------------------------------
def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("-o", "--out", default=DEFAULT_OUT, help="output directory (default: %(default)s)")
    ap.add_argument("--check", metavar="DATA_ROOT", help="unpacked arcs root for the framing report")
    ap.add_argument("--fit", action="store_true",
                    help="with --check: also print, per key, the tightest vis/ly keeping cuts <= 4 %%")
    ap.add_argument("--only", action="append", default=[], metavar="SUBSTR",
                    help="limit the report / fit to clips whose name contains SUBSTR (repeatable)")
    ap.add_argument("--dry-run", action="store_true", help="build + verify, write nothing")
    args = ap.parse_args()
    if args.fit and not args.check:
        ap.error("--fit needs --check DATA_ROOT")

    cloud = pose_cloud(args.check) if args.check else None
    if cloud is not None:
        print("framing check over %d choreography poses (female %.1f, male %.1f scale)" % (
            len(cloud), BODIES["female"][1], BODIES["male"][1]))
    if not args.dry_run:
        os.makedirs(args.out, exist_ok=True)
        for old in glob.glob(os.path.join(args.out, "movie_*.camanm")):
            os.remove(old)
    for name, frames, keys, desc, dancers in variants():
        cams = clip_frames(keys, frames)
        out = ad.write_anm(build_spec(cams, frames))
        e_eye, e_dir, e_fov = verify(out, cams)
        if e_eye > 1e-3 or e_dir > 0.05 or e_fov > 0.05:
            sys.exit("%s: verification failed (eye %.2e m, dir %.3f deg, fov %.3f deg)" % (name, e_eye, e_dir, e_fov))
        d0 = math.dist(cams[0][0], cams[0][1])
        d1 = math.dist(cams[-1][0], cams[-1][1])
        line = "%-18s %3d fr  d %.1f->%.1f m  %s" % (name, frames, d0, d1, desc)
        report = cloud is not None and (not args.only or any(o in name for o in args.only))
        if report:
            def vis_at(f, keys=keys, frames=frames):
                return params_at(keys, f / float(frames))["vis"]
            cut, hcut, h50, h10, cls = check(ad.parse_anm(out), frames, dancers, cloud, vis_at)
            line += "\n%18s %-6s cut %5.1f %%  head cut %4.1f %%  figure height %.0f %% of frame (p10 %.0f %%)" % (
                "", cls, cut, hcut, 100 * h50, 100 * h10)
        print(line, flush=True)
        if report and args.fit:
            for u, p, best in fit(keys, dancers, cloud):
                print("%18s   u=%.2f az %4.0f el %3.0f hfov %2.0f: vis %.2f ly %.2f -> %s" % (
                    "", u, p["az"], p["el"], p["hfov"], p["vis"], p["ly"],
                    "fit vis %.2f ly %.2f (%.1f %%)" % best if best else "no fit"), flush=True)
        if not args.dry_run:
            with open(os.path.join(args.out, name + ".camanm"), "wb") as fh:
                fh.write(out)
    if not args.dry_run:
        print("written to", os.path.relpath(args.out, REPO))


if __name__ == "__main__":
    main()
