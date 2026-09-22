"""Shared helpers for porting a rigged humanoid from another game / MMD / a Blender rig onto the
DDR 33-bone dancer rig (the runbook's pose-conform + class/position weight-retarget flow, see
README "Porting an existing model" and port_character_ue_rig.py — this module is that script's
per-step machinery made reusable so a new character is a ~150-line config).

Import this from a per-character config script running inside Blender:

    import port_lib as P
    env = P.read_env()                       # SRC, OUT_DIR, DDR_3D_DATA, DDR_3D_RLIST, CHARA_KEY, DONOR
    arm, J, order = P.load_ddr_rig(env)      # the stock donor rig (joints J in Blender Z-up metres)
    fa, meshes = P.import_source(env['SRC']) # the character (GLB / FBX / DAE / OBJ)
    ... build targets / next_of from J and the source joints ...
    P.conform(fa, s, targets, next_of, terminal_len)
    P.bake_meshes(meshes, fa)                # world-space bake incl. evaluated normals, drops the rig
    P.retarget_weights(meshes, arm, J, order, classify)
    ... materials ...
    P.export_and_check(env, arm)
    P.preview_renders(env, arm, clips=[...])

Conventions: Blender Z-up metres everywhere in here (the add-on converts to the game's Y-up on
export). The donor rig's bind joints are the law — dance clips carry translation tracks for all
33 bones equal to that sex's bind offsets, so the game forces those joint positions at runtime.
"""
import math
import os
import re
import struct
import sys

import bpy
from mathutils import Matrix, Vector

ADDON_DIR = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), '..'))
if os.path.dirname(ADDON_DIR) not in sys.path:
    sys.path.insert(0, os.path.dirname(ADDON_DIR))
addon = __import__(os.path.basename(ADDON_DIR))
try:
    addon.register()
except Exception:  # noqa: BLE001 — already registered by a previous import in the same session
    pass
from blender_ddr_addon import import_character, export_character, import_anm  # noqa: E402
from blender_ddr_addon.codec import ktmdl as K  # noqa: E402

SIDES = {'l': 'Left', 'r': 'Right'}


# ---------------------------------------------------------------------------------------------
# environment / scene
# ---------------------------------------------------------------------------------------------
def read_env(default_key: str, default_donor: str = 'pl_emi00') -> dict:
    out_dir = os.environ['OUT_DIR']
    env: dict = {
        'SRC': os.environ['SRC'],
        'OUT': out_dir,
        'DATA': os.environ['DDR_3D_DATA'],
        'RLIST': os.environ['DDR_3D_RLIST'],
        'KEY': os.environ.get('CHARA_KEY') or default_key,
        'DONOR': os.environ.get('DONOR') or default_donor,
        'EXPORT': os.path.join(out_dir, 'export'),
        'TEX_DIR': os.environ.get('TEX_DIR') or os.path.join(out_dir, 'tex'),
    }
    os.makedirs(env['EXPORT'], exist_ok=True)
    return env


def fresh_scene():
    bpy.ops.wm.read_factory_settings(use_empty=True)


def purge_orphans():
    for db in (bpy.data.meshes, bpy.data.materials, bpy.data.images, bpy.data.armatures, bpy.data.actions):
        for d in list(db):
            if d.users == 0:
                db.remove(d)


# ---------------------------------------------------------------------------------------------
# the DDR rig
# ---------------------------------------------------------------------------------------------
def load_ddr_rig(env):
    """Import the donor body, drop its meshes/parts, return (armature, {bone: head Vector}, bone order).
    The armature's rlist scale is reset to 1 — the character's own scale goes in the sidecar row."""
    donor = env['DONOR']
    arm, body, parts, info = import_character.load_character(
        os.path.join(env['DATA'], 'chara', donor, donor + '.model'), import_textures=False, rlist_path=env['RLIST'])
    for o in body + parts:
        bpy.data.objects.remove(o, do_unlink=True)
    purge_orphans()
    assert len(arm.data.bones) == 33, len(arm.data.bones)
    arm.scale = (1.0, 1.0, 1.0)
    arm.name = env['KEY'] + '_Armature'
    arm['ddr_chara_key'] = env['KEY']
    order = list(arm['ddr_bone_order'])
    J = {n: Vector(arm.data.bones[n].head_local) for n in order}
    print('DDR rig', donor, 'rlist row', list(arm.get('ddr_rlist_row', [])))
    return arm, J, order


# ---------------------------------------------------------------------------------------------
# the source character
# ---------------------------------------------------------------------------------------------
def strip_glb_animations(src, dst):
    """Blender 5.2's glTF importer trips over shape-key animations targeting meshes without shape
    keys; we never need the source animations anyway."""
    d = open(src, 'rb').read()
    assert d[:4] == b'glTF', 'not a GLB'
    ln = struct.unpack_from('<I', d, 12)[0]
    import json
    j = json.loads(d[20:20 + ln])
    rest = d[20 + ln:]
    j.pop('animations', None)
    js = json.dumps(j, separators=(',', ':')).encode()
    while len(js) % 4:
        js += b' '
    out = b'glTF' + struct.pack('<II', 2, 12 + 8 + len(js) + len(rest)) + struct.pack('<II', len(js), 0x4E4F534A) + js + rest
    open(dst, 'wb').write(out)


def import_source(path, work_dir=None):
    """Import a GLB/FBX/DAE/OBJ; returns (source armature or None, [mesh objects])."""
    before = set(bpy.data.objects)
    ext = os.path.splitext(path)[1].lower()
    if ext in ('.glb', '.gltf'):
        if ext == '.glb':
            tmp = os.path.join(work_dir or os.path.dirname(path), '_noanim_' + os.path.basename(path))
            strip_glb_animations(path, tmp)
            path = tmp
        bpy.ops.import_scene.gltf(filepath=path, directory=os.path.dirname(path), files=[{'name': os.path.basename(path)}])
    elif ext == '.fbx':
        bpy.ops.import_scene.fbx(filepath=path, use_anim=False)
    elif ext == '.dae':
        bpy.ops.wm.collada_import(filepath=path)
    elif ext == '.obj':
        bpy.ops.wm.obj_import(filepath=path)
    else:
        raise ValueError('unsupported source ' + path)
    new = [o for o in bpy.data.objects if o not in before]
    arms = [o for o in new if o.type == 'ARMATURE']
    meshes = [o for o in new if o.type == 'MESH']
    bpy.context.view_layer.update()
    print('imported', os.path.basename(path), 'armatures', [(a.name, len(a.data.bones)) for a in arms],
          'meshes', [(o.name, len(o.data.vertices)) for o in meshes])
    return (arms[0] if arms else None), meshes, new


def delete_objects(objs):
    for o in objs:
        if o.name in bpy.data.objects:
            bpy.data.objects.remove(o, do_unlink=True)


def rest_head(fa, name):
    return fa.matrix_world @ fa.data.bones[name].head_local


def rest_tail(fa, name):
    return fa.matrix_world @ fa.data.bones[name].tail_local


def polyline_point(poly, t):
    """Point at arc-length fraction t (0..1) along the polyline [Vector, ...]."""
    segs = [(poly[i + 1] - poly[i]).length for i in range(len(poly) - 1)]
    total = sum(segs)
    d = max(0.0, min(1.0, t)) * total
    for i, sl in enumerate(segs):
        if d <= sl or i == len(segs) - 1:
            return poly[i] + (poly[i + 1] - poly[i]) * (d / sl if sl else 0.0)
        d -= sl
    return poly[-1]


def map_chain_arclength(fa, chain, ddr_poly):
    """Targets for a source bone chain [names] whose heads run along a body segment: each bone's
    head lands at the same arc-length fraction of the DDR polyline (the runbook's spine remap)."""
    pos = [rest_head(fa, n) for n in chain]
    cum = [0.0]
    for i in range(1, len(pos)):
        cum.append(cum[-1] + (pos[i] - pos[i - 1]).length)
    return {n: polyline_point(ddr_poly, c / cum[-1]) for n, c in zip(chain, cum)}


def mirror_x(v):
    return Vector((-v.x, v.y, v.z))


def bone_depth(b):
    d = 0
    while b.parent:
        b = b.parent
        d += 1
    return d


# ---------------------------------------------------------------------------------------------
# pose-conform
# ---------------------------------------------------------------------------------------------
def conform(fa, s, targets, next_of, terminal_len=None, y_scale=None, identity_rot=(), angle_warn_deg=12.0):
    """Pose the source rig so every bone in `targets` has its head at the given world point.

    s            uniform pre-scale (source units -> metres) applied to every placed bone's X/Z
    targets      {bone: Vector}  world target for the bone HEAD
    next_of      {bone: child}   the bone's Y axis is rotated + stretched so that `child`'s head
                                 (rest offset) lands on `targets[child]` — limb/spine chain bones
    terminal_len {bone: L|None}  bones without a next: stretch to world length L (None/absent = s)
    y_scale      {bone: sy}      explicit metres-per-source-unit along Y for identity bones (pelvis)
    identity_rot bones whose rotation is kept even if listed in next_of (rare)

    Every placed bone gets inherit_scale NONE (explicit placement); all other bones (fingers,
    twist/helper/physics bones) keep FULL inheritance and ride their parents. Uses REST data for
    every direction/length so processing order can never leak an already-moved neighbour in.
    """
    terminal_len = terminal_len or {}
    y_scale = y_scale or {}
    rest = {}
    R3w = fa.matrix_world.to_3x3()
    for b in fa.data.bones:
        rest[b.name] = (fa.matrix_world @ b.head_local, R3w @ b.matrix_local.to_3x3())
    chain = [n for n in targets if n in fa.data.bones]
    missing = [n for n in targets if n not in fa.data.bones]
    if missing:
        print('WARN conform: bones not in the source rig, ignored:', missing)
    for n in chain:
        fa.data.bones[n].inherit_scale = 'NONE'
    order = sorted(chain, key=lambda n: bone_depth(fa.data.bones[n]))
    report = []
    for n in order:
        pb = fa.pose.bones[n]
        head0, R0 = rest[n]
        ydir0 = R0.col[1].normalized()
        tgt = Vector(targets[n])
        rot = Matrix.Identity(3)
        nxt = next_of.get(n)
        if nxt is not None and nxt in rest and nxt in targets and n not in identity_rot:
            seg0 = rest[nxt][0] - head0
            seg1 = Vector(targets[nxt]) - tgt
            if seg0.length < 1e-6 or seg1.length < 1e-6:
                sy = s
            else:
                rot = seg0.normalized().rotation_difference(seg1.normalized()).to_matrix()
                sy = seg1.length / seg0.length
                ang = math.degrees(seg0.angle(ydir0))
                if ang > angle_warn_deg:
                    print('WARN conform: %s Y axis is %.1f deg off its child %s — stretch lands off-axis' % (n, ang, nxt))
        else:
            L = terminal_len.get(n)
            sy = (L / pb.bone.length) if L else s
            if n in y_scale:
                sy = y_scale[n]
        S = Matrix.Diagonal((s, sy, s, 1.0))
        new = Matrix.Translation(tgt) @ (rot @ R0).to_4x4() @ S
        pb.matrix = fa.matrix_world.inverted() @ new
        bpy.context.view_layer.update()
        got = fa.matrix_world @ pb.head
        report.append((n, tuple(round(x, 3) for x in got), round(sy / s, 3)))
        if (got - tgt).length > 1e-3:
            print('WARN conform miss', n, tuple(round(x, 4) for x in got), '->', tuple(round(x, 4) for x in tgt))
    print('conform (bone, head, stretch k relative to s):', report)
    return report


# ---------------------------------------------------------------------------------------------
# bake
# ---------------------------------------------------------------------------------------------
def bake_meshes(meshes, fa):
    """Freeze the posed deformation into each mesh (world space, evaluated normals kept as custom
    split normals), drop modifiers/parents/shape keys, delete the source armature."""
    for o in meshes:
        if o.data.shape_keys:
            bpy.context.view_layer.objects.active = o
            o.shape_key_clear()
    bpy.context.view_layer.update()
    dg = bpy.context.evaluated_depsgraph_get()
    baked = []
    for o in meshes:
        ev = o.evaluated_get(dg)
        me = ev.data
        n = len(o.data.vertices)
        if len(me.vertices) != n:
            raise RuntimeError('%s: evaluated vertex count %d != %d (a generative modifier?)' % (o.name, len(me.vertices), n))
        M = ev.matrix_world
        N = M.to_3x3().inverted().transposed()
        co = [0.0] * (3 * n)
        me.vertices.foreach_get('co', co)
        flat = []
        for i in range(n):
            p = M @ Vector(co[3 * i:3 * i + 3])
            flat.extend(p)
        normals = [(N @ cn.vector).normalized() for cn in me.corner_normals]
        baked.append((o, flat, normals))
    for o, flat, normals in baked:
        for m in list(o.modifiers):
            o.modifiers.remove(m)
        o.parent = None
        o.matrix_world = Matrix.Identity(4)
        o.data.vertices.foreach_set('co', flat)
        if len(normals) == len(o.data.loops):
            o.data.normals_split_custom_set(normals)
        o.data.update()
    if fa is not None:
        bpy.data.objects.remove(fa, do_unlink=True)
    bpy.context.view_layer.update()
    for o in meshes:
        xs = [v.co.x for v in o.data.vertices]
        ys = [v.co.y for v in o.data.vertices]
        zs = [v.co.z for v in o.data.vertices]
        if xs:
            print('baked %-28s x %.3f..%.3f y %.3f..%.3f z %.3f..%.3f' % (o.name, min(xs), max(xs), min(ys), max(ys), min(zs), max(zs)))


# ---------------------------------------------------------------------------------------------
# mesh surgery
# ---------------------------------------------------------------------------------------------
def delete_faces_by_material(o, predicate):
    """Delete every polygon whose material satisfies predicate(material) and compact the slots
    (material_index remapped by hand — `materials.clear()` zeroes every index)."""
    import bmesh
    me = o.data
    kill = {i for i, m in enumerate(me.materials) if predicate(m)}
    if not kill:
        return 0
    bm = bmesh.new()
    bm.from_mesh(me)
    faces = [f for f in bm.faces if f.material_index in kill]
    n = len(faces)
    bmesh.ops.delete(bm, geom=faces, context='FACES')
    bm.to_mesh(me)
    bm.free()
    keep = [i for i in range(len(me.materials)) if i not in kill]
    remap = {old: new for new, old in enumerate(keep)}
    mats = [me.materials[i] for i in keep]
    idx = [remap.get(p.material_index, 0) for p in me.polygons]
    me.materials.clear()
    for m in mats:
        me.materials.append(m)
    for p, i in zip(me.polygons, idx):
        p.material_index = i
    me.update()
    return n


def remove_loose_verts(o):
    import bmesh
    bm = bmesh.new()
    bm.from_mesh(o.data)
    loose = [v for v in bm.verts if not v.link_faces]
    n = len(loose)
    if loose:
        bmesh.ops.delete(bm, geom=loose, context='VERTS')
    bm.to_mesh(o.data)
    bm.free()
    return n


def keep_uv_layer(o, name=None):
    """Keep one UV layer (the named one, else the active/first) and make it active."""
    me = o.data
    if not me.uv_layers:
        return None
    keep = me.uv_layers.get(name) if name else None
    if keep is None:
        keep = me.uv_layers.active or me.uv_layers[0]
    for uv in list(me.uv_layers):
        if uv != keep:
            me.uv_layers.remove(uv)
    me.uv_layers.active = keep
    return keep


def white_color_attribute(o):
    """ONE colour attribute, opaque white: the game's `_vc` shaders multiply COLOR0 in and
    alpha-test the result (README COLOR0 rule)."""
    me = o.data
    for ca in list(me.color_attributes):
        me.color_attributes.remove(ca)
    col = me.color_attributes.new('Col', 'BYTE_COLOR', 'CORNER')
    col.data.foreach_set('color', [1.0] * (4 * len(col.data)))
    me.color_attributes.active_color = col
    me.color_attributes.active = col
    return col


# ---------------------------------------------------------------------------------------------
# weight retarget
# ---------------------------------------------------------------------------------------------
def blend_chain(p, pts, axis):
    """Distribute weight 1.0 over joints [(name, Vector)] ordered along `axis` by the projection
    of p: linear between neighbours, clamped at the ends."""
    vals = [(q[axis], nm) for nm, q in pts]
    sv = p[axis]
    asc = vals[0][0] <= vals[-1][0]
    if (asc and sv <= vals[0][0]) or (not asc and sv >= vals[0][0]):
        return {vals[0][1]: 1.0}
    if (asc and sv >= vals[-1][0]) or (not asc and sv <= vals[-1][0]):
        return {vals[-1][1]: 1.0}
    for (a, na), (b, nb) in zip(vals, vals[1:]):
        lo, hi = (a, b) if a <= b else (b, a)
        if lo <= sv <= hi and b != a:
            t = (sv - a) / (b - a)
            return {na: 1.0 - t, nb: t}
    return {vals[-1][1]: 1.0}


def ddr_weights(J, cls, side, p, skirt_leg_share=0.6):
    """Body class + side + vertex position (Blender metres) -> {DDR bone: weight}.
    Classes: hips spine neck head collar upperarm lowerarm hand thigh calf foot toe skirt."""
    S = SIDES.get(side)
    if cls == 'hips':
        return {'Hips': 1.0}
    if cls == 'spine':
        return blend_chain(p, [('Spine', J['Spine']), ('Spine1', J['Spine1']), ('Spine2', J['Spine2']), ('Neck', J['Neck'])], 2)
    if cls == 'neck':
        return blend_chain(p, [('Spine2', J['Spine2']), ('Neck', J['Neck']), ('Head', J['Head'])], 2)
    if cls == 'head':
        return {'Head': 1.0}
    if cls == 'skirt':
        # a skirt hangs from the pelvis: Hips above the hip joints, then a growing share moves to
        # the nearer leg so a lifted thigh carries the cloth in front of it instead of poking through
        z_hi = J['Hips'].z
        z_lo = J['LeftLeg'].z
        t = max(0.0, min(1.0, (z_hi - p.z) / (z_hi - z_lo))) if z_hi > z_lo else 0.0
        leg = t * skirt_leg_share
        w_half = max(0.05, abs(J['LeftUpLeg'].x))
        fl = max(0.0, min(1.0, 0.5 + p.x / (2.0 * w_half)))
        out = {'Hips': 1.0 - leg}
        if leg > 0:
            if fl > 0:
                out['LeftUpLeg'] = leg * fl
            if fl < 1:
                out['RightUpLeg'] = leg * (1.0 - fl)
        return out
    if S is None:
        return None
    if cls == 'collar':
        return {S + 'Collar': 1.0}
    if cls == 'upperarm':
        return blend_chain(p, [(S + 'Arm', J[S + 'Arm']), (S + 'ArmRoll', J[S + 'ArmRoll']), (S + 'ForeArm', J[S + 'ForeArm'])], 0)
    if cls == 'lowerarm':
        return blend_chain(p, [(S + 'ForeArm', J[S + 'ForeArm']), (S + 'ForeArmRoll', J[S + 'ForeArmRoll']), (S + 'Hand', J[S + 'Hand'])], 0)
    if cls == 'hand':
        return {S + 'Hand': 1.0}
    if cls == 'thigh':
        return blend_chain(p, [(S + 'UpLeg', J[S + 'UpLeg']), (S + 'UpLegRoll', J[S + 'UpLegRoll']), (S + 'Leg', J[S + 'Leg'])], 2)
    if cls == 'calf':
        return blend_chain(p, [(S + 'Leg', J[S + 'Leg']), (S + 'LegRoll', J[S + 'LegRoll']), (S + 'Foot', J[S + 'Foot'])], 2)
    if cls == 'foot':
        return {S + 'Foot': 1.0}
    if cls == 'toe':
        return blend_chain(p, [(S + 'ToeBase', J[S + 'ToeBase']), (S + 'Toe_end', J[S + 'Toe_end'])], 1)
    return None


def side_of_vertex(J, p):
    return 'l' if p.x >= 0 else 'r'


def retarget_weights(meshes, arm, J, order, classify, max_influences=4):
    """Replace every source vertex group by DDR bone groups: classify(group_name) -> (class, side)
    (side 'l'/'r'/None; a sided class with side None takes the vertex's own side), then
    ddr_weights() splits the class over its DDR bones by position. Parents the meshes to `arm`
    with an Armature modifier. Returns (unknown_groups_mass, ddr_mass)."""
    unknown = {}
    mass = {}
    for o in meshes:
        names = {vg.index: vg.name for vg in o.vertex_groups}
        classes = {i: classify(n) for i, n in names.items()}
        per_vertex = []
        for v in o.data.vertices:
            acc = {}
            for g in v.groups:
                if g.weight <= 0.0:
                    continue
                cls, side = classes[g.group]
                w = None
                if cls:
                    if side is None and cls in ('collar', 'upperarm', 'lowerarm', 'hand', 'thigh', 'calf', 'foot', 'toe'):
                        side = side_of_vertex(J, v.co)
                    w = ddr_weights(J, cls, side, v.co)
                if w is None:
                    unknown[names[g.group]] = unknown.get(names[g.group], 0.0) + g.weight
                    continue
                for nm, f in w.items():
                    acc[nm] = acc.get(nm, 0.0) + g.weight * f
            if not acc:
                nm = min(J, key=lambda k: (J[k] - v.co).length)
                acc = {nm: 1.0}
            top = sorted(acc.items(), key=lambda kv: -kv[1])[:max_influences]
            tot = sum(w for _, w in top)
            per_vertex.append([(nm, w / tot) for nm, w in top])
        for vg in list(o.vertex_groups):
            o.vertex_groups.remove(vg)
        groups = {n: o.vertex_groups.new(name=n) for n in order}
        for v, ws in zip(o.data.vertices, per_vertex):
            for nm, w in ws:
                groups[nm].add([v.index], w, 'REPLACE')
                mass[nm] = mass.get(nm, 0.0) + w
        o.parent = arm
        o.parent_type = 'OBJECT'
        o.matrix_parent_inverse = Matrix.Identity(4)
        mod = o.modifiers.new('Armature', 'ARMATURE')
        mod.object = arm
    print('unknown groups (weight mass dropped):', {k: round(v, 2) for k, v in sorted(unknown.items(), key=lambda kv: -kv[1])})
    print('DDR weight mass:', {k: round(v, 1) for k, v in sorted(mass.items(), key=lambda kv: -kv[1])})
    return unknown, mass


# ---------------------------------------------------------------------------------------------
# textures / materials
# ---------------------------------------------------------------------------------------------
def load_texture(stem, path, max_size=1024):
    """Load a PNG as a Blender image named `stem` (the DDS file name the game will look up:
    alnum + '_' only, <= 20 alphanumerics, unique after lower-casing and dropping '_'),
    downscaled to <= max_size (power-of-two sizes only)."""
    stem = re.sub(r'[^A-Za-z0-9_]', '', stem)
    assert len(stem.replace('_', '')) <= 20, stem
    img = bpy.data.images.get(stem)
    if img is None:
        img = bpy.data.images.load(path)
        img.name = stem
    w, h = img.size
    if w > max_size or h > max_size:
        f = max_size / max(w, h)
        img.scale(int(round(w * f)), int(round(h * f)))
    for d in img.size:
        assert d & (d - 1) == 0, 'texture %s size %s is not a power of two' % (stem, tuple(img.size))
    return img


def palette_texture(stem, colors_srgb, size=64):
    """A size x size image split into len(colors) vertical bands (sRGB 0..1 RGB tuples). Use
    palette_uv(i, n) for the band centre. For untextured flat-colour materials."""
    n = len(colors_srgb)
    img = bpy.data.images.new(stem, size, size, alpha=True)
    px = []
    for y in range(size):
        for x in range(size):
            c = colors_srgb[min(n - 1, x * n // size)]
            px.extend((c[0], c[1], c[2], 1.0))
    img.pixels.foreach_set(px)
    img.update()
    img.pack()   # generated pixels are lost on save/reload unless packed
    return img


def palette_uv(i, n):
    return ((i + 0.5) / n, 0.5)


def linear_to_srgb(c):
    out = []
    for v in c[:3]:
        out.append(12.92 * v if v <= 0.0031308 else 1.055 * (v ** (1 / 2.4)) - 0.055)
    return tuple(max(0.0, min(1.0, x)) for x in out)


def make_material(name, image, two_sided=False, shader=None):
    mat = bpy.data.materials.new(name)
    mat.use_nodes = True
    nt = mat.node_tree
    bsdf = nt.nodes.get('Principled BSDF')
    tex = nt.nodes.new('ShaderNodeTexImage')
    tex.image = image
    nt.links.new(tex.outputs['Color'], bsdf.inputs['Base Color'])
    bsdf.inputs['Roughness'].default_value = 1.0
    if 'Specular IOR Level' in bsdf.inputs:
        bsdf.inputs['Specular IOR Level'].default_value = 0.0
    mat.use_backface_culling = not two_sided
    mat.surface_render_method = 'DITHERED'   # opaque -> the game alpha-tests the texture alpha
    if shader:
        mat['ddr_shader'] = shader
    return mat


def set_face_uvs(o, material_index, uv):
    """Pin every loop of the polygons using `material_index` to one UV (palette swatch)."""
    me = o.data
    layer = me.uv_layers.active.data
    for p in me.polygons:
        if p.material_index == material_index:
            for li in p.loop_indices:
                layer[li].uv = uv


# ---------------------------------------------------------------------------------------------
# export + checks + previews
# ---------------------------------------------------------------------------------------------
def export_and_check(env, arm):
    key = env['KEY']
    bpy.context.view_layer.update()
    rep = export_character.export_character(env['EXPORT'], arm, key=key, write_textures=True, write_rlist=False)
    print('EXPORT', rep)
    body_dir = os.path.join(env['EXPORT'], 'pl_' + key)
    data = open(os.path.join(body_dir, 'pl_' + key + '.model'), 'rb').read()
    m = K.parse_model(data)
    print('MODEL bones', len(m['bones']), 'meshes', len(m['meshes']), 'palettes', [len(me['palette']) for me in m['meshes']],
          'palette_count', struct.unpack_from('<I', data, 0x20)[0], 'bytes', len(data))
    bad = 0
    n = 0
    for me in m['meshes']:
        for v in K.read_vertices(m, me):
            n += 1
            if abs(sum(v['WEIGHTS4']) - 1.0) > 2e-3:
                bad += 1
    print('VERTS', n, 'bad weight sums', bad)
    assert len(m['bones']) == 33
    assert bad == 0
    assert K.write_model(K.model_to_spec(m)) == data, 'codec round-trip mismatch'
    print('FILES', sorted(os.listdir(body_dir)))
    return rep


def write_sidecar_rlist_txt(env, sex='F', cls='A', model_scale=0.9, shadow_scale=0.75, unlock=0.0):
    """The modpack's text sidecar (data_mods/custom_models/dancers/<Name>/chara_resources.rlist.txt):
    one row `key, pl, sex, class, model_scale, shadow_scale, unlock`."""
    row = '%s, pl, %s, %s, %s, %s, %s\n' % (env['KEY'], sex, cls, export_character.fmt_num(model_scale),
                                             export_character.fmt_num(shadow_scale), export_character.fmt_num(unlock))
    path = os.path.join(env['EXPORT'], 'chara_resources.rlist.txt')
    open(path, 'w').write(row)
    print('SIDECAR', path, row.strip())
    return path


def studio(world_rgb=(0.63, 0.68, 0.72)):
    sc = bpy.context.scene
    sc.render.engine = 'BLENDER_WORKBENCH'
    sh = sc.display.shading
    sh.light = 'FLAT'
    sh.color_type = 'TEXTURE'
    sh.show_shadows = False
    sh.show_cavity = False
    sh.show_specular_highlight = False
    sh.show_object_outline = False
    sh.show_backface_culling = True
    sh.background_type = 'WORLD'
    sc.world = bpy.data.worlds.get('Neutral preview') or bpy.data.worlds.new('Neutral preview')
    sc.world.color = world_rgb
    sc.view_settings.view_transform = 'Standard'
    sc.view_settings.look = 'None'
    sc.render.resolution_percentage = 100
    sc.render.image_settings.file_format = 'PNG'
    sc.display.render_aa = '32'
    sc.render.film_transparent = False


def render_camera(path, pos, target, scale=2.4, res=(1000, 1200)):
    sc = bpy.context.scene
    cam = bpy.data.objects.get('Preview camera')
    if not cam:
        cam = bpy.data.objects.new('Preview camera', bpy.data.cameras.new('Preview camera'))
        sc.collection.objects.link(cam)
    cam.location = pos
    cam.rotation_euler = (Vector(target) - Vector(pos)).to_track_quat('-Z', 'Y').to_euler()
    cam.data.type = 'ORTHO'
    cam.data.ortho_scale = scale
    sc.camera = cam
    sc.render.resolution_x, sc.render.resolution_y = res
    sc.render.filepath = path
    bpy.ops.render.render(write_still=True)


def preview_renders(env, arm, clips=(), frames=(), label='preview', target_z=0.9, scale=2.4):
    """Workbench FLAT/TEXTURE renders (front + three-quarter rest pose, then dance frames of the
    given .anm clips) into OUT_DIR. Approximates the game's unlit look."""
    studio()
    out = env['OUT']
    t = Vector((0, 0, target_z))
    render_camera(os.path.join(out, label + '_front.png'), t + Vector((0, -6, 0)), t, scale)
    render_camera(os.path.join(out, label + '_threeq.png'), t + Vector((4.2, -4.2, 0.6)), t, scale)
    render_camera(os.path.join(out, label + '_back.png'), t + Vector((0, 6, 0)), t, scale)
    bpy.context.view_layer.objects.active = arm
    for clip in clips:
        action = import_anm.load_anm(clip, arm, frame_step=10)
        arm.data.pose_position = 'POSE'
        name = os.path.splitext(os.path.basename(clip))[0]
        for fr in frames:
            bpy.context.scene.frame_set(fr)
            bpy.context.view_layer.update()
            hips = arm.matrix_world @ arm.pose.bones['Hips'].matrix.translation
            tt = Vector((hips.x, hips.y, hips.z + 0.05))
            render_camera(os.path.join(out, '%s_%s_%04d.png' % (label, name, fr)), tt + Vector((3, -6, 1.0)), tt, scale + 0.3)
        bpy.data.actions.remove(action)
    arm.animation_data_clear()
    arm.data.pose_position = 'REST'
    for pb in arm.pose.bones:
        pb.matrix_basis = Matrix.Identity(4)
    arm.data.pose_position = 'POSE'
    bpy.context.scene.frame_set(1)
