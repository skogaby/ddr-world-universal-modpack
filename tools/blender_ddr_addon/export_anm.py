"""Export an armature's action as .anm, or a camera object as .camanm.

Skeletal export inverts import_anm: for every frame the pose bone's Blender
armature-space matrix is converted to the game's world row-matrix, the parent's is
removed with the game's own rule (local = world_b · inverse(world_parent), then the
segment-scale compensation undone: columns multiplied by the parent's scale), and the
resulting local TRS is written as kind 0x1C rotation / 0x1D translation / 10 scale
uniform tracks (one key per frame) — exactly what the stock choreography uses.
"""
import math
import os

import bpy
from mathutils import Matrix, Vector

from . import convert
from .codec import anm
from .import_anm import CAMERA_UNIT_SCALE, camanm_fov_from_half_tangent


def _decompose_game_rowmat(m):
    """Row-vector 4x4 (list of 4 rows) -> (quat xyzw, pos, scale) the way FUN_18013ba50 does:
    scale = row lengths, rotation = normalized rows, translation = row 3."""
    rows = [Vector(m[i][:3]) for i in range(3)]
    scale = [r.length for r in rows]
    rot = Matrix([[rows[i][j] / (scale[i] or 1.0) for j in range(3)] for i in range(3)])
    # rows are the local axes expressed in the parent frame (row-vector convention) ->
    # the column-vector rotation is its transpose
    q = rot.transposed().to_quaternion()  # (w, x, y, z)
    return (q.x, q.y, q.z, q.w), tuple(m[3][:3]), tuple(scale)


def _blender_to_game_rows(mat_bl):
    m16 = convert.rowmat_from_blender(mat_bl)
    return [list(m16[i * 4:i * 4 + 4]) for i in range(4)]


def _mul(a, b):
    return [[sum(a[i][k] * b[k][j] for k in range(4)) for j in range(4)] for i in range(4)]


def _inv(m):
    return [list(r) for r in Matrix(m).inverted()]


def sample_armature(arm_obj, frames, bone_order, parents):
    """Returns per-frame list of per-bone (q, t, s) LOCAL game TRS."""
    scene = bpy.context.scene
    out = []
    for f in frames:
        scene.frame_set(int(f), subframe=f - int(f))
        world = [_blender_to_game_rows(arm_obj.pose.bones[n].matrix) for n in bone_order]
        locals_ = []
        for i, name in enumerate(bone_order):
            p = parents[i]
            if p < 0:
                local = world[i]
            else:
                local = _mul(world[i], _inv(world[p]))
                # undo the game's segment-scale compensation: it divides the child's local
                # columns by the PARENT'S LOCAL scale before multiplying by the parent world
                ps = locals_[p][2]
                for r_ in range(3):
                    for c in range(3):
                        local[r_][c] *= ps[c]
            locals_.append(_decompose_game_rowmat(local))
        out.append(locals_)
    return out


def _canonical_quats(qs):
    """Keep consecutive quaternions in the same hemisphere (slerp-friendly; the 48-bit
    encoder normalizes sign anyway, but continuity keeps decode->encode stable)."""
    out = []
    prev = None
    for q in qs:
        if prev is not None and sum(a * b for a, b in zip(prev, q)) < 0:
            q = tuple(-c for c in q)
        out.append(q)
        prev = q
    return out


# Tolerances for collapsing a per-frame track to ONE key (what stock files do for every bone
# whose channel never moves — e.g. 9 of 33 rotation and 32 of 33 translation tracks of the
# Lesson-by-DJ clip are 1-key constants; writing them per frame made our files ~4x stock).
# Rotation keys are compared AFTER the 48-bit smallest-three encoding, so "constant" means
# the game would decode the very same quaternion from every key. Translation/scale are
# stored as float32, so a metre/ratio tolerance well below any visible motion is used.
CONST_TRANSLATION_TOL = 1e-6
CONST_SCALE_TOL = 1e-6


def _constant_keys(keys, tol):
    k0 = keys[0]
    return all(abs(c - c0) <= tol for k in keys for c, c0 in zip(k, k0))


def _collapse_rotation(qs):
    """Per-frame quaternion keys -> the same list, or a 1-key list when every key encodes
    to the same 48-bit value."""
    if len(qs) <= 1:
        return qs
    enc0 = anm.encode_q48(qs[0])
    if all(anm.encode_q48(q) == enc0 for q in qs[1:]):
        return [qs[0]]
    return qs


def _collapse_floats(keys, tol):
    if len(keys) > 1 and _constant_keys(keys, tol):
        return [keys[0]]
    return keys


def export_anm(filepath, arm_obj, frame_start=None, frame_end=None, include_scale="auto", collapse_constant=True):
    """Armature action -> .anm. Uniform (per-frame) keys on every animated channel, one key
    on channels that never change (``collapse_constant``), a kind-10 scale track only when
    a bone's local scale actually moves (``include_scale="auto"``; True forces it, False
    omits it — a non-unit constant scale is then LOST, the game falls back to the bind
    scale)."""
    scene = bpy.context.scene
    fs = scene.frame_start if frame_start is None else frame_start
    fe = scene.frame_end if frame_end is None else frame_end
    if fe < fs:
        raise ValueError("empty frame range")
    bone_order = list(arm_obj.get("ddr_bone_order", [])) or [b.name for b in arm_obj.data.bones]
    bones = arm_obj.data.bones
    index = {n: i for i, n in enumerate(bone_order)}
    parents = [index[bones[n].parent.name] if bones[n].parent else -1 for n in bone_order]
    frames = list(range(fs, fe + 1))
    saved = scene.frame_current
    try:
        samples = sample_armature(arm_obj, [float(f) for f in frames], bone_order, parents)
    finally:
        scene.frame_set(saved)

    tracks = []
    for b, name in enumerate(bone_order):
        qs = _canonical_quats([s[b][0] for s in samples])
        ts = [s[b][1] for s in samples]
        ss = [s[b][2] for s in samples]
        if collapse_constant:
            qs = _collapse_rotation(qs)
            ts = _collapse_floats(ts, CONST_TRANSLATION_TOL)
        tracks.append(dict(kind=0x1C, target=b, sub=0, times=None, keys=qs))
        tracks.append(dict(kind=0x1D, target=b, sub=0, times=None, keys=ts))
        # "animated" = the scale moves over the clip OR sits away from 1 (a constant non-unit
        # scale must still be written: bones without a track fall back to the BIND scale)
        scaled = any(abs(c - 1.0) > 1e-4 for s in ss for c in s)
        if include_scale is True or (include_scale == "auto" and scaled):
            if collapse_constant:
                ss = _collapse_floats(ss, CONST_SCALE_TOL)
            tracks.append(dict(kind=10, target=b, sub=0, times=None, keys=ss))
    spec = dict(frame_count=len(frames) - 1, flag=1, fps=None, hierarchy=parents, hierarchy_trailer=0, tracks=tracks)
    data = anm.write_anm(spec)
    with open(filepath, "wb") as f:
        f.write(data)
    return data, spec


def export_camanm(filepath, cam_obj, frame_start=None, frame_end=None, fps=None, aspect_file=4.0 / 3.0):
    """Camera object -> .camanm (position in centimetres, orientation, FOV via the §6 inverse)."""
    scene = bpy.context.scene
    fs = scene.frame_start if frame_start is None else frame_start
    fe = scene.frame_end if frame_end is None else frame_end
    fps = fps or int(round(scene.render.fps / scene.render.fps_base))
    cam = cam_obj.data
    if cam.type != "PERSP":
        raise ValueError("only perspective cameras can be exported")
    frames = list(range(fs, fe + 1))
    saved = scene.frame_current
    rots, poss, fovs = [], [], []
    try:
        for f in frames:
            scene.frame_set(f)
            rows = _blender_to_game_rows(cam_obj.matrix_world)
            q, t, _ = _decompose_game_rowmat(rows)
            rots.append(q)
            poss.append(tuple(c / CAMERA_UNIT_SCALE for c in t))
            # Blender lens/sensor -> horizontal half-tangent the game must end up with
            sensor_w = cam.sensor_width if cam.sensor_fit != "VERTICAL" else cam.sensor_height * (16.0 / 9.0)
            t_prime = (sensor_w / 2.0) / cam.lens
            fovs.append(camanm_fov_from_half_tangent(t_prime, aspect_file))
    finally:
        scene.frame_set(saved)
    rots = _canonical_quats(rots)
    times = frames if fs != 0 else None
    n = len(frames)
    const = lambda v: dict(kind=8, target=0, sub=0, times=[0], keys=[(v,)])
    fov_track = dict(kind=8, target=2, sub=0, times=[0], keys=[(fovs[0],)]) if all(abs(x - fovs[0]) < 1e-4 for x in fovs) \
        else dict(kind=8, target=2, sub=0, times=list(range(n)), keys=[(x,) for x in fovs])
    camera = [
        dict(kind=1, target=0, sub=0, times=list(range(n)), keys=rots),
        dict(kind=4, target=1, sub=0, times=list(range(n)), keys=poss),
        fov_track,
        dict(const(cam.clip_start), target=3),
        dict(const(cam.clip_end), target=4),
        dict(const(aspect_file), target=5),
    ]
    spec = dict(frame_count=n - 1, flag=0, fps=fps, camera=camera)
    data = anm.write_anm(spec)
    with open(filepath, "wb") as f:
        f.write(data)
    return data, spec
