"""Import a whole dancer: the body ``pl_<char>NN.model`` plus its sibling PART models
(``pl_<char>NN_faceNN`` / ``_headNN`` / ``_chestNN`` / ``_forearmNN`` / ``_hipsNN``), each
rigidly attached to ONE body bone exactly as the game's dancer actor does, and the
per-character uniform scale from ``chara_resources.rlist``.

Game facts reproduced here (docs/3d_model_format_research.md §8, decompiled from
``FUN_18005d5d0`` / ``FUN_18005e560`` / ``FUN_18015c000`` of gamemdx_20240402.dll):

* Part models have a single identity root bone and are authored in the ATTACH BONE'S
  bind frame: the game parents the part's ModelNode to the body node with ``+0x2C`` =
  bone index, and the node update composes ``partExtra(+0x88) · boneMatrix[i] · body``
  (row-vector: the part's own matrix first, then the animated bone's model-space
  matrix, then the body node). So in Blender a part is a bone-parented object whose
  mesh data is the file's raw coordinates (NO Y-up->Z-up conversion — the bone frame
  supplies the axes) and whose local matrix is the game's extra matrix.
* Bones by ORIGINAL name (the body's .b2it): ``face``/``head`` -> ``Head``, ``hips`` ->
  ``Hips``, ``chest`` -> ``Spine2``, ``forearm`` -> ``LeftForeArmRoll`` AND a second
  instance on ``RightForeArmRoll``.
* The right-forearm extra matrix is ``scale · MirrorX(-1,1,1) · RotX(180°)`` =
  ``diag(-s, -s, -s)`` — a point inversion (verified from the disassembly: the mirror
  is ``DAT_1802626d4 = -1.0`` on m[0], the rotation is ``sin(3π/2)`` on m[5]/m[10] and
  ``±sin(π)`` on m[6]/m[9], i.e. about X).
* Every part gets the SAME uniform scale ``s`` (rlist field 3) as the body, applied in
  its own bone-local space, and the part's attach point is the scaled bone position —
  together exactly "scale the assembled character about the body origin", which is
  what putting ``s`` on the armature OBJECT does here.
* Faces: the EmotionController loads ``_face01..03`` and shows only the first (the
  others' visible bit is cleared); we import all and hide 02/03.
* A missing part arc is skipped by the game (``FUN_18005e560`` destroys the node when
  the model resource is null) — parts are optional.
"""
import os
import re

import bpy
from mathutils import Matrix

from . import import_model
from .codec import ktmdl

# part kind -> [(body bone name, mirrored)]
PART_BONES = {
    "face": [("Head", False)],
    "head": [("Head", False)],
    "hips": [("Hips", False)],
    "chest": [("Spine2", False)],
    "forearm": [("LeftForeArmRoll", False), ("RightForeArmRoll", True)],
}
PART_KINDS = tuple(PART_BONES)
RLIST_NAME = "chara_resources.rlist"

# The game's extra matrix for the second forearm instance (row-vector, bone-local):
# Scale(s) · diag(-1,1,1) · RotX(pi) == diag(-s,-s,-s); with s carried by the armature
# object this is what remains on the part object.
MIRROR_BASIS = Matrix.Diagonal((-1.0, -1.0, -1.0, 1.0))

_PART_RE = re.compile(r"^(?P<body>pl_[a-z0-9]+?)_(?P<kind>face|head|chest|forearm|hips)(?P<num>\d{2})$")


def body_key(body_name):
    """``pl_emi00`` -> ``emi00`` (the chara_resources.rlist key)."""
    return body_name[3:] if body_name.startswith("pl_") else body_name


def find_parts(body_model_path):
    """Sibling part models of a body: [(part_name e.g. 'face01', model_path)], sorted.
    Looks for ``<chara dir>/<body>_<part>NN/<body>_<part>NN.model`` (the unpacked
    ``data/chara/`` layout) and also for the model file directly in the body's directory."""
    body_dir = os.path.dirname(os.path.abspath(body_model_path))
    body = os.path.splitext(os.path.basename(body_model_path))[0]
    chara_dir = os.path.dirname(body_dir)
    found = {}
    prefix = body + "_"
    for root in (chara_dir, body_dir):
        try:
            entries = sorted(os.listdir(root))
        except OSError:
            continue
        for e in entries:
            m = _PART_RE.match(os.path.splitext(e)[0])
            if not m or m.group("body") != body:
                continue
            part = m.group("kind") + m.group("num")
            cand = os.path.join(root, e, e + ".model") if os.path.isdir(os.path.join(root, e)) else os.path.join(root, e)
            if cand.lower().endswith(".model") and os.path.isfile(cand) and part not in found:
                found[part] = cand
    return sorted(found.items())


def find_rlist(body_model_path, explicit=None):
    """Locate chara_resources.rlist: an explicit path, else next to the pl_* directories
    (``data/chara/``), else ``startup/data/chara/`` beside a ``data/`` root."""
    if explicit:
        return explicit if os.path.isfile(explicit) else None
    body_dir = os.path.dirname(os.path.abspath(body_model_path))
    chara_dir = os.path.dirname(body_dir)          # .../data/chara
    data_dir = os.path.dirname(chara_dir)          # .../data
    root = os.path.dirname(data_dir)               # .../
    for cand in (
        os.path.join(chara_dir, RLIST_NAME),
        os.path.join(body_dir, RLIST_NAME),
        os.path.join(root, "startup", "data", "chara", RLIST_NAME),
        os.path.join(data_dir, "startup", "data", "chara", RLIST_NAME),
    ):
        if os.path.isfile(cand):
            return cand
    return None


def rlist_row(rlist_path, key):
    """The [fields] of ``key`` in an MRL0 rlist, or None."""
    try:
        rows = ktmdl.parse_rlist(open(rlist_path, "rb").read())
    except (OSError, ValueError):
        return None
    for k, fields in rows:
        if k == key:
            return list(fields)
    return None


def attach_to_bone(obj, arm_obj, bone_name, basis=None):
    """Bone-parent ``obj`` so that its object space == the bone's HEAD frame (Blender bone
    parenting is tail-relative; matrix_parent_inverse rewinds the length) with an optional
    extra local matrix (the game's ``+0x88`` extra transform)."""
    bone = arm_obj.data.bones[bone_name]
    obj.parent = arm_obj
    obj.parent_type = "BONE"
    obj.parent_bone = bone_name
    obj.matrix_parent_inverse = Matrix.Translation((0.0, -bone.length, 0.0))
    obj.matrix_basis = (basis or Matrix.Identity(4)).copy()


def load_part(model_path, arm_obj, part_name, import_textures=True, body_name=None):
    """Import one part model (raw axes, no armature) and attach it to its bone(s).
    Returns the created objects (two for a forearm: L + a mirrored linked duplicate on R)."""
    kind_m = re.match(r"[a-z]+", part_name)
    kind = kind_m.group(0) if kind_m else ""
    targets = PART_BONES.get(kind)
    if not targets:
        raise ValueError("unknown part kind %r" % part_name)
    missing = [b for b, _ in targets if b not in arm_obj.data.bones]
    if missing:
        raise ValueError("body armature lacks bone(s) %s needed by part %s" % (missing, part_name))
    label = "%s_%s" % (body_name or arm_obj.name.replace("_Armature", ""), part_name)
    _, objs = import_model.load_model(model_path, import_textures, axis_convert=False, with_armature=False,
                                      name=label)
    out = []
    for obj in objs:
        first = True
        for bone_name, mirrored in targets:
            if first:
                inst = obj
            else:
                inst = bpy.data.objects.new(obj.name + "_R", obj.data)  # linked duplicate: one mesh serves both arms
                bpy.context.collection.objects.link(inst)
                for k in obj.keys():
                    inst[k] = obj[k]
            first = False
            attach_to_bone(inst, arm_obj, bone_name, MIRROR_BASIS if mirrored else None)
            inst["ddr_part"] = part_name
            inst["ddr_part_bone"] = bone_name
            inst["ddr_part_mirror"] = mirrored
            inst["ddr_part_source"] = os.path.basename(model_path)
            out.append(inst)
    return out


def load_character(body_model_path, import_textures=True, import_parts=True, all_faces=True,
                   apply_rlist_scale=True, rlist_path=None):
    """Body + parts + rlist scale. Returns (armature, body mesh objects, part objects, info dict)."""
    arm, body_objs = import_model.load_model(body_model_path, import_textures)
    if arm is None:
        raise ValueError("%s has no skeleton — not a character body" % os.path.basename(body_model_path))
    body = os.path.splitext(os.path.basename(body_model_path))[0]
    key = body_key(body)
    parts, skipped = [], []
    info: dict = dict(body=body, key=key, parts=parts, skipped=skipped, rlist=None, scale=1.0)
    arm["ddr_chara_key"] = key

    part_objs = []
    if import_parts:
        faces_seen = 0
        for part, path in find_parts(body_model_path):
            if part.startswith("face"):
                faces_seen += 1
                if faces_seen > 1 and not all_faces:
                    skipped.append(part)
                    continue
            try:
                objs = load_part(path, arm, part, import_textures, body_name=body)
            except Exception as e:  # noqa: BLE001 — one bad part must not sink the body
                print("[ddr] part %s skipped: %s" % (part, e))
                skipped.append(part)
                continue
            if part.startswith("face") and faces_seen > 1:
                for o in objs:  # the EmotionController shows face01 only (eye-icon hide keeps them evaluated)
                    o.hide_set(True)
                    o.hide_render = True
            part_objs += objs
            parts.append(part)
    arm["ddr_parts"] = list(parts)

    rl = find_rlist(body_model_path, rlist_path)
    if rl:
        row = rlist_row(rl, key)
        info["rlist"] = rl
        arm["ddr_rlist_path"] = rl
        if row:
            arm["ddr_rlist_row"] = row
            try:
                s = float(row[3]) if len(row) > 3 else 1.0
            except ValueError:
                s = 1.0
            info["scale"] = s
            if apply_rlist_scale and s > 0:
                arm.scale = (s, s, s)
                arm["ddr_rlist_scale_applied"] = s
        else:
            info["rlist_missing_row"] = True
    bpy.context.view_layer.update()  # bone parenting / scale -> matrix_world is stale until evaluated
    return arm, body_objs, part_objs, info
