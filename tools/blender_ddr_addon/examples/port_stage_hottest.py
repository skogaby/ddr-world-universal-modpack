"""EXAMPLE / PORT: the Dancing Stage / DanceDanceRevolution HOTTEST PARTY (Wii, 2007) 3D stages as
Background Dancers custom stages, fed by the HSF decoder in scripts/hsf_dump.py (formats + RE:
docs/wii_ddr_hottest_party_research.md).

A Hottest Party stage pack (data/stgNN.bin) is a list of (model, motion) pairs -- the BG
backdrop, the STG floor pieces and the OBJ props, each an HSFV037 scene of static meshes under a
tree of null objects, its motion one loop of its own length (180 .. 6000 frames at 60 fps) with
object SRT tracks, attribute (UV scroll) and material (litColor) tracks -- then the dancers'
`chr*` / `look*` formation markers and six camera motions. Per stage:
  1. every mesh corner (position, normal, colour, st -- indexed separately in HSF) becomes one
     vertex, baked through its object's rest world into game metres (hsf_dump.GAME_SCALE, the
     dancers' scale); the material picks the World blend group (hsf_dump.material_kind:
     ADDCOL -> `add`, INVCOL -> `sub`, a translucent pass with real partial alpha -> `ble`, else
     `dec` alpha-tested); CULLING as on the Wii: NOCULL (object | material flags bit 1,
     FUN_8006a3a8) -> two-sided, else single-sided in the GX winding (hsf_dump.cull_winding; a
     mesh under a mirroring world stays two-sided) -- until 2026-10-05 every mesh shipped
     two-sided and back-to-back faces (stg04's fans) z-fought;
     vertex colours are kept for vtxMode 5 materials (COLOR0 x texture, the `_vc` shader);
  2. parts: the backdrop model's opaque meshes -> `bg` (priority -2, World's `_bg` skydome
     rules); everything else one part per (blend group, loop group). Models whose loop length
     divides a longer one share its part (sampled at t mod L); a part holds at most 63 animated
     anchors (World's frame board: 64 bones per instance), overflow opens `dec2`, ...;
  3. rig per part: `root` + one FLAT bone per animated anchor (a mesh's deepest animated
     ancestor; the static chain below it is baked into the vertices; bind = the anchor's rest
     world rotation + translation), every vertex rigidly weighted to its anchor;
  4. `gm_<key>_<part>_play_loop.anm` (loop bit): per bone q / t / scale relative to the rest
     scale, keys every 2nd frame + a wrap key, checked against the HSF worlds;
  5. `gm_<key>_<part>_play_loop.sanm`: the attribute UV translation (the GX texture matrix
     translates by -T, so offU / offV = -T; unwrapped across loop repeats so a scroll keeps
     going) on params 2 / 3, the material litColor animation on params 4..6 (`_c` shader);
  6. cameras: the pack's six camera motions -> `camera/<key>_st01..06.camanm`, the 59 generic
     dance cameras of data/ddrcam.bin -> `_non01..59` (position + aim + roll; the vertical FOV
     kept on World's 16:9 frame);
  7. sidecar `map_resources.rlist.txt`: `<key>, 000000, 000000, bg:-2, dec, ..., ble:-1`.
data/stg10 / 15 / 20 are byte-identical copies of stg05, a two-object test stub (BG00 + one
floor piece and a `dammy` camera) -- none of the four is ported.

Inputs (environment):
  HP_DIR      the extraction (scripts/extract_wii_ddr_data.py extract ...), default
              ~/Desktop/DDR Wii ISOs/hottest_party_extracted
  STAGES      comma list (stg00 .. stg50), default stg01, or 'all'
  OUT_BASE    default data_mods/custom_models/stages/HOTTEST PARTY 1 (one folder per stage,
              `Stage 01` ..; keys hpstage01 ..)
  PREVIEW     1 = render Workbench previews of the RE-IMPORTED parts into PREVIEW_DIR, plus a
              ported Hottest Party dancer through two of the written .camanm clips
Run: STAGES=all /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
       --python tools/blender_ddr_addon/examples/port_stage_hottest.py
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
import hsf_dump as H  # noqa: E402
import tzm_dump as Z  # noqa: E402  (look_at_rows, rowmat_to_quat, world_camanm_fov)
import extract_wii_ddr_data as W  # noqa: E402
from blender_ddr_addon import convert, export_model, import_anm, import_model  # noqa: E402
from blender_ddr_addon.codec import anm as A  # noqa: E402
from blender_ddr_addon.codec import ktmdl as K  # noqa: E402

HP_DIR = os.path.expanduser(os.environ.get('HP_DIR', '~/Desktop/DDR Wii ISOs/hottest_party_extracted'))
OUT_BASE = os.path.expanduser(os.environ.get(
    'OUT_BASE', os.path.join(REPO, 'data_mods', 'custom_models', 'stages', 'HOTTEST PARTY 1')))
PREVIEW = os.environ.get('PREVIEW', '0') == '1'
PREVIEW_DIR = os.environ.get('PREVIEW_DIR') or os.path.join(tempfile.gettempdir(), 'hottest_party_stage_previews')
PREVIEW_DANCER = os.path.expanduser(os.environ.get('PREVIEW_DANCER', os.path.join(
    REPO, 'data_mods', 'custom_models', 'dancers', 'HOTTSTPARTY 1-3', 'Rena 1', 'pl_hprena01', 'pl_hprena01.model')))

SKIPPED = {'stg05', 'stg10', 'stg15', 'stg20'}   # the test stub and its three copies
STAGES = sorted(d for d in (os.listdir(os.path.join(HP_DIR, 'data')) if os.path.isdir(os.path.join(HP_DIR, 'data')) else [])
                if d.startswith('stg') and d not in SKIPPED)
S = H.GAME_SCALE
KEY_STEP = 2                 # keys every 2nd frame of the 60 fps timeline (the loops are smooth)
MAX_ANCHORS = 63             # + root = scene3d::frame_board::MAX_BONES
MAX_MAT_PARAMS = 48          # scene3d::frame_board::MAX_MAT_PARAMS
FLAGS = {'dec': (0x0001, 0), 'bg': (0x0001, 0), 'ble': (0x02C1, 0), 'add': (0x06C1, 4), 'sub': (0x06C1, 8)}
KIND_ORDER = ['bg', 'dec', 'add', 'sub', 'ble']
PRIORITY = {'bg': -2, 'ble': -1}
CONSTANT_C_SHADER = 'mdl_ch_constant_c_vc'
NEAR, FAR = 0.1, 32768.0     # the SuperNova ports' camanm clip planes


def stage_label(stage):
    return 'Stage %s' % stage[3:]


def stage_key(stage):
    return 'hpstage%s' % stage[3:]


# ---------------------------------------------------------------------------------------------
# loading
# ---------------------------------------------------------------------------------------------
def load_stage(stage):
    """{'models': [dict(index, model, motion, length)], 'cameras': [motion]} of a stage pack."""
    d = os.path.join(HP_DIR, 'data', stage)
    files = sorted(f for f in os.listdir(d) if f.endswith('.hsf'))
    parsed = [(f, H.parse_hsf(open(os.path.join(d, f), 'rb').read())) for f in files]
    models, cameras = [], []
    i = 0
    while i < len(parsed):
        f, m = parsed[i]
        if m['objects']:
            mo = None
            if i + 1 < len(parsed) and not parsed[i + 1][1]['objects'] and parsed[i + 1][1]['motions'] \
                    and not H.is_camera_motion(parsed[i + 1][1]['motions'][0]):
                mo = parsed[i + 1][1]['motions'][0]
                i += 1
            if not H.is_marker_model(m):
                models.append(dict(index=int(f[:3]), model=m, motion=mo,
                                   length=int(round(H.motion_length(mo))) if mo else 0))
        elif m['motions'] and H.is_camera_motion(m['motions'][0]):
            cameras.append(m['motions'][0])
        i += 1
    return dict(models=models, cameras=cameras)


def generic_cameras():
    d = os.path.join(HP_DIR, 'data', 'ddrcam')
    out = []
    for f in sorted(os.listdir(d)):
        if f.endswith('.hsf'):
            m = H.parse_hsf(open(os.path.join(d, f), 'rb').read())
            if m['motions'] and H.is_camera_motion(m['motions'][0]):
                out.append(m['motions'][0])
    return out


# ---------------------------------------------------------------------------------------------
# meshes -> part groups
# ---------------------------------------------------------------------------------------------
_TEX_CACHE = {}


def bitmap_rgba(model, b):
    key = (id(model), b)
    if key not in _TEX_CACHE:
        _TEX_CACHE[key] = H.decode_bitmap(model['bitmaps'][b])
    return _TEX_CACHE[key]


def alpha_class(rgba):
    """'opaque' | 'binary' (0 / 255 only: alpha test is enough) | 'partial'."""
    a = rgba[..., 3]
    if a.min() == 255:
        return 'opaque'
    mid = (a > 8) & (a < 247)
    return 'partial' if mid.mean() > 0.002 else 'binary'


def world_kind(model, mi, has_vertex_alpha):
    """The World blend group of HSF material `mi` (see hsf_dump.material_kind): a translucent
    pass only becomes `ble` when something is actually semi-transparent."""
    mat = model['materials'][mi]
    kind = H.material_kind(mat)
    if kind != 'ble':
        return kind
    if mat['inv_alpha'] > 0 or has_vertex_alpha:
        return 'ble'
    b = H.mesh_bitmap(model, mi)
    if b is not None and alpha_class(bitmap_rgba(model, b)) == 'partial':
        return 'ble'
    return 'dec'


def mesh_records(entry, rest, unit_rest, animated):
    """Every (mesh object, material) piece of one model, baked into game space: dict(kind,
    anchor, material, bitmap, pos, nrm, uv, col, tris)."""
    model = entry['model']
    objs = model['objects']
    out = []
    for o in objs:
        if o['type'] != H.OBJ_MESH or o['vertex'] < 0 or o['face'] < 0:
            continue
        anchor, i = None, o['index']
        while i is not None:
            if i in animated:
                anchor = i
                break
            i = objs[i]['parent']
        tris, corners, mats = H.mesh_corners(model, o)
        if not tris:
            continue
        pos_buf = model['vertex'][o['vertex']]['data']
        nrm_buf = model['normal'][o['normal']]['data'] if o['normal'] >= 0 else None
        st_buf = model['st'][o['st']]['data'] if o['st'] >= 0 else None
        col_buf = model['color'][o['color']]['data'] if o['color'] >= 0 else None
        Wm = rest[o['index']]
        Nm = np.linalg.pinv(Wm[:3, :3]).T
        # a mirroring world reverses the screen winding (the draw's cull-mode swap, model flag
        # 0x800000, is not tied to it): those meshes stay two-sided
        mirrored = bool(np.linalg.det(Wm[:3, :3]) < 0)
        by_mat = {}
        for t, mt in zip(tris, mats):
            by_mat.setdefault(mt, []).append(t)
        for mt, ts in by_mat.items():
            mat = model['materials'][mt] if 0 <= mt < len(model['materials']) else None
            use_col = mat is not None and mat['vtx_mode'] == H.MATERIAL_VTX_COLOUR and col_buf is not None
            index, P_, N_, UV_, C_ = {}, [], [], [], []
            tri_out = []
            for t in ts:
                tri = []
                for ci in t:
                    c = corners[ci]
                    key = tuple(c)
                    if key not in index:
                        index[key] = len(P_)
                        p = pos_buf[c[0]]
                        P_.append((Wm @ np.r_[p, 1.0])[:3] * S)
                        n = Nm @ nrm_buf[c[1]] if nrm_buf is not None and 0 <= c[1] < len(nrm_buf) else np.array([0, 1.0, 0])
                        N_.append(n / (np.linalg.norm(n) or 1.0))
                        UV_.append(st_buf[c[3]] if st_buf is not None and 0 <= c[3] < len(st_buf) else (0.0, 0.0))
                        C_.append(col_buf[c[2]] / 255.0 if use_col and 0 <= c[2] < len(col_buf) else np.ones(4))
                    tri.append(index[key])
                if len(set(tri)) == 3:
                    tri_out.append(tri)
            if not tri_out:
                continue
            col = np.array(C_)
            out.append(dict(kind=world_kind(model, mt, bool((col[:, 3] < 0.999).any())), anchor=anchor, material=mt,
                            bitmap=H.mesh_bitmap(model, mt), pos=np.array(P_), nrm=np.array(N_), uv=np.array(UV_),
                            col=col, tris=np.array(tri_out), obj=o['index'], mirrored=mirrored,
                            two_sided=mirrored or bool((o['flags'] | (model['materials'][mt]['flags'] if 0 <= mt < len(model['materials'])
                                                                      else 0)) & H.MATERIAL_FLAG_NOCULL)))
    return out


def plan_parts(stage_src):
    """[(part name, loop length, [(entry, records)])]: models grouped by loop length (a model
    joins a group whose length its own divides), then by blend group, then split at
    MAX_ANCHORS animated anchors."""
    entries = stage_src['models']
    groups = []   # [length, [entries]]
    for e in sorted(entries, key=lambda e: -e['length']):
        animated = bool(e['motion']) and (bool(e['animated']) or bool(H.attribute_tracks(e['motion']))
                                          or bool(H.material_color_tracks(e['motion'])))
        if not animated:
            e['length'] = 0
        for g in groups:
            if e['length'] == 0 or (g[0] and g[0] % e['length'] == 0):
                g[1].append(e)
                break
        else:
            groups.append([e['length'], [e]])
    parts = []
    for gi, (length, members) in enumerate(groups):
        for kind in KIND_ORDER:
            # chunks of <= MAX_ANCHORS animated anchors; a record joins the first chunk that
            # already has its anchor or still has room (static records go to the first chunk)
            chunks = []   # [{entry index: (entry, [records])}, anchors]
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
    # names: bg for the backdrop's opaque meshes; kind + n for the rest
    out = []
    counts = {}
    for kind, gi, ci, length, ch in parts:
        counts[kind] = counts.get(kind, 0) + 1
        name = kind if counts[kind] == 1 else '%s%d' % (kind, counts[kind])
        out.append((name, kind, length, ch))
    return out


def split_backdrop(stage_src):
    """The first model (BG*) is the backdrop: its `dec` meshes become kind `bg`."""
    first = min(stage_src['models'], key=lambda e: e['index'])
    names = [o['name'] for o in first['model']['objects'] if o['type'] == H.OBJ_MESH or o['parent'] is None]
    root_names = [o['name'] for o in first['model']['objects'] if o['type'] == H.OBJ_NULL1]
    if any(n.upper().startswith(('BG', 'STG01*BG', '*BG')) or 'BG' in n.upper()[:6] for n in root_names + names):
        for r in first['records']:
            if r['kind'] == 'dec':
                r['kind'] = 'bg'


# ---------------------------------------------------------------------------------------------
# build + export one part
# ---------------------------------------------------------------------------------------------
def texture_stem(key, model, b):
    """`hp<NN>_<hash>`: the same picture in two models of a stage shares one DDS."""
    rgba = bitmap_rgba(model, b)
    return '%s_%s' % (key.replace('hpstage', 'hp'), hashlib.md5(rgba.tobytes()).hexdigest()[:8])


def pow2(rgba):
    h, w = rgba.shape[:2]
    nh, nw = 1 << max(0, int(round(math.log2(h)))), 1 << max(0, int(round(math.log2(w))))
    if (nh, nw) == (h, w):
        return rgba
    ys = (np.arange(nh) * h // nh).clip(0, h - 1)
    xs = (np.arange(nw) * w // nw).clip(0, w - 1)
    return rgba[ys][:, xs]


def texture_image(key, model, b):
    stem = texture_stem(key, model, b)
    img = bpy.data.images.get(stem)
    if img is not None:
        return img
    path = os.path.join(tempfile.gettempdir(), 'hottest_party_stage_textures', stem + '.png')
    os.makedirs(os.path.dirname(path), exist_ok=True)
    rgba = np.ascontiguousarray(pow2(bitmap_rgba(model, b)))
    W.write_png(path, rgba.shape[1], rgba.shape[0], rgba.tobytes())
    return P.load_texture(stem, path)


def rigid(m, m_unit):
    """(rotation + translation, per-axis scale) of a column-form world (port_stage_supernova)."""
    scale = np.linalg.norm(m[:3, :3], axis=0)
    out = np.eye(4)
    out[:3, :3] = m_unit[:3, :3]
    out[:3, 3] = m[:3, 3]
    return out, scale


def game_col(m):
    Sm, Si = np.diag([S, S, S, 1.0]), np.diag([1 / S, 1 / S, 1 / S, 1.0])
    return Sm @ m @ Si


def build_part(key, part, chunk):
    """Armature (root + flat anchor bones) + one mesh object per (model, material). Returns
    (arm, objects, bone keys [(entry index, object index)], binds (col, game))."""
    anchors = sorted({(e['index'], r['anchor']) for e, recs in chunk for r in recs if r['anchor'] is not None})
    entry_of = {e['index']: e for e, _r in chunk}
    bone_names = ['root'] + ['m%d.%d' % a for a in anchors]
    binds = [np.eye(4)] + [game_col(rigid(entry_of[ei]['rest'][oi], entry_of[ei]['unit_rest'][oi])[0])
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
        eb.matrix = convert.rowmat_to_blender([float(x) for x in b.T.reshape(16)])
        ebs[n] = eb
    for n in bone_names[1:]:
        ebs[n].parent = ebs['root']
    bpy.ops.object.mode_set(mode='OBJECT')
    arm['ddr_bone_order'] = bone_names

    objects = []
    flags, flags2 = FLAGS[chunk[0][1][0]['kind']] if chunk and chunk[0][1] else FLAGS['dec']
    for e, recs in chunk:
        model = e['model']
        by_mat = {}
        for r in recs:
            by_mat.setdefault((r['material'], r['two_sided'], r['mirrored']), []).append(r)
        for (mi, two, mirrored), rs in sorted(by_mat.items()):
            pos = np.concatenate([r['pos'] for r in rs])
            nrm = np.concatenate([r['nrm'] for r in rs])
            uv = np.concatenate([r['uv'] for r in rs])
            col = np.concatenate([r['col'] for r in rs])
            offs = np.cumsum([0] + [len(r['pos']) for r in rs])
            tris = np.concatenate([r['tris'] + o for r, o in zip(rs, offs)])
            bones = np.concatenate([np.full(len(r['pos']), 0 if r['anchor'] is None else 1 + anchors.index((e['index'], r['anchor'])))
                                    for r in rs])
            tris = H.cull_winding(pos, nrm, tris, two)
            name = 'gm_%s_%s_m%02d_%03d%s' % (key, part, e['index'], mi, 'r' if mirrored else 'n' if two else '')
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
            # additive keeps its vertex alpha: the HSF draw (main.dol FUN_8006a3a8) blends ADDCOL as
            # SRCALPHA + ONE, as World's flags2 = 4 does (until 2026-10-04 it shipped at 1.0)
            rgba = col[loops_v].astype(np.float32)
            # color_srgb = the raw bytes the exporter writes (the linear `color` accessor would re-encode
            # them: a file 0.5 would ship as 0.74)
            c_attr.data.foreach_set('color_srgb', rgba.ravel())
            b = rs[0]['bitmap']
            image = texture_image(key, model, b) if b is not None else P.palette_texture(
                '%s_white' % key.replace('hpstage', 'hp'), [(1.0, 1.0, 1.0)], size=8)
            mat = P.make_material(name, image, two_sided=two)
            if rs[0]['kind'] in ('ble', 'add', 'sub'):
                mat.surface_render_method = 'BLENDED'
            mat['ddr_hsf_material'] = '%d.%d' % (e['index'], mi)
            me.materials.append(mat)
            f1, f2 = FLAGS[rs[0]['kind']]
            ob['ddr_flags'] = f1 if two else f1 & ~K.MESH_FLAG_TWO_SIDED
            ob['ddr_flags2'] = f2
            objects.append(ob)
    return arm, objects, anchors, binds


def part_times(length):
    n = max(1, length // KEY_STEP)
    return np.arange(n) * KEY_STEP, n * KEY_STEP


def loop_spec(anchors, entry_of, my_binds, file_binds, length):
    """write_anm spec for the flat rig: bone b's world per key = the anchor's game rotation, its
    translation, scale relative to rest (sampled at t mod its model's loop), + a wrap key."""
    times, total = part_times(length)
    key_times = list(times) + [total]
    tracks = [dict(kind=0x1C, target=0, keys=[(0.0, 0.0, 0.0, 1.0)]), dict(kind=0x1D, target=0, keys=[(0.0, 0.0, 0.0)])]
    expected = np.zeros((len(times), 1 + len(anchors), 4, 4))
    expected[:, 0] = np.eye(4)
    cache = {}
    for b, (ei, oi) in enumerate(anchors, start=1):
        e = entry_of[ei]
        if ei not in cache:
            src = times % e['length'] if e['length'] else times * 0
            cache[ei] = (H.object_worlds_series(e['model'], e['motion'], src),
                         H.object_worlds_series(e['model'], e['motion'], src, unit_scale=True))
        wf, uf = cache[ei]
        s_rest = rigid(e['rest'][oi], e['unit_rest'][oi])[1]
        qf = np.asarray(file_binds[b]) @ np.linalg.inv(np.asarray(my_binds[b]))
        assert np.abs(qf - np.eye(4)).max() < 1e-4, 'bone %d: the exporter re-framed the bind' % b
        quats, trans, scales, prev = [], [], [], None
        for f in range(len(times)):
            rig, sc = rigid(wf[f, oi], uf[f, oi])
            g = game_col(rig)
            rel = np.where(s_rest > 1e-9, sc / np.where(s_rest > 1e-9, s_rest, 1.0), 1.0)
            r_row = g[:3, :3].T
            world = np.eye(4)
            world[:3, :3] = np.diag(rel) @ r_row
            world[3, :3] = g[:3, 3]
            expected[f, b] = world
            qv = Z.rowmat_to_quat(r_row)
            if prev is not None and sum(a * c for a, c in zip(prev, qv)) < 0:
                qv = tuple(-c for c in qv)
            prev = qv
            quats.append(qv)
            trans.append(tuple(float(x) for x in g[:3, 3]))
            scales.append(tuple(float(x) for x in rel))
        for lst in (quats, trans, scales):
            lst.append(lst[0])
        tracks.append(dict(kind=0x1C, target=b, times=key_times, keys=quats))
        tracks.append(dict(kind=0x1D, target=b, times=key_times, keys=trans))
        if any(abs(c - 1.0) > 1e-4 for s_ in scales for c in s_):
            tracks.append(dict(kind=10, target=b, times=key_times, keys=scales))
    return dict(frame_count=total, flag=1, hierarchy=[-1] + [0] * len(anchors), tracks=tracks), expected


def check_loop(anm_bytes, expected):
    parsed = A.parse_anm(anm_bytes)
    n_f, n_b = expected.shape[:2]
    parents = [-1] + [0] * (n_b - 1)
    worst_r = worst_t = 0.0
    for f in sorted({0, 1, n_f // 3, n_f // 2, n_f - 1}):
        pose = A.evaluate_pose(parsed, KEY_STEP * f, parents)
        for b in range(n_b):
            w = np.array(pose[b]['world'], dtype=float).reshape(4, 4)
            # relative to the 3x3's magnitude: an animated scale multiplies the quaternion's
            # quantisation error
            worst_r = max(worst_r, float(np.abs(w[:3, :3] - expected[f, b][:3, :3]).max())
                          / max(1.0, float(np.abs(expected[f, b][:3, :3]).max())))
            t_err = float(np.abs(w[3, :3] - expected[f, b][3, :3]).max())
            worst_t = max(worst_t, t_err / max(1.0, float(np.abs(expected[f, b][3, :3]).max())))
    return worst_r, worst_t


def material_channels(e, mi, times, total):
    """{param: [values per key + wrap]} of one HSF material: params 2 / 3 = -attribute T x / y
    (unwrapped across repeats of a shorter loop), 4..6 = litColor rgb."""
    model, mo, L = e['model'], e['motion'], e['length']
    out = {}
    if not mo or not L:
        return out
    key_times = np.r_[times, total]
    attrs = H.attribute_tracks(mo)
    mat = model['materials'][mi]
    for a in mat['attributes'][:1]:
        for ch, sub in ((8, 2), (9, 3)):
            tr = attrs.get(a, {}).get(ch)
            if tr is None:
                continue
            v0, vL = H.sample_curve(tr, [0.0, float(L)])
            vals = H.sample_curve(tr, key_times % L) + (key_times // L) * (vL - v0)
            vals[-1] = H.sample_curve(tr, [0.0])[0] + (total / L) * (vL - v0)
            if np.abs(vals - vals[0]).max() > 1e-6:
                out[sub] = list(-vals)
        unsupported = [ch for ch in attrs.get(a, {}) if ch not in (8, 9)]
        if unsupported:
            print('    UNSUPPORTED attribute channels %s on material %d.%d' % (unsupported, e['index'], mi))
    lit = H.material_color_tracks(mo).get(mi, {})
    if lit:
        for ch, sub in zip(H.MATERIAL_LIT, (4, 5, 6)):
            tr = lit.get(ch)
            base = mat['lit_color'][ch] / 255.0
            vals = np.clip(H.sample_curve(tr, key_times % L), 0.0, 1.0) if tr is not None else np.full(len(key_times), base)
            vals[-1] = vals[0]
            out[sub] = list(vals)
    return out


def material_anim(objects, entry_of, length):
    """(.sanm spec or None, {material name: seed params}) for one part."""
    times, total = part_times(length)
    key_times = list(times) + [total]
    targets, tracks, seeds = [], [], {}
    n_params = 0
    for ob in objects:
        for mat in ob.data.materials:
            ei, mi = (int(x) for x in mat['ddr_hsf_material'].split('.'))
            chans = material_channels(entry_of[ei], mi, times, total)
            if not chans:
                continue
            if n_params + len(chans) > MAX_MAT_PARAMS:
                print('    DROPPED material animation of %s (> %d animated params in the part)' % (mat.name, MAX_MAT_PARAMS))
                continue
            n_params += len(chans)
            slot = len(targets)
            if any(k in chans for k in (4, 5, 6)):
                mat['ddr_shader'] = CONSTANT_C_SHADER
                mat['ddr_params'] = [1.0, 1.0, float(chans.get(2, [0.0])[0]), float(chans.get(3, [0.0])[0]),
                                     float(chans[4][0]), float(chans[5][0]), float(chans[6][0]), 1.0, 0.0, 0.0, 0.0, 0.0]
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
    """HSF vertical FOV -> camanm slot-2 degrees, keeping the vertical extent on World's 16:9."""
    return Z.world_camanm_fov(math.tan(math.radians(fov_v_deg) / 2.0) * 16.0 / 9.0)


def camera_spec(motion):
    L = max(1, int(round(H.motion_length(motion))))
    times = list(range(0, L + 1))
    pos, aim, roll, fov, _near, _far = H.camera_samples(motion, times)
    pos_m, aim_m = pos * S, aim * S
    quats, prev = [], None
    for i in range(len(times)):
        qv = Z.rowmat_to_quat(Z.look_at_rows(pos_m[i], aim_m[i], math.radians(float(roll[i]))))
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

    cam = [collapse(1, 0, quats, 1e-7), collapse(4, 1, pos_cm, 1e-4), collapse(8, 2, degs, 1e-4),
           const(3, NEAR), const(4, FAR), const(5, 4.0 / 3.0)]
    return dict(frame_count=max(times[-1], 1), flag=0, fps=60, camera=cam), times, (pos_m, aim_m)


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


def export_cameras(src, set_dir, key):
    global _GENERIC
    if _GENERIC is None:
        _GENERIC = generic_cameras()
    cam_dir = os.path.join(set_dir, 'camera')
    os.makedirs(cam_dir, exist_ok=True)
    for stale in os.listdir(cam_dir):
        if stale.lower().endswith('.camanm'):
            os.remove(os.path.join(cam_dir, stale))
    plan = [('%s_st%02d' % (key, i + 1), mo) for i, mo in enumerate(src['cameras'])]
    plan += [('%s_non%02d' % (key, i + 1), mo) for i, mo in enumerate(_GENERIC)]
    stems = []
    worst = (0.0, 0.0)
    for stem, mo in plan:
        spec, times, (pos_m, aim_m) = camera_spec(mo)
        data = A.write_anm(spec)
        ep, ed = check_camera(data, times, pos_m, aim_m)
        assert ep < 1e-3 and ed < 1e-3, '%s: camera error pos %.4f m dir %.4f' % (stem, ep, ed)
        worst = (max(worst[0], ep), max(worst[1], ed))
        open(os.path.join(cam_dir, stem + '.camanm'), 'wb').write(data)
        stems.append(stem)
    print('  CAMERAS %d main + %d close-ups, worst err %.1e m / %.1e' % (len(src['cameras']), len(_GENERIC), *worst))
    return stems


# ---------------------------------------------------------------------------------------------
# port
# ---------------------------------------------------------------------------------------------
def port(stage):
    label, key = stage_label(stage), stage_key(stage)
    assert len(label.encode()) <= 15, label
    src = load_stage(stage)
    for e in src['models']:
        # the frame-0 pose: a constant track re-poses its object for good (Hu3DMotionExec)
        rest = H.object_worlds_series(e['model'], e['motion'], [0.0])[0]
        e['rest'] = rest
        e['unit_rest'] = H.object_worlds_series(e['model'], e['motion'], [0.0], unit_scale=True)[0]
        e['animated'] = H.animated_objects(e['model'], e['motion']) if e['motion'] else set()
        e['records'] = mesh_records(e, rest, e['unit_rest'], e['animated'])
    split_backdrop(src)
    parts = plan_parts(src)
    print('STAGE %s: %d models (loops %s), %d cameras, parts %s' % (
        stage, len(src['models']), sorted({e['length'] for e in src['models']}), len(src['cameras']),
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
        _TEX_CACHE.clear()
        arm, objects, anchors, binds = build_part(key, part, chunk)
        if not objects:
            continue
        entry_of = {e['index']: e for e, _r in chunk}
        sspec = material_anim(objects, entry_of, length) if length else None
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
        if anchors:
            lspec, expected = loop_spec(anchors, entry_of, [b.T for b in binds], file_binds, length)
            data = A.write_anm(lspec)
            er, et = check_loop(data, expected)
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
        f.write('# Dancing Stage HOTTEST PARTY (Wii) %s.bin, ported with its models as parts and their loops\n' % stage)
        f.write('# (tools/blender_ddr_addon/examples/port_stage_hottest.py)\n')
        f.write('%s, 000000, 000000, %s\n' % (key, ', '.join(fields)))
    print('SIDECAR', fields)
    stems = export_cameras(src, set_dir, key)
    if PREVIEW:
        preview(set_dir, key, [p for p, _k in written], stems)
    return out_dir


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
    """Rewire every imported material to emit texture x COLOR0 (alpha from both) -- what the
    game's unlit `_vc` shaders draw; Workbench cannot multiply the vertex colour in, and these
    stages colour white alpha-mask textures through COLOR0. Additive materials add."""
    for ob in bpy.data.objects:
        if ob.type != 'MESH':
            continue
        for mat in ob.data.materials:
            if mat is not None and mat.use_nodes and not mat.get('ddr_preview_unlit'):
                _unlit(mat, int(ob.get('ddr_flags', 1)), int(ob.get('ddr_flags2', 0) or 0))


def _unlit(mat, flags, flags2):
    if True:
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
        mat.use_backface_culling = not (flags & K.MESH_FLAG_TWO_SIDED)   # the exported cull mode
        mat['ddr_preview_unlit'] = 1


def eevee_scene():
    sc = bpy.context.scene
    sc.render.engine = 'BLENDER_EEVEE'
    sc.world = bpy.data.worlds.get('Preview black') or bpy.data.worlds.new('Preview black')
    sc.world.color = (0.02, 0.02, 0.03)
    sc.view_settings.view_transform = 'Standard'
    sc.render.image_settings.file_format = 'PNG'


def preview(set_dir, key, parts, stems):
    """Re-import every exported part; render unlit from the front and through two written
    cameras with a ported Hottest Party dancer at the origin."""
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
    want = os.environ.get('STAGES', 'stg01')
    todo = STAGES if want == 'all' else [s.strip() for s in want.split(',') if s.strip()]
    for stage in todo:
        port(stage)
    print('DONE')
