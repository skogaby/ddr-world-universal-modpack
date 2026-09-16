"""Export a whole dancer in the game's ``data/chara/`` layout:

    <out_dir>/pl_<key>/pl_<key>.model (+ .b2it, .grp2it, .dds)       the body (skinned)
    <out_dir>/pl_<key>_<part>/pl_<key>_<part>.model (+ .grp2it, .dds)  one per attached part
    <out_dir>/chara_resources.rlist                                    (optional) registration row

Parts are the bone-parented objects import_character.py creates (or any mesh the user
bone-parents to Head / Hips / Spine2 / LeftForeArmRoll); they are written in their own
raw coordinates (see export_model.build_spec raw_axes) because the game attaches them
to the bone at runtime. The mirrored right-forearm instance is never exported — the game
builds it from the left model (§8).

Each of these directories is what the game finds inside ``data/arc/<dir name>.arc``
(one arc per model directory); the rlist row goes into ``startup.arc``'s
``data/chara/chara_resources.rlist``.
"""
import os
import re

from .codec import ktmdl
from . import export_model
from .import_character import PART_BONES, RLIST_NAME, body_key

# bone -> part kind for user-attached meshes without a ddr_part tag
_BONE_TO_KIND = {"Head": "head", "Hips": "hips", "Spine2": "chest", "LeftForeArmRoll": "forearm"}
DEFAULT_RLIST_FIELDS = ["pl", "F", "A", "1.0", "0.8", "-1.0"]  # type, sex, class, model scale, shadow scale, unlock id


def fmt_num(x):
    """Stock rlist numbers look like "0.9", "1.0", "0.75", "-1.0" — always with a decimal point."""
    s = ("%.4f" % float(x)).rstrip("0")
    return s + "0" if s.endswith(".") else s


def body_meshes(arm_obj):
    return [o for o in arm_obj.children_recursive if o.type == "MESH" and not export_model.is_part_object(o)]


def part_groups(arm_obj):
    """{part_name: [mesh objects]} for the exportable parts of an armature (mirrored
    right-forearm instances and parts on unsupported bones are reported in the second
    return value)."""
    groups = {}
    skipped = []
    for o in arm_obj.children_recursive:
        if not export_model.is_part_object(o) or o.parent != arm_obj:
            continue
        if o.get("ddr_part_mirror"):
            continue  # the game derives the right forearm from the left model
        part = o.get("ddr_part")
        if not part:
            kind = _BONE_TO_KIND.get(o.parent_bone)
            if kind is None:
                skipped.append((o.name, "bone %s is not a game attach point %s" % (o.parent_bone, sorted(_BONE_TO_KIND))))
                continue
            part = kind + "00"
        kind_m = re.match(r"[a-z]+", part)
        kind = kind_m.group(0) if kind_m else ""
        expect = [b for b, m in PART_BONES.get(kind, []) if not m]
        if expect and o.parent_bone not in expect:
            skipped.append((o.name, "part %s must hang from %s, not %s" % (part, expect[0], o.parent_bone)))
            continue
        groups.setdefault(part, []).append(o)
    return groups, skipped


def rlist_fields_for(arm_obj, model_scale=None):
    """The chara_resources.rlist fields for this armature: the imported row when present,
    with the model scale replaced by the armature object's uniform scale."""
    fields = list(arm_obj.get("ddr_rlist_row", [])) or list(DEFAULT_RLIST_FIELDS)
    fields = [str(f) for f in fields]
    while len(fields) < 6:
        fields.append(DEFAULT_RLIST_FIELDS[len(fields)])
    if model_scale is None:
        sx, sy, sz = arm_obj.scale
        model_scale = sx if abs(sx - sy) < 1e-6 and abs(sx - sz) < 1e-6 else 1.0
    fields[3] = fmt_num(model_scale)
    return fields


def export_character(out_dir, arm_obj, key=None, write_textures=True, write_rlist=True, rlist_source=None,
                     rlist_fields=None):
    """Returns a report dict: written files, parts, skipped objects, rlist outcome."""
    key = key or arm_obj.get("ddr_chara_key") or body_key(os.path.splitext(str(arm_obj.get("ddr_source", arm_obj.name)))[0])
    key = re.sub(r"[^a-z0-9]", "", key.lower())
    if not key:
        raise ValueError("empty character key")
    body = "pl_" + key
    written_all, parts_out, skipped_all = [], [], []
    report: dict = dict(key=key, body=body, written=written_all, parts=parts_out, skipped=skipped_all, rlist=None)

    meshes = body_meshes(arm_obj)
    if not meshes:
        raise ValueError("armature %s has no (non-part) mesh children" % arm_obj.name)
    body_dir = os.path.join(out_dir, body)
    written, spec = export_model.export_model(os.path.join(body_dir, body + ".model"), arm_obj, meshes,
                                             write_textures, raw_axes=False)
    written_all += written
    report["body_spec"] = dict(bones=len(spec["bones"]), meshes=len(spec["meshes"]), materials=len(spec["materials"]))

    groups, skipped = part_groups(arm_obj)
    skipped_all += skipped
    for part, objs in sorted(groups.items()):
        name = "%s_%s" % (body, part)
        pdir = os.path.join(out_dir, name)
        w, pspec = export_model.export_model(os.path.join(pdir, name + ".model"), None, objs, write_textures, raw_axes=True)
        # the game never opens a part's .b2it (only the body's); keep the folder like stock
        b2it = os.path.join(pdir, name + ".b2it")
        if os.path.exists(b2it):
            os.remove(b2it)
            w = [x for x in w if x != b2it]
        written_all += w
        parts_out.append((part, len(pspec["meshes"])))

    if write_rlist:
        fields = rlist_fields or rlist_fields_for(arm_obj)
        src = rlist_source or arm_obj.get("ddr_rlist_path")
        rows = []
        if src and os.path.isfile(src):
            try:
                rows = ktmdl.parse_rlist(open(src, "rb").read())
            except (OSError, ValueError) as e:
                report["rlist"] = "source %s unreadable (%s); wrote a single-row list" % (src, e)
        rows = ktmdl.upsert_rlist_row(rows, key, fields)
        path = os.path.join(out_dir, RLIST_NAME)
        with open(path, "wb") as f:
            f.write(ktmdl.write_rlist(rows))
        written_all.append(path)
        report["rlist_row"] = (key, fields)
        if report["rlist"] is None:
            report["rlist"] = ("%d rows (source %s)" % (len(rows), src)) if src and os.path.isfile(src) else \
                "single row — merge it into the stock chara_resources.rlist (the game reads ONE list)"
    return report
