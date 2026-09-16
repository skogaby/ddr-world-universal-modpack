"""Import a KTMDL .model (+ sibling .b2it / .dds) as armature + skinned meshes.

Everything format-related comes from scripts/ktmdl_dump.py (via codec.py); this
module only builds Blender data. Raw per-object game values that an exporter must
reproduce byte-for-byte are stored as custom properties prefixed ``ddr_``.
"""
import os

import bpy
from mathutils import Matrix, Vector

from . import convert
from .codec import ktmdl


def _find_b2it_names(model_path, bone_count):
    """Original bone names from the sibling .b2it (case + underscores intact)."""
    base = os.path.splitext(model_path)[0]
    path = base + ".b2it"
    if not os.path.exists(path):
        return None
    d = open(path, "rb").read()
    if d[:4] != b"B2IT":
        return None
    import struct

    count = struct.unpack_from("<I", d, 0x10)[0]
    names_off, idx_off = struct.unpack_from("<II", d, 0x14)
    names = [None] * bone_count
    for i in range(count):
        so = struct.unpack_from("<I", d, names_off + 4 * i)[0]
        bone = struct.unpack_from("<I", d, idx_off + 4 * i)[0]
        s = d[so:d.index(b"\0", so)].decode("ascii", "replace")
        if 0 <= bone < bone_count:
            names[bone] = s
    return names


def _find_texture_file(model_dir, texname):
    """Resolve a packed 20-char texname (lower-case, underscores stripped) to a
    .dds in the model's directory using the game's registry key rule (§3.8)."""
    want = ktmdl.texture_registry_key(texname)
    try:
        entries = os.listdir(model_dir)
    except OSError:
        return None
    for f in entries:
        stem, ext = os.path.splitext(f)
        if ext.lower() == ".dds" and ktmdl.texture_registry_key(stem) == want:
            return os.path.join(model_dir, f)
    return None


def _build_armature(model, names, obj_name):
    arm_data = bpy.data.armatures.new(obj_name + "_Armature")
    arm_obj = bpy.data.objects.new(obj_name + "_Armature", arm_data)
    bpy.context.collection.objects.link(arm_obj)
    bpy.context.view_layer.objects.active = arm_obj
    bpy.ops.object.mode_set(mode="EDIT")
    bones = model["bones"]
    edit_bones = []
    bone_names = []
    for b in bones:
        name = (names[b["index"]] if names else None) or b["name_folded"] or "bone_%d" % b["index"]
        eb = arm_data.edit_bones.new(name)
        mat = convert.rowmat_to_blender(b["bind"])
        # Give the bone a length first (the matrix setter keeps it), then impose the
        # game's exact joint frame so bone-local animation data applies unchanged.
        eb.head = mat.translation
        eb.tail = mat.translation + Vector((0.0, 0.05, 0.0))
        eb.matrix = mat
        edit_bones.append(eb)
        bone_names.append(eb.name)  # Blender may have de-duplicated the name
    for b, eb in zip(bones, edit_bones):
        if b["parent"] >= 0:
            eb.parent = edit_bones[b["parent"]]
    bpy.ops.object.mode_set(mode="OBJECT")
    # Exporter round-trip data. NOTE: armature.bones is re-ordered hierarchically once
    # edit mode ends, so bones must be addressed by NAME; ddr_bone_order keeps the
    # file order (= the .anm track target indices).
    for b, name in zip(bones, bone_names):
        bone = arm_data.bones[name]
        bone["ddr_bind"] = list(b["bind"])
        bone["ddr_aabb_min"] = list(b["aabb_min"])
        bone["ddr_aabb_max"] = list(b["aabb_max"])
        bone["ddr_identity"] = "%016x" % b["identity"]
    arm_obj["ddr_bone_order"] = bone_names
    return arm_obj


IDENTITY_TEXANIME = (1.0, 1.0, 0.0, 0.0)

# What each stock shader family actually computes (fxc /dumpbin of the A3 shader.arc blobs,
# doc §3.7). All of them are UNLIT — no mdl_* shader reads NORMAL for lighting.
#   mdl_*_constant_vc      : oC0 = tex2D(s0, uv') * (tint * COLOR0)
#   mdl_*_constant         : oC0 = tex2D(s0, uv') * tint                (no COLOR0)
#   mdl_*_constant_c[_vc]  : oC0.rgb = tex * (tint*COLOR0) * p1.rgb + p2.rgb ; .a = tex.a * a
#   mdl_*_constant_vc_notex: oC0 = tint * COLOR0
#   mdl_*_lambert          : NOT SHIPPED in A3's shader.arc -> falls back to
#                            gs_model[_skinning]_default = tex2D(s0, uv) * tint, COLOR0 and the
#                            material params IGNORED (uv NOT transformed), plus a screen-space
#                            stipple texkill driven by ModelParameters.y (a dissolve, off at 1.0)
#   uv' = uv * (1/p0.x, 1/p0.y) + p0.zw   (m_vTexAnime, mdl_* VS only)
# tint = the per-draw ModelUnitParameters.m_color (white unless the game fades the actor).
LAMBERT_NOTE = ("A3 shader.arc ships no mdl_*_lambert.gsp: the game renders this material with "
                "gs_model[_skinning]_default = unlit texture x draw tint; COLOR0, m_vTexAnime and the "
                "constant colour are ignored")


def _shader_traits(shader):
    """(uses_vertex_colour, uses_constant_colour, uses_texture, uses_texanime) for a mdl_* name."""
    s = shader.lower()
    if "lambert" in s or s.startswith("gs_model"):
        return False, False, True, False
    toks = s.split("_")
    return ("vc" in toks), ("c" in toks), ("notex" not in toks), True


def _material_for(model, mat_index, model_dir, cache, import_textures, mesh):
    key = mat_index
    if key in cache:
        return cache[key]
    m = model["materials"][mat_index]
    shader = m.get("shader_debug_name") or m["shader"] or "%08x" % m["shader_hash"]
    mat = bpy.data.materials.new("%s_%s" % (m["name_folded"] or "mat%d" % mat_index, shader))
    mat.use_nodes = True
    nt = mat.node_tree
    bsdf = nt.nodes.get("Principled BSDF")
    out = nt.nodes.get("Material Output")
    if bsdf is None or out is None:
        cache[key] = mat
        return mat
    # the game is unlit: kill the specular lobe, leave the diffuse for the viewport lights
    bsdf.inputs["Roughness"].default_value = 1.0
    if "Specular IOR Level" in bsdf.inputs:
        bsdf.inputs["Specular IOR Level"].default_value = 0.0
    use_vc, use_cc, use_tex, use_texanime = _shader_traits(shader)
    params = [tuple(p) for p in m["params"]]

    # Texture: the mesh's first texture slot -> texture record -> packed texname.
    tex_node = None
    if import_textures and use_tex and mesh["texture_slot_count"] > 0:
        tex_idx = mesh["texture_slots"][0] & 0xFFFF
        if tex_idx < len(model["textures"]):
            texname = model["textures"][tex_idx]["name"]
            path = _find_texture_file(model_dir, texname) if texname else None
            if path:
                img = bpy.data.images.get(os.path.basename(path)) or bpy.data.images.load(path, check_existing=True)
                tex_node = nt.nodes.new("ShaderNodeTexImage")
                tex_node.image = img
                tex_node.location = (-600, 300)
                # m_vTexAnime: uv' = uv*(1/x,1/y) + zw in D3D (top-down) space -> Blender's
                # bottom-up v: v_bl' = v_bl/y + (1 - 1/y - w)
                p0 = params[0] if params else IDENTITY_TEXANIME
                if use_texanime and tuple(round(c, 6) for c in p0) != IDENTITY_TEXANIME and p0[0] and p0[1]:
                    uv = nt.nodes.new("ShaderNodeUVMap")
                    uv.location = (-1000, 300)
                    mp = nt.nodes.new("ShaderNodeMapping")
                    mp.vector_type = "POINT"
                    mp.location = (-800, 300)
                    mp.inputs["Scale"].default_value = (1.0 / p0[0], 1.0 / p0[1], 1.0)
                    mp.inputs["Location"].default_value = (p0[2], 1.0 - 1.0 / p0[1] - p0[3], 0.0)
                    nt.links.new(uv.outputs["UV"], mp.inputs["Vector"])
                    nt.links.new(mp.outputs["Vector"], tex_node.inputs["Vector"])

    color_src = tex_node.outputs["Color"] if tex_node else None
    alpha_src = tex_node.outputs["Alpha"] if tex_node else None
    has_vc = any(e["usage"] == "COLOR0" for e in mesh["vertex_buffers"][0]["elements"])
    if use_vc and has_vc:
        vc = nt.nodes.new("ShaderNodeVertexColor")
        vc.layer_name = "Col"
        vc.location = (-600, 0)
        if color_src is not None:
            mix = nt.nodes.new("ShaderNodeMix")
            mix.data_type = "RGBA"
            mix.blend_type = "MULTIPLY"
            mix.inputs["Factor"].default_value = 1.0
            mix.location = (-300, 200)
            nt.links.new(color_src, mix.inputs[6])
            nt.links.new(vc.outputs["Color"], mix.inputs[7])
            color_src = mix.outputs[2]
            amix = nt.nodes.new("ShaderNodeMath")
            amix.operation = "MULTIPLY"
            amix.location = (-300, -100)
            nt.links.new(alpha_src, amix.inputs[0])
            nt.links.new(vc.outputs["Alpha"], amix.inputs[1])
            alpha_src = amix.outputs[0]
        else:
            color_src = vc.outputs["Color"]
            alpha_src = vc.outputs["Alpha"]
    if use_cc and len(params) > 1 and color_src is not None:
        p1 = params[1]
        cmul = nt.nodes.new("ShaderNodeMix")
        cmul.data_type = "RGBA"
        cmul.blend_type = "MULTIPLY"
        cmul.inputs["Factor"].default_value = 1.0
        cmul.inputs[7].default_value = (p1[0], p1[1], p1[2], 1.0)
        cmul.location = (-100, 200)
        cmul.label = "vConstantColor (p1)"
        nt.links.new(color_src, cmul.inputs[6])
        color_src = cmul.outputs[2]
        p2 = params[2] if len(params) > 2 else (0.0, 0.0, 0.0, 0.0)
        if any(abs(c) > 1e-6 for c in p2[:3]):
            cadd = nt.nodes.new("ShaderNodeMix")
            cadd.data_type = "RGBA"
            cadd.blend_type = "ADD"
            cadd.inputs["Factor"].default_value = 1.0
            cadd.inputs[7].default_value = (p2[0], p2[1], p2[2], 1.0)
            cadd.location = (100, 200)
            cadd.label = "vOffsetColor (p2)"
            nt.links.new(color_src, cadd.inputs[6])
            color_src = cadd.outputs[2]
    if color_src is not None:
        nt.links.new(color_src, bsdf.inputs["Base Color"])
    if alpha_src is not None:
        nt.links.new(alpha_src, bsdf.inputs["Alpha"])

    rs = mesh["render"]
    mat.use_backface_culling = not rs["two_sided"]
    if hasattr(mat, "surface_render_method"):
        mat.surface_render_method = "BLENDED" if rs["blend"] != "none" else "DITHERED"
    if hasattr(mat, "blend_method"):
        try:
            mat.blend_method = "BLEND" if rs["blend"] != "none" else ("CLIP" if rs["alpha_test"] else "OPAQUE")
        except TypeError:
            pass
    mat["ddr_shader"] = shader
    mat["ddr_param_count"] = m["param_count"]
    mat["ddr_params"] = [x for p in m["params"] for x in p]
    mat["ddr_identity"] = "%016x" % m["identity"]
    if "lambert" in shader.lower():
        mat["ddr_shader_note"] = LAMBERT_NOTE
    cache[key] = mat
    return mat


def _winding_needs_flip(verts_bl, normals_bl, tris):
    """Compare face winding against the stored vertex normals on a sample of
    triangles; True when the majority of computed face normals point the wrong way."""
    agree = disagree = 0
    step = max(1, len(tris) // 200)
    for tri in tris[::step]:
        a, b, c = (verts_bl[i] for i in tri)
        n = (b - a).cross(c - a)
        if n.length_squared == 0.0:
            continue
        avg = normals_bl[tri[0]] + normals_bl[tri[1]] + normals_bl[tri[2]]
        if n.dot(avg) >= 0:
            agree += 1
        else:
            disagree += 1
    return disagree > agree


def _split_duplicate_faces(tris, nverts):
    """Blender cannot hold two polygons over the same vertex set (validate() deletes them),
    but stock props draw some triangles twice on purpose (additive glows). Give every repeat
    its own copies of the vertices; returns (tris, vertex_source_index list of length new_n)."""
    seen = set()
    src = list(range(nverts))
    out = []
    for tri in tris:
        key = tuple(sorted(tri))
        if key in seen:
            new = []
            for vi in tri:
                src.append(vi)
                new.append(len(src) - 1)
            tri = tuple(new)
        else:
            seen.add(key)
        out.append(tri)
    return out, src


def _build_mesh(model, mesh, arm_obj, bone_names, obj_name, model_dir, mat_cache, import_textures,
                axis_convert=True):
    verts = ktmdl.read_vertices(model, mesh)
    indices = ktmdl.read_indices(model, mesh)
    tris = [tuple(indices[i:i + 3]) for i in range(0, len(indices) - 2, 3)]
    tris, src = _split_duplicate_faces(tris, len(verts))
    verts = [verts[i] for i in src]  # duplicated records for the split faces
    # axis_convert=False keeps the file's own coordinates (used for character PARTS, whose
    # vertices live in the attach bone's frame — the bone-parented object supplies the axes).
    to_bl = convert.vec_to_blender if axis_convert else (lambda v: Vector(v[:3]))
    positions = [to_bl(v["POSITION"]) for v in verts]
    has_normals = bool(verts) and "NORMAL" in verts[0]
    normals = [to_bl(v["NORMAL"]) for v in verts] if has_normals else []

    flip = has_normals and _winding_needs_flip(positions, normals, tris)
    if flip:
        tris = [(a, c, b) for a, b, c in tris]

    me = bpy.data.meshes.new("%s_mesh%d" % (obj_name, mesh["index"]))
    me.from_pydata([tuple(p) for p in positions], [], tris)
    me.validate(verbose=False)

    if has_normals:
        try:
            me.normals_split_custom_set_from_vertices(
                [tuple(n.normalized()) if n.length else (0.0, 0.0, 1.0) for n in normals])
        except (AttributeError, RuntimeError):
            pass
        # Blender's custom-normal encoding is lossy on sliver faces (up to ~10 deg seen); keep
        # the exact file normals so the exporter can round-trip them byte-for-byte.
        attr = me.attributes.new(name="ddr_normal", type="FLOAT_VECTOR", domain="POINT")
        flat = [c for n in normals for c in n]
        attr.data.foreach_set("vector", flat)

    if verts and "TEXCOORD0" in verts[0]:
        uv_layer = me.uv_layers.new(name="UVMap")
        uv_data = uv_layer.data
        for poly in me.polygons:
            for li in poly.loop_indices:
                u, v = verts[me.loops[li].vertex_index]["TEXCOORD0"]
                uv_data[li].uv = (u, 1.0 - v)  # D3D top-down -> Blender bottom-up (§3.6)

    if verts and "COLOR0" in verts[0]:
        col = me.color_attributes.new(name="Col", type="BYTE_COLOR", domain="POINT")
        for i, v in enumerate(verts):
            r, g, b, a = v["COLOR0"]
            # color_srgb = the raw bytes (the .color accessor would treat them as linear and re-encode)
            col.data[i].color_srgb = (r / 255.0, g / 255.0, b / 255.0, a / 255.0)

    obj = bpy.data.objects.new("%s_mesh%d" % (obj_name, mesh["index"]), me)
    bpy.context.collection.objects.link(obj)
    obj["ddr_flags"] = mesh["flags"]
    obj["ddr_flags2"] = mesh["flags2"]
    obj["ddr_material_index"] = mesh["material"]
    obj["ddr_bounding_sphere"] = list(mesh["bounding_sphere"])
    obj["ddr_layout"] = ["%s:%s@%d" % (e["usage"], e["type"], e["offset"]) for e in mesh["vertex_buffers"][0]["elements"]]
    obj["ddr_winding_flipped"] = flip

    if mesh["material"] < len(model["materials"]):
        me.materials.append(_material_for(model, mesh["material"], model_dir, mat_cache, import_textures, mesh))

    if arm_obj is not None and verts and "WEIGHTS4" in verts[0]:
        palette = mesh["palette"]
        groups = {}
        for vi, v in enumerate(verts):
            for local_idx, w in zip(v["BLENDINDICES"], v["WEIGHTS4"]):
                if w <= 0.0 or local_idx >= len(palette):
                    continue
                bone = palette[local_idx]
                g = groups.get(bone)
                if g is None:
                    g = groups[bone] = obj.vertex_groups.new(name=bone_names[bone])
                g.add([vi], w, "ADD")
        mod = obj.modifiers.new("Armature", "ARMATURE")
        mod.object = arm_obj
        obj.parent = arm_obj
    elif arm_obj is not None:
        obj.parent = arm_obj
    return obj


def load_model(filepath, import_textures=True, axis_convert=True, with_armature=True, name=None):
    """Import one .model; returns (armature_object_or_None, [mesh objects]).

    axis_convert=False keeps the file's coordinates (no Y-up -> Z-up rotation) and implies
    with_armature=False: used for character PART models, which are authored in the attach
    bone's frame and get their axes from bone parenting (import_character.py)."""
    data = open(filepath, "rb").read()
    model = ktmdl.parse_model(data)
    obj_name = name or os.path.splitext(os.path.basename(filepath))[0]
    model_dir = os.path.dirname(os.path.abspath(filepath))
    names = _find_b2it_names(filepath, len(model["bones"]))

    if bpy.context.object and bpy.context.object.mode != "OBJECT":
        bpy.ops.object.mode_set(mode="OBJECT")

    if not axis_convert:
        with_armature = False
    arm_obj = _build_armature(model, names, obj_name) if (model["bones"] and with_armature) else None
    bone_names = arm_obj["ddr_bone_order"] if arm_obj else []
    mat_cache = {}
    objs = [
        _build_mesh(model, mesh, arm_obj, bone_names, obj_name, model_dir, mat_cache, import_textures,
                    axis_convert=axis_convert)
        for mesh in model["meshes"]
    ]
    root = arm_obj or (objs[0] if objs else None)
    if root is not None:
        root["ddr_source"] = os.path.basename(filepath)
        root["ddr_shader_names"] = model["debug"]["shader_names"]
        root["ddr_bbox_min"] = list(model["info"]["bbox_min"])
        root["ddr_bbox_max"] = list(model["info"]["bbox_max"])
    return arm_obj, objs
