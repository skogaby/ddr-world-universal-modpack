"""Import a whole stage set: every ``gm_<stage>_<part>`` model listed in ``map_resources.rlist``
(doc §8 — parts become `gm_%s_%s` models; a ``:N`` suffix is a draw-priority hint, stripped
here), each with its ``_play_loop.anm`` baked when present. Optionally the stage's camera set
from ``stage_camera_resources.rlist`` (one camera object per ``.camanm``, the first one active).

Layout expected (an unpacked ``mapset_<stage>.arc`` + ``camera/*.arc`` + ``startup.arc``):
    <data>/map/gm_<stage>_<part>/gm_<stage>_<part>.model (+ .anm, .dds ...)
    <data>/camera/long/<set>/<set>_stNN.camanm            (set = the rlist entry's first 5 chars)
    <data>/../startup/data/map/map_resources.rlist          (or <data>/map/map_resources.rlist)
"""
import os
import re

import bpy

from . import import_anm, import_model
from .codec import ktmdl

MAP_RLIST = "map_resources.rlist"
CAMERA_RLIST = "stage_camera_resources.rlist"


def _find_rlist(data_root, sub, name, explicit=None):
    if explicit:
        return explicit if os.path.isfile(explicit) else None
    root = os.path.dirname(data_root)
    for cand in (os.path.join(data_root, sub, name), os.path.join(root, "startup", "data", sub, name),
                 os.path.join(data_root, "startup", "data", sub, name)):
        if os.path.isfile(cand):
            return cand
    return None


def stage_from_model_path(model_path):
    """``.../map/gm_boom00_bg/gm_boom00_bg.model`` -> ('boom00', <data root>)."""
    name = os.path.splitext(os.path.basename(model_path))[0]
    m = re.match(r"^gm_([a-z0-9]+)_[a-z0-9]+$", name)
    if not m:
        raise ValueError("%s is not a gm_<stage>_<part> model" % name)
    map_dir = os.path.dirname(os.path.dirname(os.path.abspath(model_path)))
    return m.group(1), os.path.dirname(map_dir)


def stage_parts(rlist_path, stage):
    rows = dict(ktmdl.parse_rlist(open(rlist_path, "rb").read()))
    if stage not in rows:
        raise ValueError("stage %r not in %s (have %s)" % (stage, os.path.basename(rlist_path), sorted(rows)[:8]))
    fields = rows[stage]
    colours, parts = fields[:2], []
    for f in fields[2:]:
        name, _, prio = f.partition(":")
        parts.append((name, int(prio) if prio.lstrip("-").isdigit() else None))
    return colours, parts


def load_stage(model_or_stage, data_root=None, import_textures=True, import_anims=True, import_cameras=False,
               map_rlist=None, camera_rlist=None, frame_step=1):
    """Returns dict(stage, parts=[(name, armature|None, [meshes])], skipped=[...], cameras=[objs])."""
    if os.path.isfile(model_or_stage):
        stage, data_root = stage_from_model_path(model_or_stage)
    else:
        stage = model_or_stage
        if not data_root:
            raise ValueError("data_root required when passing a stage name")
    rl = _find_rlist(data_root, "map", MAP_RLIST, map_rlist)
    if rl is None:
        raise ValueError("map_resources.rlist not found near %s" % data_root)
    colours, parts = stage_parts(rl, stage)
    coll = bpy.data.collections.new("stage_" + stage)
    bpy.context.scene.collection.children.link(coll)
    out = dict(stage=stage, colours=colours, parts=[], skipped=[], cameras=[], rlist=rl)
    for part, prio in parts:
        name = "gm_%s_%s" % (stage, part)
        mp = os.path.join(data_root, "map", name, name + ".model")
        if not os.path.isfile(mp):
            out["skipped"].append((name, "no model"))
            continue
        arm, objs = import_model.load_model(mp, import_textures)
        for o in ([arm] if arm else []) + objs:
            for c in list(o.users_collection):
                c.objects.unlink(o)
            coll.objects.link(o)
            o["ddr_stage"] = stage
            o["ddr_stage_part"] = part
            if prio is not None:
                o["ddr_stage_priority"] = prio
        anm = os.path.join(data_root, "map", name, name + "_play_loop.anm")
        if import_anims and arm is not None and os.path.isfile(anm):
            bpy.context.view_layer.objects.active = arm
            try:
                import_anm.load_anm(anm, arm, frame_step)
            except Exception as e:  # noqa: BLE001
                out["skipped"].append((name + "_play_loop.anm", str(e)))
        out["parts"].append((name, arm, objs))
    if import_cameras:
        crl = _find_rlist(data_root, "camera", CAMERA_RLIST, camera_rlist)
        if crl:
            rows = dict(ktmdl.parse_rlist(open(crl, "rb").read()))
            for cam_name in rows.get(stage, []):
                path = os.path.join(data_root, "camera", "long", cam_name[:5], cam_name + ".camanm")
                if not os.path.isfile(path):
                    out["skipped"].append((cam_name, "no .camanm"))
                    continue
                try:
                    cam = import_anm.load_camanm(path, frame_step)
                except Exception as e:  # noqa: BLE001
                    out["skipped"].append((cam_name, str(e)))
                    continue
                for c in list(cam.users_collection):
                    c.objects.unlink(cam)
                coll.objects.link(cam)
                out["cameras"].append(cam)
            if out["cameras"]:
                bpy.context.scene.camera = out["cameras"][0]
        else:
            out["skipped"].append((CAMERA_RLIST, "not found"))
    bpy.context.view_layer.update()
    return out
