"""Import .anm (skeletal) onto an armature, or .camanm as an animated camera.

Skeletal keys are baked per frame: the game-equivalent evaluator in
scripts/anm_dump.py produces every bone's WORLD matrix (local TRS chain with Maya
segment-scale compensation, §5.1); each is converted to Blender armature space
and turned into the pose bone's matrix_basis with Blender's own parenting rule
    pose_b = pose_p @ (rest_p^-1 @ rest_b) @ basis_b
so the result is exact regardless of how the bones were oriented on import.
"""
import math
import os

import bpy
from mathutils import Matrix, Vector

from . import convert
from .codec import anm

FOV_OUTPUT_ASPECT = 16.0 / 9.0  # DAT_180265264 — the game re-projects every camera to 16:9 (§6)
CAMERA_UNIT_SCALE = 0.01        # DAT_1802921e8 — .camanm positions are centimetres (§6)


def _bone_tracks_chunk(parsed):
    return next((c for c in parsed["chunks"] if c["type"] == 0), None)


def _hierarchy_parents(parsed, fallback):
    hier = next((c for c in parsed["chunks"] if c["type"] == 1), None)
    if hier is None:
        return fallback
    return [(-1 if p == 0xFF else p) for _, p in hier["pairs"]]


def _fps(parsed):
    hdr = parsed["header"]
    has_camera = any(c["type"] == 4 for c in parsed["chunks"])
    return float(hdr["fps_or_one"]) if has_camera and hdr["fps_or_one"] > 1 else 60.0


def _ensure_action(obj, name):
    """Create an action assigned to obj; returns (action, fcurve_factory) where
    fcurve_factory(data_path, index, group) works on both the legacy (<= 4.3) and
    the layered (>= 4.4, slots/layers/strips/channelbags) action API."""
    action = bpy.data.actions.new(name)
    anim = obj.animation_data or obj.animation_data_create()
    anim.action = action
    if hasattr(action, "fcurves"):
        def factory(data_path, index, group=None):
            if group:
                return action.fcurves.new(data_path, index=index, action_group=group)
            return action.fcurves.new(data_path, index=index)
        return action, factory
    slot = action.slots.new(obj.id_type, obj.name)
    anim.action_slot = slot
    layer = action.layers.new("Layer")
    strip = layer.strips.new(type="KEYFRAME")
    channelbag = strip.channelbag(slot, ensure=True)

    def factory(data_path, index, group=None):
        return channelbag.fcurves.new(data_path, index=index, group_name=group or "")
    return action, factory


def _fcurves(factory, data_path, count, group=None):
    return [factory(data_path, i, group) for i in range(count)]


def _fill(fcurve, frames, values, interpolation="LINEAR"):
    kp = fcurve.keyframe_points
    kp.add(len(frames))
    flat = [0.0] * (2 * len(frames))
    flat[0::2] = frames
    flat[1::2] = values
    kp.foreach_set("co", flat)
    ipo = bpy.types.Keyframe.bl_rna.properties["interpolation"].enum_items[interpolation].value
    kp.foreach_set("interpolation", [ipo] * len(frames))
    fcurve.update()


def load_anm(filepath, arm_obj, frame_step=1):
    """Bake a skeletal .anm onto arm_obj (imported by import_model). Returns the action."""
    parsed = anm.parse_anm(open(filepath, "rb").read())
    if _bone_tracks_chunk(parsed) is None:
        raise ValueError("%s has no bone-track chunk (not a skeletal .anm)" % os.path.basename(filepath))
    bone_order = list(arm_obj.get("ddr_bone_order", [b.name for b in arm_obj.data.bones]))
    bones = arm_obj.data.bones
    fallback_parents = [
        (bone_order.index(bones[n].parent.name) if bones[n].parent else -1) for n in bone_order
    ]
    parents = _hierarchy_parents(parsed, fallback_parents)
    n = min(len(parents), len(bone_order))
    if len(parents) != len(bone_order):
        print("[ddr] warning: %s has %d bones, armature has %d — animating the first %d"
              % (os.path.basename(filepath), len(parents), len(bone_order), n))

    rest = [bones[name].matrix_local.copy() for name in bone_order]
    rest_inv = [m.inverted() for m in rest]

    frame_count = parsed["header"]["frame_count"]
    frames = list(range(0, frame_count + 1, max(1, frame_step)))
    fps = _fps(parsed)

    per_bone = [dict(loc=([], [], []), rot=([], [], [], []), sca=([], [], [])) for _ in range(n)]
    for f in frames:
        pose = anm.evaluate_pose(parsed, float(f), parents)
        world = [convert.rowmat_to_blender([x for row in p["world"] for x in row]) for p in pose[:n]]
        for b in range(n):
            p = parents[b]
            if 0 <= p < n:
                local = world[p].inverted() @ world[b]
                basis = rest_inv[b] @ rest[p] @ local
            else:
                basis = rest_inv[b] @ world[b]
            loc, rot, sca = basis.decompose()
            slot = per_bone[b]
            for i in range(3):
                slot["loc"][i].append(loc[i])
                slot["sca"][i].append(sca[i])
            for i in range(4):
                slot["rot"][i].append(rot[i])

    action, new_fc = _ensure_action(arm_obj, os.path.splitext(os.path.basename(filepath))[0])
    fframes = [float(f) for f in frames]
    for b in range(n):
        name = bone_order[b]
        pb = arm_obj.pose.bones[name]
        pb.rotation_mode = "QUATERNION"
        base = 'pose.bones["%s"].' % name
        for i, fc in enumerate(_fcurves(new_fc, base + "location", 3, name)):
            _fill(fc, fframes, per_bone[b]["loc"][i])
        for i, fc in enumerate(_fcurves(new_fc, base + "rotation_quaternion", 4, name)):
            _fill(fc, fframes, per_bone[b]["rot"][i])
        for i, fc in enumerate(_fcurves(new_fc, base + "scale", 3, name)):
            _fill(fc, fframes, per_bone[b]["sca"][i])

    scene = bpy.context.scene
    scene.render.fps = int(round(fps))
    scene.frame_start = 0
    scene.frame_end = frame_count
    action["ddr_source"] = os.path.basename(filepath)
    action["ddr_frame_count"] = frame_count
    return action


def game_camera_half_tangent(fov_v_deg, aspect_file):
    """Horizontal half-tangent the GAME ends up with for a .camanm FOV/aspect pair (§6)."""
    h = math.tan(math.radians(fov_v_deg) * 0.5) * aspect_file
    fov_prime = math.atan2(2.0, 2.0 * h)
    return math.tan(0.5 * fov_prime)


def camanm_fov_from_half_tangent(t_prime, aspect_file=4.0 / 3.0):
    """Inverse of game_camera_half_tangent: the slot-2 degrees to write on export."""
    fov_prime = 2.0 * math.atan(t_prime)
    h = 1.0 / math.tan(fov_prime)
    return math.degrees(2.0 * math.atan(h / aspect_file))


def load_camanm(filepath, frame_step=1, apply_game_fov=True):
    """Create a camera object animated from a .camanm. Returns the camera object."""
    parsed = anm.parse_anm(open(filepath, "rb").read())
    chunk = next((c for c in parsed["chunks"] if c["type"] == 4), None)
    if chunk is None:
        raise ValueError("%s has no camera chunk" % os.path.basename(filepath))
    slots = chunk["tracks"]  # 0 rot, 1 pos, 2 fov deg, 3 near, 4 far, 5 aspect
    data = parsed["data"]
    fps = _fps(parsed)
    frame_count = parsed["header"]["frame_count"]
    frames = list(range(0, frame_count + 1, max(1, frame_step)))

    name = os.path.splitext(os.path.basename(filepath))[0]
    cam_data = bpy.data.cameras.new(name)
    cam_obj = bpy.data.objects.new(name, cam_data)
    bpy.context.collection.objects.link(cam_obj)
    cam_obj.rotation_mode = "QUATERNION"
    cam_data.sensor_fit = "HORIZONTAL"
    cam_data.sensor_width = 36.0

    def scalar(slot, f, default):
        return anm.sample_track(data, slots[slot], f)[0] if slots[slot] else default

    near = scalar(3, 0.0, 0.1)
    far = scalar(4, 0.0, 10000.0)
    cam_data.clip_start = max(1e-4, near)
    cam_data.clip_end = max(cam_data.clip_start + 1e-3, far)

    locs = ([], [], [])
    rots = ([], [], [], [])
    lens = []
    for f in frames:
        q = anm.sample_track(data, slots[0], float(f)) if slots[0] else (0.0, 0.0, 0.0, 1.0)
        p = anm.sample_track(data, slots[1], float(f)) if slots[1] else (0.0, 0.0, 0.0)
        rot_rows = convert.quat_xyzw_to_blender_rowmat(q)
        mat = convert.game_frame_to_blender(rot_rows, Vector(p) * CAMERA_UNIT_SCALE)
        loc, rot, _ = mat.decompose()
        for i in range(3):
            locs[i].append(loc[i])
        for i in range(4):
            rots[i].append(rot[i])
        fov_v = scalar(2, float(f), 41.53)
        aspect = scalar(5, float(f), 4.0 / 3.0)
        if apply_game_fov:
            t_prime = game_camera_half_tangent(fov_v, aspect)
        else:
            t_prime = math.tan(math.radians(fov_v) * 0.5) * aspect  # Maya's own horizontal half-tangent
        lens.append(18.0 / t_prime)

    action, new_fc = _ensure_action(cam_obj, name)
    fframes = [float(f) for f in frames]
    for i, fc in enumerate(_fcurves(new_fc, "location", 3)):
        _fill(fc, fframes, locs[i])
    for i, fc in enumerate(_fcurves(new_fc, "rotation_quaternion", 4)):
        _fill(fc, fframes, rots[i])
    _, new_cam_fc = _ensure_action(cam_data, name + "_lens")
    _fill(_fcurves(new_cam_fc, "lens", 1)[0], fframes, lens)

    scene = bpy.context.scene
    scene.render.fps = int(round(fps))
    scene.frame_start = 0
    scene.frame_end = frame_count
    if scene.camera is None:
        scene.camera = cam_obj
    cam_obj["ddr_source"] = os.path.basename(filepath)
    cam_obj["ddr_camanm_constants"] = [scalar(2, 0.0, 41.53), near, far, scalar(5, 0.0, 4.0 / 3.0)]
    return cam_obj
