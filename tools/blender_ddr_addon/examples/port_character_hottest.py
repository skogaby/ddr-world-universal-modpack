"""EXAMPLE / PORT: the Dancing Stage / DanceDanceRevolution HOTTEST PARTY (Wii, 2007) polygon
dancers WITH THEIR OWN RIG AND THEIR OWN CHOREOGRAPHY, as Background Dancers custom dancers --
the Omnimix path of the SuperNova / Ultramix ports (port_character_supernova.py), fed by the HSF
decoder in scripts/hsf_dump.py (formats + RE: docs/wii_ddr_hottest_party_research.md).

Hottest Party runs on Hudson's Mario Party engine: every dancer is an HSFV037 model
(data/c_<costume><character>.bin entry 0) skinned with envelopes to a 26-joint Maya rig
(Hips .. Head, the arms to `*Wrist*end`, the legs to `*Toe*end`; MayaConverter's `*root` /
`*leaf` helper objects between two joints are identity and are dropped), and every character
shares ONE rig and ONE choreography library: data/c_000.bin's 256 dance clips (the per-song
bundles data/c_000_NN.bin carry byte-identical copies of the clips each song uses). Per dancer:
  1. rebuild the rig from the rest world matrices in World game space (the file frame is
     World's: Y up, facing +Z, left at +X), one uniform scale (hsf_dump.GAME_SCALE: the Hips at
     0.97 m like the SuperNova ports);
  2. build ONE skinned mesh from every mesh object (the envelopes' single / dual / multi
     weights, <= 5 influences -> the exporter keeps the 4 largest), the file's normals via
     `ddr_normal`, two-sided materials (the HSF materials carry NOCULL), winding made
     consistent with the normals, white COLOR0; textures = the model's own bitmaps (the 512^2
     body sheet + the 128 x 64 open-eyes sheet the game swaps for its blink sprites);
  3. add the World ROLE-BONE ALIAS `Spine2` -> `Spine1` to the body `.b2it` (Hips, Head and the
     toe joints are named alike);
  4. convert this dancer's share of the library to `motion/<clip>.anm`. MayaConverter cut the
     takes into short pieces whose last pose is the next one's first; consecutive pieces are
     joined back into one clip per take (hsf_dump.chain_clips: 55 takes; the 34 of >= 3 bars
     that are not the "Lesson by DJ" tutorial's step demonstrations, 617.5 bars in all, are kept
     -- MIN_BARS, lesson_only_clips). Each
     piece's length in bars is the dance viewer's own table (dll/danceviewDll.rel, read by
     extract_wii_ddr_data.danceview_clip_bars); a take is retimed so one bar = 120 frames
     (World's dance clock runs at chart BPM / 120 under `bpm_sync`) -- the library mixes takes
     authored at 120, 145, 177 and 70 BPM. Keys every 2nd frame of the 60 fps timeline; the
     Hips' x/z path is re-centred on the dancer's mark (ROOT_MODE). Each clip is checked
     against the HSF pose (< 1 mm per joint). The library is DEALT across a character's four
     costumes (greedy by length, so every costume gets ~a quarter of the bars and the four
     together dance all of it): ~1.7 MB of clips per dancer instead of ~6.9 MB;
  5. write the `chara_resources.rlist.txt` sidecar (sex from the model, shadow 0.75 F / 0.8 M).

Inputs (environment):
  HP_DIR      the extraction (scripts/extract_wii_ddr_data.py extract <game> <out>), default
              ~/Desktop/DDR Wii ISOs/hottest_party_extracted; HP_GAME (default the unpacked disc
              beside it) supplies dll/danceviewDll.rel when the extraction has no dol/ tables
  DANCERS     comma list of keys (hpemi01 .. hpbackupm04) or character stems (emi, jenny, ...:
              all four costumes), default hpemi01, or 'all'
  OUT_BASE    default data_mods/custom_models/dancers/HOTTEST PARTY 1 (one folder per dancer). RETIRED
              2026-10-04: the shipped HOTTEST PARTY 1 cast is MUSIC FIT's remakes on the zan rig
              (port_character_hottest2.py, dancers/HOTTSTPARTY 1-3); this port stays as the HSF reference
  ROOT_MODE   recentre (default: the take's Hips x/z box centre on the mark) | travel
  PREVIEW     1 = also render Workbench previews of the RE-IMPORTED export into PREVIEW_DIR
              (default the system temp dir -- never into the repo)
Run: DANCERS=all /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
       --python tools/blender_ddr_addon/examples/port_character_hottest.py
"""
import csv
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
import extract_wii_ddr_data as W  # noqa: E402
from blender_ddr_addon import convert, export_character, import_anm, import_character  # noqa: E402
from blender_ddr_addon.codec import anm as A  # noqa: E402
from blender_ddr_addon.codec import ktmdl as K  # noqa: E402

SHADER = 'mdl_ch_constant_vc'
HP_DIR = os.path.expanduser(os.environ.get('HP_DIR', '~/Desktop/DDR Wii ISOs/hottest_party_extracted'))
HP_GAME = os.path.expanduser(os.environ.get('HP_GAME', '~/Desktop/DDR Wii ISOs/Dancing Stage - Hottest Party (Europe)'))
OUT_BASE = os.path.expanduser(os.environ.get(
    'OUT_BASE', os.path.join(REPO, 'data_mods', 'custom_models', 'dancers', 'HOTTEST PARTY 1')))
ROOT_MODE = os.environ.get('ROOT_MODE', 'recentre')
PREVIEW = os.environ.get('PREVIEW', '0') == '1'
PREVIEW_DIR = os.environ.get('PREVIEW_DIR') or os.path.join(tempfile.gettempdir(), 'hottest_party_port_previews')

# The ten models (main.dol's eye-texture table at 0x80200F8C lists the slots in this order,
# AFRO's empty: he has no blink sprites) and their costume files: c_0<k-1><n>.bin is costume k
# of character n (1..8), c_1<k-1><1|2>.bin costume k of the female / male back-up dancer.
# Labels are neutral placeholders for the four new characters (the EU message bank names
# Harmony, Root, Gaku, Rena, Domi, Danca, Chordia and U.G., but nothing on the disc maps those
# names to models). stem -> (label, sex, file pattern)
CHARACTERS = {
    'emi': ('Emi', 'F', 'c_0%d1'),
    'jenny': ('Jenny', 'F', 'c_0%d2'),
    'afro': ('Afro', 'M', 'c_0%d3'),
    'rage': ('Rage', 'M', 'c_0%d4'),
    'dancera': ('Dancer A', 'F', 'c_0%d5'),   # model `hispanic`
    'dancerb': ('Dancer B', 'F', 'c_0%d6'),   # model `black_f`
    'dancerc': ('Dancer C', 'M', 'c_0%d7'),   # model `korea_m`
    'dancerd': ('Dancer D', 'M', 'c_0%d8'),   # model `jamaika`
    'backupf': ('Backup F', 'F', 'c_1%d1'),   # model `dancer_f01`
    'backupm': ('Backup M', 'M', 'c_1%d2'),   # model `dancer_m`
}
COSTUMES = (1, 2, 3, 4)
SHADOW = {'F': 0.75, 'M': 0.8}
# Takes shorter than this are left out: World cuts a clip 1.5 s before its end (schedule.rs
# CUT_LEAD), so a 1- or 2-bar piece would barely show (19 takes, 34.5 bars of 789).
MIN_BARS = 3.0
# World role bone -> the Hottest Party joint playing it (Hips, Head, Left/RightToeBase alike).
ROLE_ALIASES = {'Spine2': 'Spine1'}


def dancers():
    """{key: dict(label, sex, file, costume, stem)} for every costume of every character."""
    out = {}
    for stem, (label, sex, pattern) in CHARACTERS.items():
        for c in COSTUMES:
            out['hp%s%02d' % (stem, c)] = dict(label='%s %d' % (label, c), sex=sex, file=pattern % (c - 1),
                                               costume=c, stem=stem)
    return out


DANCERS_TABLE = dancers()


def bone_name(hsf_name):
    """World bone names: alphanumerics and `_` (`Head*end` -> `Head_end`)."""
    return hsf_name.replace('*', '_')


# ---------------------------------------------------------------------------------------------
# the choreography library
# ---------------------------------------------------------------------------------------------
def clip_bars():
    """{c_000 index: bars} from the extraction's dol/dance_clip_bars.csv, else the disc's REL."""
    path = os.path.join(HP_DIR, 'dol', 'dance_clip_bars.csv')
    if os.path.exists(path):
        with open(path) as f:
            return {int(r['c_000_index']): float(r['bars']) for r in csv.DictReader(f)}
    return W.danceview_clip_bars(open(os.path.join(HP_GAME, 'dll', 'danceviewDll.rel'), 'rb').read())


LESSON_CATEGORY = 14   # the song table's category of "Lesson by DJ", the step tutorial


def lesson_only_clips():
    """Library indices used ONLY by the lesson song's bundle (its step demonstrations, not
    dances): the bundles carry byte-identical copies of the library clips they use."""
    import hashlib
    songs = os.path.join(HP_DIR, 'dol', 'songs.csv')
    if not os.path.exists(songs):
        return set()
    with open(songs) as f:
        rows = list(csv.DictReader(f))
    lib_dir = os.path.join(HP_DIR, 'data', 'c_000')

    def digest(path):
        return hashlib.md5(open(path, 'rb').read()).hexdigest()

    by_hash = {digest(os.path.join(lib_dir, f)): int(f[:3]) for f in os.listdir(lib_dir) if f.endswith('.hsf')}
    use = {}
    for r in rows:
        d = os.path.join(HP_DIR, 'data', 'c_000_%02d' % int(r['song']))
        if not os.path.isdir(d):
            continue
        for f in sorted(os.listdir(d))[1:]:
            if f.endswith('.hsf') and digest(os.path.join(d, f)) in by_hash:
                use.setdefault(by_hash[digest(os.path.join(d, f))], set()).add(int(r['category']) == LESSON_CATEGORY)
    return {i for i, kinds in use.items() if kinds == {True}}


_LIBRARY = None


def library(body):
    """[take dict(name, pieces [(index, motion, frames, bars)], bars, fpb)] of the dance library:
    every c_000 clip with a bar length, joined into takes, static poses left out."""
    global _LIBRARY
    if _LIBRARY is not None:
        return _LIBRARY
    bars = clip_bars()
    motions = []
    for i in sorted(bars):
        path = os.path.join(HP_DIR, 'data', 'c_000', '%03d.hsf' % i)
        motions.append((i, H.parse_hsf(open(path, 'rb').read())['motions'][0]))
    by_id = dict(motions)
    lesson = lesson_only_clips()
    takes = []
    for chain in H.chain_clips(motions, model=body):
        if lesson and all(i in lesson for i in chain):
            continue
        pieces = [(i, by_id[i], H.motion_length(by_id[i]), bars[i]) for i in chain]
        if sum(p[3] for p in pieces) < MIN_BARS:
            continue
        # a take that never moves (a hold pose) is no dance
        moving = False
        for _i, mo, L, _b in pieces:
            w = H.joint_worlds(body, mo, np.linspace(0.0, L, 9))
            if any(np.abs(w[n][:, :3, 3] - w[n][:1, :3, 3]).max() > 0.5 for n in w):
                moving = True
                break
        if not moving:
            continue
        # bars per piece from the table; a piece whose table value is off the take's tempo by
        # > 5 % (6 of 256: e.g. c_000 #1, 120 frames listed as 3 bars) gets its frames / tempo
        fpm = float(np.median([L / b for _i, _m, L, b in pieces if b > 0]))
        fixed = [(i, mo, L, b if b > 0 and abs(L / b - fpm) / fpm < 0.05 else max(1.0, round(L / fpm)))
                 for i, mo, L, b in pieces]
        total_bars = sum(p[3] for p in fixed)
        takes.append(dict(name='hp%03d_%03d' % (chain[0], chain[-1]), pieces=fixed, bars=total_bars,
                          fpb=sum(p[2] for p in fixed) / total_bars))
    _LIBRARY = takes
    return takes


def deal(takes, hands=len(COSTUMES)):
    """Split the takes over `hands` costumes, longest first onto the shortest hand (each costume
    ~1/hands of the bars; together they hold every take). Returns [[take], ...]."""
    out = [[] for _ in range(hands)]
    load = [0.0] * hands
    for t in sorted(takes, key=lambda t: (-t['bars'], t['name'])):
        k = min(range(hands), key=lambda h: (load[h], h))
        out[k].append(t)
        load[k] += t['bars']
    return [sorted(h, key=lambda t: t['name']) for h in out]


def take_worlds(body, take, names, frames):
    """(frames x joints x 4 x 4) ROW-vector game worlds of one take, retimed so a bar lasts 120
    output frames (source time = frame * fpb / 120 across the joined pieces)."""
    starts = np.cumsum([0.0] + [p[2] for p in take['pieces']])
    src = np.asarray(frames, dtype=np.float64) * take['fpb'] / 120.0
    piece = np.clip(np.searchsorted(starts, src, side='right') - 1, 0, len(take['pieces']) - 1)
    out = np.empty((len(frames), len(names), 4, 4))
    for k, (_i, mo, L, _b) in enumerate(take['pieces']):
        sel = np.nonzero(piece == k)[0]
        if not len(sel):
            continue
        jw = H.joint_worlds(body, mo, np.minimum(src[sel] - starts[k], L))
        for b, n in enumerate(names):
            out[sel, b] = np.transpose(jw[n], (0, 2, 1))
    out[:, :, 3, :3] *= H.GAME_SCALE
    return out


# ---------------------------------------------------------------------------------------------
# model
# ---------------------------------------------------------------------------------------------
def load_model(entry):
    return H.parse_hsf(open(os.path.join(HP_DIR, 'data', entry['file'], '000.hsf'), 'rb').read())


def build_armature(key, joints, binds):
    arm_data = bpy.data.armatures.new(key + '_rig')
    arm = bpy.data.objects.new(key + '_Armature', arm_data)
    bpy.context.scene.collection.objects.link(arm)
    bpy.context.view_layer.objects.active = arm
    bpy.ops.object.mode_set(mode='EDIT')
    ebs = {}
    for n, _p in joints:
        eb = arm_data.edit_bones.new(bone_name(n))
        eb.head = (0.0, 0.0, 0.0)
        eb.tail = (0.0, 0.04, 0.0)
        eb.matrix = convert.rowmat_to_blender([float(x) for x in np.asarray(binds[n]).reshape(16)])
        ebs[n] = eb
    for n, p in joints:
        if p is not None:
            ebs[n].parent = ebs[p]
    bpy.ops.object.mode_set(mode='OBJECT')
    arm['ddr_bone_order'] = [bone_name(n) for n, _ in joints]
    arm['ddr_chara_key'] = key
    return arm


def pow2_rgba(rgba):
    """Nearest-neighbour resample to power-of-two dimensions (DANCER A's eye sheet is 129 x 64)."""
    h, w = rgba.shape[:2]
    nh, nw = 1 << max(0, int(round(np.log2(h)))), 1 << max(0, int(round(np.log2(w))))
    if (nh, nw) == (h, w):
        return rgba
    ys = (np.arange(nh) * h // nh).clip(0, h - 1)
    xs = (np.arange(nw) * w // nw).clip(0, w - 1)
    return rgba[ys][:, xs]


def texture_png(stem, rgba):
    out = os.path.join(tempfile.gettempdir(), 'hottest_party_port_textures', stem + '.png')
    os.makedirs(os.path.dirname(out), exist_ok=True)
    rgba = np.ascontiguousarray(pow2_rgba(rgba))
    W.write_png(out, rgba.shape[1], rgba.shape[0], rgba.tobytes())
    return out


def build_mesh(key, model, arm):
    """ONE skinned mesh, one material slot per HSF bitmap (body sheet, eye sheet)."""
    pos, nrm, uv, _col, weights, tris, mats, _objs = H.game_mesh(model)
    tris, flipped = H.consistent_winding(pos, nrm, tris)
    me = bpy.data.meshes.new(key + '_body')
    me.from_pydata([tuple(convert.vec_to_blender(p)) for p in pos], [], tris.tolist())
    me.update()
    lay = me.uv_layers.new(name='UVMap')
    loops_v = np.zeros(len(me.loops), dtype=np.int64)
    me.loops.foreach_get('vertex_index', loops_v)
    luv = uv[loops_v].copy()
    luv[:, 1] = 1.0 - luv[:, 1]  # v-down -> Blender v-up (the exporter flips back)
    lay.data.foreach_set('uv', luv.astype(np.float32).ravel())
    exact = me.attributes.new('ddr_normal', 'FLOAT_VECTOR', 'POINT')
    exact.data.foreach_set('vector', np.array([tuple(convert.vec_to_blender(n)) for n in nrm]).ravel())
    ob = bpy.data.objects.new(key + '_body', me)
    bpy.context.scene.collection.objects.link(ob)
    ob.parent = arm
    groups = {n: ob.vertex_groups.new(name=n) for n in arm['ddr_bone_order']}
    for vi, ws in enumerate(weights):
        for joint, w in ws:
            if w > 0:
                groups[bone_name(joint)].add([vi], w, 'ADD')
    mod = ob.modifiers.new('Armature', 'ARMATURE')
    mod.object = arm
    P.white_color_attribute(ob)
    # one material per bitmap the faces use (body sheet first)
    bitmaps = sorted({H.mesh_bitmap(model, int(m)) for m in np.unique(mats)} - {None})
    slot_of = {}
    for b in bitmaps:
        bm = model['bitmaps'][b]
        part = 'body' if b == bitmaps[0] else 'eye' if 'eye' in (bm['name'] or '') else 'tex%d' % b
        stem = '%s_%s' % (key, part)
        image = P.load_texture(stem, texture_png(stem, H.decode_bitmap(bm)))
        nocull = any(model['materials'][int(m)]['flags'] & H.MATERIAL_FLAG_NOCULL
                     for m in np.unique(mats) if H.mesh_bitmap(model, int(m)) == b)
        me.materials.append(P.make_material(stem, image, two_sided=nocull, shader=SHADER))
        slot_of[b] = len(me.materials) - 1
    idx = np.array([slot_of.get(H.mesh_bitmap(model, int(m)), 0) for m in mats], dtype=np.int32)
    if len(me.materials) > 1:
        me.polygons.foreach_set('material_index', idx)
        me.update()
    return ob, flipped, [model['bitmaps'][b]['name'] for b in bitmaps]


def add_role_aliases(b2it_path, names):
    entries = K.parse_b2it(open(b2it_path, 'rb').read())
    have = {n for n, _ in entries}
    for role, joint in ROLE_ALIASES.items():
        if role not in have and joint in names:
            entries.append((role, names.index(joint)))
    open(b2it_path, 'wb').write(K.write_b2it(entries))
    return sorted(n for n, _ in entries if n in ROLE_ALIASES)


def exported_rig(body_dir, body):
    m = K.parse_model(open(os.path.join(body_dir, body + '.model'), 'rb').read())
    table = K.parse_b2it(open(os.path.join(body_dir, body + '.b2it'), 'rb').read())
    by_index = {i: n for n, i in table if n not in ROLE_ALIASES}
    names = [by_index[i] for i in range(len(m['bones']))]
    parents = [b['parent'] for b in m['bones']]
    binds = [np.array(b['bind'], dtype=float).reshape(4, 4) for b in m['bones']]
    return m, names, parents, binds


def check_clip(anm_bytes, worlds, parents, times):
    """Max joint-position error (m) of the written .anm against the HSF pose at a few keys."""
    parsed = A.parse_anm(anm_bytes)
    n = worlds.shape[0]
    worst = 0.0
    for k in sorted({0, 1, n // 3, n // 2, n - 2, n - 1}):
        pose = A.evaluate_pose(parsed, times[k], parents)
        for i in range(worlds.shape[1]):
            w = np.array(pose[i]['world'], dtype=float).reshape(4, 4)
            worst = max(worst, float(np.abs(w[3, :3] - worlds[k][i][3, :3]).max()))
    return worst


def port(key):
    entry = DANCERS_TABLE[key]
    label, sex = entry['label'], entry['sex']
    assert len(label.encode()) <= 15, label  # the options row's SSO budget (catalog::MAX_LABEL_BYTES)
    P.fresh_scene()
    model = load_model(entry)
    joints = H.rig_joints(model)
    binds = H.game_bind_matrices(model)
    arm = build_armature(key, joints, binds)
    _ob, flipped, textures = build_mesh(key, model, arm)
    bpy.context.view_layer.update()
    print('MODEL %s (%s): %d joints, %d triangles re-wound, textures %s' % (
        key, entry['file'], len(joints), flipped, textures))

    out_dir = os.path.join(OUT_BASE, label)
    body = 'pl_' + key
    body_dir = os.path.join(out_dir, body)
    motion_dir = os.path.join(body_dir, 'motion')
    os.makedirs(motion_dir, exist_ok=True)
    for d, ext in ((body_dir, '.dds'), (motion_dir, '.anm')):
        for stale in os.listdir(d):
            if stale.endswith(ext):
                os.remove(os.path.join(d, stale))
    rep = export_character.export_character(out_dir, arm, key=key, write_textures=True, write_rlist=False)
    print('EXPORT', key, rep['body_spec'], [os.path.relpath(w, out_dir) for w in rep['written']])

    m, file_names, parents, file_binds = exported_rig(body_dir, body)
    hsf_of = {bone_name(n): n for n, _p in joints}
    assert sorted(file_names) == sorted(hsf_of), (file_names, joints)
    idents = [b['identity'] for b in m['bones']]
    assert len(set(idents)) == len(idents), 'bone identity collision'
    assert all(len(me['palette']) <= 52 for me in m['meshes'])
    assert len(file_names) <= 64, 'more posed bones than the frame board holds'
    assert K.write_model(K.model_to_spec(m)) == open(os.path.join(body_dir, body + '.model'), 'rb').read()
    aliases = add_role_aliases(os.path.join(body_dir, body + '.b2it'), file_names)
    assert set(aliases) == set(ROLE_ALIASES), aliases
    print('B2IT role aliases', aliases)

    names = [hsf_of[n] for n in file_names]
    hand = deal(library(model))[entry['costume'] - 1]
    total = 0
    for take in hand:
        n_out = int(round(take['bars'] * 120))
        frames = list(range(0, n_out + 1, 2))
        worlds = take_worlds(model, take, names, frames)
        shift = None
        if ROOT_MODE == 'recentre':
            hips = worlds[:, names.index('Hips'), 3]
            shift = ((hips[:, 0].min() + hips[:, 0].max()) / 2, (hips[:, 2].min() + hips[:, 2].max()) / 2)
        spec, wq = H.worlds_to_anm_spec(worlds, names, parents, file_binds, binds, frames, frames[-1], shift)
        data = A.write_anm(spec)
        err = check_clip(data, wq, parents, frames)
        assert err < 1e-3, '%s: joint error %.5f m' % (take['name'], err)
        open(os.path.join(motion_dir, take['name'] + '.anm'), 'wb').write(data)
        total += len(data)
        print('CLIP %s %2d piece(s) %5.1f bars @%5.1f fr/bar -> %5d frames @60, %7d bytes, max joint err %.2e m' % (
            take['name'], len(take['pieces']), take['bars'], take['fpb'], frames[-1], len(data), err))
    print('CLIPS %s: %d takes, %.1f bars, %.1f MB' % (key, len(hand), sum(t['bars'] for t in hand), total / 1e6))

    sidecar = os.path.join(out_dir, 'chara_resources.rlist.txt')
    with open(sidecar, 'w') as f:
        f.write('# Dancing Stage HOTTEST PARTY (Wii) "%s" (%s.bin), ported with its own rig and choreography\n' % (
            label, entry['file']))
        f.write('# (tools/blender_ddr_addon/examples/port_character_hottest.py; takes %s)\n' % ' '.join(
            t['name'] for t in hand))
        f.write('%s, pl, %s, A, 1.0, %s, 0.0\n' % (key, sex, export_character.fmt_num(SHADOW[sex])))
    print('SIDECAR', os.path.relpath(sidecar, REPO) if sidecar.startswith(REPO) else sidecar)
    if PREVIEW:
        preview(out_dir, key, max(hand, key=lambda t: t['bars'])['name'])
    return out_dir


def preview(out_dir, key, clip):
    """Round trip through the GAME formats: re-import the export + one exported clip, render."""
    P.fresh_scene()
    body = 'pl_' + key
    arm, _meshes, _parts, _info = import_character.load_character(
        os.path.join(out_dir, body, body + '.model'), import_textures=True)
    P.studio()
    os.makedirs(PREVIEW_DIR, exist_ok=True)
    P.render_camera(os.path.join(PREVIEW_DIR, '%s_rest.png' % key), Vector((0.0, -4.5, 0.95)),
                    Vector((0.0, 0.0, 0.95)), scale=2.3, res=(600, 800))
    import_anm.load_anm(os.path.join(out_dir, body, 'motion', clip + '.anm'), arm)
    for f in (0, 240, 480, 720):
        bpy.context.scene.frame_set(f)
        P.render_camera(os.path.join(PREVIEW_DIR, '%s_%s_f%04d.png' % (key, clip, f)),
                        Vector((0.0, -6.0, 1.0)), Vector((0.0, 0.0, 1.0)), scale=4.0, res=(600, 600))


if __name__ == '__main__':
    want = os.environ.get('DANCERS', 'hpemi01')
    if want == 'all':
        todo = list(DANCERS_TABLE)
    else:
        todo = []
        for w in (s.strip().lower() for s in want.split(',') if s.strip()):
            todo += [k for k, e in DANCERS_TABLE.items() if e['stem'] == w] if w in CHARACTERS else [w]
    unknown = [k for k in todo if k not in DANCERS_TABLE]
    if unknown:
        sys.exit('unknown DANCERS %s (have %s)' % (unknown, ' '.join(DANCERS_TABLE)))
    for k in todo:
        port(k)
    print('DONE')
