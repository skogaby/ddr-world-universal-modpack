"""Preview the HOTTEST PARTY flight effects (boss_ddr3.TEB through scripts/teb_dump.simulate) on a
ported dancer's flight clip -- a check of the reference simulator, docs/wii_ddr_zan_effects_research.md.

  blender -b --factory-startup --python preview_flight_fx.py -- <mode 0|2> <out_dir> [frames...]

mode 0 = the flight effects (orb at the Hips, orbiting stars at the hands) on the dancer's first
flight clip, in the flight's scrolling world (teb_dump.FLIGHT_SCROLL: the rainbow trail); mode 2 = the leap burst from frame 544 of the take-off. Needs the MUSIC FIT dump
(MUSIC_FIT_DIR, default ~/Desktop/DDR Wii ISOs/Dance Dance Revolution - Music Fit (Japan)) and the
ported HOTTSTPARTY 1-3 hprena05."""
import sys, os, glob, math
argv = sys.argv[sys.argv.index('--') + 1:] if '--' in sys.argv else []
MODE = int(argv[0]) if argv else 0
OUT = argv[1] if len(argv) > 1 else os.path.join(__import__('tempfile').gettempdir(), 'flight_fx_preview')
SNAPS = [int(x) for x in argv[2:]] or [150, 200, 260]
REPO = os.path.abspath(os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', '..'))
sys.path.insert(0, os.path.join(REPO, 'scripts'))
sys.path.insert(0, os.path.join(REPO, 'tools/blender_ddr_addon/examples'))
os.environ.setdefault('GAME', 'hp3'); os.environ['STAGES'] = 'STG201'
import numpy as np
import bpy
from mathutils import Vector, Matrix
import port_stage_hottest2 as M
import port_lib as P
import teb_dump as T
import zan_dump as Z
from blender_ddr_addon import import_character, import_anm
os.makedirs(OUT, exist_ok=True)
S = Z.GAME_SCALE
C = np.array([[1, 0, 0], [0, 0, -1], [0, 1, 0]], float)     # zan (y up) -> Blender (z up)

MUSIC_FIT = os.path.expanduser(os.environ.get('MUSIC_FIT_DIR', '~/Desktop/DDR Wii ISOs/Dance Dance Revolution - Music Fit (Japan)'))
blob = open(os.path.join(MUSIC_FIT, 'game', 'GAME_CHR_EFF.bin'), 'rb').read()
(_p, teb, tpl), = T.teb_members(blob)
fx = T.parse_teb(teb)
imgs = Z.tpl_images(tpl)
dancer = glob.glob(os.path.join(REPO, 'data_mods/custom_models/dancers/HOTTSTPARTY 1-3/*/pl_hprena05/pl_hprena05.model'))[0]
clip = 'takeoff' if MODE == 2 else sorted(os.path.basename(f)[:-4] for f in glob.glob(os.path.join(os.path.dirname(dancer), 'motion/flight/fly_*.anm')))[0]

P.fresh_scene()
arm, *_ = import_character.load_character(dancer, import_textures=True)
import_anm.load_anm(os.path.join(os.path.dirname(dancer), 'motion/flight', clip + '.anm'), arm)
M.unlit_preview_materials(); M.eevee_scene()
sc = bpy.context.scene
names = {b.name.lower(): b.name for b in arm.pose.bones}
def bone(key):
    return next(v for k, v in names.items() if k.endswith(key.lower()))
JOINTS = {0: [bone('hips'), bone('lefthand'), bone('righthand')], 2: [bone('hips'), bone('leftfoot'), bone('rightfoot')]}[MODE]
EFFECTS = [8 * MODE + 0, 8 * MODE + 1, 8 * MODE + 1]          # player 1: FUN_8004b5a8's table
cam_pos_b = Vector((2.5, -6.0, 2.0)); cam_tgt_b = Vector((0.0, 0.0, 1.0))
cam_rot_b = (cam_tgt_b - cam_pos_b).to_track_quat('-Z', 'Y').to_matrix()
# zan-space camera: view rotation (world -> camera) and position
Rb = np.array(cam_rot_b)
view_rot = (C.T @ Rb).T            # rows = camera axes in zan space
cam_pos = C.T @ np.array(cam_pos_b) / S
start = 544 if MODE == 2 else 0
N = max(SNAPS) + 1
attach = {j: [] for j in JOINTS}
for f in range(N):
    sc.frame_set(start + f)
    bpy.context.view_layer.update()
    for j in JOINTS:
        m = np.array(arm.matrix_world @ arm.pose.bones[j].matrix)
        a = np.eye(4)
        r = m[:3, :3] / np.linalg.norm(m[:3, :3], axis=0)
        if MODE == 2 and j == JOINTS[0]:
            r = np.eye(3)                                            # joint 5: position only
        else:
            # the bone's armature matrix is C . M (game frame, local axes untouched --
            # convert.rowmat_to_blender): the zan / World joint frame is C^T . m (until
            # 2026-10-05 this also post-multiplied by C, turning the joint's local axes)
            r = C.T @ r
        a[:3, :3] = r
        a[:3, 3] = C.T @ m[:3, 3] / S
        attach[j].append(a)
frames = {}
for ei, j in zip(EFFECTS, JOINTS):
    for k, sprites in enumerate(T.simulate(fx['effects'][ei], lambda i, j=j: attach[j][i], N,
                                           view_rot=lambda i: view_rot, cam_pos=lambda i: cam_pos, seed=ei * 7 + len(frames),
                                           scroll=T.FLIGHT_SCROLL if MODE != 2 else None)):
        if k in SNAPS:
            frames.setdefault(k, []).extend(sprites)
texs = {}
def tex(i):
    if i not in texs:
        im = bpy.data.images.new('fx%d' % i, imgs[i].shape[1], imgs[i].shape[0], alpha=True)
        im.pixels = (np.flipud(imgs[i]).astype(np.float32) / 255.0).ravel()
        texs[i] = im
    return texs[i]
def mat(i, rgba, key):
    m = bpy.data.materials.new(key); m.use_nodes = True; nt = m.node_tree
    for n in list(nt.nodes):
        nt.nodes.remove(n)
    out = nt.nodes.new('ShaderNodeOutputMaterial'); tx = nt.nodes.new('ShaderNodeTexImage'); tx.image = tex(i)
    em = nt.nodes.new('ShaderNodeEmission'); mul = nt.nodes.new('ShaderNodeMixRGB'); mul.blend_type = 'MULTIPLY'; mul.inputs[0].default_value = 1
    mul.inputs[2].default_value = (rgba[0] / 255, rgba[1] / 255, rgba[2] / 255, 1)
    nt.links.new(tx.outputs['Color'], mul.inputs[1])
    am = nt.nodes.new('ShaderNodeMath'); am.operation = 'MULTIPLY'; am.inputs[1].default_value = rgba[3] / 255.0
    nt.links.new(tx.outputs['Alpha'], am.inputs[0])
    sm = nt.nodes.new('ShaderNodeMixRGB'); sm.blend_type = 'MULTIPLY'; sm.inputs[0].default_value = 1
    nt.links.new(mul.outputs[0], sm.inputs[1]); nt.links.new(am.outputs[0], sm.inputs[2])
    nt.links.new(sm.outputs[0], em.inputs['Color'])
    tr = nt.nodes.new('ShaderNodeBsdfTransparent'); add = nt.nodes.new('ShaderNodeAddShader')
    nt.links.new(tr.outputs[0], add.inputs[0]); nt.links.new(em.outputs[0], add.inputs[1])
    nt.links.new(add.outputs[0], out.inputs['Surface'])
    m.surface_render_method = 'BLENDED'; m.use_backface_culling = False
    return m
def zb(p):
    return Vector(C @ np.asarray(p) * S)
for k in SNAPS:
    sc.frame_set(start + k)
    objs = []
    for si, s in enumerate(frames.get(k, [])):
        if s['tex'] >= 0 and s['rgba'][3] > 1:
            vs = [zb(s['centre'] + s['axes'] @ q) for q in T.QUAD]
            me = bpy.data.meshes.new('q'); me.from_pydata(vs, [], [(0, 1, 3, 2)])
            uvl = me.uv_layers.new(); order = [0, 1, 3, 2]
            for li, vi in enumerate(order):
                u, v = s['uv'][vi]; uvl.data[li].uv = (u, 1 - v)
            me.materials.append(mat(s['tex'], s['rgba'], 'm%d_%d' % (k, si)))
            ob = bpy.data.objects.new('q', me); sc.collection.objects.link(ob); objs.append(ob)
        rb = s.get('ribbon')
        if rb and len(rb['points']) > 1 and rb['rgba'][3] > 1 and rb['tex'] >= 0:
            vs, faces = [], []
            for pi, (l, r) in enumerate(rb['points']):
                vs += [zb(l), zb(r)]
                if pi:
                    faces.append((2 * pi - 2, 2 * pi - 1, 2 * pi + 1, 2 * pi))
            me = bpy.data.meshes.new('r'); me.from_pydata(vs, [], faces)
            uvl = me.uv_layers.new()
            for poly in me.polygons:
                for li in poly.loop_indices:
                    vi = me.loops[li].vertex_index; pi = vi // 2
                    uvl.data[li].uv = (float(vi % 2), 1 - rb['v'][pi])
            me.materials.append(mat(rb['tex'], rb['rgba'], 'r%d_%d' % (k, si)))
            ob = bpy.data.objects.new('r', me); sc.collection.objects.link(ob); objs.append(ob)
    M.render_persp(os.path.join(OUT, 'fx_mode%d_f%04d.png' % (MODE, k)), cam_pos_b, cam_tgt_b, lens=30)
    print('SNAP', k, len(frames.get(k, [])), 'sprites', flush=True)
    for ob in objs:
        bpy.data.objects.remove(ob)
