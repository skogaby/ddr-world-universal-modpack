"""Export an armature (+ its child meshes) as a KTMDL .model with sibling .b2it / .grp2it
(and optionally uncompressed .dds textures).

All format work is done by scripts/ktmdl_dump.py::write_model; this module turns Blender
data into the writer's spec:
  * bones: the armature's rest pose IS the bind pose (Blender matrix_local -> game
    row-vector world matrix, inverse computed), file order = obj["ddr_bone_order"] when
    the armature came from an import, else Blender's bone order; names -> .b2it,
    XOR-folded 6-bit identities -> the .model.
  * meshes: triangulated, split per (material) into KTMDL meshes, vertex layout A/D
    (skinned, with/without COLOR0) or B/C (static); UV v flipped back (1 - v); D3DCOLOR
    BGRA; <=4 weights/vertex normalized with w0 implicit; ONE shared bone palette <= 52.
  * materials: shader name from mat["ddr_shader"] (default mdl_ch_constant_vc /
    mdl_bg_constant_vc), params from mat["ddr_params"] or the stock defaults, texture
    from the Principled BSDF's Image Texture.
"""
import math
import os
import re
import struct

import bpy
from mathutils import Matrix, Vector

from . import convert
from .codec import ktmdl

STOCK_PARAMS_3 = [[1.0, 1.0, 0.0, 0.0], [1.0, 1.0, 1.0, 0.0], [0.0, 0.0, 0.0, 0.0]]
STOCK_PARAMS_2 = [[1.0, 1.0, 0.0, 0.0], [1.0, 1.0, 1.0, 0.0]]

# (stream, offset, type_raw, usage_raw) — doc §3.6 layouts
DECL_FLOAT3, DECL_UBYTE4, DECL_FLOAT16_2, DECL_D3DCOLOR = 2, 0xB, 0x10, 0x12
USE_POSITION, USE_NORMAL, USE_COLOR0, USE_TEXCOORD0, USE_BLENDWEIGHT, USE_BLENDINDICES = 0x10, 0x12, 0x13, 0x16, 0x20, 0x21


def layout_for(skinned, has_uv, has_color):
    els = [(0, 0, DECL_FLOAT3, USE_POSITION)]
    off = 12
    if skinned:
        els.append((0, off, DECL_UBYTE4, USE_BLENDINDICES)); off += 4
        els.append((0, off, DECL_FLOAT3, USE_BLENDWEIGHT)); off += 12
    els.append((0, off, DECL_FLOAT3, USE_NORMAL)); off += 12
    if has_uv:
        els.append((0, off, DECL_FLOAT16_2, USE_TEXCOORD0)); off += 4
    if has_color:
        els.append((0, off, DECL_D3DCOLOR, USE_COLOR0)); off += 4
    return els, off


def _f16(v):
    return struct.unpack("<H", struct.pack("<e", max(-65504.0, min(65504.0, v))))[0]


def sanitize_texture_stem(name):
    """Texture file stem the game can find: alnum + '_' only, <= 20 alphanumerics (§3.8)."""
    stem = os.path.splitext(os.path.basename(name))[0]
    stem = re.sub(r"[^A-Za-z0-9_]", "", stem)
    if len(re.sub(r"_", "", stem)) > 20:
        raise ValueError("texture name %r has more than 20 alphanumerics" % name)
    if not stem:
        raise ValueError("texture name %r is empty after sanitizing" % name)
    return stem


def _game_world_rowmat(mat_bl):
    """Blender local->world (armature space) matrix -> the game's row-vector bind matrix."""
    return [float(x) for x in convert.rowmat_from_blender(mat_bl)]


def _collect_bones(arm_obj):
    order = list(arm_obj.get("ddr_bone_order", [])) if arm_obj else []
    bones = arm_obj.data.bones if arm_obj else []
    if not order or any(n not in bones for n in order) or len(order) != len(bones):
        # Blender's own order is hierarchical (parents first) which is all the format needs.
        order = [b.name for b in bones]
    index = {n: i for i, n in enumerate(order)}
    spec_bones = []
    for name in order:
        b = bones[name]
        bind = _game_world_rowmat(b.matrix_local)
        parent = index[b.parent.name] if b.parent else -1
        if parent >= index[name]:
            raise ValueError("bone %s comes before its parent in ddr_bone_order" % name)
        spec_bones.append(dict(
            identity=ktmdl.pack_identity(name), bind=bind, inverse_bind=None,
            aabb_min=[0.0, 0.0, 0.0], aabb_max=[0.0, 0.0, 0.0], parent=parent, flags=None,
        ))
    return order, index, spec_bones


def _material_info(mat, skinned, has_color=True):
    # The `_vc` programs read COLOR0 from the vertex declaration; a mesh without a colour
    # attribute (layout C/D) fed to a `_vc` shader gets no COLOR0 -> the multiplied-in colour
    # is (0,0,0,0) and the alpha test (ref 127) kills every pixel: the mesh is INVISIBLE
    # (2026-09-15, the first ported character showed an empty dance pad). Pick the colourless
    # variant by default and warn when an explicit `_vc` shader meets a colourless mesh.
    default_shader = ("mdl_ch_constant_vc" if has_color else "mdl_ch_constant") if skinned else \
                     ("mdl_bg_constant_vc" if has_color else "mdl_bg_constant")
    shader = (mat.get("ddr_shader") if mat else None) or default_shader
    if not has_color and "_vc" in shader and "notex" not in shader:
        print("[ddr] WARNING material %s uses %s but its mesh has no colour attribute: the game draws "
              "nothing (alpha-test on the missing COLOR0). Add a white colour attribute or pick a "
              "shader without _vc." % (mat.name if mat else "<none>", shader))
    params = mat.get("ddr_params") if mat else None
    if params:
        params = [list(params[i:i + 4]) for i in range(0, len(params) - len(params) % 4, 4)] or None
    if not params:
        params = STOCK_PARAMS_2 if "lambert" in shader else STOCK_PARAMS_3
    image = None
    if mat and mat.use_nodes:
        for node in mat.node_tree.nodes:
            if node.type == "TEX_IMAGE" and node.image:
                image = node.image
                break
    ident = mat.get("ddr_identity") if mat else None
    identity = int(ident, 16) if ident else ktmdl.pack_identity(mat.name if mat else "lambert1")
    return shader, params, image, identity


def _mesh_flags(obj, mat):
    if "ddr_flags" in obj:
        return int(obj["ddr_flags"]), int(obj.get("ddr_flags2", 0))
    flags, flags2 = 0, 0
    if mat is not None:
        if not mat.use_backface_culling:
            flags |= ktmdl.MESH_FLAG_TWO_SIDED
        blended = getattr(mat, "surface_render_method", "") == "BLENDED" or getattr(mat, "blend_method", "") == "BLEND"
        if blended:
            flags |= ktmdl.MESH_FLAG_TRANSPARENT | 0x0280  # stock alpha-blended props use 0x02C0
            flags2 = 0
    return flags, flags2


def _eval_mesh(obj, depsgraph):
    ob_eval = obj.evaluated_get(depsgraph)
    me = ob_eval.to_mesh()
    me.calc_loop_triangles()
    return ob_eval, me


class _RestPose:
    """Evaluate meshes in the armature's REST pose (the bind pose is what the file stores;
    the depsgraph would otherwise bake the current frame's deformation into the vertices)."""

    def __init__(self, arm_obj):
        self.arm = arm_obj
        self.saved = None

    def __enter__(self):
        if self.arm is not None:
            self.saved = self.arm.data.pose_position
            self.arm.data.pose_position = "REST"
            bpy.context.view_layer.update()
        return self

    def __exit__(self, *exc):
        if self.arm is not None and self.saved is not None:
            self.arm.data.pose_position = self.saved
            bpy.context.view_layer.update()


def _vkey(vi, uv, col, weights):
    """Dedup key: Blender vertex + uv + colour + weights. Normals are merged separately by
    angle (Blender's evaluated corner normals jitter in the 6th decimal across the loops
    of one vertex, and rounding would split on boundaries)."""
    return (vi, tuple(round(c, 6) for c in uv), tuple(round(c, 4) for c in col), weights)


NORMAL_MERGE_COS = 0.99995  # ~0.6 degrees


def build_spec(arm_obj, mesh_objs, texture_stems=None, raw_axes=False):
    """Returns (spec, b2it_entries, textures_to_write) where textures_to_write is
    [(stem, bpy.types.Image)].

    raw_axes=True is the character-PART mode: vertices are written in the objects' own
    coordinates (no Z-up -> Y-up conversion, no armature) because the game supplies the
    axes through bone attachment (import_character.py); any object-level transform beyond
    the game's own mirror (obj["ddr_part_mirror"]) is baked into the vertices."""
    with _RestPose(arm_obj):
        return _build_spec(arm_obj, mesh_objs, texture_stems, raw_axes)


def _part_to_model(obj):
    """Object -> part-model space for a bone-parented part: the game applies its own extra
    matrix (uniform scale / the right-forearm inversion) at runtime, so only what the user
    changed on top of the imported basis belongs in the file."""
    basis = obj.matrix_basis.copy()
    if obj.get("ddr_part_mirror"):
        basis = Matrix.Diagonal((-1.0, -1.0, -1.0, 1.0)) @ basis  # undo the game's diag(-1,-1,-1)
    return basis


def partition_tris_by_bones(tri_bones, cap):
    """Greedy split of a triangle list into runs whose union of influencing bones stays
    <= cap. tri_bones = [set(global bone)] per triangle (<= 12 each, so any triangle fits an
    empty group). Returns [(bone_set, [tri index])]."""
    groups = []
    cur_set, cur = set(), []
    for t, bones in enumerate(tri_bones):
        if cur and len(cur_set | bones) > cap:
            groups.append((cur_set, cur))
            cur_set, cur = set(), []
        cur_set |= bones
        cur.append(t)
    if cur:
        groups.append((cur_set, cur))
    return groups


def _pack_vertex(rec, skinned, has_uv, has_color, local_of):
    """One vertex record -> bytes for the chosen layout; local_of maps global bone -> palette slot."""
    pg, ng, uv, col, weights = rec
    out = bytearray(struct.pack("<3f", *pg))
    if skinned:
        locals_ = [local_of[b] for _, b in weights] + [0] * (4 - len(weights))
        wlist = [w for w, _ in weights] + [0.0] * (4 - len(weights))
        out += struct.pack("<4B", *locals_)
        out += struct.pack("<3f", wlist[1], wlist[2], wlist[3])
    out += struct.pack("<3f", *ng)
    if has_uv:
        out += struct.pack("<HH", _f16(uv[0]), _f16(1.0 - uv[1]))
    if has_color:
        r_, g_, b_, a_ = (max(0, min(255, int(round(c * 255)))) for c in col)
        out += bytes((b_, g_, r_, a_))
    return bytes(out)


def _build_spec(arm_obj, mesh_objs, texture_stems, raw_axes=False):
    depsgraph = bpy.context.evaluated_depsgraph_get()
    if raw_axes:
        arm_obj = None
    order, bone_index, spec_bones = _collect_bones(arm_obj)
    skinned_any = arm_obj is not None
    to_game = (lambda v: tuple(v[:3])) if raw_axes else convert.vec_to_game
    cap = ktmdl.PALETTE_BLOCK

    global_order = []  # global bone indices in order of first use (the shared palette candidate)
    seen_global = set()
    materials = []
    mat_index = {}
    texnames = []
    textures = []
    tex_index_by_stem = {}
    tex_key_stem = {}  # registry key -> stem (collision guard)
    debug_tex = []
    debug_shaders = []
    tex_write = []
    meshes = []       # spec dicts, vertex bytes filled in after the palette decision
    pending = []      # per mesh: (skinned, has_uv, has_color, records, group_bone_order)
    all_min = [math.inf] * 3
    all_max = [-math.inf] * 3
    bone_pts = {}

    for obj in mesh_objs:
        ob_eval, me = _eval_mesh(obj, depsgraph)
        arm_mod = next((m for m in obj.modifiers if m.type == "ARMATURE" and m.object == arm_obj), None)
        skinned = skinned_any and arm_mod is not None and bool(obj.vertex_groups)
        # object -> armature (model) space -> game space; parts: object -> part-model space
        if raw_axes:
            to_model = _part_to_model(obj) if obj.parent_type == "BONE" else Matrix.Identity(4)
        else:
            to_model = (arm_obj.matrix_world.inverted() if arm_obj else Matrix.Identity(4)) @ obj.matrix_world
        normal_m = to_model.to_3x3().inverted().transposed()
        has_uv = bool(me.uv_layers)
        color_layer = me.color_attributes.active_color if me.color_attributes else None
        has_color = color_layer is not None
        uv_layer = me.uv_layers.active.data if has_uv else None
        # Exact per-vertex normals kept by the importer (Blender's custom-normal encoding is
        # lossy on slivers); only trusted when the vertex count still matches.
        exact_n = me.attributes.get("ddr_normal")
        if exact_n is not None and (exact_n.domain != "POINT" or len(exact_n.data) != len(me.vertices)):
            exact_n = None
        vg_bone = {}
        for vg in obj.vertex_groups:
            if vg.name in bone_index:
                vg_bone[vg.index] = bone_index[vg.name]

        # split loops per material slot -> one KTMDL mesh per used slot (more when the slot's
        # triangles reference more than `cap` bones)
        per_slot = {}
        for tri in me.loop_triangles:
            per_slot.setdefault(tri.material_index, []).append(tri)
        for slot, tris in sorted(per_slot.items()):
            mat = obj.material_slots[slot].material if slot < len(obj.material_slots) else None
            shader, params, image, identity = _material_info(mat, skinned, has_color)
            mkey = (mat.name if mat else None, shader)
            if mkey not in mat_index:
                mat_index[mkey] = len(materials)
                materials.append(dict(identity=identity, shader=shader, params=params))
                if shader not in debug_shaders:
                    debug_shaders.append(shader)
            tex_slots = []
            if image is not None:
                stem = (texture_stems or {}).get(image.name) or sanitize_texture_stem(image.name)
                if stem not in tex_index_by_stem:
                    # The game registers every loaded DDS process-wide under FNV-1(lower(stem) minus
                    # '_') (§3.8): two images whose stems fold to the same key would overwrite each
                    # other at load time. Stock textures are in the same registry, so also avoid
                    # reusing a stock name for different art.
                    key = ktmdl.texture_registry_key(stem)
                    other = tex_key_stem.get(key)
                    if other is not None and other != stem:
                        raise ValueError("texture names %r and %r collide (the game keys textures by "
                                         "case-insensitive name without underscores)" % (other, stem))
                    tex_key_stem[key] = stem
                    tex_index_by_stem[stem] = len(textures)
                    texnames.append(ktmdl.pack_texname(stem))
                    textures.append(dict(identity=ktmdl.pack_identity("file%d" % (len(textures) + 1)), kind=0,
                                         texname_index=len(texnames) - 1, f10=1.0, f14=1.0))
                    debug_tex.append(stem + ".dds")
                    tex_write.append((stem, image))
                tex_slots = [tex_index_by_stem[stem]]

            # per-corner records (game space, GLOBAL bone indices) + each triangle's bone set
            corners = []
            tri_bones = []
            for tri in tris:
                recs = []
                bones = set()
                for li in tri.loops:
                    loop = me.loops[li]
                    v = me.vertices[loop.vertex_index]
                    if exact_n is not None:
                        # keep the file's exact (sometimes non-unit) normal; no re-normalization
                        n_bl = normal_m @ Vector(exact_n.data[loop.vertex_index].vector)
                    else:
                        n_bl = (normal_m @ loop.normal).normalized()
                    uv = tuple(uv_layer[li].uv) if uv_layer is not None else (0.0, 0.0)
                    if color_layer is not None:
                        ci = li if color_layer.domain == "CORNER" else loop.vertex_index
                        cd = color_layer.data[ci]
                        # BYTE_COLOR stores sRGB bytes; the game's D3DCOLOR is those same bytes
                        col = tuple(cd.color_srgb) if hasattr(cd, "color_srgb") else tuple(cd.color)
                    else:
                        col = (1.0, 1.0, 1.0, 1.0)
                    weights = ()
                    if skinned:
                        ws = sorted(((g.weight, vg_bone[g.group]) for g in v.groups if g.group in vg_bone and g.weight > 0),
                                    reverse=True)[:4]
                        total = sum(w for w, _ in ws)
                        if total <= 0:
                            ws = [(1.0, 0)]
                            total = 1.0
                        weights = tuple((w / total, b) for w, b in ws)
                        bones.update(b for _, b in weights)
                    recs.append((loop.vertex_index, uv, col, weights, n_bl, v.co))
                corners.append(recs)
                tri_bones.append(bones)
            groups = partition_tris_by_bones(tri_bones, cap) if skinned else [(set(), list(range(len(tris))))]

            for g_bones, g_tris in groups:
                # unique vertices: same Blender vertex + uv + colour + weights, normals merged by angle
                verts = {}       # key -> [(normal Vector, index), ...]
                records = []     # per emitted vertex: (pg, ng, uv, col, weights)
                positions = []
                indices = []
                group_order = []
                for t in g_tris:
                    for (vi, uv, col, weights, n_bl, co) in corners[t]:
                        key = _vkey(vi, uv, col, weights)
                        bucket = verts.setdefault(key, [])
                        n_unit = n_bl.normalized() if n_bl.length_squared > 0 else n_bl
                        idx = next((i for n_, i in bucket if n_.dot(n_unit) >= NORMAL_MERGE_COS), None)
                        if idx is None:
                            idx = len(records)
                            bucket.append((n_unit, idx))
                            pg = to_game(to_model @ co)
                            ng = to_game(n_bl)
                            records.append((pg, ng, uv, col, weights))
                            positions.append(pg)
                            for _, b in weights:
                                bone_pts.setdefault(b, []).append(pg)
                                if b not in seen_global:
                                    seen_global.add(b)
                                    global_order.append(b)
                                if b not in group_order:
                                    group_order.append(b)
                            for k in range(3):
                                all_min[k] = min(all_min[k], pg[k])
                                all_max[k] = max(all_max[k], pg[k])
                        indices.append(idx)
                if len(positions) > 65535:
                    raise ValueError("%s: more than 65535 vertices in one KTMDL mesh" % obj.name)
                # bounding sphere (centre of the AABB, radius to the farthest vertex)
                bmin = [min(p[k] for p in positions) for k in range(3)]
                bmax = [max(p[k] for p in positions) for k in range(3)]
                cx = [(a + b) * 0.5 for a, b in zip(bmin, bmax)]
                rad = max(math.dist(p, cx) for p in positions)
                flags, flags2 = _mesh_flags(obj, mat)
                els, stride = layout_for(skinned, has_uv, has_color)
                meshes.append(dict(
                    flags=flags, flags2=flags2, primitive_raw=1, material=mat_index[mkey], node=0,
                    texture_slots=tex_slots, texture_slot_count=len(tex_slots),
                    bounding_sphere=cx + [rad], elements=els, stride=stride,
                    vertex_count=len(records), vertex_data=b"",
                    index_count=len(indices), index_data=struct.pack("<%dH" % len(indices), *indices),
                    palette_count=None, palette=None,
                ))
                pending.append((skinned, has_uv, has_color, records, group_order))
        ob_eval.to_mesh_clear()

    if not meshes:
        raise ValueError("no mesh data to export")

    # Palette decision: one shared table (the stock layout) whenever the whole model fits in
    # 52 slots, else a per-mesh table (52-slot blocks, see ktmdl.write_model).
    shared = len(global_order) <= cap
    palette = list(global_order) if global_order else [0]
    for m, (skinned, has_uv, has_color, records, group_order) in zip(meshes, pending):
        table = palette if (shared or not skinned) else group_order
        local_of = {b: i for i, b in enumerate(table)}
        m["vertex_data"] = b"".join(_pack_vertex(r, skinned, has_uv, has_color, local_of) for r in records)
        if skinned and not shared:
            m["palette"] = list(table)
            m["palette_count"] = len(table)
    if not shared:
        palette = list(pending[0][4]) or [0]  # writer fallback for meshes without their own table

    # bone AABBs: the stock exporter takes the MODEL-space box of the bone's influenced vertices
    # and pushes its two corners through the inverse bind matrix (verified exact on 313 stock
    # bones; NOT a tight bone-space box, and the corners are stored unsorted).
    for b_i, pts in bone_pts.items():
        inv = ktmdl.invert_matrix4(spec_bones[b_i]["bind"])
        mn = [min(p[k] for p in pts) for k in range(3)]
        mx = [max(p[k] for p in pts) for k in range(3)]

        def xf(p):
            return [p[0] * inv[c] + p[1] * inv[4 + c] + p[2] * inv[8 + c] + inv[12 + c] for c in range(3)]
        spec_bones[b_i]["aabb_min"] = xf(mn)
        spec_bones[b_i]["aabb_max"] = xf(mx)
    if not spec_bones:
        spec_bones = [dict(identity=ktmdl.pack_identity("root"), bind=[1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1],
                           inverse_bind=None, aabb_min=[0, 0, 0], aabb_max=[0, 0, 0], parent=-1, flags=None)]
        order = ["root"]

    info = dict(bbox_max=all_max + [0.0], bbox_min=all_min + [0.0])
    spec = dict(bones=spec_bones, palette=palette, meshes=meshes, nodes=None, info=info,
                materials=materials, texnames=texnames, textures=textures,
                debug=dict(texture_names=debug_tex, shader_names=debug_shaders))
    b2it = [(name, i) for i, name in enumerate(order)]
    return spec, b2it, tex_write


def image_rows_rgba(image):
    """Blender image pixels (bottom row first, float RGBA) -> list of top-first RGBA byte rows."""
    w, h = image.size
    px = [0.0] * (w * h * 4)
    image.pixels.foreach_get(px)
    rows = []
    for y in range(h - 1, -1, -1):
        row = px[y * w * 4:(y + 1) * w * 4]
        rows.append(bytes(max(0, min(255, int(round(c * 255)))) for c in row))
    return rows


def is_part_object(obj):
    """A character part imported/attached by import_character (bone-parented, raw axes)."""
    return obj.type == "MESH" and obj.parent is not None and obj.parent_type == "BONE"


SCREEN_TEXTURE_KEY = "offscreen1"
"""Stage-screen texture name (the DLL's Background Movies = STAGE SCREENS): the game registers its
1280x1280 movie render target at boot under this name, and the first registration of a name wins,
so a material textured `offscreen1` samples the song's movie in game. The DDS the exporter writes
for it is only a placeholder (it also marks the stage as "has screens" for the DLL)."""
SCREEN_PLACEHOLDER_SIZE = 8


def is_screen_texture(stem):
    """The stem folds (lower-case, '_' removed — the game's texture key) to `offscreen1`."""
    return stem.lower().replace("_", "") == SCREEN_TEXTURE_KEY


def write_textures_for(tex_write, out_dir):
    """Copy clean source .dds files or write A8R8G8B8 .dds next to the model; returns paths.
    A stage-screen image (`is_screen_texture`) gets an 8x8 opaque-black `offscreen1.dds` placeholder
    instead of its pixels — the game never binds it (the movie render target owns the name)."""
    written = []
    for stem, image in tex_write:
        if is_screen_texture(stem):
            path = os.path.join(out_dir, SCREEN_TEXTURE_KEY + ".dds")
            n = SCREEN_PLACEHOLDER_SIZE
            black = [bytes([0, 0, 0, 255]) * n for _ in range(n)]
            with open(path, "wb") as fo:
                fo.write(ktmdl.write_dds_a8r8g8b8(n, n, black))
            written.append(path)
            continue
        path = os.path.join(out_dir, stem + ".dds")
        src = bpy.path.abspath(image.filepath) if image.filepath else ""
        if src and src.lower().endswith(".dds") and os.path.exists(src) and not image.is_dirty:
            if os.path.abspath(src) != os.path.abspath(path):
                with open(src, "rb") as fi, open(path, "wb") as fo:
                    fo.write(fi.read())
        else:
            w, h = image.size
            with open(path, "wb") as fo:
                fo.write(ktmdl.write_dds_a8r8g8b8(w, h, image_rows_rgba(image)))
        written.append(path)
    return written


def export_model(filepath, arm_obj, mesh_objs, write_textures=True, raw_axes=None):
    """Write <filepath> (.model) + sibling .b2it / .grp2it (+ .dds). raw_axes=None auto-detects
    the part mode: no armature and every mesh is a bone-parented part object."""
    if raw_axes is None:
        raw_axes = arm_obj is None and bool(mesh_objs) and all(is_part_object(o) for o in mesh_objs)
    spec, b2it, tex_write = build_spec(arm_obj, mesh_objs, raw_axes=raw_axes)
    data = ktmdl.write_model(spec)
    base = os.path.splitext(filepath)[0]
    out_dir = os.path.dirname(os.path.abspath(filepath))
    os.makedirs(out_dir, exist_ok=True)
    with open(filepath, "wb") as f:
        f.write(data)
    with open(base + ".b2it", "wb") as f:
        f.write(ktmdl.write_b2it(b2it))
    with open(base + ".grp2it", "wb") as f:
        f.write(ktmdl.write_b2it([("model", 0)]))
    written = [filepath, base + ".b2it", base + ".grp2it"]
    if write_textures:
        written += write_textures_for(tex_write, out_dir)
    return written, spec
