"""EXAMPLE: port a current-generation Fortnite rip (UE5 FBX, F_MED / M_MED body, toon materials,
an optional separate tail rig and an optional pickaxe / melee weapon FBX) onto the DDR 33-bone rig.

Written for the Ironmouse port (2026-10-04; ships as data_mods/custom_models/dancers/Custom/Ironmouse
without the weapon and Custom/Ironmouse 2 holding it); copy and adapt. Uses examples/port_lib.py.
The Peter Griffin port (port_character_ue_rig.py) is the same family of rig, ripped from an older
build; this one differs in the ways listed below.

Inputs (environment):
  SRC          the character FBX (its textures are looked up next to it, or in TEX_DIR)
  DDR_3D_DATA  unpacked game data root with chara/ (pl_emi00 / pl_rage00 + mc_* clips for previews)
  DDR_3D_RLIST chara_resources.rlist (from startup.arc)
  OUT_DIR      work / output dir (export/ inside it is the folder that ships)
  CHARA_KEY    default ironmouse00
  DONOR        default pl_emi00 (female; pl_rage00 for a M_MED body, and SEX=M)
  WEAPON       none (default) | hand (held in the right fist) | back (slung across the back)
  WEAPON_SRC   the weapon FBX (required unless WEAPON=none); its textures next to it
  WEAPON_SCALE default 1.0; WEAPON_TILT degrees the blade is tilted up from the fist (default 0)
  TAIL_ATTACH  "y,z" of the tail root in SOURCE metres (default 0.07,0.99 — just inside the lower back)
  TAIL_PITCH   comma list, degrees below horizontal of each tail segment, root first
  PRESCALE     default 1.0 (the FBX imports in metres once its 0.01 object scale is applied)
  MODEL_SCALE  sidecar model scale (default 0.9, the stock female value); SEX (default F)
  PREVIEW_ANM  optional .anm list (os.pathsep-separated) for the dance-frame renders
Run: /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
       --python tools/blender_ddr_addon/examples/port_character_fortnite.py

Source specifics this handles:
  * 304-bone UE5 rig in A-pose (arms ~45 deg down). The conform stretches along each bone's Y axis,
    and the raw UE orientation has Y ACROSS the limb (the stretch would thicken the arm instead of
    lengthening it); Blender's automatic bone orientation fixes most bones but aims thigh / spine_04
    at a helper child. port_lib.align_chain_bones re-aims every chain bone at its next_of child.
  * A-pose: the hands are terminal bones, so conform(follow_parent_rot=hands, toes) carries the
    forearm's swing into them; without it the hands stay bent 45 deg down at the wrist.
  * one arm chain (upperarm/lowerarm/hand + twist + deform_* helpers) — not Peter's twin
    control/deform chains.
  * Fortnite pelvis sits 1 cm above the thigh sockets, DDR's Hips 6 cm: the trunk takes a linear
    z-map from the hip joints to the neck (the CJ port's torso_map), thighs keep the source width.
  * 83 facial shape keys per head mesh (cleared; the basis is the neutral face).
  * a TAIL on its own root (C_Root_Main_Root_Jnt -> C_Tail_A_*), authored lying flat on the floor
    behind the feet: posed into a hanging curve from the lower back (TAIL_PITCH) and weighted
    rigidly to Hips.
  * toon textures: *_ColorL is the lit colour (used); _ColorS the shadow colour, _DFL/_DFLC an ink-
    line mask + its colour, _STT shading thresholds, _N normals, _FX an emissive mask (unused —
    the game's shaders are unlit). 2048^2 TGAs downscaled to 1024^2.
  * all the physics chains (dyn_hair / pigTail / skirt / jacket / bow / hood / belt / heart) are
    folded into their body class; head + hair rigid on Head from the jaw up (README "Keep the head
    rigid").
  * WEAPON=hand: the right-hand fingers are curled into a fist (FIST_CURL degrees per phalanx) and
    the weapon's handle origin is put in the fist, blade out of the thumb side, edge toward the
    knuckles, weighted 1.0 to RightHand (a body-mesh material slot: a forearm part would be
    mirrored onto the left arm by the game). WEAPON=back: across the back on Spine2.
"""
import math
import os
import sys

import bpy
from mathutils import Matrix, Vector

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import port_lib as P  # noqa: E402

env = P.read_env(default_key='ironmouse00', default_donor='pl_emi00')
KEY = env['KEY']
SRC = env['SRC']
TEX = os.environ.get('TEX_DIR') or os.path.dirname(SRC)
S = float(os.environ.get('PRESCALE', '1.0'))
WEAPON = os.environ.get('WEAPON', 'none')
WEAPON_SRC = os.environ.get('WEAPON_SRC', '')
WEAPON_SCALE = float(os.environ.get('WEAPON_SCALE', '1.0'))
WEAPON_TILT = math.radians(float(os.environ.get('WEAPON_TILT', '0')))
assert WEAPON in ('none', 'hand', 'back'), WEAPON
assert WEAPON == 'none' or os.path.isfile(WEAPON_SRC), 'WEAPON=%s needs WEAPON_SRC' % WEAPON

# source material -> (game texture stem, texture file next to the FBX). Stems: <= 20 alnum, unique
# after lower-casing and dropping '_' (README "Authoring a new asset").
MAT_TEX = {
    'MI_FeelUnionSki_Body': ('irm_body', 'T_FeelUnionSki_Body_ColorL.tga'),
    'MI_FeelUnionSki_FaceAcc': ('irm_hair', 'T_FeelUnionSki_FaceAcc_ColorL.tga'),
    'MI_FeelUnionMoo_Head': ('irm_head', 'T_FeelUnionMoo_Head_ColorL.tga'),
    'MI_Pickaxe_FeelUnionSki': ('irm_blade', 'T_FeelUnionSki_Pickaxe_CL.tga'),
}
TWO_SIDED = {'irm_body', 'irm_hair', 'irm_blade'}   # skirt frills, jacket lining, hair cards, wings
FIST_CURL = {'_01_': 70.0, '_02_': 85.0, '_03_': 55.0}

P.fresh_scene()
arm, J, order = P.load_ddr_rig(env)


def import_fbx(path):
    before = set(bpy.data.objects)
    bpy.ops.import_scene.fbx(filepath=path, use_anim=False)
    new = [o for o in bpy.data.objects if o not in before]
    a = next(o for o in new if o.type == 'ARMATURE')
    ms = [o for o in new if o.type == 'MESH']
    bpy.context.view_layer.update()
    print('imported', os.path.basename(path), a.name, len(a.data.bones), 'bones;', [(o.name, len(o.data.vertices)) for o in ms])
    return a, ms


fa, meshes = import_fbx(SRC)
# source rest positions (world metres) — the rigid-head pass below keys on the source height
SRC_CO = {o.name: [o.matrix_world @ v.co for v in o.data.vertices] for o in meshes}

# --- conform targets ---------------------------------------------------------------------------------
rh = lambda n: P.rest_head(fa, n)  # noqa: E731
THIGH = rh('thigh_l')
JAW_Z = rh('C_jaw').z
NECK = rh('neck_01')
kz = (J['Neck'].z - J['LeftUpLeg'].z) / (NECK.z - THIGH.z)
print('prescale S %.4f  trunk kz %.4f' % (S, kz))


def torso_map(p):
    """Linear trunk map: the thigh sockets land on the DDR UpLeg height, neck_01 on the DDR Neck."""
    return Vector((p.x * S, J['LeftUpLeg'].y + (p.y - THIGH.y) * S, J['LeftUpLeg'].z + (p.z - THIGH.z) * kz))


targets, next_of = {}, {}
spine = ['pelvis', 'spine_01', 'spine_02', 'spine_03', 'spine_04', 'spine_05']
for a_, b_ in zip(spine, spine[1:] + ['neck_01']):
    targets[a_] = torso_map(rh(a_))
    next_of[a_] = b_
targets['neck_01'] = J['Neck']
next_of['neck_01'] = 'neck_02'
f_neck = (rh('neck_02') - rh('neck_01')).length / ((rh('neck_02') - rh('neck_01')).length + (rh('head') - rh('neck_02')).length)
targets['neck_02'] = J['Neck'].lerp(J['Head'], f_neck)
next_of['neck_02'] = 'head'
targets['head'] = J['Head']                       # terminal: rest orientation, size S
for sd, Sd in (('l', 'Left'), ('r', 'Right')):
    targets['clavicle_' + sd] = J[Sd + 'Collar']
    next_of['clavicle_' + sd] = 'upperarm_' + sd
    targets['upperarm_' + sd] = J[Sd + 'Arm']
    next_of['upperarm_' + sd] = 'lowerarm_' + sd
    targets['lowerarm_' + sd] = J[Sd + 'ForeArm']
    next_of['lowerarm_' + sd] = 'hand_' + sd
    targets['hand_' + sd] = J[Sd + 'Hand']        # terminal: follows the forearm's swing
    targets['thigh_' + sd] = torso_map(rh('thigh_' + sd))   # source socket width (CJ waist rule)
    next_of['thigh_' + sd] = 'calf_' + sd
    targets['calf_' + sd] = J[Sd + 'Leg']
    next_of['calf_' + sd] = 'foot_' + sd
    targets['foot_' + sd] = J[Sd + 'Foot']
    next_of['foot_' + sd] = 'ball_' + sd
    targets['ball_' + sd] = J[Sd + 'ToeBase']     # terminal: follows the foot
follow = ('hand_l', 'hand_r', 'ball_l', 'ball_r')
P.align_chain_bones(fa, next_of)
P.conform(fa, S, targets, next_of, follow_parent_rot=follow)


# --- tail: authored flat on the floor along +Y (behind) from its own root; pose it into a curve ------
def set_world(pb, M):
    pb.matrix = fa.matrix_world.inverted() @ M
    bpy.context.view_layer.update()


def tail_index(n):
    k = n[len('C_Tail_A_'):-len('_Jnt')]
    return 0 if k == 'Base' else int(k)


# joint k sits at the rest offset k * 0.11 m along +Y; the mesh between joint k and k+1 is weighted
# to bone k, so bone k gets pitch[k] and joint k+1 = joint k + seg_k * (0, cos, -sin)(pitch[k])
tail = sorted((b.name for b in fa.data.bones if b.name.startswith('C_Tail_A_')), key=tail_index)
if tail:
    attach = [float(x) for x in os.environ.get('TAIL_ATTACH', '0.07,0.99').split(',')]
    pitch = [math.radians(float(x)) for x in os.environ.get('TAIL_PITCH', '62,55,45,32,18,4,-10,-25,-42,-58').split(',')]
    pitch += [pitch[-1]] * (len(tail) - len(pitch))
    R3w = fa.matrix_world.to_3x3()
    pos = [torso_map(Vector((0.0, attach[0], attach[1])))]
    for k in range(1, len(tail)):
        seg = (rh(tail[k]) - rh(tail[k - 1])).length
        pos.append(pos[-1] + Vector((0.0, math.cos(pitch[k - 1]), -math.sin(pitch[k - 1]))) * seg)
    chain = ([('C_Root_Main_Root_Jnt', 0)] if 'C_Root_Main_Root_Jnt' in fa.data.bones else []) + list(zip(tail, range(len(tail))))
    for n, k in chain:
        R0 = R3w @ fa.data.bones[n].matrix_local.to_3x3()
        set_world(fa.pose.bones[n], Matrix.Translation(pos[k]) @ (Matrix.Rotation(-pitch[k], 3, 'X') @ R0).to_4x4())
    print('tail: %d joints, root %s, tip joint %s' % (len(tail), tuple(round(c, 3) for c in pos[0]), tuple(round(c, 3) for c in pos[-1])))


# --- fist (WEAPON=hand) ----------------------------------------------------------------------------------
def fist(side='r'):
    """Curl index..pinky of one hand toward the palm (each phalanx by FIST_CURL degrees about the
    knuckle axis, in the bone's own rest frame so it composes with the conform)."""
    k = (rh('index_01_' + side) - rh('pinky_01_' + side)).normalized()        # knuckle line
    f = (rh('middle_01_' + side) - rh('hand_' + side)).normalized()          # finger direction
    n = k.cross(f).normalized()
    if (rh('thumb_03_' + side) - rh('index_01_' + side)).dot(n) < 0:        # palm side = thumb side
        n = -n
    axis_w = f.cross(n).normalized()                                          # rotates f toward n
    R3w = fa.matrix_world.to_3x3()
    for fin in ('index', 'middle', 'ring', 'pinky'):
        for tag, deg in FIST_CURL.items():
            nm = fin + tag + side
            b = fa.data.bones.get(nm)
            if b is None:
                continue
            R0 = (R3w @ b.matrix_local.to_3x3()).normalized()
            axis_l = (R0.inverted() @ axis_w).normalized()
            fa.pose.bones[nm].rotation_mode = 'QUATERNION'
            fa.pose.bones[nm].rotation_quaternion = Matrix.Rotation(math.radians(deg), 3, axis_l).to_quaternion()
    bpy.context.view_layer.update()


def posed(n):
    return fa.matrix_world @ fa.pose.bones[n].head


def frame(x, z, origin):
    x = (x - z * x.dot(z)).normalized()
    y = z.cross(x)
    M = Matrix.Identity(4)
    for i in range(3):
        M[i][0], M[i][1], M[i][2], M[i][3] = x[i], y[i], z[i], origin[i]
    return M


weapon_grip = None
if WEAPON == 'hand':
    fist('r')
    joints = [posed(f + t + 'r') for f in ('index', 'middle', 'ring', 'pinky') for t in ('_01_', '_02_', '_03_')]
    grip = sum(joints, Vector()) / len(joints)
    d = (posed('index_01_r') - posed('pinky_01_r')).normalized()            # blade out of the thumb side
    edge = (posed('middle_01_r') - posed('hand_r')).normalized()            # edge toward the knuckles
    if WEAPON_TILT:
        d = Matrix.Rotation(-WEAPON_TILT, 3, edge.cross(d).normalized()) @ d
    weapon_grip = frame(edge, d, grip)
    print('weapon grip (fist centre) %s, blade dir %s' % (tuple(round(c, 3) for c in grip), tuple(round(c, 3) for c in d)))
elif WEAPON == 'back':
    c = torso_map(Vector((0.0, 0.20, 1.22)))
    d = Vector((-0.55, 0.0, 0.835)).normalized()                           # handle low-left, blade up-right
    weapon_grip = frame(Vector((0.0, 0.0, 0.0)) + d.cross(Vector((0, 1, 0))), d, c - d * 0.36 * WEAPON_SCALE)

P.bake_meshes(meshes, fa)

weapon = []
if weapon_grip is not None:
    wa, weapon = import_fbx(WEAPON_SRC)
    wa.matrix_world = weapon_grip @ Matrix.Scale(WEAPON_SCALE, 4) @ wa.matrix_world
    bpy.context.view_layer.update()
    P.bake_meshes(weapon, wa)


# --- weights -----------------------------------------------------------------------------------------------
def classify(g):
    side = None
    if g.endswith(('_l', '_lf')) or '_l_' in g or '_lf_' in g:
        side = 'l'
    elif g.endswith(('_r', '_rt')) or '_r_' in g or '_rt_' in g:
        side = 'r'
    if g == 'pelvis' or g.startswith(('dyn_belt', 'C_Root_', 'C_Tail_')) or g == 'dyn_heart':
        return 'hips', None
    if g.startswith('dyn_skirt'):
        return 'skirt', None
    if g.startswith(('spine_', 'deform_pec', 'dyn_jacket', 'dyn_main_bow', 'dyn_bow', 'dyn_ribbon', 'dyn_chest', 'dyn_hood')):
        return 'spine', None
    if g.startswith(('neck_', 'dyn_necklace')):
        return 'neck', None
    if g == 'head' or g.startswith(('C_', 'L_', 'R_', 'teeth', 'tongue', 'faceAttach', 'dyn_hair', 'dyn_pigTail',
                                    'dyn_front_hair', 'dyn_heart_head', 'FX_', 'hat', 'earpiece')):
        return 'head', None
    if g.startswith('clavicle_'):
        return 'collar', side
    if g.startswith(('upperarm', 'deform_upperarm', 'deform_elbow')):
        return 'upperarm', side
    if g.startswith(('lowerarm', 'deform_wrist')):
        return 'lowerarm', side
    if g.startswith(('hand_', 'thumb_', 'index_', 'middle_', 'ring_', 'pinky_')):
        return 'hand', side
    if g.startswith(('thigh', 'deform_knee', 'deform_glute', 'deform_groin')):
        return 'thigh', side
    if g.startswith('calf'):
        return 'calf', side
    if g.startswith('foot_'):
        return 'foot', side
    if g.startswith('ball_'):
        return 'toe', side
    if g == 'Melee_TwoHanded_Handle':
        return ('hand', 'r') if WEAPON == 'hand' else ('spine', None)
    return None, side


P.retarget_weights(meshes + weapon, arm, J, order, classify)
if WEAPON == 'back':
    for o in weapon:      # rigid on Spine2 (the spine blend would bend the blade with the back)
        for vg in o.vertex_groups:
            vg.remove(range(len(o.data.vertices)))
        o.vertex_groups['Spine2'].add(range(len(o.data.vertices)), 1.0, 'REPLACE')


def rigid_head(objs, z_min):
    """README 'Keep the head rigid': every vertex that started above z_min (source metres, ~the jaw)
    and carries any Head weight goes 1.00 to Head, so a Head-vs-Neck difference never shears the face."""
    n = 0
    for o in objs:
        src = SRC_CO.get(o.name)
        if src is None:
            continue
        head = o.vertex_groups['Head']
        for v in o.data.vertices:
            if src[v.index].z < z_min:
                continue
            ws = {o.vertex_groups[g.group].name: g.weight for g in v.groups if g.weight > 0}
            if 'Head' in ws and ws['Head'] < 0.999:
                for g in list(v.groups):
                    o.vertex_groups[g.group].remove([v.index])
                head.add([v.index], 1.0, 'REPLACE')
                n += 1
    print('rigid head: %d vertices above z %.3f forced to Head 1.0' % (n, z_min))


rigid_head(meshes, JAW_Z - 0.045)   # the chin sits ~4 cm below the jaw pivot on this rig

# --- materials / textures --------------------------------------------------------------------------------------
mats = {}
for o in meshes + weapon:
    P.keep_uv_layer(o)
    P.white_color_attribute(o)
    src_mat = o.data.materials[0].name if o.data.materials else None
    base = src_mat.split('.')[0] if src_mat else None
    stem, fname = MAT_TEX[base]
    if stem not in mats:
        tex_dir = os.path.dirname(WEAPON_SRC) if o in weapon else TEX
        img = P.load_texture(stem, os.path.join(tex_dir, fname))
        mats[stem] = P.make_material(KEY + '_' + stem, img, two_sided=stem in TWO_SIDED)
    o.data.materials.clear()
    o.data.materials.append(mats[stem])
    o.name = KEY + '_' + stem + ('_tail' if 'Tail' in o.name else '')
    o.data.name = o.name
    print('material', o.name, stem, tuple(mats[stem].node_tree.nodes['Image Texture'].image.size), 'polys', len(o.data.polygons))

bpy.context.view_layer.update()
lo = min(v.co.z for o in meshes for v in o.data.vertices)
print('lowest vertex z %.4f (stock soles ~0.013)' % lo)
bpy.ops.wm.save_as_mainfile(filepath=os.path.join(env['OUT'], KEY + '.blend'))

# --- export --------------------------------------------------------------------------------------------------------
P.export_and_check(env, arm)
P.write_sidecar_rlist_txt(env, sex=os.environ.get('SEX', 'F'), cls='A',
                          model_scale=float(os.environ.get('MODEL_SCALE', '0.9')), shadow_scale=0.75)
clips = [c for c in os.environ.get('PREVIEW_ANM', '').split(os.pathsep) if c]
P.preview_renders(env, arm, clips=clips, frames=(200, 600, 900, 1400), label=KEY)
P.render_camera(os.path.join(env['OUT'], KEY + '_side.png'), Vector((6, 0, 0.9)), Vector((0, 0, 0.9)), 2.6)
for tag, c, off, sc in (('face', (0.0, 0.0, 1.50), (0.0, -3.0, 0.0), 0.5), ('face34', (0.0, 0.0, 1.50), (2.1, -2.1, 0.2), 0.5),
                        ('hand', (-0.58, 0.0, 1.32), (0.0, -3.0, 0.0), 0.45), ('handtop', (-0.58, 0.0, 1.32), (0.0, 0.0, 3.0), 0.45),
                        ('feet', (0.0, 0.0, 0.1), (0.0, -3.0, 0.0), 0.6)):
    c = Vector(c)
    P.render_camera(os.path.join(env['OUT'], '%s_close_%s.png' % (KEY, tag)), c + Vector(off), c, sc, res=(800, 800))
print('PORT COMPLETE', KEY)
