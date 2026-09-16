"""EXAMPLE: port a rigged character from a UE5 (Fortnite-style) FBX onto the DDR 33-bone dancer rig.

Written for the Peter Griffin port (in-game verified on DDR A3, 2026-09-15); copy and adapt.
Inputs (environment): SRC_FBX (the FBX), DDR_3D_DATA (unpacked game data root with chara/),
DDR_3D_RLIST (chara_resources.rlist), OUT_DIR (work/output dir), CHARA_KEY (default peter00),
DONOR (stock body of the same sex, default pl_rage00), TEX_DIR (pre-downscaled PNGs, see TEX).
Run: /Applications/Blender.app/Contents/MacOS/Blender -b --python examples/port_character_ue_rig.py


Pipeline (all in one headless Blender run):
  1. import the FBX (372-bone UE rig, A-pose, 3 meshes: head / body / glasses+eyes+hair)
  2. POSE-CONFORM: pose the Fortnite rig so its main joints land exactly on the DDR joints
     (T-pose, Rage's bind offsets — the game forces those offsets at runtime anyway), each chain
     bone scaled along its axis so the mesh between two joints stretches to the DDR segment
     length; helper/twist bones inherit; then bake the deformation into the meshes
  3. import the stock DDR rig (Rage) via the add-on, drop Rage's meshes/parts, parent Peter
  4. retarget skin weights: Fortnite groups -> "classes" -> DDR bones, splitting each limb
     segment between its two DDR bones (Arm/ArmRoll, ForeArm/ForeArmRoll, ...) by the vertex's
     position along the segment (the stock rigs put roughly half the segment on the Roll bone)
  5. flat '_D' textures downscaled to <= 1024, one Image Texture per material, vertex colours
     dropped (the _vc shader would multiply them in)
  6. export_character(key='peter00') + codec sanity checks + preview renders
"""
import json
import math
import os
import re
import sys

import bpy
from mathutils import Matrix, Vector

SRC = os.environ['SRC_FBX']
OUT = os.environ['OUT_DIR']
EXPORT = os.path.join(OUT, 'export')
os.makedirs(EXPORT, exist_ok=True)
ADDON_DIR = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), '..'))
sys.path.insert(0, os.path.dirname(ADDON_DIR))
addon = __import__(os.path.basename(ADDON_DIR))
addon.register()
from blender_ddr_addon import import_character, export_character, import_anm  # noqa: E402
from blender_ddr_addon.codec import ktmdl as K  # noqa: E402

DATA = os.environ['DDR_3D_DATA']
RLIST = os.environ['DDR_3D_RLIST']
KEY = os.environ.get('CHARA_KEY', 'peter00')
DONOR = os.environ.get('DONOR', 'pl_rage00')       # stock body whose rig (and sex) we borrow
TEX_DIR = os.environ.get('TEX_DIR', os.path.join(OUT, 'tex'))
# source mesh-object name fragment -> image stem (a PNG <stem>.png in TEX_DIR, power-of-two, <= 1024)
TEX = {
    'material01': 'pg_head',
    'material02': 'pg_body',
    'griffin_1_0_0': 'pg_face',
}

# ---------------------------------------------------------------------------------------------
bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.import_scene.fbx(filepath=SRC)
fa = next(o for o in bpy.data.objects if o.type == 'ARMATURE')
meshes = [o for o in bpy.data.objects if o.type == 'MESH']
print('imported', fa.name, len(fa.data.bones), 'bones;', [(o.name, len(o.data.vertices)) for o in meshes])

# --- DDR joint targets (Blender Z-up) — the MALE rig (pl_rage00); read them from the donor if
# you port onto the female rig (pl_emi00: the same names, offsets differ by a few cm) -----------
J = {
    'Hips': (0.000, 0.000, 0.970), 'Spine': (0.000, 0.000, 0.970), 'Spine1': (0.000, -0.006, 1.155),
    'Spine2': (0.000, 0.000, 1.299), 'Neck': (0.000, 0.010, 1.389), 'Head': (0.000, -0.007, 1.486),
    'LeftCollar': (0.064, 0.020, 1.351), 'LeftArm': (0.157, 0.020, 1.331), 'LeftArmRoll': (0.272, 0.020, 1.331),
    'LeftForeArm': (0.364, 0.020, 1.331), 'LeftForeArmRoll': (0.459, 0.020, 1.331), 'LeftHand': (0.599, 0.020, 1.331),
    'LeftUpLeg': (0.098, 0.003, 0.912), 'LeftUpLegRoll': (0.093, 0.001, 0.719), 'LeftLeg': (0.087, 0.004, 0.536),
    'LeftLegRoll': (0.090, 0.014, 0.305), 'LeftFoot': (0.091, 0.014, 0.113), 'LeftToeBase': (0.092, -0.084, 0.043),
    'LeftToe_end': (0.091, -0.184, 0.037),
}
for k in list(J):
    if k.startswith('Left'):
        x, y, z = J[k]
        J['Right' + k[4:]] = (-x, y, z)
J = {k: Vector(v) for k, v in J.items()}
HEAD_TOP_TARGET_Z = 1.486 + 0.131  # keep the head bone length (Fortnite head len 0.131)


def bone_head(name):
    return fa.matrix_world @ fa.pose.bones[name].head


def bone_tail(name):
    return fa.matrix_world @ fa.pose.bones[name].tail


# spine: arc-length remap of the Fortnite chain onto the DDR polyline Hips->Spine1->Spine2->Neck->Head
spine_chain = ['pelvis', 'spine_01', 'spine_02', 'spine_03', 'spine_04', 'spine_05', 'neck_01', 'neck_02', 'head']
ddr_spine = [J['Hips'], J['Spine1'], J['Spine2'], J['Neck'], J['Head']]


def polyline_point(poly, t):
    segs = [(poly[i + 1] - poly[i]).length for i in range(len(poly) - 1)]
    total = sum(segs)
    d = t * total
    for i, s in enumerate(segs):
        if d <= s or i == len(segs) - 1:
            return poly[i] + (poly[i + 1] - poly[i]) * (d / s if s else 0.0)
        d -= s


fn_pos = [bone_head(n) for n in spine_chain]
fn_cum = [0.0]
for i in range(1, len(fn_pos)):
    fn_cum.append(fn_cum[-1] + (fn_pos[i] - fn_pos[i - 1]).length)
targets = {}
for n, c in zip(spine_chain, fn_cum):
    targets[n] = polyline_point(ddr_spine, c / fn_cum[-1])
targets['head'] = J['Head']
for side, S in (('l', 'Left'), ('r', 'Right')):
    targets['clavicle_' + side] = J[S + 'Collar']
    # the UE rig carries TWO arm chains: the control chain (upperarm/lowerarm/hand, parent of the
    # fingers + wrist helpers) and the deform chain (deform_*, carries most skin weights); the FBX
    # lost the constraints that tie them, so both get posed onto the DDR joints
    for pre in ('deform_', ''):
        targets[pre + 'upperarm_' + side] = J[S + 'Arm']
        targets[pre + 'lowerarm_' + side] = J[S + 'ForeArm']
        targets[pre + 'hand_' + side] = J[S + 'Hand']
    targets['thigh_' + side] = J[S + 'UpLeg']
    targets['calf_' + side] = J[S + 'Leg']
    targets['foot_' + side] = J[S + 'Foot']
    targets['ball_' + side] = J[S + 'ToeBase']
# the "next joint" that defines each chain bone's direction + stretch
next_of = {spine_chain[i]: spine_chain[i + 1] for i in range(len(spine_chain) - 1)}
for side in 'lr':
    next_of.update({'clavicle_' + side: 'deform_upperarm_' + side, 'thigh_' + side: 'calf_' + side,
                    'calf_' + side: 'foot_' + side, 'foot_' + side: 'ball_' + side})
    for pre in ('deform_', ''):
        next_of[pre + 'upperarm_' + side] = pre + 'lowerarm_' + side
        next_of[pre + 'lowerarm_' + side] = pre + 'hand_' + side
# terminal bones: keep their direction, stretch to a nominal length target
terminal_len = {'head': 0.131}
for side, S in (('l', 'Left'), ('r', 'Right')):
    terminal_len['ball_' + side] = (J[S + 'Toe_end'] - J[S + 'ToeBase']).length
    terminal_len['deform_hand_' + side] = None  # keep
    terminal_len['hand_' + side] = None

chain_bones = list(targets)
for n in chain_bones:
    fa.data.bones[n].inherit_scale = 'NONE'   # explicit placement below; helpers keep FULL inheritance

# hierarchy order
def depth(b):
    d = 0
    while b.parent:
        b = b.parent
        d += 1
    return d


order = sorted(chain_bones, key=lambda n: depth(fa.data.bones[n]))
report_conform = []
for n in order:
    pb = fa.pose.bones[n]
    bpy.context.view_layer.update()
    M = fa.matrix_world @ pb.matrix
    cur_head = M.to_translation()
    R3 = M.to_3x3().normalized()
    cur_dir = R3.col[1].normalized()
    tgt = targets[n]
    if n == 'pelvis':
        rot = Matrix.Identity(3)
        k = 1.0
    elif n in next_of:
        nxt = next_of[n]
        cur_len = (bone_head(nxt) - cur_head).length
        tgt_dir = targets[nxt] - tgt
        rot = cur_dir.rotation_difference(tgt_dir.normalized()).to_matrix()
        k = tgt_dir.length / cur_len if cur_len > 1e-6 else 1.0
        if n.startswith('neck'):
            k = 1.0   # keep the head/neck/chin at full size; only the spine absorbs the torso compression
    else:
        rot = Matrix.Identity(3)
        L = terminal_len.get(n)
        k = (L / pb.length) if L else 1.0
    new3 = rot @ R3
    new = Matrix.Translation(tgt) @ new3.to_4x4() @ Matrix.Diagonal((1.0, k, 1.0, 1.0))
    pb.matrix = fa.matrix_world.inverted() @ new
    bpy.context.view_layer.update()
    got = bone_head(n)
    report_conform.append((n, tuple(round(x, 3) for x in got), round(k, 3)))
    if (got - tgt).length > 1e-3:
        print('WARN conform miss', n, tuple(round(x, 4) for x in got), '->', tuple(round(x, 4) for x in tgt))
print('conform:', report_conform)

# --- bake the posed deformation into the meshes, drop the Fortnite rig ----------------------
dg = bpy.context.evaluated_depsgraph_get()
for o in meshes:
    ev = o.evaluated_get(dg)
    co = [0.0] * (len(o.data.vertices) * 3)
    ev.data.vertices.foreach_get('co', co)
    o.data.vertices.foreach_set('co', co)
    o.data.update()
    for m in list(o.modifiers):
        o.modifiers.remove(m)
    o.parent = None
    o.matrix_world = Matrix.Identity(4)
bpy.data.objects.remove(fa, do_unlink=True)
bpy.context.view_layer.update()
for o in meshes:
    zs = [v.co.z for v in o.data.vertices]
    xs = [v.co.x for v in o.data.vertices]
    print('baked', o.name, 'x %.3f..%.3f z %.3f..%.3f' % (min(xs), max(xs), min(zs), max(zs)))

# --- the DDR rig ------------------------------------------------------------------------------
arm, rage_body, rage_parts, info = import_character.load_character(
    os.path.join(DATA, 'chara', DONOR, DONOR + '.model'), import_textures=False, rlist_path=RLIST)
for o in rage_body + rage_parts:
    bpy.data.objects.remove(o, do_unlink=True)
assert len(arm.data.bones) == 33 and tuple(arm.scale) == (1.0, 1.0, 1.0)
arm.name = KEY + '_Armature'
arm['ddr_chara_key'] = KEY
order_ddr = list(arm['ddr_bone_order'])
for n, v in J.items():
    got = arm.data.bones[n].head_local
    assert (Vector(got) - v).length < 2e-3, (n, tuple(got), tuple(v))

# --- weight retarget ---------------------------------------------------------------------------
def blend_chain(p, pts, axis):
    """Distribute weight 1.0 over the joints `pts` [(name, Vector)] (ordered along `axis`) by the
    projection of p: linear between neighbours, clamped at the ends."""
    vals = [(q[axis], nm) for nm, q in pts]
    s = p[axis]
    if (vals[0][0] <= vals[-1][0] and s <= vals[0][0]) or (vals[0][0] > vals[-1][0] and s >= vals[0][0]):
        return {vals[0][1]: 1.0}
    if (vals[0][0] <= vals[-1][0] and s >= vals[-1][0]) or (vals[0][0] > vals[-1][0] and s <= vals[-1][0]):
        return {vals[-1][1]: 1.0}
    for (a, na), (b, nb) in zip(vals, vals[1:]):
        lo, hi = (a, b) if a <= b else (b, a)
        if lo <= s <= hi:
            t = (s - a) / (b - a)
            return {na: 1.0 - t, nb: t}
    return {vals[-1][1]: 1.0}


def classify(g):
    """Fortnite vertex-group name -> (class, side) ; side in ('l','r',None)."""
    side = None
    m = re.search(r'_(l|r)(?:_\d+)?$', g)
    if m:
        side = m.group(1)
    if g.startswith(('L_', 'R_')):
        side = g[0].lower()
    if g == 'pelvis':
        return 'hips', None
    if g.startswith('spine_') or g.startswith('deform_pec') or g.startswith('deform_neckline'):
        return 'spine', None
    if g.startswith('neck'):
        return 'neck', None
    if g == 'head' or g.startswith(('C_', 'L_', 'R_', 'teeth', 'tongue', 'deform_head', 'brow_glasses', 'faceAttach', 'jaw')):
        return 'head', None
    if g.startswith(('clavicle_', 'deform_clavicle_')):
        return 'collar', side
    if g.startswith(('deform_upperarm', 'deform_bicep', 'deform_elbow')):
        return 'upperarm', side
    if g.startswith(('deform_lowerarm', 'deform_wrist')):
        return 'lowerarm', side
    if g.startswith(('deform_hand', 'thumb', 'index', 'middle', 'pinky', 'ring', 'deform_middle', 'deform_index',
                     'deform_pinky', 'deform_ring', 'deform_thumb')):
        return 'hand', side
    if g.startswith(('deform_belt',)):
        return 'hips', None
    if g.startswith(('deform_lat',)):
        return 'spine', None
    if g.startswith(('deform_foot',)):
        return 'foot', side
    if g.startswith(('thigh', 'deform_thigh', 'deform_knee', 'deform_butt', 'deform_glute', 'deform_hip', 'deform_groin')):
        return 'thigh', side
    if g.startswith(('calf', 'deform_calf', 'deform_ankle', 'deform_shin')):
        return 'calf', side
    if g.startswith('foot'):
        return 'foot', side
    if g.startswith(('ball', 'toe')):
        return 'toe', side
    return None, side


def ddr_weights(cls, side, p):
    S = {'l': 'Left', 'r': 'Right'}.get(side)
    if cls == 'hips':
        return {'Hips': 1.0}
    if cls == 'spine':
        return blend_chain(p, [('Spine', J['Spine']), ('Spine1', J['Spine1']), ('Spine2', J['Spine2']), ('Neck', J['Neck'])], 2)
    if cls == 'neck':
        return blend_chain(p, [('Spine2', J['Spine2']), ('Neck', J['Neck']), ('Head', J['Head'])], 2)
    if cls == 'head':
        return {'Head': 1.0}
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


unknown = {}
mass = {}
for o in meshes:
    names = {vg.index: vg.name for vg in o.vertex_groups}
    classes = {i: classify(n) for i, n in names.items()}
    per_vertex = []
    for v in o.data.vertices:
        acc = {}
        total_in = 0.0
        for g in v.groups:
            if g.weight <= 0.0:
                continue
            cls, side = classes[g.group]
            w = ddr_weights(cls, side, v.co) if cls else None
            if w is None:
                unknown[names[g.group]] = unknown.get(names[g.group], 0.0) + g.weight
                continue
            total_in += g.weight
            for nm, f in w.items():
                acc[nm] = acc.get(nm, 0.0) + g.weight * f
        if not acc:
            # weightless / unknown-only vertex: nearest DDR joint
            nm = min(J, key=lambda k: (J[k] - v.co).length)
            acc = {nm: 1.0}
        top = sorted(acc.items(), key=lambda kv: -kv[1])[:4]
        s = sum(w for _, w in top)
        per_vertex.append([(nm, w / s) for nm, w in top])
    for vg in list(o.vertex_groups):
        o.vertex_groups.remove(vg)
    groups = {n: o.vertex_groups.new(name=n) for n in order_ddr}
    for v, ws in zip(o.data.vertices, per_vertex):
        for nm, w in ws:
            groups[nm].add([v.index], w, 'REPLACE')
            mass[nm] = mass.get(nm, 0.0) + w
    o.parent = arm
    o.parent_type = 'OBJECT'
    o.matrix_parent_inverse = Matrix.Identity(4)
    mod = o.modifiers.new('Armature', 'ARMATURE')
    mod.object = arm
    # the game's `_vc` shaders multiply COLOR0 in (and alpha-test the result): keep ONE colour
    # attribute and force it to opaque white -> stock layout A + mdl_ch_constant_vc, the combination
    # 203 stock materials use. (Dropping the attribute + a _vc shader = invisible mesh, boot #1.)
    for ca in list(o.data.color_attributes):
        o.data.color_attributes.remove(ca)
    col = o.data.color_attributes.new('Col', 'BYTE_COLOR', 'CORNER')
    col.data.foreach_set('color', [1.0] * (4 * len(col.data)))
print('unknown groups (weight mass dropped):', {k: round(v, 2) for k, v in sorted(unknown.items(), key=lambda kv: -kv[1])})
print('DDR weight mass:', {k: round(v, 1) for k, v in sorted(mass.items(), key=lambda kv: -kv[1])})

# --- materials / textures (pre-downscaled flat "_D" maps in work/tex, made with PIL) ------------
for o in meshes:
    frag = next(k for k in TEX if k in o.name)
    stem = TEX[frag]
    img = bpy.data.images.get(stem) or bpy.data.images.load(os.path.join(TEX_DIR, stem + '.png'))
    img.name = stem
    mat = bpy.data.materials.new('pg_' + frag)
    mat.use_nodes = True
    nt = mat.node_tree
    bsdf = nt.nodes.get('Principled BSDF')
    tex = nt.nodes.new('ShaderNodeTexImage')
    tex.image = img
    tex.interpolation = 'Closest'
    nt.links.new(tex.outputs['Color'], bsdf.inputs['Base Color'])
    bsdf.inputs['Roughness'].default_value = 1.0
    mat.use_backface_culling = True
    o.data.materials.clear()
    o.data.materials.append(mat)
    o.name = KEY + '_' + stem
    o.data.name = o.name
    print('material', o.name, '->', stem, tuple(img.size), 'uv', [u.name for u in o.data.uv_layers], 'polys', len(o.data.polygons))

bpy.context.view_layer.update()
bpy.ops.wm.save_as_mainfile(filepath=os.path.join(OUT, KEY + '.blend'))

# --- export -------------------------------------------------------------------------------------
rep = export_character.export_character(EXPORT, arm, key=KEY, write_textures=True, write_rlist=False)
print('EXPORT', rep)
data = open(os.path.join(EXPORT, 'pl_' + KEY, 'pl_' + KEY + '.model'), 'rb').read()
m = K.parse_model(data)
import struct
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
print('FILES', sorted(os.listdir(os.path.join(EXPORT, 'pl_' + KEY))))
assert K.write_model(K.model_to_spec(m)) == data
