"""EXAMPLE: port a rigidly skinned game rip saved as ASCII FBX 6.1 (GTA-style skeleton) onto the DDR
33-bone MALE dancer rig (donor pl_rage00).

Written for the GTA San Andreas Carl Johnson port (2026-09-26; ships as data_mods/custom_models/dancers/
Carl Johnson); copy and adapt. Uses examples/port_lib.py + fbx6_gta_source.py (+ fbx6_ascii.py).
Inputs (environment): SRC (the .FBX), DDR_3D_DATA, DDR_3D_RLIST, OUT_DIR, CHARA_KEY (default cj00),
DONOR (default pl_rage00 — male), TEX_DIR (the model's 4 PNGs), PREVIEW_ANM (optional .anm list,
os.pathsep-separated), PRESCALE (default 0.0229 m per inch), NECK_K, WAIST_BLEND, SOLE_Z, MODEL_SCALE,
COLLAR_RELAX / COLLAR_SMOOTH (0 disables the rigid-skin repairs, for comparison).
Run: /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup --python examples/port_character_gta_fbx6.py

Source specifics this handles:
  * ASCII FBX (Blender cannot import it): fbx6_gta_source.build() reads it with fbx6_ascii.py and builds
    the armature from the skin clusters' TransformLink matrices + the mesh with UVs / normals / materials
  * 3ds Max inches, Z-up, facing -Y, T-pose (arms ~3 deg below horizontal), 58 bones, one mesh
  * 30 heel vertices skinned to 'root ground' (the rig's floor root): moved to the nearer ankle
    before the conform so they follow the shoes
  * the pelvis sits 5 in above the hip joints (DDR: 6 cm) -> the trunk takes a linear z-map from the
    hip joints to the neck, like the MMD port; the thighs keep CJ's own socket width and the belt band
    is re-baked with the trunk map (the DDR hip joints are 1.9 cm/side wider: a lip at the waist)
  * a short neck: stretched by NECK_K only, the rigid head rides a little below the DDR Head joint
  * unused 'eyes' material slot dropped (the eyes are in the head texture)
  * rigid skinning (every vertex 1.00 on one bone): the collar region is re-baked with a harmonic
    displacement field (no tears under the arms / at the strap tops) and the DDR weights are
    Laplacian-blended across every Collar border (no shoulder flaps when an arm lifts)
"""
import os
import sys

import bpy
from mathutils import Vector

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fbx6_gta_source  # noqa: E402
import port_lib as P  # noqa: E402

env = P.read_env(default_key='cj00', default_donor='pl_rage00')
KEY = env['KEY']
TEX = env['TEX_DIR']
S = float(os.environ.get('PRESCALE', '0.0229'))      # metres per source inch (head-joint height match)
SOLE_TARGET_Z = float(os.environ.get('SOLE_Z', '0.012'))  # stock rage00 soles sit at ~0.013 m

# source material -> (game texture stem, PNG in TEX_DIR)
MAT_TEX = {
    'body upper': ('cj_body', 'upper body.png'),
    'head': ('cj_head', 'head.png'),
    'legs': ('cj_legs', 'legs.png'),
    'shoes': ('cj_shoes', 'shoes.png'),
}

P.fresh_scene()
arm, J, order = P.load_ddr_rig(env)
fa, body, _fbx = fbx6_gta_source.build(env['SRC'], name=KEY)

# --- heel vertices on the floor root -> the nearer ankle ---------------------------------------------------
rg = body.vertex_groups.get('root ground')
if rg:
    moved = 0
    for v in body.data.vertices:
        w = next((g.weight for g in v.groups if g.group == rg.index), 0.0)
        if w > 0:
            side = 'left' if v.co.x >= 0 else 'right'
            body.vertex_groups['leg %s ankle' % side].add([v.index], w, 'ADD')
            moved += 1
    body.vertex_groups.remove(rg)
    print('root ground: %d heel vertices moved to the ankles' % moved)

# --- conform targets ----------------------------------------------------------------------------------------
rh = lambda n: P.rest_head(fa, n)  # noqa: E731
LEG_Z = rh('leg left thigh').z
LEG_Y = rh('leg left thigh').y
NECK_Z = rh('head neck lower').z
WAIST_TOP = rh('spine middle').z                        # trunk bones == trunk map up to here
kz = (J['Neck'].z - J['LeftUpLeg'].z) / (NECK_Z - LEG_Z)   # trunk metres per source inch (z)
print('prescale S %.5f  trunk kz %.5f (%.3f x S)' % (S, kz, kz / S))


def torso_map(p):
    return Vector((p.x * S, J['LeftUpLeg'].y + (p.y - LEG_Y) * S, J['LeftUpLeg'].z + (p.z - LEG_Z) * kz))


targets = {}
next_of = {}
terminal = {}
y_scale = {}
for n in ('root ground', 'root hips', 'pelvis'):      # weightless roots + the down-pointing pelvis
    targets[n] = torso_map(rh(n))
targets['root ground'] = Vector((0.0, 0.0, 0.0))
y_scale['pelvis'] = kz                                # pelvis Y axis = world -Z: trunk z-scale
for n in ('spine lower', 'spine middle', 'spine upper'):
    targets[n] = torso_map(rh(n))
next_of.update({'spine lower': 'spine middle', 'spine middle': 'spine upper', 'spine upper': 'head neck lower',
                'head neck lower': 'head neck upper'})
targets['head neck lower'] = J['Neck']
# CJ's neck (2.8 in) is ~0.65x the DDR Neck->Head segment at S: stretching it all the way to the Head
# joint gives a long thin neck. Stretch it by NECK_K only and let the head ride a little below the
# joint (the head is rigid on Head, so the offset is just a pivot a couple of cm above the skull base).
NECK_K = float(os.environ.get('NECK_K', '1.15'))
neck_len = (rh('head neck upper') - rh('head neck lower')).length * S * NECK_K
targets['head neck upper'] = J['Neck'] + (J['Head'] - J['Neck']).normalized() * min(neck_len, (J['Head'] - J['Neck']).length)
print('neck: %.3f m (DDR Neck->Head %.3f), head %.3f m below the Head joint' % (
    neck_len, (J['Head'] - J['Neck']).length, (J['Head'] - targets['head neck upper']).length))
for sd, Sd in (('left', 'Left'), ('right', 'Right')):
    a = 'arm %s ' % sd
    lg = 'leg %s ' % sd
    targets[a + 'shoulder 1'] = J[Sd + 'Collar']
    next_of[a + 'shoulder 1'] = a + 'shoulder 2'
    targets[a + 'shoulder 2'] = J[Sd + 'Arm']
    next_of[a + 'shoulder 2'] = a + 'elbow'
    targets[a + 'elbow'] = J[Sd + 'ForeArm']
    next_of[a + 'elbow'] = a + 'wrist'
    targets[a + 'wrist'] = J[Sd + 'Hand']              # terminal: hand + fingers keep their size
    # the DDR hip joints are 1.9 cm per side wider than CJ's at S: dragging the thighs out to them
    # widens the jeans at the waist (a lip over the tank top). Keep CJ's socket width — the DDR UpLeg
    # pivot then sits 2 cm outboard of the mesh socket, invisible for a rigid limb.
    targets[lg + 'thigh'] = torso_map(rh(lg + 'thigh'))
    next_of[lg + 'thigh'] = lg + 'knee'
    targets[lg + 'knee'] = J[Sd + 'Leg']
    next_of[lg + 'knee'] = lg + 'ankle'
    targets[lg + 'ankle'] = J[Sd + 'Foot']
    next_of[lg + 'ankle'] = lg + 'toes'
    targets[lg + 'toes'] = J[Sd + 'ToeBase']           # terminal: the shoe tip keeps its size

SRC_CO = [v.co.copy() for v in body.data.vertices]      # source rest positions (inches)
P.conform(fa, S, targets, next_of, terminal, y_scale)
P.bake_meshes([body], fa)


def waist_report(tag):
    """Front width (x extent) of the source (x S) vs the bake in source-z slices around the waist."""
    bco = [v.co for v in body.data.vertices]
    rows = []
    for z0 in (34, 36, 38, 40, 41, 42, 43, 44, 45, 46, 48):
        sel = [i for i, p in enumerate(SRC_CO) if z0 - 0.5 <= p.z < z0 + 0.5]
        if not sel:
            continue
        sw = (max(SRC_CO[i].x for i in sel) - min(SRC_CO[i].x for i in sel)) * S
        bw = max(bco[i].x for i in sel) - min(bco[i].x for i in sel)
        bz = sum(bco[i].z for i in sel) / len(sel)
        rows.append('z%-3d src %.3f baked %.3f (x%.2f) at %.3f m' % (z0, sw, bw, bw / sw, bz))
    print('WAIST %s:\n  ' % tag + '\n  '.join(rows))


waist_report('plain bake')

# --- waist band: the belt / boxer band / hip region is skinned partly to the thighs, whose conform
# (rotation about the hip + the 1.19x thigh stretch) swings points ABOVE the hip joint outward and up,
# while the trunk above them follows the linear trunk map. Bake the band with the trunk map instead,
# ramping back to the limb bake over WAIST_BLEND inches below the hip joint (weights are untouched:
# the band still follows the thighs when the dancer moves). -------------------------------------------
WAIST_BLEND = float(os.environ.get('WAIST_BLEND', '3.0'))


def waist_w(z):
    if z <= LEG_Z - WAIST_BLEND or z >= WAIST_TOP:
        return 0.0
    if z < LEG_Z:
        t = (z - (LEG_Z - WAIST_BLEND)) / WAIST_BLEND
    elif z > WAIST_TOP - 2.0:
        t = (WAIST_TOP - z) / 2.0
    else:
        return 1.0
    return t * t * (3.0 - 2.0 * t)                    # smoothstep


n_w = 0
for v, p in zip(body.data.vertices, SRC_CO):
    w = waist_w(p.z)
    if w > 0.0:
        v.co = v.co.lerp(torso_map(p), w)
        n_w += 1
body.data.update()
print('waist band: %d vertices re-baked with the trunk map' % n_w)
waist_report('waist fix')

# --- seam diagnostics: the rip is rigidly skinned (every vertex 1.00 on one bone), so wherever two
# neighbouring bones get different conform transforms the bake tears along the weight border. D = bake
# minus the trunk map; report the mesh edges (split vertices welded) with the largest D jump ----------
GRAPH = P.weld_graph(body, key_co=SRC_CO)             # welded on the source rest positions


def seam_report(tag, zmin=44.0, top=12):
    node, nbr, members = GRAPH
    vs = body.data.vertices
    D = [vs[ms[0]].co - torso_map(SRC_CO[ms[0]]) for ms in members]
    rows = []
    for a in range(len(nbr)):
        for b in nbr[a]:
            ia, ib = members[a][0], members[b][0]
            if b <= a or min(SRC_CO[ia].z, SRC_CO[ib].z) < zmin:
                continue
            jump = (D[a] - D[b]).length
            rows.append((jump, jump / max((SRC_CO[ia] - SRC_CO[ib]).length * S, 1e-6), ia, ib))
    rows.sort(key=lambda r: -r[0])
    print('SEAMS %s (largest D jumps across an edge):' % tag)
    for jump, rel, ia, ib in rows[:top]:
        pa, pb = SRC_CO[ia], SRC_CO[ib]
        print('  %.4f m (%.2fx edge)  [%5.1f %5.1f %5.1f] %-22s | [%5.1f %5.1f %5.1f] %s' % (
            jump, rel, pa.x, pa.y, pa.z, P.dominant_group(body, vs[ia]), pb.x, pb.y, pb.z, P.dominant_group(body, vs[ib])))


seam_report('after waist fix')

# --- collar seams: the DDR Collar joint sits ~2 cm outboard and 2.5 cm higher than CJ's clavicle, and the
# rigid 'shoulder 1' region reaches from the strap tops down the shirt side to 6 in below it, so the
# plain bake lifts it 4-6 cm off its spine / neck neighbours (the tank-top tears under the arms and the
# steps at the strap tops). Re-bake the collar vertices with a harmonic displacement field: D (bake -
# trunk map) is held on every other vertex and relaxed over the collar region, so it blends smoothly
# from the trunk (D ~ 0) and neck into the upper arm. Positions only; weights are untouched. ----------
SEAM_FREE = ('arm left shoulder 1', 'arm right shoulder 1')
if os.environ.get('COLLAR_RELAX', '1') != '0':
    _node, _nbr, _members = GRAPH
    free = [n for n, ms in enumerate(_members) if _nbr[n] and P.dominant_group(body, body.data.vertices[ms[0]]) in SEAM_FREE]
    moved = P.relax_displacement(body, GRAPH, lambda i: torso_map(SRC_CO[i]), free)
    print('collar relax: %d welded vertices in %s, max move %.4f m' % (len(free), SEAM_FREE, moved))
    seam_report('after collar relax')

# --- shoes: the DDR ankle sits higher above the floor than CJ's (0.113 m vs 3.8 in * S): stretch the
# part of the shoe below the ankle vertically so the soles land where the stock soles do -----------------
ankle_z = J['LeftFoot'].z
me = body.data
zs = [v.co.z for v in me.vertices]
sole = min(zs)
print('sole z after bake %.4f (target %.4f), ankle %.4f' % (sole, SOLE_TARGET_Z, ankle_z))
if sole > SOLE_TARGET_Z + 0.002:
    f = (ankle_z - SOLE_TARGET_Z) / (ankle_z - sole)
    co = [0.0] * (3 * len(me.vertices))
    me.vertices.foreach_get('co', co)
    for i in range(len(me.vertices)):
        z = co[3 * i + 2]
        if z < ankle_z:
            co[3 * i + 2] = ankle_z - (ankle_z - z) * f
    me.vertices.foreach_set('co', co)
    me.update()
    print('shoes: below-ankle z stretch x%.3f -> sole %.4f' % (f, min(v.co.z for v in me.vertices)))


# --- weights ---------------------------------------------------------------------------------------------------
def classify(g):
    side = 'l' if ' left ' in ' %s ' % g else ('r' if ' right ' in ' %s ' % g else None)
    if g in ('root ground', 'root hips', 'pelvis'):
        return ('hips', None) if g != 'root ground' else ('foot', None)
    if g.startswith('spine '):
        return 'spine', None
    if g == 'head neck lower':
        return 'neck', None
    if g.startswith('head '):
        return 'head', None
    if g.endswith('shoulder 1'):
        return 'collar', side
    if g.endswith('shoulder 2'):
        return 'upperarm', side
    if g.endswith('elbow'):
        return 'lowerarm', side
    if g.endswith('wrist') or ' finger ' in g:
        return 'hand', side
    if g.endswith('thigh'):
        return 'thigh', side
    if g.endswith('knee'):
        return 'calf', side
    if g.endswith('ankle'):
        return 'foot', side
    if g.endswith('toes'):
        return 'toe', side
    return None, side


P.retarget_weights([body], arm, J, order, classify)


# --- shoulder weight blend: the retarget keeps the rip's rigid borders — a hard Collar 1.0 | Arm 1.0 edge
# right at the DDR shoulder pivot and a hard Collar | Spine edge down the tank-top sides. Posed, those
# edges stretch 3-5x (a pointy flap on a raised arm). Laplacian-blend the DDR weights across every
# border that involves a Collar (COLLAR_RINGS rings, COLLAR_ITERS passes). ----------------------------
if os.environ.get('COLLAR_SMOOTH', '1') != '0':
    rings, iters = int(os.environ.get('COLLAR_RINGS', '2')), int(os.environ.get('COLLAR_ITERS', '3'))
    n = P.blend_weights_across(body, GRAPH, lambda a, b: 'Collar' in a + b, rings=rings, iters=iters)
    print('collar weight blend: %d welded vertices (%d rings, %d passes)' % (n, rings, iters))

# --- materials / textures ---------------------------------------------------------------------------------------
P.keep_uv_layer(body, 'UVMap')
P.white_color_attribute(body)
old = [m.name for m in me.materials]
idx = [p.material_index for p in me.polygons]
used = sorted(set(idx))
unused = [old[i] for i in range(len(old)) if i not in used]
print('material slots', old, 'used', used, 'dropped unused', unused)
new_mats = {}
slot_of = {}
me.materials.clear()
for i in used:
    stem, png = MAT_TEX[old[i]]
    if stem not in new_mats:
        new_mats[stem] = P.make_material(KEY + '_' + stem, P.load_texture(stem, os.path.join(TEX, png)))
        me.materials.append(new_mats[stem])
    slot_of[i] = list(me.materials).index(new_mats[stem])
for p, i in zip(me.polygons, idx):
    p.material_index = slot_of[i]
me.update()
body.name = KEY + '_body'
me.name = body.name
print('materials', [(m.name, tuple(m.node_tree.nodes['Image Texture'].image.size)) for m in me.materials], 'polys', len(me.polygons))

bpy.context.view_layer.update()
bpy.ops.wm.save_as_mainfile(filepath=os.path.join(env['OUT'], KEY + '.blend'))

# --- export ------------------------------------------------------------------------------------------------------
P.export_and_check(env, arm)
P.write_sidecar_rlist_txt(env, sex='M', cls='A', model_scale=float(os.environ.get('MODEL_SCALE', '1.0')), shadow_scale=0.8)
clips = [c for c in os.environ.get('PREVIEW_ANM', '').split(os.pathsep) if c]
P.preview_renders(env, arm, clips=clips, frames=(100, 500, 900, 1400), label=KEY)
# close-ups: a wrong UV layer / dropped texture shows here first (README "Previews")
for tag, c, sc in (('face', (0.0, 0.0, 1.52), 0.45), ('face34', (0.0, 0.0, 1.52), 0.45), ('hand', (0.56, 0.0, 1.32), 0.35),
                   ('feet', (0.0, 0.0, 0.1), 0.6)):
    c = Vector(c)
    off = Vector((2.1, -2.1, 0.2)) if tag == 'face34' else Vector((0.0, -3.0, 0.0))
    P.render_camera(os.path.join(env['OUT'], '%s_close_%s.png' % (KEY, tag)), c + off, c, sc, res=(800, 800))
print('PORT COMPLETE', KEY)
