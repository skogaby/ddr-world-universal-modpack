"""EXAMPLE: port an MMD character (mmd_tools FBX export, Japanese standard bone names) onto the DDR
33-bone dancer rig.

Written for the Project SEKAI Hatsune Miku port (2026-09-22); copy and adapt. Uses examples/port_lib.py.
Inputs (environment): SRC (the .fbx), DDR_3D_DATA, DDR_3D_RLIST, OUT_DIR, CHARA_KEY (default miku00),
DONOR (default pl_emi00 — female), TEX_DIR (the model's PNGs), PREVIEW_ANM (optional .anm),
PMX_SINGLE_SIDED (default 1: the PMX material flags said single-sided).
Run: /Applications/Blender.app/Contents/MacOS/Blender -b --python examples/port_character_mmd_fbx.py

Source specifics this handles:
  * MMD units (1 unit ~ 8 cm), T-pose, 531 bones incl. IK / _dummy_ / _shadow_ / collider helpers,
    twist bones IN the parent chain (腕 -> 腕捩 -> ひじ -> 手捩 -> 手首), skirt + hair physics chains
  * one mesh with 9 material slots and NO textures bound in the FBX: the material -> texture map
    comes from the PMX (dump it: work/pmx_dump.py) and is written into MAT_TEX below
  * 27 facial shape keys (cleared before the bake), 7 UV layers (only `UVMap` kept)
  * mmd_tools rigid-body / joint helper objects + a stray `batch_*` mesh (deleted)
"""
import os
import sys

import bpy
from mathutils import Vector

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import port_lib as P  # noqa: E402

env = P.read_env(default_key='miku00', default_donor='pl_emi00')
KEY = env['KEY']
TEX = env['TEX_DIR']
S = float(os.environ.get('PRESCALE', '0.08'))
SINGLE_SIDED = os.environ.get('PMX_SINGLE_SIDED', '1') == '1'

# PMX material name -> (texture stem for the game, PNG in TEX_DIR)
MAT_TEX = {
    'Acc': ('mk_acc', 'tex_acc_MIK_00_C.png'),
    'Eyes': ('mk_eye', 'tex_eye_MIK_00_C.png'),
    'HL': ('mk_ehl', 'tex_ehl_MIK_00_C.png'),       # eye highlight, alpha-cut in the game
    'Face': ('mk_chr', 'tex_chr_MIK_00_C.png'),
    'Eyelash': ('mk_chr', 'tex_chr_MIK_00_C.png'),
    'Eyebrow': ('mk_chr', 'tex_chr_MIK_00_C.png'),
    'Hair': ('mk_chr', 'tex_chr_MIK_00_C.png'),
    'Clothes': ('mk_bdy', 'tex_bdy_MIK_00_C.png'),
    'Skin': ('mk_bdy', 'tex_bdy_MIK_00_C.png'),
}

P.fresh_scene()
arm, J, order = P.load_ddr_rig(env)
fa, meshes, imported = P.import_source(env['SRC'], work_dir=env['OUT'])

# --- junk -----------------------------------------------------------------------------------------------
meshes = [o for o in meshes if o.parent == fa and o.vertex_groups]
junk = [o for o in imported if o.type == 'MESH' and o not in meshes]                       # rigid-body proxies, batch_*
junk += [o for o in imported if o.type == 'EMPTY' and o != fa.parent and
         (o.name in ('rigidbodies', 'joints') or (o.parent is not None and o.parent.name in ('rigidbodies', 'joints')))]
P.delete_objects(junk)
assert len(meshes) == 1, [o.name for o in meshes]
body = meshes[0]

# --- conform targets --------------------------------------------------------------------------------------
rh = lambda n: P.rest_head(fa, n)  # noqa: E731
LEG_Z = rh('足.L').z            # hip joints
NECK_Z = rh('首').z
LEG_Y = rh('足.L').y
kz = (J['Neck'].z - J['LeftUpLeg'].z) / (NECK_Z - LEG_Z)   # torso metres per source unit (z)


def torso_map(p):
    """Linear z-map of the trunk: leg joints -> UpLeg level, neck -> Neck; x/y pre-scaled."""
    return Vector((p.x * S, J['LeftUpLeg'].y + (p.y - LEG_Y) * S, J['LeftUpLeg'].z + (p.z - LEG_Z) * kz))


targets = {}
next_of = {}
terminal = {}
y_scale = {}
for n in ('下半身', '腰'):        # pelvis pivots (no DDR joint): identity rotation, torso z-stretch
    targets[n] = torso_map(rh(n))
    y_scale[n] = kz
for n in ('上半身', '上半身1', '上半身2'):
    targets[n] = torso_map(rh(n))
next_of.update({'上半身': '上半身1', '上半身1': '上半身2', '上半身2': '首', '首': '頭'})
targets['首'] = J['Neck']
targets['頭'] = J['Head']           # terminal: head keeps its size (scale S)
for sd, Sd in (('L', 'Left'), ('R', 'Right')):
    targets['肩.' + sd] = J[Sd + 'Collar']
    next_of['肩.' + sd] = '腕.' + sd
    # arm chain with the twist bones inside it: place them by arc-length on Arm -> ForeArm -> Hand
    chain = ['腕.' + sd, '腕捩.' + sd, 'ひじ.' + sd, '手捩.' + sd, '手首.' + sd]
    targets.update(P.map_chain_arclength(fa, chain, [J[Sd + 'Arm'], J[Sd + 'ForeArm'], J[Sd + 'Hand']]))
    # pin the real joints exactly (arc-length puts them within float noise anyway)
    targets['腕.' + sd] = J[Sd + 'Arm']
    targets['ひじ.' + sd] = J[Sd + 'ForeArm']
    targets['手首.' + sd] = J[Sd + 'Hand']
    for a, b in zip(chain, chain[1:]):
        next_of[a] = b
    targets['足.' + sd] = J[Sd + 'UpLeg']
    next_of['足.' + sd] = 'ひざ.' + sd
    targets['ひざ.' + sd] = J[Sd + 'Leg']
    next_of['ひざ.' + sd] = '足首.' + sd
    targets['足首.' + sd] = J[Sd + 'Foot']
    next_of['足首.' + sd] = '足先EX.' + sd
    targets['足先EX.' + sd] = J[Sd + 'ToeBase']   # terminal, keep the shoe tip's size

P.conform(fa, S, targets, next_of, terminal, y_scale)
P.bake_meshes(meshes, fa)


# --- weights ---------------------------------------------------------------------------------------------------
def classify(g):
    side = None
    if g.endswith('.L') or g.startswith('Left_') or '.L_' in g:
        side = 'l'
    elif g.endswith('.R') or g.startswith('Right_') or '.R_' in g:
        side = 'r'
    base = g.replace('.L', '').replace('.R', '')
    if base.startswith(('_dummy_', '_shadow_')):
        base = base.split('_', 2)[2]
    if base in ('頭', '目', '両目', 'メガネ', 'Acc', 'face', 'face (merge)', 'BS', 'eye', 'eyeblow', 'look', 'mouth') \
            or base.startswith(('a0', 'a1')) or 'hair' in base.lower():
        return 'head', None
    if base == '首':
        return 'neck', None
    if base in ('上半身', '上半身1', '上半身2') or base.startswith('EX_Center_acc') or 'Bust' in base or 'Pectoralis' in base:
        return 'spine', None
    if base in ('下半身', '腰', 'センター', 'グルーブ', '全ての親', '全ての親2', '操作中心', '腰キャンセル'):
        return 'hips', None
    if 'skirt' in base.lower():
        return 'skirt', None
    if base in ('肩', '肩P', '肩C'):
        return 'collar', side
    if base in ('腕', '腕捩', '腕捩1', '腕捩2', '腕捩3'):
        return 'upperarm', side
    if base in ('ひじ', '手捩', '手捩1', '手捩2', '手捩3') or 'EllbowSupport' in base:
        return 'lowerarm', side
    if base == '手首' or base == 'ダミー' or base[:2] in ('人指', '中指', '薬指', '小指', '親指') or base == '_Magic Bone':
        return 'hand', side
    if base in ('足', '足D'):
        return 'thigh', side
    if base in ('ひざ', 'ひざD'):
        return 'calf', side
    if base in ('足首', '足首D') or base.startswith('足首_'):
        return 'foot', side
    if base == '足先EX':
        return 'toe', side
    return None, side   # IK targets, colliders


P.retarget_weights(meshes, arm, J, order, classify)

# --- materials / textures ---------------------------------------------------------------------------------------
P.keep_uv_layer(body, 'UVMap')
P.white_color_attribute(body)
mats = {}
new_mats = []
for m in body.data.materials:
    stem, png = MAT_TEX[m.name]
    if stem not in mats:
        mats[stem] = P.make_material(KEY + '_' + stem, P.load_texture(stem, os.path.join(TEX, png)), two_sided=not SINGLE_SIDED)
    new_mats.append(mats[stem])
idx = [p.material_index for p in body.data.polygons]
body.data.materials.clear()
for m in new_mats:
    body.data.materials.append(m)
for p, i in zip(body.data.polygons, idx):
    p.material_index = i
body.name = KEY + '_body'
body.data.name = body.name
print('materials', [m.name for m in body.data.materials], 'polys', len(body.data.polygons))

bpy.context.view_layer.update()
bpy.ops.wm.save_as_mainfile(filepath=os.path.join(env['OUT'], KEY + '.blend'))

# --- export ------------------------------------------------------------------------------------------------------
P.export_and_check(env, arm)
P.write_sidecar_rlist_txt(env, sex='F', cls='A', model_scale=float(os.environ.get('MODEL_SCALE', '0.9')), shadow_scale=0.75)
clips = [c for c in os.environ.get('PREVIEW_ANM', '').split(os.pathsep) if c]
P.preview_renders(env, arm, clips=clips, frames=(100, 500, 900, 1400), label=KEY)
print('PORT COMPLETE', KEY)
