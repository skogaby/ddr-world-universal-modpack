"""Headless exporter edge-case tests that need no game data. Run via scripts/validate_blender_addon.sh.

  * a rig with MORE than 52 influencing bones exports as several KTMDL meshes, each with its
    own <= 52-slot palette block (doc §3.4 — the loader remaps blend indices through the
    mesh's own +0x1C slice and copies header.palette_count x 52 slots), and every vertex
    still resolves to the intended GLOBAL bone after re-parsing
  * a rig that fits in 52 bones keeps the stock single-table layout (header palette_count 1)
  * a stage-screen quad (image named `offscreen1`) exports the KTMDL texture name `offscreen1`
    and an 8x8 `offscreen1.dds` placeholder instead of the image pixels (Background Movies =
    STAGE SCREENS — the game binds its movie render target by that name)
"""
import math
import os
import shutil
import struct
import sys
import tempfile

import bpy

ADDON_DIR = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
sys.path.insert(0, os.path.dirname(ADDON_DIR))
addon = __import__(os.path.basename(ADDON_DIR))
addon.register()
from blender_ddr_addon import export_model  # noqa: E402
from blender_ddr_addon.codec import ktmdl as K  # noqa: E402

failures = []


def check(cond, msg):
    print(("  ok   " if cond else "  FAIL ") + msg)
    if not cond:
        failures.append(msg)


def build_chain_rig(n_bones, verts_per_bone=4):
    """n_bones bones along +Z (each 0.1 m), a strip mesh with verts_per_bone vertices weighted
    100 % to bone i at height 0.1*i; triangles connect consecutive rings."""
    bpy.ops.wm.read_factory_settings(use_empty=True)
    arm_data = bpy.data.armatures.new("Rig")
    arm = bpy.data.objects.new("Rig", arm_data)
    bpy.context.collection.objects.link(arm)
    bpy.context.view_layer.objects.active = arm
    bpy.ops.object.mode_set(mode="EDIT")
    prev = None
    names = []
    for i in range(n_bones):
        eb = arm_data.edit_bones.new("b%03d" % i)
        eb.head = (0.0, 0.0, 0.1 * i)
        eb.tail = (0.0, 0.0, 0.1 * i + 0.1)
        if prev is not None:
            eb.parent = prev
        prev = eb
        names.append(eb.name)
    bpy.ops.object.mode_set(mode="OBJECT")
    arm["ddr_bone_order"] = names

    verts, faces = [], []
    for i in range(n_bones):
        for k in range(verts_per_bone):
            a = 2 * math.pi * k / verts_per_bone
            verts.append((0.05 * math.cos(a), 0.05 * math.sin(a), 0.1 * i))
    for i in range(n_bones - 1):
        for k in range(verts_per_bone):
            a, b = i * verts_per_bone + k, i * verts_per_bone + (k + 1) % verts_per_bone
            c, d = a + verts_per_bone, b + verts_per_bone
            faces.append((a, b, d))
            faces.append((a, d, c))
    me = bpy.data.meshes.new("Strip")
    me.from_pydata(verts, [], faces)
    me.validate()
    me.uv_layers.new(name="UVMap")
    obj = bpy.data.objects.new("Strip", me)
    bpy.context.collection.objects.link(obj)
    obj.parent = arm
    mod = obj.modifiers.new("Armature", "ARMATURE")
    mod.object = arm
    for i in range(n_bones):
        vg = obj.vertex_groups.new(name=names[i])
        vg.add(list(range(i * verts_per_bone, (i + 1) * verts_per_bone)), 1.0, "REPLACE")
    mat = bpy.data.materials.new("m")
    mat.use_nodes = True
    me.materials.append(mat)
    bpy.context.view_layer.update()
    return arm, obj, names


def bone_of_every_vertex(model):
    """{(mesh index, vertex index): global bone} via each mesh's own palette."""
    out = {}
    for me in model["meshes"]:
        for vi, v in enumerate(K.read_vertices(model, me)):
            top = max(zip(v["WEIGHTS4"], v["BLENDINDICES"]))[1]
            out[(me["index"], vi)] = me["palette"][top]
    return out


for n_bones, expect_multi in ((64, True), (40, False)):
    print("== %d-bone chain rig" % n_bones)
    arm, obj, names = build_chain_rig(n_bones)
    spec, b2it, _ = export_model.build_spec(arm, [obj])
    data = K.write_model(spec)
    model = K.parse_model(data)
    hdr_pal = struct.unpack_from("<I", data, 0x20)[0]
    check(len(model["bones"]) == n_bones, "%d bones written" % n_bones)
    if expect_multi:
        check(len(model["meshes"]) >= 2, "split into %d meshes" % len(model["meshes"]))
        check(hdr_pal == len(model["meshes"]) or hdr_pal >= 2, "header palette_count %d (52-slot blocks)" % hdr_pal)
    else:
        check(len(model["meshes"]) == 1 and hdr_pal == 1, "single mesh, stock single-table palette (count %d)" % hdr_pal)
    check(all(len(me["palette"]) <= 52 for me in model["meshes"]), "every mesh palette <= 52 (%s)" % [len(me["palette"]) for me in model["meshes"]])
    # every vertex resolves to the bone at its own height
    ok = True
    total = 0
    for (mi, vi), bone in bone_of_every_vertex(model).items():
        v = K.read_vertices(model, model["meshes"][mi])[vi]
        z_game = v["POSITION"][1]  # Blender Z -> game Y
        want = int(round(z_game / 0.1))
        ok &= (bone == want)
        total += 1
    check(ok, "%d vertices resolve to the intended global bone through their mesh palette" % total)
    check(sum(me["index_buffers"][0]["count"] for me in model["meshes"]) == 3 * len(obj.data.polygons),
          "all triangles kept across the split")
    # re-serialise through model_to_spec: multi-palette files must round-trip byte-identically too
    check(K.write_model(K.model_to_spec(model)) == data, "writer round-trip byte-identical")


def build_screen_quad(image_name):
    """A static 1x1 m quad (no armature) with a white colour attribute and a material whose Image
    Texture node shows a 64x32 image called `image_name`."""
    bpy.ops.wm.read_factory_settings(use_empty=True)
    me = bpy.data.meshes.new("Screen")
    me.from_pydata([(-0.5, 0.0, 0.0), (0.5, 0.0, 0.0), (0.5, 0.0, 1.0), (-0.5, 0.0, 1.0)], [],
                   [(0, 1, 2), (0, 2, 3)])
    me.validate()
    me.uv_layers.new(name="UVMap")
    col = me.color_attributes.new(name="Col", type="BYTE_COLOR", domain="CORNER")
    for d in col.data:
        d.color_srgb = (1.0, 1.0, 1.0, 1.0)
    obj = bpy.data.objects.new("gm_test00_screen", me)
    bpy.context.collection.objects.link(obj)
    img = bpy.data.images.new(image_name, width=64, height=32)
    img.pixels = [0.2, 0.4, 0.8, 1.0] * (64 * 32)
    mat = bpy.data.materials.new("screen")
    mat.use_nodes = True
    node = mat.node_tree.nodes.new("ShaderNodeTexImage")
    node.image = img
    me.materials.append(mat)
    bpy.context.view_layer.update()
    return obj


def dds_dims(path):
    with open(path, "rb") as f:
        head = f.read(20)
    if head[:4] != b"DDS ":
        return None
    height, width = struct.unpack_from("<II", head, 12)
    return width, height


for image_name in ("offscreen1", "OffScreen_1.001"):
    print("== stage-screen quad (image %r)" % image_name)
    obj = build_screen_quad(image_name)
    out = tempfile.mkdtemp(prefix="ddr_screen_")
    path = os.path.join(out, "gm_test00_screen.model")
    written, spec = export_model.export_model(path, None, [obj], write_textures=True)
    model = K.parse_model(open(path, "rb").read())
    names = [t["name"] for t in model["texnames"]]
    check(names == ["offscreen1"], "KTMDL texture name %s" % names)
    placeholder = os.path.join(out, "offscreen1.dds")
    check(placeholder in written and os.path.exists(placeholder), "offscreen1.dds written (%s)" %
          sorted(os.path.basename(w) for w in written))
    check(dds_dims(placeholder) == (8, 8), "placeholder DDS is 8x8 (%s)" % (dds_dims(placeholder),))
    others = [w for w in written if w.lower().endswith(".dds") and w != placeholder]
    check(not others, "no other DDS written (%s)" % [os.path.basename(w) for w in others])
    shutil.rmtree(out, ignore_errors=True)

if failures:
    print("FAILED: %d check(s)" % len(failures))
    sys.exit(1)
print("ALL SYNTHETIC CHECKS PASSED")
