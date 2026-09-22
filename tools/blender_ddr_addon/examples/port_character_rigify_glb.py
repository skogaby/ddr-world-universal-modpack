"""EXAMPLE: port a Blender/Rigify-style rigged anime character (GLB) onto the DDR 33-bone dancer rig.

Written for the Kasane Teto port (2026-09-22); copy and adapt. Uses examples/port_lib.py.
Inputs (environment): SRC (the .glb), DDR_3D_DATA (unpacked game data root with chara/),
DDR_3D_RLIST (chara_resources.rlist), OUT_DIR, CHARA_KEY (default teto00), DONOR (default pl_emi00 —
a FEMALE stock body: the sex decides which dance loops + bind offsets the game uses),
TEX_DIR (the source PNGs), PREVIEW_ANM (optional .anm for the dance-frame renders).
Run: /Applications/Blender.app/Contents/MacOS/Blender -b --python examples/port_character_rigify_glb.py

Source specifics this handles (check yours with a quick inspection first):
  * Rigify-flavoured deform names: spine / chest / neck / head, shoulder.L, upper_arm.L, lower_arm.L,
    hand.L, upper_leg.L, lower_leg.L, foot.L, toes.L, hips (+ Chinese-named IK controls, physics
    chains for dress / hair / drills / ahoge, finger bones) — T-pose, ~4.5 units tall
  * inverted-hull outline shells on an untextured `Edge_Col` material (deleted — the modpack's
    Background Dancers mod draws its own outlines)
  * two untextured flat-colour hair materials -> a 2-band palette texture
  * KHR_materials_unlit everywhere (matches the game)
"""
import os
import sys

import bpy
from mathutils import Vector

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import port_lib as P  # noqa: E402

env = P.read_env(default_key='teto00', default_donor='pl_emi00')
KEY = env['KEY']
TEX = env['TEX_DIR']
S = float(os.environ.get('PRESCALE', '0.37'))   # source units -> metres before the per-segment conform

P.fresh_scene()
arm, J, order = P.load_ddr_rig(env)
fa, meshes, imported = P.import_source(env['SRC'], work_dir=env['OUT'])

# --- junk + outline shells ------------------------------------------------------------------------
meshes = [o for o in meshes if o.name != 'Icosphere' and o.parent == fa]
P.delete_objects([o for o in imported if o.type == 'MESH' and o not in meshes])
for o in meshes:
    n = P.delete_faces_by_material(o, lambda m: m is not None and m.name.startswith('Edge_Col'))
    if n:
        print('dropped %d outline-shell faces from %s' % (n, o.name))
    P.remove_loose_verts(o)

# --- conform targets --------------------------------------------------------------------------------
rh = lambda n: P.rest_head(fa, n)  # noqa: E731
src_hip_c = (rh('upper_leg.L') + rh('upper_leg.R')) / 2
ddr_hip_c = (J['LeftUpLeg'] + J['RightUpLeg']) / 2


def prescale(p):
    """Default target for a bone with no DDR joint of its own: uniform scale about the hip joints."""
    return ddr_hip_c + (Vector(p) - src_hip_c) * S


targets = {}
next_of = {}
terminal = {}
# torso: the source runs spine(head) -> spine.001/chest(head) -> chest tail = neck head; map by arc length
torso_poly = [J['Hips'], J['Spine1'], J['Spine2'], J['Neck']]
torso_pts = [rh('spine'), rh('spine.001'), rh('neck')]
cum = [0.0, (torso_pts[1] - torso_pts[0]).length]
cum.append(cum[1] + (torso_pts[2] - torso_pts[1]).length)
t_chest = cum[1] / cum[2]
targets['root'] = Vector((0.0, 0.0, 0.0))
targets['spine.001'] = P.polyline_point(torso_poly, t_chest)
targets['spine'] = J['Hips']
next_of['spine'] = 'spine.001'          # spine points UP from the hips to the chest pivot
targets['chest'] = targets['spine.001']
next_of['chest'] = 'neck'
targets['chest.001'] = J['Neck']
targets['neck'] = J['Neck']
next_of['neck'] = 'head'
targets['head'] = J['Head']             # terminal: keeps the head's size (scale S)
targets['胯'] = J['Hips']               # pelvis pivot (points down)
targets['hips'] = prescale(rh('hips'))  # pelvis bone below the hip joints, pointing up
for sd, Sd in (('L', 'Left'), ('R', 'Right')):
    targets['shoulder.' + sd] = J[Sd + 'Collar']
    next_of['shoulder.' + sd] = 'upper_arm.' + sd
    targets['upper_arm.' + sd] = J[Sd + 'Arm']
    next_of['upper_arm.' + sd] = 'lower_arm.' + sd
    targets['lower_arm.' + sd] = J[Sd + 'ForeArm']
    next_of['lower_arm.' + sd] = 'hand.' + sd
    targets['hand.' + sd] = J[Sd + 'Hand']                      # terminal, keep size
    targets['upper_leg.' + sd] = J[Sd + 'UpLeg']
    next_of['upper_leg.' + sd] = 'lower_leg.' + sd
    targets['lower_leg.' + sd] = J[Sd + 'Leg']
    next_of['lower_leg.' + sd] = 'foot.' + sd
    targets['foot.' + sd] = J[Sd + 'Foot']
    next_of['foot.' + sd] = 'toes.' + sd
    targets['toes.' + sd] = J[Sd + 'ToeBase']
    terminal['toes.' + sd] = (J[Sd + 'Toe_end'] - J[Sd + 'ToeBase']).length

P.conform(fa, S, targets, next_of, terminal)
P.bake_meshes(meshes, fa)


# --- weights -----------------------------------------------------------------------------------------
def classify(g):
    side = None
    if g.endswith('.L') or '.L.' in g:
        side = 'l'
    elif g.endswith('.R') or '.R.' in g:
        side = 'r'
    base = g.split('.')[0]
    if g == 'root' or base in ('hips', '胯'):
        return 'hips', None
    if base in ('spine', 'chest'):
        return 'spine', None
    if base == 'neck':
        return 'neck', None
    if base in ('head', 'eye', 'hair', 'Bone', '角', '呆毛') and not g.startswith('Bone.001.L.001') and not g.startswith('Bone.001.R.001'):
        return 'head', None
    if g in ('Bone.001.L.001', 'Bone.001.R.001'):   # shawl shoulder-pad helpers under shoulder.L/R
        return 'collar', side
    if base == 'shoulder':
        return 'collar', side
    if base == 'upper_arm':
        return 'upperarm', side
    if base == 'lower_arm':
        return 'lowerarm', side
    if base in ('hand', 'thumb_proximal', 'thumb_intermediate', 'thumb_distal', 'index_proximal', 'index_intermediate',
                'index_distal', 'middle_proximal', 'middle_intermediate', 'middle_distal', 'ring_proximal',
                'ring_intermediate', 'ring_distal', 'little_proximal', 'little_intermediate', 'little_distal'):
        return 'hand', side
    if base == 'upper_leg':
        return 'thigh', side
    if base == 'lower_leg':
        return 'calf', side
    if base == 'foot':
        return 'foot', side
    if base == 'toes':
        return 'toe', side
    if base == 'dress':
        return 'skirt', None
    return None, side   # IK controls (footcot, 手部控制器, 手肘*, 腿部*) carry no weight


P.retarget_weights(meshes, arm, J, order, classify)

# --- materials / textures ----------------------------------------------------------------------------
# glTF material name -> (image stem, PNG in TEX_DIR)   (stems: <= 20 alnum, unique sans '_')
IMG = {
    'dress': ('tt_dress', 'dress_0.png'),
    'cloth': ('tt_cloth', 'cloth_1.png'),
    'cloth.001': ('tt_cloth', 'cloth_1.png'),
    'skin': ('tt_skin', 'skin_2.png'),
    'face.001': ('tt_face', 'tttttty_3.png'),
}
# untextured hair colours (glTF baseColorFactor, linear) -> one palette texture
HAIR = [('Material', (0.5775790810585022, 0.001820709789171815, 0.038204092532396317)),
        ('Material.002', (0.04666468873620033, 0.0036764685064554214, 0.012983156368136406))]
hair_img = P.palette_texture('tt_hair', [P.linear_to_srgb(c) for _, c in HAIR])
mats = {}
for o in meshes:
    P.keep_uv_layer(o, 'UVMap')
    P.white_color_attribute(o)
    slots = [m.name if m else None for m in o.data.materials]
    new_mats = []
    for i, name in enumerate(slots):
        if name in IMG:
            stem, png = IMG[name]
            if stem not in mats:
                mats[stem] = P.make_material(KEY + '_' + stem, P.load_texture(stem, os.path.join(TEX, png)), two_sided=True)
            new_mats.append(mats[stem])
        elif name in dict(HAIR):
            if 'tt_hair' not in mats:
                mats['tt_hair'] = P.make_material(KEY + '_tt_hair', hair_img, two_sided=True)
            P.set_face_uvs(o, i, P.palette_uv([h for h, _ in HAIR].index(name), len(HAIR)))
            new_mats.append(mats['tt_hair'])
        else:
            raise RuntimeError('unmapped material %r on %s' % (name, o.name))
    idx = [p.material_index for p in o.data.polygons]
    o.data.materials.clear()
    for m in new_mats:
        o.data.materials.append(m)
    for p, i in zip(o.data.polygons, idx):
        p.material_index = i
    o.name = KEY + '_' + o.name.replace('.', '_')
    o.data.name = o.name
    print('material', o.name, [m.name for m in o.data.materials], 'polys', len(o.data.polygons))

bpy.context.view_layer.update()
bpy.ops.wm.save_as_mainfile(filepath=os.path.join(env['OUT'], KEY + '.blend'))

# --- export -------------------------------------------------------------------------------------------
P.export_and_check(env, arm)
P.write_sidecar_rlist_txt(env, sex='F', cls='A', model_scale=float(os.environ.get('MODEL_SCALE', '0.9')), shadow_scale=0.75)
clips = [c for c in os.environ.get('PREVIEW_ANM', '').split(os.pathsep) if c]
P.preview_renders(env, arm, clips=clips, frames=(200, 900, 1600), label=KEY)
print('PORT COMPLETE', KEY)
