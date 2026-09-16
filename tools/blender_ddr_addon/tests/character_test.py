"""Headless test of the character (body + parts + rlist) import/export path.
Run via scripts/validate_blender_addon.sh (which also runs smoke_test.py).

Checks, on pl_rinon00 (the stock body that has every part kind):
  * every sibling part is attached to the game's bone (Head/Hips/Spine2/LeftForeArmRoll)
    with the mirrored second forearm instance on RightForeArmRoll
  * a part vertex lands, in the body's model space, exactly at  v_part · E · Bind[bone]
    (row-vector; E = I or diag(-1,-1,-1)) — the game's node composition (§8)
  * the armature carries the rlist scale (0.65) and the row
  * export_character writes the data/chara layout: body + one dir per part, part vertex
    data identical to the stock part files (raw axes), a chara_resources.rlist whose row for
    the key matches, every other stock row untouched
  * the exported folder re-imports with the same attachment
"""
import os
import sys

import bpy
from mathutils import Matrix, Vector

ADDON_DIR = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
sys.path.insert(0, os.path.dirname(ADDON_DIR))
addon = __import__(os.path.basename(ADDON_DIR))
addon.register()
from blender_ddr_addon import convert, export_character, import_character, import_model  # noqa: E402
from blender_ddr_addon.codec import ktmdl as K  # noqa: E402

DATA = os.environ["DDR_3D_DATA"]
OUT_DIR = os.environ.get("DDR_3D_OUT_DIR") or os.path.join(os.path.dirname(DATA), "blender_export_test")
CHAR_OUT = os.path.join(OUT_DIR, "character")
os.makedirs(CHAR_OUT, exist_ok=True)
BODY = os.path.join(DATA, "chara", "pl_rinon00", "pl_rinon00.model")
RLIST = os.environ.get("DDR_3D_RLIST") or os.path.join(os.path.dirname(DATA), "startup", "data", "chara", "chara_resources.rlist")

failures = []


def check(cond, msg):
    print(("  ok   " if cond else "  FAIL ") + msg)
    if not cond:
        failures.append(msg)


def game_rows(m16):
    return [list(m16[i * 4:i * 4 + 4]) for i in range(4)]


def mul(a, b):
    return [[sum(a[i][k] * b[k][j] for k in range(4)) for j in range(4)] for i in range(4)]


bpy.ops.wm.read_factory_settings(use_empty=True)
print("== character", BODY)
arm, body_objs, part_objs, info = import_character.load_character(BODY, rlist_path=RLIST if os.path.isfile(RLIST) else None)
check(len(body_objs) >= 1 and len(arm.data.bones) == 33, "body imported (%d meshes, 33 bones)" % len(body_objs))
parts = set(info["parts"])
check({"chest00", "face01", "face02", "face03", "forearm00", "head00", "hips00"} <= parts, "all 7 stock parts found: %s" % sorted(parts))
by_part = {}
for o in part_objs:
    by_part.setdefault(o["ddr_part"], []).append(o)
check(len(by_part.get("forearm00", [])) == 2, "forearm imported twice (L + mirrored R)")
fl = [o for o in by_part.get("forearm00", []) if not o["ddr_part_mirror"]]
fr = [o for o in by_part.get("forearm00", []) if o["ddr_part_mirror"]]
check(fl and fl[0].parent_bone == "LeftForeArmRoll" and fr and fr[0].parent_bone == "RightForeArmRoll", "forearm bones L/R")
check(fr and fr[0].data == fl[0].data, "R forearm is a linked duplicate of L")
check(fr and all(abs(fr[0].matrix_basis[i][i] + 1.0) < 1e-6 for i in range(3)), "R forearm basis = diag(-1,-1,-1)")
check(all(o.parent_bone == "Head" for o in by_part.get("face01", []) + by_part.get("head00", [])), "face/head on Head")
check(all(o.parent_bone == "Hips" for o in by_part.get("hips00", [])), "hips on Hips")
check(all(o.parent_bone == "Spine2" for o in by_part.get("chest00", [])), "chest on Spine2")
check(all(o.hide_get() for o in by_part.get("face02", []) + by_part.get("face03", [])) and
      not any(o.hide_get() for o in by_part.get("face01", [])), "face02/03 hidden, face01 visible")
if os.path.isfile(RLIST):
    check(abs(arm.scale.x - 0.65) < 1e-6 and abs(arm.scale.z - 0.65) < 1e-6, "rlist scale 0.65 applied to the armature (%s)" % (tuple(arm.scale),))
    check(list(arm["ddr_rlist_row"]) == ["pl", "F", "C", "0.65", "0.65", "0.0"], "rlist row stored")
else:
    print("  skip rlist checks (no %s)" % RLIST)

# geometric placement: part vertex -> body model space == v · E · Bind[bone]
body_model = K.parse_model(open(BODY, "rb").read())
b2it = dict(K.parse_b2it(open(os.path.splitext(BODY)[0] + ".b2it", "rb").read()))
worst = 0.0
for o in part_objs:
    bone_i = b2it[o["ddr_part_bone"]]
    bind = game_rows(body_model["bones"][bone_i]["bind"])
    E = [[-1.0 if o["ddr_part_mirror"] else 1.0, 0, 0, 0], [0, -1.0 if o["ddr_part_mirror"] else 1.0, 0, 0],
         [0, 0, -1.0 if o["ddr_part_mirror"] else 1.0, 0], [0, 0, 0, 1.0]]
    EB = mul(E, bind)
    to_model = arm.matrix_world.inverted() @ o.matrix_world  # armature (model) space, rlist scale removed
    for v in list(o.data.vertices)[:: max(1, len(o.data.vertices) // 25)]:
        raw = tuple(v.co)  # part mesh data = the file's bone-local coordinates
        want = [sum((raw + (1.0,))[k] * EB[k][c] for k in range(4)) for c in range(3)]
        got = convert.vec_to_game(to_model @ v.co)
        worst = max(worst, max(abs(a - b) for a, b in zip(want, got)))
check(worst < 1e-5, "part vertices land at v·E·Bind[bone] in body model space (max err %.1e)" % worst)

# ---------------------------------------------------------------------------------------
print("== export character ->", CHAR_OUT)
rep = export_character.export_character(CHAR_OUT, arm, write_textures=True)
check(rep["body"] == "pl_rinon00", "body name from ddr_chara_key")
check(sorted(p for p, _ in rep["parts"]) == sorted(parts), "every part exported once: %s" % sorted(p for p, _ in rep["parts"]))
check(not rep["skipped"], "nothing skipped %s" % rep["skipped"])
check(os.path.isfile(os.path.join(CHAR_OUT, "pl_rinon00", "pl_rinon00.model")) and
      os.path.isfile(os.path.join(CHAR_OUT, "pl_rinon00", "pl_rinon00.b2it")), "body files in pl_rinon00/")
for part in sorted(parts):
    d = os.path.join(CHAR_OUT, "pl_rinon00_" + part)
    check(os.path.isfile(os.path.join(d, "pl_rinon00_%s.model" % part)) and os.path.isfile(os.path.join(d, "pl_rinon00_%s.grp2it" % part))
          and not os.path.exists(os.path.join(d, "pl_rinon00_%s.b2it" % part)), "part dir %s (.model + .grp2it, no .b2it like stock)" % os.path.basename(d))


def positions_multiset(model):
    out = []
    for me in model["meshes"]:
        V = K.read_vertices(model, me)
        out.append(sorted(tuple(round(x, 5) for x in v["POSITION"]) for v in V))
    return out


for part in sorted(parts):
    src = os.path.join(DATA, "chara", "pl_rinon00_%s" % part, "pl_rinon00_%s.model" % part)
    dst = os.path.join(CHAR_OUT, "pl_rinon00_%s" % part, "pl_rinon00_%s.model" % part)
    m0 = K.parse_model(open(src, "rb").read())
    m1 = K.parse_model(open(dst, "rb").read())
    p0, p1 = positions_multiset(m0), positions_multiset(m1)
    same = len(p0) == len(p1) and all(set(a) == set(b) for a, b in zip(p0, p1))
    check(same, "part %s: exported vertex positions == stock (raw bone-local axes), %d mesh(es)" % (part, len(m1["meshes"])))
    check([m["shader_debug_name"] for m in m1["materials"]] == [m["shader_debug_name"] for m in m0["materials"]],
          "part %s: shader names preserved" % part)
    tex0 = [t["name"] for t in m0["texnames"]]
    tex1 = [t["name"] for t in m1["texnames"]]
    check(tex0 == tex1, "part %s: texture names preserved %s" % (part, tex1))

rl_out = os.path.join(CHAR_OUT, "chara_resources.rlist")
check(os.path.isfile(rl_out), "chara_resources.rlist written")
if os.path.isfile(rl_out):
    rows = K.parse_rlist(open(rl_out, "rb").read())
    d = dict(rows)
    check(d.get("rinon00") == ["pl", "F", "C", "0.65", "0.65", "0.0"], "rlist row for rinon00 == stock (%s)" % d.get("rinon00"))
    if os.path.isfile(RLIST):
        stock = K.parse_rlist(open(RLIST, "rb").read())
        check([k for k, _ in rows] == [k for k, _ in stock] and all(d[k] == f for k, f in stock if k != "rinon00"),
              "all other stock rows untouched, order kept")
        check(open(rl_out, "rb").read() == open(RLIST, "rb").read(), "rlist byte-identical to stock (unchanged row)")

# re-import the exported folder
bpy.ops.wm.read_factory_settings(use_empty=True)
arm2, body2, parts2, info2 = import_character.load_character(
    os.path.join(CHAR_OUT, "pl_rinon00", "pl_rinon00.model"), rlist_path=rl_out if os.path.isfile(rl_out) else None)
check(set(info2["parts"]) == parts, "re-import finds every exported part")
check(len(parts2) == len(part_objs), "re-import part object count %d == %d" % (len(parts2), len(part_objs)))
check(abs(arm2.scale.x - 0.65) < 1e-6, "re-import applies the exported rlist scale")

# ---------------------------------------------------------------------------------------
print("== stage boom00 + dancer render")
from blender_ddr_addon import import_anm, import_stage  # noqa: E402

bpy.ops.wm.read_factory_settings(use_empty=True)
MAP_RLIST = os.path.join(os.path.dirname(RLIST), "..", "map", "map_resources.rlist")
stage_model = os.path.join(DATA, "map", "gm_boom00_bg", "gm_boom00_bg.model")
if os.path.isfile(stage_model) and os.path.isfile(MAP_RLIST):
    rep = import_stage.load_stage(stage_model, import_cameras=True, map_rlist=MAP_RLIST,
                                  camera_rlist=os.path.join(os.path.dirname(RLIST), "..", "camera", "stage_camera_resources.rlist"),
                                  frame_step=10)
    check(rep["stage"] == "boom00" and len(rep["parts"]) == 6, "boom00: 6 parts imported (%d)" % len(rep["parts"]))
    check(not [s for s in rep["skipped"] if not s[0].endswith(".camanm")], "no part skipped %s" % rep["skipped"])
    check(len(rep["cameras"]) >= 10, "%d stage cameras imported" % len(rep["cameras"]))
    anim_parts = [n for n, arm, _ in rep["parts"] if arm is not None and arm.animation_data and arm.animation_data.action]
    check(len(anim_parts) >= 3, "play-loop animations baked on %s" % anim_parts)
    arm3, body3, parts3, _ = import_character.load_character(BODY, rlist_path=RLIST if os.path.isfile(RLIST) else None)
    bpy.context.view_layer.objects.active = arm3
    anm = os.path.join(DATA, "chara", "mc_female", "mc_female_hh01_exec.anm")
    if os.path.isfile(anm):
        import_anm.load_anm(anm, arm3, frame_step=10)
    sc = bpy.context.scene
    sc.camera = next((c for c in rep["cameras"] if c.name.startswith("st001_st05")), rep["cameras"][0])
    sc.frame_set(300)
    sc.render.engine = "BLENDER_WORKBENCH"
    sc.display.shading.light = "FLAT"
    sc.display.shading.color_type = "TEXTURE"
    sc.render.resolution_x, sc.render.resolution_y = 640, 360
    sc.render.filepath = os.path.join(OUT_DIR, "stage_boom00_rinon00.png")
    bpy.ops.render.render(write_still=True)
    check(os.path.getsize(sc.render.filepath) > 20000, "stage+dancer render written (%s)" % os.path.basename(sc.render.filepath))
else:
    print("  skip stage test (no %s / %s)" % (stage_model, MAP_RLIST))

if failures:
    print("FAILED: %d check(s)" % len(failures))
    sys.exit(1)
print("ALL CHARACTER CHECKS PASSED")
