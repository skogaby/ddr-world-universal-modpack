"""EXAMPLE / PORT: the DanceDanceRevolution FuruFuru Party (Wii, JP 2008 = HOTTEST PARTY 2) and
DanceDanceRevolution MUSIC FIT (Wii, JP 2009 = HOTTEST PARTY 3) 3D stages as Background Dancers
custom stages, fed by the zan decoder in scripts/zan_dump.py (formats + RE:
docs/wii_ddr_hottest_party_2_3_research.md). The part / loop / camera machinery is
port_stage_hottest.py's (HOTTEST PARTY 1); only the sources differ.

A stage (stage/STG<nnn>.bin) is a set of ZMB models, each with its own one-loop ZAB motion of node
SRT tracks: DRAW_* (the stage and set), BG_* (the backdrop), OBJ[AB]_[NZS]_<name>_* props placed by
the COL_* layout model's OBJSET_<name>_<nn> nodes (COL's own meshes are cull hulls; its EFF / LIG
nodes are effect and light spots), plus its camera shots. Per stage:
  1. every (model, instance) becomes an entry; a mesh node's vertices (node-local) are baked
     through its frame-0 world (node world under its motion x the instance world under COL's
     motion) into game metres (zan_dump.GAME_SCALE, the dancers' scale); the material picks the
     World blend group (zan_dump.material_mode: additive -> `add`, ZERO+INVSRCALPHA -> `sub`, a
     soft alpha blend with real partial alpha -> `ble`, else `dec`, alpha-tested; every mesh
     two-sided like HOTTEST PARTY 1's port); COLOR0 = the vertex colours;
  2. parts: the backdrop's opaque meshes -> `bg` (priority -2); everything else one part per
     (blend group, loop group). Entries whose loop length divides a longer one share its part; a
     part holds at most 63 animated anchors (overflow opens `dec2`, ...);
  3. rig per part: `root` + one FLAT bone per animated anchor (a mesh's deepest animated
     ancestor, or the instance itself when only COL moves it), every vertex rigidly on it;
  4. `gm_<key>_<part>_play_loop.anm` (loop bit): per bone q / t / scale relative to rest, keys every
     2nd frame + a wrap key, checked against the zan worlds;
  5. `gm_<key>_<part>_play_loop.sanm`: the materials' UV-offset keys (version-3 materials, +0x38)
     on params 2 / 3, unwrapped across loop repeats; texture flip-books keep their first frame;
  6. cameras: the stage's shots -> `camera/<key>_st01..`, the generic dance cameras of
     game/GAME_DEF_CAM.bin /#0 -> `_non01..` (position + aim, no roll; the FOV is MTXPerspective's
     vertical angle, kept on World's 16:9 frame);
  7. sidecar `map_resources.rlist.txt`: `<key>, 000000, 000000, bg:-2, dec, ..., ble:-1`.
STG<nnn>_S.bin (the split-screen copies) and MUSIC FIT's STG000 (byte-identical to FuruFuru
Party's) are not ported.

Inputs (environment):
  GAME        hp2 | hp3 (default hp2); HP2_GAME / HP3_GAME the dumped disc trees
  STAGES      comma list (STG027, 27, ...), default the first stage, or 'all'
  OUT_BASE    default data_mods/custom_models/stages/HOTTEST PARTY 2|3 (one folder per stage,
              `Stage 27` ..; keys hp2stage027 ..)
  PREVIEW     1 = render Workbench / EEVEE previews of the RE-IMPORTED parts into PREVIEW_DIR, plus
              a ported dancer through two of the written .camanm clips
Run: GAME=hp2 STAGES=all /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
       --python tools/blender_ddr_addon/examples/port_stage_hottest2.py
"""
import hashlib
import math
import os
import sys
import tempfile

import bpy
import numpy as np
from mathutils import Vector

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.normpath(os.path.join(HERE, '..', '..', '..'))
sys.path.insert(0, HERE)
sys.path.insert(0, os.path.join(REPO, 'scripts'))
import port_lib as P  # noqa: E402  (registers the add-on)
import zan_dump as Z  # noqa: E402
import hsf_dump as H  # noqa: E402  (consistent_winding)
import tzm_dump as T  # noqa: E402  (look_at_rows, rowmat_to_quat, world_camanm_fov)
import extract_wii_ddr_data as W  # noqa: E402
from blender_ddr_addon import convert, export_model, import_anm, import_model  # noqa: E402
from blender_ddr_addon.codec import anm as A  # noqa: E402
from blender_ddr_addon.codec import ktmdl as K  # noqa: E402

GAME = os.environ.get('GAME', 'hp2').lower()
assert GAME in ('hp2', 'hp3'), GAME
DISCS = {'hp2': '~/Desktop/DDR Wii ISOs/Furu Furu Party (Japan)', 'hp3': '~/Desktop/DDR Wii ISOs/Music Fit (Japan)'}
DISC = os.path.expanduser(os.environ.get('%s_GAME' % GAME.upper(), DISCS[GAME]))
OTHER_DISC = os.path.expanduser(os.environ.get('HP2_GAME', DISCS['hp2']))
SOURCE = {'hp2': 'HOTTEST PARTY 2', 'hp3': 'HOTTEST PARTY 3'}[GAME]
TITLE = {'hp2': 'DanceDanceRevolution FuruFuru Party (Wii, JP) = HOTTEST PARTY 2',
         'hp3': 'DanceDanceRevolution MUSIC FIT (Wii, JP) = HOTTEST PARTY 3'}[GAME]
OUT_BASE = os.path.expanduser(os.environ.get('OUT_BASE', os.path.join(REPO, 'data_mods', 'custom_models', 'stages', SOURCE)))
PREVIEW = os.environ.get('PREVIEW', '0') == '1'
PREVIEW_DIR = os.environ.get('PREVIEW_DIR') or os.path.join(tempfile.gettempdir(), 'hottest_party_%s_stage_previews' % GAME)
PREVIEW_DANCER = os.path.expanduser(os.environ.get('PREVIEW_DANCER', os.path.join(
    REPO, 'data_mods', 'custom_models', 'dancers', 'HOTTEST PARTY 2', 'Rena 1', 'pl_hp2rena01', 'pl_hp2rena01.model')))
STAGE_DIR = os.path.join(DISC, 'stage')

S = Z.GAME_SCALE
KEY_STEP = 2                 # keys every 2nd frame of the 60 fps timeline
MAX_ANCHORS = 63             # + root = scene3d::frame_board::MAX_BONES
MAX_MAT_PARAMS = 48          # scene3d::frame_board::MAX_MAT_PARAMS
MAX_LOOP = 7200              # a combined (prop x layout) loop longer than this keeps the longer one
FLAGS = {'dec': (0x0001, 0), 'bg': (0x0001, 0), 'ble': (0x02C1, 0), 'add': (0x06C1, 4), 'sub': (0x06C1, 8)}
KIND_ORDER = ['bg', 'dec', 'add', 'sub', 'ble']
PRIORITY = {'bg': -2, 'ble': -1}
NEAR, FAR = 0.1, 32768.0


def stage_list():
    out = []
    for f in sorted(os.listdir(STAGE_DIR)):
        if not (f.startswith('STG') and f.endswith('.bin')) or '_S' in f or '_EFF' in f:
            continue
        if GAME == 'hp3':
            other = os.path.join(OTHER_DISC, 'stage', f)
            if os.path.exists(other) and open(other, 'rb').read() == open(os.path.join(STAGE_DIR, f), 'rb').read():
                continue   # byte-identical to FuruFuru Party's: shipped once, in HOTTEST PARTY 2
        out.append(f[:-4])
    return out


STAGES = stage_list()


def stage_number(stage):
    return int(stage[3:6])


def stage_label(stage):
    return 'Stage %02d' % stage_number(stage)


def stage_key(stage):
    return '%sstage%03d' % (GAME, stage_number(stage))


# ---------------------------------------------------------------------------------------------
# loading
# ---------------------------------------------------------------------------------------------
def animated_nodes(model, motion, tol=1e-5):
    """Indices of the nodes whose own channels change over the loop."""
    out = set()
    if not motion:
        return out
    for nm, ch in motion['bones'].items():
        i = model['by_name'].get(nm)
        if i is None:
            continue
        for _k, (_fr, v) in ch.items():
            if len(v) and np.abs(v - v[0]).max() > tol:
                out.add(i)
                break
    return out


def load_stage(stage):
    """([entry dict(index, stem, kind, model, textures, motion, inst, length, worlds)], cams)."""
    blob = open(os.path.join(STAGE_DIR, stage + '.bin'), 'rb').read()
    src = Z.stage_sources(blob)
    inst = Z.stage_instances(src)
    col_model, col_motion = src['col'] if src['col'] else (None, None)
    col_anim = animated_nodes(col_model, col_motion) if col_model else set()
    entries = []
    for e in src['models']:
        frames = inst.get(e['stem'], []) if e['kind'] == 'obj' else [None]
        if e['kind'] == 'obj' and not frames:
            print('  PROP %s: no OBJSET node places it -- drawn where it was modelled' % e['stem'])
            frames = [None]
        own = animated_nodes(e['model'], e['motion'])
        L_own = int(e['motion']['length']) if own else 0
        for fr in frames:
            fi = col_model['by_name'][fr] if fr else None
            inst_anim = fi is not None and fi in col_anim
            L_inst = int(col_motion['length']) if inst_anim else 0
            if L_own and L_inst:
                L = L_own * L_inst // math.gcd(L_own, L_inst)
                if L > MAX_LOOP:
                    print('  LOOP %s x %s: lcm %d > %d, keeping %d' % (e['stem'], fr, L, MAX_LOOP, max(L_own, L_inst)))
                    L = max(L_own, L_inst)
            else:
                L = L_own or L_inst
            entry = dict(index=len(entries), stem=e['stem'], kind=e['kind'], model=e['model'], textures=e['textures'],
                         motion=e['motion'], inst=fr, inst_index=fi, L_own=L_own, L_inst=L_inst, length=L,
                         animated=set(own) | ({0} if inst_anim else set()))
            entries.append(entry)
    for e in entries:
        e['worlds'] = make_worlds(e, col_model, col_motion)
        e['rest'] = e['worlds']([0.0])[0]
        e['unit_rest'] = e['worlds']([0.0], True)[0]
    return entries, src['cams']


def make_worlds(e, col_model, col_motion):
    def worlds(times, unit=False):
        t = np.asarray(times, dtype=np.float64)
        w = Z.posed_worlds(e['model'], e['motion'], t % e['L_own'] if e['L_own'] else t * 0, unit_scale=unit)
        if e['inst'] is not None:
            ti = t % e['L_inst'] if e['L_inst'] else t * 0
            wi = Z.posed_worlds(col_model, col_motion, ti, unit_scale=unit)[:, e['inst_index']]
            w = np.einsum('fnij,fjk->fnik', w, wi)
        return w
    return worlds


def generic_cameras():
    blob = open(os.path.join(DISC, 'game', 'GAME_DEF_CAM.bin'), 'rb').read()
    return [(p, Z.parse_cam(b)) for p, _n, b in Z.members(blob, 'cam') if p.startswith('/#0/') and p.count('/') == 2]


# ---------------------------------------------------------------------------------------------
# meshes -> part groups
# ---------------------------------------------------------------------------------------------
def alpha_class(rgba):
    a = rgba[..., 3]
    if a.min() == 255:
        return 'opaque'
    mid = (a > 8) & (a < 247)
    return 'partial' if mid.mean() > 0.002 else 'binary'


_ALPHA = {}


def world_kind(e, mi, has_vertex_alpha):
    mt = e['model']['materials'][mi]
    blend, soft, _two, _lit = Z.material_mode(mt)
    if blend == Z.BLEND_ADD:
        return 'add'
    if blend == Z.BLEND_DARKEN:
        return 'sub'
    if blend == Z.BLEND_OPAQUE or not soft:
        return 'dec'
    if has_vertex_alpha:
        return 'ble'
    b = mt['textures'][0] if mt['textures'] else None
    if b is not None and b < len(e['textures']):
        key = (id(e['textures']), b)
        if key not in _ALPHA:
            _ALPHA[key] = alpha_class(e['textures'][b])
        if _ALPHA[key] == 'partial':
            return 'ble'
    return 'dec'


def mesh_records(e):
    """Every (mesh node, material) piece of one entry, baked into game space at its frame-0
    world: dict(kind, anchor, material, bitmap, pos, nrm, uv, col, tris, obj)."""
    model = e['model']
    nodes = model['nodes']
    out = []
    for nd in nodes:
        if not nd['submeshes']:
            continue
        anchor, i = None, nd['index']
        while i >= 0:
            if i in e['animated']:
                anchor = i
                break
            i = nodes[i]['parent']
        Wm = e['rest'][nd['index']]
        for sm in nd['submeshes']:
            if not len(sm['pos']) or not sm['packets']:
                continue
            mi = sm['material']
            if mi >= len(model['materials']):
                continue
            mt = model['materials'][mi]
            index, P_, N_, UV_, C_, tri_out = {}, [], [], [], [], []
            for pk in sm['packets']:
                n = pk['corners']
                cs = []
                for c in range(n):
                    key = (int(pk['pos'][c]), int(pk['nrm'][c]) if pk['nrm'] is not None else 0,
                           int(pk['col'][c]) if pk['col'] is not None else 0, int(pk['uv0'][c]) if pk['uv0'] is not None else 0)
                    if key not in index:
                        index[key] = len(P_)
                        P_.append((sm['pos'][key[0]] @ Wm[:3, :3] + Wm[3, :3]) * S)
                        if len(sm['nrm']) and key[1] < len(sm['nrm']):
                            nv = sm['nrm'][key[1]] @ np.linalg.pinv(Wm[:3, :3]).T
                        else:
                            nv = np.array([0.0, 1.0, 0.0])
                        N_.append(nv / (np.linalg.norm(nv) or 1.0))
                        UV_.append(sm['uv'][key[3]] if len(sm['uv']) and key[3] < len(sm['uv']) else (0.0, 0.0))
                        C_.append(sm['col'][key[2]] if sm['col'] is not None and key[2] < len(sm['col']) else np.ones(4))
                    cs.append(index[key])
                for a, b, c in Z.strip_triangles(n):
                    t = (cs[a], cs[b], cs[c])
                    if len(set(t)) == 3:
                        tri_out.append(t)
            if not tri_out:
                continue
            col = np.array(C_)
            out.append(dict(kind=world_kind(e, mi, bool((col[:, 3] < 0.999).any())), anchor=anchor, material=mi,
                            bitmap=mt['textures'][0] if mt['textures'] and mt['textures'][0] < len(e['textures']) else None,
                            pos=np.array(P_), nrm=np.array(N_), uv=np.array(UV_), col=col, tris=np.array(tri_out),
                            obj=nd['index'], two_sided=Z.material_mode(mt)[2]))
    return out


def plan_parts(entries):
    """[(part name, kind, loop length, [(entry, records)])]: entries grouped by loop length (an
    entry joins a group whose length its own divides), then by blend group, then split at
    MAX_ANCHORS animated anchors."""
    groups = []
    for e in sorted(entries, key=lambda e: -e['length']):
        for g in groups:
            if e['length'] == 0 or (g[0] and g[0] % e['length'] == 0):
                g[1].append(e)
                break
        else:
            groups.append([e['length'], [e]])
    parts = []
    for gi, (length, members) in enumerate(groups):
        for kind in KIND_ORDER:
            chunks = []
            for e in members:
                for r in e['records']:
                    if r['kind'] != kind:
                        continue
                    a = None if r['anchor'] is None else (e['index'], r['anchor'])
                    for ch in chunks:
                        if a is None or a in ch[1] or len(ch[1]) < MAX_ANCHORS:
                            break
                    else:
                        ch = [{}, set()]
                        chunks.append(ch)
                    if a is not None:
                        ch[1].add(a)
                    ch[0].setdefault(e['index'], (e, []))[1].append(r)
            for ci, ch in enumerate(chunks):
                parts.append([kind, gi, ci, length, list(ch[0].values())])
    out, counts = [], {}
    for kind, gi, ci, length, ch in parts:
        counts[kind] = counts.get(kind, 0) + 1
        name = kind if counts[kind] == 1 else '%s%d' % (kind, counts[kind])
        out.append((name, kind, length, ch))
    return out


def split_backdrop(entries):
    for e in entries:
        if e['kind'] == 'bg':
            for r in e['records']:
                if r['kind'] == 'dec':
                    r['kind'] = 'bg'


# ---------------------------------------------------------------------------------------------
# build + export one part
# ---------------------------------------------------------------------------------------------
def texture_stem(key, e, b):
    rgba = e['textures'][b]
    return '%s_%s' % (key.replace('stage', 's'), hashlib.md5(np.ascontiguousarray(rgba).tobytes()).hexdigest()[:8])


def pow2(rgba):
    h, w = rgba.shape[:2]
    nh, nw = 1 << max(0, int(round(math.log2(h)))), 1 << max(0, int(round(math.log2(w))))
    if (nh, nw) == (h, w):
        return rgba
    ys = (np.arange(nh) * h // nh).clip(0, h - 1)
    xs = (np.arange(nw) * w // nw).clip(0, w - 1)
    return rgba[ys][:, xs]


def texture_image(key, e, b):
    stem = texture_stem(key, e, b)
    img = bpy.data.images.get(stem)
    if img is not None:
        return img
    path = os.path.join(tempfile.gettempdir(), 'hottest_party_%s_stage_textures' % GAME, stem + '.png')
    os.makedirs(os.path.dirname(path), exist_ok=True)
    rgba = np.ascontiguousarray(pow2(e['textures'][b]))
    W.write_png(path, rgba.shape[1], rgba.shape[0], rgba.tobytes())
    return P.load_texture(stem, path)


def rigid_row(m, m_unit):
    """(rotation + translation, per-axis scale) of a ROW-vector world, M = diag(scale) . R with
    det R = +1 (a mirrored node keeps its reflection as a negative x scale); a flattened prop
    (a ~0 scale) takes its rotation from the unit-scale chain. Translation from the real chain."""
    scale = np.linalg.norm(m[:3, :3], axis=1)
    if scale.min() > 1e-6:
        r = m[:3, :3] / scale[:, None]
    else:
        u = m_unit[:3, :3]
        r = u / np.maximum(np.linalg.norm(u, axis=1, keepdims=True), 1e-12)
    if np.linalg.det(r) < 0:
        r = r.copy()
        r[0] *= -1.0
        scale = scale.copy()
        scale[0] *= -1.0
    out = np.eye(4)
    out[:3, :3] = r
    out[3, :3] = m[3, :3]
    return out, scale


def game_rowm(m):
    out = np.array(m, copy=True)
    out[3, :3] *= S
    return out


def build_part(key, part, chunk):
    """Armature (root + flat anchor bones) + one mesh object per (entry, material). Returns
    (arm, objects, anchors [(entry index, node index)], binds (row, game))."""
    anchors = sorted({(e['index'], r['anchor']) for e, recs in chunk for r in recs if r['anchor'] is not None})
    entry_of = {e['index']: e for e, _r in chunk}
    bone_names = ['root'] + ['m%d.%d' % a for a in anchors]
    binds = [np.eye(4)] + [game_rowm(rigid_row(entry_of[ei]['rest'][oi], entry_of[ei]['unit_rest'][oi])[0])
                           for ei, oi in anchors]
    arm_data = bpy.data.armatures.new('%s_%s_rig' % (key, part))
    arm = bpy.data.objects.new('gm_%s_%s_arm' % (key, part), arm_data)
    bpy.context.scene.collection.objects.link(arm)
    bpy.context.view_layer.objects.active = arm
    bpy.ops.object.mode_set(mode='EDIT')
    ebs = {}
    for n, b in zip(bone_names, binds):
        eb = arm_data.edit_bones.new(n)
        eb.head = (0.0, 0.0, 0.0)
        eb.tail = (0.0, 0.04, 0.0)
        eb.matrix = convert.rowmat_to_blender([float(x) for x in b.reshape(16)])
        ebs[n] = eb
    for n in bone_names[1:]:
        ebs[n].parent = ebs['root']
    bpy.ops.object.mode_set(mode='OBJECT')
    arm['ddr_bone_order'] = bone_names

    objects = []
    for e, recs in chunk:
        by_mat = {}
        for r in recs:
            by_mat.setdefault(r['material'], []).append(r)
        for mi, rs in sorted(by_mat.items()):
            pos = np.concatenate([r['pos'] for r in rs])
            nrm = np.concatenate([r['nrm'] for r in rs])
            uv = np.concatenate([r['uv'] for r in rs])
            col = np.concatenate([r['col'] for r in rs])
            offs = np.cumsum([0] + [len(r['pos']) for r in rs])
            tris = np.concatenate([r['tris'] + o for r, o in zip(rs, offs)])
            bones = np.concatenate([np.full(len(r['pos']), 0 if r['anchor'] is None else 1 + anchors.index((e['index'], r['anchor'])))
                                    for r in rs])
            tris, _f = H.consistent_winding(pos, nrm, tris)
            name = 'gm_%s_%s_m%02d_%03d' % (key, part, e['index'], mi)
            me = bpy.data.meshes.new(name)
            me.from_pydata([tuple(convert.vec_to_blender(p)) for p in pos], [], tris.tolist())
            me.update()
            lay = me.uv_layers.new(name='UVMap')
            loops_v = np.zeros(len(me.loops), dtype=np.int64)
            me.loops.foreach_get('vertex_index', loops_v)
            luv = uv[loops_v].copy()
            luv[:, 1] = 1.0 - luv[:, 1]
            lay.data.foreach_set('uv', luv.astype(np.float32).ravel())
            exact = me.attributes.new('ddr_normal', 'FLOAT_VECTOR', 'POINT')
            exact.data.foreach_set('vector', np.array([tuple(convert.vec_to_blender(n)) for n in nrm]).ravel())
            ob = bpy.data.objects.new(name, me)
            bpy.context.scene.collection.objects.link(ob)
            ob.parent = arm
            groups = {n: ob.vertex_groups.new(name=n) for n in bone_names}
            for bi in np.unique(bones):
                groups[bone_names[int(bi)]].add(np.nonzero(bones == bi)[0].tolist(), 1.0, 'REPLACE')
            mod = ob.modifiers.new('Armature', 'ARMATURE')
            mod.object = arm
            c_attr = P.white_color_attribute(ob)
            rgba = col[loops_v].astype(np.float32)
            if rs[0]['kind'] == 'add':
                rgba[:, 3] = 1.0
            c_attr.data.foreach_set('color', rgba.ravel())
            b = rs[0]['bitmap']
            image = texture_image(key, e, b) if b is not None else P.palette_texture(
                '%s_white' % key.replace('stage', 's'), [(1.0, 1.0, 1.0)], size=8)
            mat = P.make_material(name, image, two_sided=True)
            if rs[0]['kind'] in ('ble', 'add', 'sub'):
                mat.surface_render_method = 'BLENDED'
            mat['ddr_zan_material'] = '%d.%d' % (e['index'], mi)
            me.materials.append(mat)
            f1, f2 = FLAGS[rs[0]['kind']]
            ob['ddr_flags'] = f1
            ob['ddr_flags2'] = f2
            objects.append(ob)
    return arm, objects, anchors, binds


MAX_KEYS = 3000              # a very slow, very long loop (STG109's 32000 frames) keys sparser


def key_step(length):
    return max(KEY_STEP, -(-length // MAX_KEYS))


def part_times(length):
    step = key_step(length)
    n = max(1, length // step)
    return np.arange(n) * step, n * step


def loop_spec(anchors, entry_of, my_binds, file_binds, length):
    """write_anm spec for the flat rig: bone b's world per key = the anchor's game rotation, its
    translation, scale relative to rest, + a wrap key."""
    times, total = part_times(length)
    key_times = list(times) + [total]
    tracks = [dict(kind=0x1C, target=0, keys=[(0.0, 0.0, 0.0, 1.0)]), dict(kind=0x1D, target=0, keys=[(0.0, 0.0, 0.0)])]
    expected = np.zeros((len(times), 1 + len(anchors), 4, 4))
    expected[:, 0] = np.eye(4)
    cache = {}
    for b, (ei, oi) in enumerate(anchors, start=1):
        e = entry_of[ei]
        if ei not in cache:
            cache[ei] = (e['worlds'](times.astype(np.float64)), e['worlds'](times.astype(np.float64), True))
        wf, uf = cache[ei]
        s_rest = rigid_row(e['rest'][oi], e['unit_rest'][oi])[1]
        fb, mb = np.asarray(file_binds[b]), np.asarray(my_binds[b])
        assert np.abs(fb[:3, :3] - mb[:3, :3]).max() < 1e-3 and \
            np.abs(fb[3, :3] - mb[3, :3]).max() < 1e-4 * max(1.0, float(np.abs(mb[3, :3]).max())), \
            'bone %d: the exporter re-framed the bind\n%s\n%s' % (b, fb, mb)
        quats, trans, scales, prev = [], [], [], None
        for f in range(len(times)):
            rig, sc = rigid_row(wf[f, oi], uf[f, oi])
            rel = np.where(s_rest > 1e-9, sc / np.where(s_rest > 1e-9, s_rest, 1.0), 1.0)
            r_row = rig[:3, :3]
            world = np.eye(4)
            world[:3, :3] = np.diag(rel) @ r_row
            world[3, :3] = rig[3, :3] * S
            expected[f, b] = world
            qv = T.rowmat_to_quat(r_row)
            if prev is not None and sum(a * c for a, c in zip(prev, qv)) < 0:
                qv = tuple(-c for c in qv)
            prev = qv
            quats.append(qv)
            trans.append(tuple(float(x) for x in rig[3, :3] * S))
            scales.append(tuple(float(x) for x in rel))
        for lst in (quats, trans, scales):
            lst.append(lst[0])
        tracks.append(dict(kind=0x1C, target=b, times=key_times, keys=quats))
        tracks.append(dict(kind=0x1D, target=b, times=key_times, keys=trans))
        if any(abs(c - 1.0) > 1e-4 for s_ in scales for c in s_):
            tracks.append(dict(kind=10, target=b, times=key_times, keys=scales))
    return dict(frame_count=total, flag=1, hierarchy=[-1] + [0] * len(anchors), tracks=tracks), expected


def check_loop(anm_bytes, expected, times_of):
    parsed = A.parse_anm(anm_bytes)
    n_f, n_b = expected.shape[:2]
    parents = [-1] + [0] * (n_b - 1)
    worst_r = worst_t = 0.0
    for f in sorted({0, 1, n_f // 3, n_f // 2, n_f - 1}):
        pose = A.evaluate_pose(parsed, float(times_of[f]), parents)
        for b in range(n_b):
            w = np.array(pose[b]['world'], dtype=float).reshape(4, 4)
            worst_r = max(worst_r, float(np.abs(w[:3, :3] - expected[f, b][:3, :3]).max())
                          / max(1.0, float(np.abs(expected[f, b][:3, :3]).max())))
            t_err = float(np.abs(w[3, :3] - expected[f, b][3, :3]).max())
            worst_t = max(worst_t, t_err / max(1.0, float(np.abs(expected[f, b][3, :3]).max())))
    return worst_r, worst_t


_UV_BLOBS = {}


def material_channels(e, mi, times, total, blob_of):
    """{param: [values per key + wrap]}: params 2 / 3 = the UV offset (u, v) of a version-3
    material, unwrapped across repeats of its own period; a constant scroll (+0x20 / +0x24 per
    frame, no keys) as a ramp."""
    model = e['model']
    mt = model['materials'][mi]
    out = {}
    if model['material_version'] != 3.0:
        return out
    w = mt['words']
    key_times = np.r_[times, total].astype(np.float64)
    if w[14] and w[15]:
        keys, flags = Z.uv_keys(blob_of(model), w[14], w[15])
        period = float(keys[-1, 0])
        if period <= 0:
            return out
        t = key_times / 60.0
        v0 = Z.sample_uv(keys, flags, [0.0])[0]
        vP = Z.sample_uv(keys, flags, [period])[0]
        vals = Z.sample_uv(keys, flags, t % period) + np.floor(t / period)[:, None] * (vP - v0)
        vals[-1] = v0 + (total / 60.0 / period) * (vP - v0) if abs((total / 60.0) % period) < 1e-6 else vals[-1]
    elif w[7] and (w[8] or w[9]):
        su, sv = Z._f32(blob_of(model), mt['offset'] + 0x20), Z._f32(blob_of(model), mt['offset'] + 0x24)
        vals = np.stack([key_times * su, key_times * sv], 1)
    else:
        return out
    for ax, sub in ((0, 2), (1, 3)):
        if np.abs(vals[:, ax] - vals[0, ax]).max() > 1e-6:
            out[sub] = list(vals[:, ax])
    return out


def material_anim(objects, entry_of, length, blob_of):
    times, total = part_times(length)
    key_times = list(times) + [total]
    targets, tracks = [], []
    n_params = 0
    for ob in objects:
        for mat in ob.data.materials:
            ei, mi = (int(x) for x in mat['ddr_zan_material'].split('.'))
            chans = material_channels(entry_of[ei], mi, times, total, blob_of)
            if not chans:
                continue
            if n_params + len(chans) > MAX_MAT_PARAMS:
                print('    DROPPED material animation of %s (> %d animated params in the part)' % (mat.name, MAX_MAT_PARAMS))
                continue
            n_params += len(chans)
            slot = len(targets)
            shader = mat.get('ddr_shader') or 'mdl_ch_constant_vc'
            targets.append(dict(identity=K.pack_identity(mat.name), identity2=0, hash=K.fnv1(shader), flags=0x2000))
            for sub, vals in sorted(chans.items()):
                tracks.append(dict(kind=8, target=slot, sub=sub, times=key_times, keys=[(float(v),) for v in vals]))
    if not tracks:
        return None
    return dict(frame_count=total, flag=1, fps=60, material_tracks=tracks, material_targets=targets)


# ---------------------------------------------------------------------------------------------
# cameras
# ---------------------------------------------------------------------------------------------
def camanm_fov(fov_v_deg):
    """zan vertical FOV (MTXPerspective's fovY) -> camanm slot-2 degrees, keeping the vertical
    extent on World's 16:9."""
    return T.world_camanm_fov(math.tan(math.radians(fov_v_deg) / 2.0) * 16.0 / 9.0)


def camera_spec(cam):
    L = max(1, int(round(Z.cam_length(cam) * 60.0)))
    times = list(range(0, L + 1))
    s = Z.cam_samples(cam, np.asarray(times) / 60.0)
    pos_m, aim_m = s['pos'] * S, s['aim'] * S
    fov = s['fov'][:, 0] if 'fov' in s else np.full(len(times), 45.0)
    quats, prev = [], None
    for i in range(len(times)):
        qv = T.rowmat_to_quat(T.look_at_rows(pos_m[i], aim_m[i], 0.0))
        if prev is not None and sum(a * c for a, c in zip(prev, qv)) < 0:
            qv = tuple(-c for c in qv)
        quats.append(qv)
        prev = qv
    pos_cm = [tuple(float(x) for x in p * 100.0) for p in pos_m]
    degs = [(camanm_fov(float(f)),) for f in fov]

    def collapse(kind, target, keys, tol):
        if all(abs(c - c0) <= tol for k in keys for c, c0 in zip(k, keys[0])):
            return dict(kind=kind, target=target, times=[0], keys=[keys[0]])
        return dict(kind=kind, target=target, times=times, keys=keys)

    def const(target, value):
        return dict(kind=8, target=target, times=[0], keys=[(float(value),)])

    camt = [collapse(1, 0, quats, 1e-7), collapse(4, 1, pos_cm, 1e-4), collapse(8, 2, degs, 1e-4),
            const(3, NEAR), const(4, FAR), const(5, 4.0 / 3.0)]
    return dict(frame_count=max(times[-1], 1), flag=0, fps=60, camera=camt), times, (pos_m, aim_m)


def check_camera(data, times, pos_m, aim_m):
    parsed = A.parse_anm(data)
    cam = next(c for c in parsed['chunks'] if c['type'] == 4)
    worst_p = worst_d = 0.0
    n = len(times)
    for i in sorted({0, 1, n // 3, n // 2, n - 1}):
        q = A.sample_track(data, cam['tracks'][0], float(times[i]))
        p = np.array(A.sample_track(data, cam['tracks'][1], float(times[i]))) * 0.01
        rows = np.array(A.quat_to_rowmat(q))
        want = aim_m[i] - pos_m[i]
        want /= max(np.linalg.norm(want), 1e-12)
        worst_p = max(worst_p, float(np.abs(p - pos_m[i]).max()))
        worst_d = max(worst_d, float(np.abs(-rows[2] - want).max()))
    return worst_p, worst_d


_GENERIC = None


def export_cameras(cams, set_dir, key):
    global _GENERIC
    if _GENERIC is None:
        _GENERIC = generic_cameras()
    cam_dir = os.path.join(set_dir, 'camera')
    os.makedirs(cam_dir, exist_ok=True)
    for stale in os.listdir(cam_dir):
        if stale.lower().endswith('.camanm'):
            os.remove(os.path.join(cam_dir, stale))
    plan = [('%s_st%02d' % (key, i + 1), c) for i, (_p, c) in enumerate(cams)]
    plan += [('%s_non%02d' % (key, i + 1), c) for i, (_p, c) in enumerate(_GENERIC)]
    stems, worst = [], (0.0, 0.0)
    for stem, c in plan:
        spec, times, (pos_m, aim_m) = camera_spec(c)
        data = A.write_anm(spec)
        ep, ed = check_camera(data, times, pos_m, aim_m)
        assert ep < 1e-3 and ed < 1e-3, '%s: camera error pos %.4f m dir %.4f' % (stem, ep, ed)
        worst = (max(worst[0], ep), max(worst[1], ed))
        open(os.path.join(cam_dir, stem + '.camanm'), 'wb').write(data)
        stems.append(stem)
    print('  CAMERAS %d main + %d close-ups, worst err %.1e m / %.1e' % (len(cams), len(_GENERIC), *worst))
    return stems


# ---------------------------------------------------------------------------------------------
# port
# ---------------------------------------------------------------------------------------------
def port(stage):
    label, key = stage_label(stage), stage_key(stage)
    assert len(label.encode()) <= 15, label
    entries, cams = load_stage(stage)
    blob_cache = {}
    raw = open(os.path.join(STAGE_DIR, stage + '.bin'), 'rb').read()
    for p, _n, b in Z.members(raw, 'zmb'):
        blob_cache[p] = b
    zmb_blob = {}
    for e in entries:
        for p, b in blob_cache.items():
            if p.endswith('/' + e['stem'] + '.zmb'):
                zmb_blob[id(e['model'])] = b

    def blob_of(model):
        return zmb_blob[id(model)]

    for e in entries:
        e['records'] = mesh_records(e)
    split_backdrop(entries)
    flip = sum(1 for e in entries for mt in e['model']['materials'] if (mt['ntex_word'] & 0xFFFF) > 1)
    parts = plan_parts(entries)
    print('STAGE %s: %d entries (%d props placed), loops %s, %d cameras, %d flip-book materials (first frame), parts %s' % (
        stage, len(entries), sum(1 for e in entries if e['inst']), sorted({e['length'] for e in entries}), len(cams), flip,
        ['%s@%d' % (n, L) for n, _k, L, _c in parts]))

    out_dir = os.path.join(OUT_BASE, label)
    set_dir = os.path.join(out_dir, 'mapset_' + key)
    os.makedirs(set_dir, exist_ok=True)
    for stale in os.listdir(set_dir):
        if stale.startswith('gm_%s_' % key):
            pdir = os.path.join(set_dir, stale)
            for f in os.listdir(pdir):
                os.remove(os.path.join(pdir, f))
            os.rmdir(pdir)
    written = []
    for part, kind, length, chunk in parts:
        P.fresh_scene()
        arm, objects, anchors, binds = build_part(key, part, chunk)
        if not objects:
            continue
        entry_of = {e['index']: e for e, _r in chunk}
        sspec = material_anim(objects, entry_of, length, blob_of) if length else None
        bpy.context.view_layer.update()
        model_name = 'gm_%s_%s' % (key, part)
        pdir = os.path.join(set_dir, model_name)
        os.makedirs(pdir, exist_ok=True)
        _w, spec = export_model.export_model(os.path.join(pdir, model_name + '.model'), arm, objects, True)
        m = K.parse_model(open(os.path.join(pdir, model_name + '.model'), 'rb').read())
        assert K.write_model(K.model_to_spec(m)) == open(os.path.join(pdir, model_name + '.model'), 'rb').read()
        assert len(m['bones']) == len(anchors) + 1 <= 64, (len(m['bones']), len(anchors))
        info = '  PART %-5s %3d objects -> %3d KTMDL meshes, %2d bones, flags %s' % (
            part, len(objects), len(spec['meshes']), len(spec['bones']),
            sorted({(hex(me['flags']), me.get('flags2', 0)) for me in m['meshes']}))
        file_binds = [np.array(b['bind'], dtype=float).reshape(4, 4) for b in m['bones']]
        if anchors and length:
            lspec, expected = loop_spec(anchors, entry_of, binds, file_binds, length)
            data = A.write_anm(lspec)
            er, et = check_loop(data, expected, part_times(length)[0])
            assert er < 2e-3 and et < 2e-4, '%s %s: loop error rot %.5f trans %.5f' % (stage, part, er, et)
            open(os.path.join(pdir, model_name + '_play_loop.anm'), 'wb').write(data)
            info += ', loop %d frames (%d anchors, err %.1e / %.1e rel)' % (lspec['frame_count'], len(anchors), er, et)
        if sspec:
            idents = {mm['identity'] for mm in m['materials']}
            missing = [t for t in sspec['material_targets'] if t['identity'] not in idents]
            assert not missing, missing
            sdata = A.write_anm(sspec)
            parsed = A.parse_anm(sdata)
            worst = 0.0
            for t in sspec['material_tracks']:
                for tm, kv in list(zip(t['times'], t['keys']))[::max(1, len(t['times']) // 8)]:
                    worst = max(worst, abs(A.evaluate_materials(parsed, float(tm))[t['target']][t['sub']] - kv[0]))
            assert worst < 1e-4, '%s %s: sanm error %.2e' % (stage, part, worst)
            open(os.path.join(pdir, model_name + '_play_loop.sanm'), 'wb').write(sdata)
            info += ', sanm %d materials / %d tracks' % (len(sspec['material_targets']), len(sspec['material_tracks']))
        print(info)
        written.append((part, kind))

    fields = ['%s:%d' % (p, PRIORITY[k]) if k in PRIORITY else p for p, k in written]
    with open(os.path.join(out_dir, 'map_resources.rlist.txt'), 'w') as f:
        f.write('# %s %s.bin, ported with its models as parts and their loops\n' % (TITLE, stage))
        f.write('# (tools/blender_ddr_addon/examples/port_stage_hottest2.py GAME=%s)\n' % GAME)
        f.write('%s, 000000, 000000, %s\n' % (key, ', '.join(fields)))
    print('SIDECAR', fields)
    stems = export_cameras(cams, set_dir, key)
    if PREVIEW:
        preview(set_dir, key, [p for p, _k in written], stems)
    return out_dir


# ---------------------------------------------------------------------------------------------
# previews (port_stage_hottest.py's)
# ---------------------------------------------------------------------------------------------
def render_persp(path, pos, target, lens=24.0, res=(960, 540)):
    sc = bpy.context.scene
    cam = bpy.data.objects.get('Preview camera')
    if not cam:
        cam = bpy.data.objects.new('Preview camera', bpy.data.cameras.new('Preview camera'))
        sc.collection.objects.link(cam)
    cam.location = pos
    cam.rotation_euler = (Vector(target) - Vector(pos)).to_track_quat('-Z', 'Y').to_euler()
    cam.data.type = 'PERSP'
    cam.data.lens = lens
    cam.data.clip_end = 1000.0
    sc.camera = cam
    sc.render.resolution_x, sc.render.resolution_y = res
    sc.render.filepath = path
    bpy.ops.render.render(write_still=True)


def unlit_preview_materials():
    for ob in bpy.data.objects:
        if ob.type != 'MESH':
            continue
        for mat in ob.data.materials:
            if mat is not None and mat.use_nodes and not mat.get('ddr_preview_unlit'):
                _unlit(mat, int(ob.get('ddr_flags', 1) or 1), int(ob.get('ddr_flags2', 0) or 0))


def _unlit(mat, flags, flags2):
    nt = mat.node_tree
    tex = next((n for n in nt.nodes if n.type == 'TEX_IMAGE'), None)
    out = next((n for n in nt.nodes if n.type == 'OUTPUT_MATERIAL'), None)
    if tex is None or out is None:
        return
    vc = nt.nodes.new('ShaderNodeVertexColor')
    mul = nt.nodes.new('ShaderNodeMixRGB')
    mul.blend_type = 'MULTIPLY'
    mul.inputs['Fac'].default_value = 1.0
    nt.links.new(tex.outputs['Color'], mul.inputs['Color1'])
    nt.links.new(vc.outputs['Color'], mul.inputs['Color2'])
    am = nt.nodes.new('ShaderNodeMath')
    am.operation = 'MULTIPLY'
    nt.links.new(tex.outputs['Alpha'], am.inputs[0])
    nt.links.new(vc.outputs['Alpha'], am.inputs[1])
    em = nt.nodes.new('ShaderNodeEmission')
    nt.links.new(mul.outputs['Color'], em.inputs['Color'])
    tr = nt.nodes.new('ShaderNodeBsdfTransparent')
    if flags2 == 4:
        mix = nt.nodes.new('ShaderNodeAddShader')
        em2 = nt.nodes.new('ShaderNodeMixRGB')
        em2.blend_type = 'MULTIPLY'
        em2.inputs['Fac'].default_value = 1.0
        nt.links.new(mul.outputs['Color'], em2.inputs['Color1'])
        nt.links.new(am.outputs['Value'], em2.inputs['Color2'])
        nt.links.new(em2.outputs['Color'], em.inputs['Color'])
        nt.links.new(tr.outputs['BSDF'], mix.inputs[0])
        nt.links.new(em.outputs['Emission'], mix.inputs[1])
    else:
        mix = nt.nodes.new('ShaderNodeMixShader')
        nt.links.new(am.outputs['Value'], mix.inputs['Fac'])
        nt.links.new(tr.outputs['BSDF'], mix.inputs[1])
        nt.links.new(em.outputs['Emission'], mix.inputs[2])
    nt.links.new(mix.outputs['Shader'], out.inputs['Surface'])
    mat.surface_render_method = 'BLENDED'
    mat.use_backface_culling = False
    mat['ddr_preview_unlit'] = 1


def eevee_scene():
    sc = bpy.context.scene
    sc.render.engine = 'BLENDER_EEVEE'
    sc.world = bpy.data.worlds.get('Preview black') or bpy.data.worlds.new('Preview black')
    sc.world.color = (0.02, 0.02, 0.03)
    sc.view_settings.view_transform = 'Standard'
    sc.render.image_settings.file_format = 'PNG'


def preview(set_dir, key, parts, stems):
    P.fresh_scene()
    for part in parts:
        name = 'gm_%s_%s' % (key, part)
        import_model.load_model(os.path.join(set_dir, name, name + '.model'), import_textures=True, with_armature=True)
    unlit_preview_materials()
    eevee_scene()
    os.makedirs(PREVIEW_DIR, exist_ok=True)
    render_persp(os.path.join(PREVIEW_DIR, '%s_front.png' % key), Vector((0.0, -8.0, 4.5)), Vector((0.0, 0.0, 1.2)))
    render_persp(os.path.join(PREVIEW_DIR, '%s_wide.png' % key), Vector((0.0, -25.0, 10.0)), Vector((0.0, 0.0, 2.0)))
    if os.path.exists(PREVIEW_DANCER):
        import_model.load_model(PREVIEW_DANCER, import_textures=True, with_armature=True)
        unlit_preview_materials()
    sc = bpy.context.scene
    for stem in [s for s in stems if '_st' in s][:2] + [s for s in stems if '_non' in s][:1]:
        cam = import_anm.load_camanm(os.path.join(set_dir, 'camera', stem + '.camanm'))
        cam.data.clip_end = 1000.0
        sc.camera = cam
        sc.render.resolution_x, sc.render.resolution_y = 960, 540
        frame = sc.frame_end // 2
        sc.frame_set(frame)
        sc.render.filepath = os.path.join(PREVIEW_DIR, '%s_cam_%s_f%d.png' % (key, stem.split('_')[-1], frame))
        bpy.ops.render.render(write_still=True)


if __name__ == '__main__':
    want = os.environ.get('STAGES', STAGES[0])
    if want == 'all':
        todo = STAGES
    else:
        todo = []
        for w in (s.strip() for s in want.split(',') if s.strip()):
            st = w.upper() if w.upper().startswith('STG') else 'STG%03d' % int(w)
            if st not in STAGES:
                sys.exit('unknown stage %s (have %s)' % (w, ' '.join(STAGES)))
            todo.append(st)
    for stage in todo:
        port(stage)
    print('DONE')
