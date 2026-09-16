"""Headless smoke test for the DDR Blender add-on. Run via scripts/validate_blender_addon.sh.

Imports a dancer model + a dance loop + a song camera and checks the Blender scene
against the codec's own game-equivalent evaluation:
  * bone count / names / vertex groups
  * every bone's WORLD position at a few frames == anm_dump.evaluate_pose (converted)
  * camera position at frame 0 == .camanm position * 0.01 (converted); lens finite
"""
import os
import sys

import bpy
from mathutils import Vector

ADDON_DIR = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
sys.path.insert(0, os.path.dirname(ADDON_DIR))
addon = __import__(os.path.basename(ADDON_DIR))
addon.register()
from blender_ddr_addon import convert, export_anm, export_model, import_anm, import_model  # noqa: E402
from blender_ddr_addon.codec import anm as anm_codec  # noqa: E402
from blender_ddr_addon.codec import ktmdl as ktmdl_codec  # noqa: E402

DATA = os.environ["DDR_3D_DATA"]
OUT_DIR = os.environ.get("DDR_3D_OUT_DIR") or os.path.join(os.path.dirname(os.environ.get("DDR_3D_OUT_BLEND") or DATA), "blender_export_test")
os.makedirs(OUT_DIR, exist_ok=True)
MODEL = os.path.join(DATA, "chara", "pl_emi00", "pl_emi00.model")
ANM = os.path.join(DATA, "chara", "mc_female", "mc_female_ne01_loop.anm")
CAMANM = next(
    os.path.join(r, f) for r, _, fs in os.walk(os.path.join(DATA, "camera")) for f in sorted(fs) if f.endswith(".camanm")
)
OUT_BLEND = os.environ.get("DDR_3D_OUT_BLEND")

failures = []


def check(cond, msg):
    print(("  ok   " if cond else "  FAIL ") + msg)
    if not cond:
        failures.append(msg)


bpy.ops.wm.read_factory_settings(use_empty=True)

print("== model", MODEL)
arm, objs = import_model.load_model(MODEL)
if arm is None:
    print("FAIL: no armature built")
    sys.exit(1)
check(len(arm.data.bones) == 33, "33 bones")
check(len(objs) == 2, "2 meshes (got %d)" % len(objs))
check("Hips" in arm.data.bones and "LeftForeArmRoll" in arm.data.bones, "b2it names applied")
body = max(objs, key=lambda o: len(o.data.vertices))
check(len(body.vertex_groups) > 20, "skin weights -> %d vertex groups" % len(body.vertex_groups))
check(len(body.data.uv_layers) == 1, "uv layer present (%d)" % len(body.data.uv_layers))
has_col = any(s.startswith("COLOR0") for s in body["ddr_layout"])
check(len(body.data.color_attributes) == (1 if has_col else 0), "vertex colours iff layout has COLOR0 (%s)" % has_col)
coloured = [o for o in objs if any(s.startswith("COLOR0") for s in o["ddr_layout"])]
check(all(len(o.data.color_attributes) == 1 for o in coloured), "%d COLOR0 mesh(es) got a colour attribute" % len(coloured))
check(body.data.materials and body.data.materials[0].node_tree.nodes.get("Image Texture") is not None, "material with DDS texture")
hips = arm.data.bones["Hips"]
check(abs(hips.matrix_local.translation.z - 0.97) < 0.02, "Hips rest height ~0.97 m on Blender Z (got %.3f)" % hips.matrix_local.translation.z)

print("== anm", ANM)
bpy.context.view_layer.objects.active = arm
action = import_anm.load_anm(ANM, arm)
parsed = anm_codec.parse_anm(open(ANM, "rb").read())
hier = next(c for c in parsed["chunks"] if c["type"] == 1)
parents = [(-1 if p == 0xFF else p) for _, p in hier["pairs"]]
order = list(arm["ddr_bone_order"])
worst = 0.0
for frame in (0, 37, 120, parsed["header"]["frame_count"]):
    bpy.context.scene.frame_set(frame)
    ref = anm_codec.evaluate_pose(parsed, float(frame), parents)
    for i, name in enumerate(order):
        pb = arm.pose.bones[name]
        got = (arm.matrix_world @ pb.matrix).translation
        want = convert.vec_to_blender(ref[i]["world"][3][:3])
        worst = max(worst, (got - Vector(want)).length)
check(worst < 1e-3, "baked pose matches evaluate_pose world positions (max err %.2e m)" % worst)
check(action["ddr_frame_count"] == parsed["header"]["frame_count"], "frame range")

print("== camanm", CAMANM)
cam = import_anm.load_camanm(CAMANM, frame_step=30)
bpy.context.scene.frame_set(0)
cparsed = anm_codec.parse_anm(open(CAMANM, "rb").read())
slots = next(c for c in cparsed["chunks"] if c["type"] == 4)["tracks"]
p0 = anm_codec.sample_track(cparsed["data"], slots[1], 0.0)
want = convert.vec_to_blender(Vector(p0) * 0.01)
got = cam.matrix_world.translation
check((got - Vector(want)).length < 1e-4, "camera position frame 0 = pos*0.01 (got %s)" % (tuple(round(x, 3) for x in got),))
check(0 < cam.data.lens < 200, "lens %.1f mm" % cam.data.lens)
# The camera's -Z must be the game's -row2 (forward) converted.
q0 = anm_codec.sample_track(cparsed["data"], slots[0], 0.0)
rows = anm_codec.quat_to_rowmat(q0)
fwd_game = Vector([-rows[2][c] for c in range(3)])
fwd_bl = cam.matrix_world.to_3x3() @ Vector((0, 0, -1))
check((fwd_bl - convert.vec_to_blender(fwd_game)).length < 1e-4, "camera forward = game -Z")

# ---------------------------------------------------------------------------------------
print("== export round-trips")
# (a) .anm: the imported action, re-exported, must decode to the same local TRS as the stock
#     file (48-bit quaternion quantization ~4e-5, float3 positions exact-ish).
bpy.context.view_layer.objects.active = arm
anm_out = os.path.join(OUT_DIR, "rt_" + os.path.basename(ANM))
exp_data, exp_spec = export_anm.export_anm(anm_out, arm, frame_start=0, frame_end=parsed["header"]["frame_count"])
re_parsed = anm_codec.parse_anm(open(anm_out, "rb").read())
check(len(exp_spec["hierarchy"]) == 33 and exp_spec["hierarchy"] == parents, "exported hierarchy == stock")
worst_pos = worst_rot = 0.0
for frame in (0, 37, 120, parsed["header"]["frame_count"]):
    ref = anm_codec.evaluate_pose(parsed, float(frame), parents)
    got = anm_codec.evaluate_pose(re_parsed, float(frame), parents)
    for i in range(33):
        worst_pos = max(worst_pos, max(abs(a - b) for a, b in zip(ref[i]["world"][3][:3], got[i]["world"][3][:3])))
        q0, q1 = ref[i]["q"], got[i]["q"]
        if sum(a * b for a, b in zip(q0, q1)) < 0:
            q1 = tuple(-c for c in q1)
        worst_rot = max(worst_rot, max(abs(a - b) for a, b in zip(q0, q1)))
check(worst_pos < 2e-4, "re-exported .anm world positions match stock (max %.2e m)" % worst_pos)
check(worst_rot < 3e-4, "re-exported .anm local quaternions match stock (max %.2e)" % worst_rot)
check(re_parsed["header"]["frame_count"] == parsed["header"]["frame_count"], "frame count preserved")

# (b) .model: export the imported dancer, re-parse, compare geometry + skin against the stock file
mesh_objs = [o for o in arm.children_recursive if o.type == "MESH"]
model_out = os.path.join(OUT_DIR, "rt_" + os.path.basename(MODEL))
written, spec = export_model.export_model(model_out, arm, mesh_objs, write_textures=True)
m0 = ktmdl_codec.parse_model(open(MODEL, "rb").read())
m1 = ktmdl_codec.parse_model(open(model_out, "rb").read())
check(len(m1["bones"]) == 33 and [b["identity"] for b in m1["bones"]] == [b["identity"] for b in m0["bones"]],
      "exported bone identities == stock (pack_identity of the .b2it names)")
check([b["parent"] for b in m1["bones"]] == [b["parent"] for b in m0["bones"]], "bone parents preserved")
bind_err = max(abs(a - b) for b0, b1 in zip(m0["bones"], m1["bones"]) for a, b in zip(b0["bind"], b1["bind"]))
check(bind_err < 1e-5, "bind matrices preserved (max %.1e)" % bind_err)
check(len(m1["meshes"]) == len(m0["meshes"]), "mesh count %d == %d" % (len(m1["meshes"]), len(m0["meshes"])))


def _surface_sample(model):
    """Multiset of triangles as (position, normal, uv, weights->global bones) per corner, with
    tolerant rounding (positions 1e-3, normals 1 decimal, uv 1e-3, weights 1e-2)."""
    out = []
    for me in model["meshes"]:
        V = ktmdl_codec.read_vertices(model, me)
        I = ktmdl_codec.read_indices(model, me)
        pal = me["palette"]
        tris = []
        for t in range(0, len(I) - 2, 3):
            corners = []
            for k in range(3):
                v = V[I[t + k]]
                w = tuple(sorted((pal[i], round(wt, 2)) for i, wt in zip(v["BLENDINDICES"], v["WEIGHTS4"]) if wt > 1e-2)) if "WEIGHTS4" in v else ()
                corners.append((tuple(round(x, 3) for x in v["POSITION"]), tuple(round(x, 1) for x in v["NORMAL"]),
                                tuple(round(x, 3) for x in v["TEXCOORD0"]) if "TEXCOORD0" in v else (), w))
            # canonical rotation of the triangle so winding-preserving rotations compare equal
            k = min(range(3), key=lambda i: corners[i])
            tris.append(tuple(corners[k:] + corners[:k]))
        out.append(sorted(tris))
    return out


s0, s1 = _surface_sample(m0), _surface_sample(m1)
per_mesh = []
for a, b in zip(s0, s1):
    ca, cb = {}, {}
    for t in a:
        ca[t] = ca.get(t, 0) + 1
    for t in b:
        cb[t] = cb.get(t, 0) + 1
    mism = sum(abs(ca.get(t, 0) - cb.get(t, 0)) for t in set(ca) | set(cb))
    per_mesh.append((mism, len(a)))
frac = max(m / n for m, n in per_mesh)
check(frac < 0.02, "triangles (position/normal/uv/weights, winding) match stock — mismatch per mesh %s (rounding-boundary noise only)" % per_mesh)
check([me["vertex_buffers"][0]["count"] for me in m1["meshes"]] == [me["vertex_buffers"][0]["count"] for me in m0["meshes"]],
      "vertex counts identical to stock %s" % [me["vertex_buffers"][0]["count"] for me in m1["meshes"]])
check(m1["meshes"][0]["flags"] == m0["meshes"][0]["flags"] and m1["meshes"][0]["flags2"] == m0["meshes"][0]["flags2"], "mesh flags preserved")
check([m.get("shader_debug_name") for m in m1["materials"]] == [m.get("shader_debug_name") for m in m0["materials"]], "shader names preserved via debug block")
check([m["params"] for m in m1["materials"]] == [m["params"] for m in m0["materials"]], "material params preserved")
check([t["name"] for t in m1["texnames"]] == [t["name"] for t in m0["texnames"]], "packed texture names preserved")
b2it_rt = ktmdl_codec.parse_b2it(open(os.path.splitext(model_out)[0] + ".b2it", "rb").read())
b2it_st = ktmdl_codec.parse_b2it(open(os.path.splitext(MODEL)[0] + ".b2it", "rb").read())
check(b2it_rt == b2it_st, ".b2it identical to stock")
dds_out = [w for w in written if w.endswith(".dds")]
check(len(dds_out) == 1 and os.path.getsize(dds_out[0]) == os.path.getsize(os.path.join(os.path.dirname(MODEL), "mdx_emi01.dds")), "DDS written (%s)" % [os.path.basename(x) for x in dds_out])
bbox_err = max(abs(a - b) for a, b in zip(m0["info"]["bbox_max"][:3] + m0["info"]["bbox_min"][:3], m1["info"]["bbox_max"][:3] + m1["info"]["bbox_min"][:3]))
check(bbox_err < 1e-3, "info bbox preserved (max %.1e)" % bbox_err)
aabb_err = max(abs(a - b) for b0, b1 in zip(m0["bones"], m1["bones"]) for a, b in zip(b0["aabb_min"] + b0["aabb_max"], b1["aabb_min"] + b1["aabb_max"]))
check(aabb_err < 2e-3, "bone AABBs recomputed within %.1e of stock" % aabb_err)

# (c) .camanm: export the imported camera; positions / orientation / lens must survive
cam_out = os.path.join(OUT_DIR, "rt_" + os.path.basename(CAMANM))
bpy.context.view_layer.objects.active = cam
cdata, cspec = export_anm.export_camanm(cam_out, cam, frame_start=0, frame_end=600)
cre = anm_codec.parse_anm(open(cam_out, "rb").read())
cs = next(c for c in cre["chunks"] if c["type"] == 4)["tracks"]
worst_cp = worst_fov = 0.0
for f in (0, 120, 300, 600):
    p_ref = anm_codec.sample_track(cparsed["data"], slots[1], float(f))
    p_got = anm_codec.sample_track(cre["data"], cs[1], float(f))
    worst_cp = max(worst_cp, max(abs(a - b) for a, b in zip(p_ref, p_got)))
    fov_ref = anm_codec.sample_track(cparsed["data"], slots[2], float(f))[0]
    fov_got = anm_codec.sample_track(cre["data"], cs[2], float(f))[0]
    # the exporter re-derives the FILE fov from the lens through the game's 16:9 mapping with
    # aspect 4:3; compare the GAME-side half-tangent, which is what must match
    t_ref = import_anm.game_camera_half_tangent(fov_ref, anm_codec.sample_track(cparsed["data"], slots[5], 0.0)[0])
    t_got = import_anm.game_camera_half_tangent(fov_got, 4.0 / 3.0)
    worst_fov = max(worst_fov, abs(t_ref - t_got))
check(worst_cp < 0.05, "camera positions round-trip within %.3f cm (frame_step=30 import bake)" % worst_cp)
check(worst_fov < 1e-3, "camera half-tangent round-trips (max %.1e)" % worst_fov)

# (d) the exported model must import again and land on the same bone/vertex positions
if OUT_BLEND:
    bpy.ops.wm.save_as_mainfile(filepath=OUT_BLEND)
    print("saved", OUT_BLEND)
bpy.ops.wm.read_factory_settings(use_empty=True)
arm2, objs2 = import_model.load_model(model_out)
if arm2 is None:
    print("FAIL: re-import produced no armature")
    sys.exit(1)
check(len(arm2.data.bones) == 33, "re-import of the exported model: 33 bones")
hips2 = arm2.data.bones["Hips"]
check(abs(hips2.matrix_local.translation.z - 0.97) < 0.02, "re-imported Hips at 0.97 m")
check(sum(len(o.data.vertices) for o in objs2) == sum(me["vertex_buffers"][0]["count"] for me in m1["meshes"]), "re-imported vertex count matches file")

if failures:
    print("FAILED: %d check(s)" % len(failures))
    sys.exit(1)
print("ALL CHECKS PASSED")
