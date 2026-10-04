"""EXAMPLE / PORT: the DanceDanceRevolution FuruFuru Party (Wii, JP 2008 = HOTTEST PARTY 2) and
DanceDanceRevolution MUSIC FIT (Wii, JP 2009 = HOTTEST PARTY 3) polygon dancers WITH THEIR OWN RIG
AND THEIR OWN CHOREOGRAPHY, as Background Dancers custom dancers -- the Omnimix path of
port_character_hottest.py (HOTTEST PARTY 1), fed by the zan decoder in scripts/zan_dump.py
(formats + RE: docs/wii_ddr_hottest_party_2_3_research.md).

Both games run on Konami's own `zan` Wii library. Every dancer is a costume file
sound/stream/character/CHR<nn>0.bin = {body ZMB, head ZMB} plus one texture file per colour
variant, CHR<nn><k>.bin = {body TPL, head TPL}. All of them share ONE 37-bone Maya rig (Hips ..
Head, the hands with two finger joints each) plus the Acc* / mii_head attach joints, and ONE kind
of choreography: each song's motion/MOT010_SSQ<nnn>.bin is its dance as a run of one-bar ZAB clips.
Per dancer:
  1. rebuild the rig from the rest worlds in World game space (the file frame is World's: Y up,
     facing +Z, left at +X), one uniform scale (zan_dump.GAME_SCALE: the Hips at 0.97 m);
  2. build ONE skinned mesh: the body's skinned submeshes (joint weights by name), its rigid ones
     (on their nearest joint), and the head (rigid on `mii_head` -- the game swaps it for a Mii
     head); the file's normals via `ddr_normal`; winding made consistent with the normals; the
     vertex colours as COLOR0; one material slot per (texture, two-sided, blended). The eyes /
     mouth are overlay passes through UV set 1 (the game animates their frames): their first frame
     is baked over the face texture into one picture in UV-1 space (zan_dump.bake_overlay); the
     costume's accessories (accessory/<Joint>_<code>*.bin: rings, jun's fan) ride their joint;
  3. add the World ROLE-BONE ALIAS `Spine2` -> `Spine1` to the body `.b2it`;
  4. the choreography library of the game: every song's MOT file, chained into continuous takes
     (zan_dump.chain_motions), each piece's bars from the song's SSQ tempo (zan_dump.clip_bars),
     cut into ~8-bar clips (World cuts every clip 1.5 s before its end; stock clips are ~10 bars),
     duplicates dropped, then DEALT over the game's dancers in a seeded rotation
     (zan_dump.deal_rotating, PER_DANCER clips each; together they dance all of it). A clip is
     retimed so one bar = 120 frames (World's dance clock runs at chart BPM / 120 under
     `bpm_sync`), keyed every 2nd frame, the Hips' x/z path re-centred on the mark (ROOT_MODE),
     and checked against the zan pose (< 1 mm per joint);
  5. the `chara_resources.rlist.txt` sidecar (sex from the cast table, shadow 0.75 F / 0.8 M).

Inputs (environment):
  GAME        hp2 (FuruFuru Party) | hp3 (MUSIC FIT), default hp2
  HP2_GAME / HP3_GAME  the dumped disc trees (scripts/extract_wii_ddr_data.py disc ...), default
              ~/Desktop/DDR Wii ISOs/Furu Furu Party (Japan) / Music Fit (Japan)
  DANCERS     comma list of keys (hp2rena01 ..) or person stems (rena, backupa, ...: every
              costume), default the first dancer, or 'all'
  OUT_BASE    default data_mods/custom_models/dancers/HOTTEST PARTY 2|3 (one folder per dancer)
  PER_DANCER  clips per dancer (default 12)
  ROOT_MODE   recentre (default) | travel
  PREVIEW     1 = also render Workbench previews of the RE-IMPORTED export into PREVIEW_DIR
              (default the system temp dir -- never into the repo)
Run: GAME=hp2 DANCERS=all /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
       --python tools/blender_ddr_addon/examples/port_character_hottest2.py
"""
import hashlib
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
import hsf_dump as H  # noqa: E402  (worlds_to_anm_spec, consistent_winding)
import extract_wii_ddr_data as W  # noqa: E402
from blender_ddr_addon import convert, export_character, import_anm, import_character  # noqa: E402
from blender_ddr_addon.codec import anm as A  # noqa: E402
from blender_ddr_addon.codec import ktmdl as K  # noqa: E402

GAME = os.environ.get('GAME', 'hp2').lower()
assert GAME in ('hp2', 'hp3'), GAME
DISC = os.path.expanduser(os.environ.get('%s_GAME' % GAME.upper(), {
    'hp2': '~/Desktop/DDR Wii ISOs/Furu Furu Party (Japan)',
    'hp3': '~/Desktop/DDR Wii ISOs/Music Fit (Japan)'}[GAME]))
SOURCE = {'hp2': 'HOTTEST PARTY 2', 'hp3': 'HOTTEST PARTY 3'}[GAME]
TITLE = {'hp2': 'DanceDanceRevolution FuruFuru Party (Wii, JP) = HOTTEST PARTY 2',
         'hp3': 'DanceDanceRevolution MUSIC FIT (Wii, JP) = HOTTEST PARTY 3'}[GAME]
OUT_BASE = os.path.expanduser(os.environ.get('OUT_BASE', os.path.join(REPO, 'data_mods', 'custom_models', 'dancers', SOURCE)))
PER_DANCER = int(os.environ.get('PER_DANCER', '12'))
ROOT_MODE = os.environ.get('ROOT_MODE', 'recentre')
PREVIEW = os.environ.get('PREVIEW', '0') == '1'
PREVIEW_DIR = os.environ.get('PREVIEW_DIR') or os.path.join(tempfile.gettempdir(), 'hottest_party_%s_previews' % GAME)
CHR_DIR = os.path.join(DISC, 'sound', 'stream', 'character')

SHADER = 'mdl_ch_constant_vc'
SHADOW = {'F': 0.75, 'M': 0.8}
ROLE_ALIASES = Z.ROLE_ALIASES
SEED = {'hp2': 2, 'hp3': 3}[GAME]

# ---------------------------------------------------------------------------------------------
# the casts. A person -> [(costume file number nn, colour variants)] in in-game era order (the
# HP2 outfit first, then the HOTTEST PARTY 1 outfit, then MUSIC FIT's); variant k of costume nn is
# CHR<nn><k>.bin, numbered on across the person's costumes (`Rena 1..2` = the HP2 outfit, `Rena
# 3..6` the HP1 one, ...). Names: the select screens' name plates (select_bin*.bin) in the order of
# the costume numbers (portraits and models agree for the eight leads in both games); the
# back-up dancers get neutral labels -- see the research note.
# The Mii bodies (FuruFuru Party CHR51..54, MUSIC FIT CHR81..88) have no head and are not ported.
# stem -> (label, sex, [(nn, variants)])
# ---------------------------------------------------------------------------------------------
CASTS = {
    'hp2': {
        'rena': ('Rena', 'F', [(1, 2), (21, 4)]),
        'domi': ('Domi', 'F', [(2, 2), (22, 4)]),
        'ug': ('U.G.', 'M', [(3, 2), (23, 4)]),
        'root': ('Root', 'M', [(4, 2), (24, 4)]),
        'chordia': ('Chordia', 'F', [(5, 2), (25, 4)]),
        'harmony': ('Harmony', 'F', [(6, 2), (26, 4)]),
        'gaku': ('Gaku', 'M', [(7, 2), (27, 4)]),
        'danca': ('Danca', 'M', [(8, 2), (28, 4)]),
        'backupa': ('Backup A', 'F', [(9, 4), (29, 4)]),
        'backupb': ('Backup B', 'M', [(10, 4), (30, 4)]),
        'naoki': ('NAOKI', 'M', [(41, 4)]),
        'u1': ('U1', 'M', [(42, 4)]),
        'jun': ('jun', 'F', [(43, 4)]),
    },
    'hp3': {
        'rena': ('Rena', 'F', [(1, 2), (21, 4), (41, 2)]),
        'domi': ('Domi', 'F', [(2, 2), (22, 4)]),
        'ug': ('U.G.', 'M', [(3, 2), (23, 4), (43, 2)]),
        'root': ('Root', 'M', [(4, 2), (24, 4), (44, 2)]),
        'chordia': ('Chordia', 'F', [(5, 2), (25, 4), (45, 2)]),
        'harmony': ('Harmony', 'F', [(6, 2), (26, 4), (46, 2)]),
        'gaku': ('Gaku', 'M', [(7, 2), (27, 4)]),
        'danca': ('Danca', 'M', [(8, 2), (28, 4)]),
        'backupa': ('Backup A', 'F', [(9, 4), (29, 4), (49, 4)]),
        'backupb': ('Backup B', 'M', [(10, 4), (30, 4), (50, 4)]),
        'backupc': ('Backup C', 'F', [(14, 4), (31, 4), (54, 4)]),
        'backupd': ('Backup D', 'M', [(15, 4), (32, 4), (55, 4)]),
        'naoki': ('NAOKI', 'M', [(11, 4), (51, 2)]),
        'u1': ('U1', 'M', [(12, 4), (52, 2)]),
        'jun': ('jun', 'F', [(13, 4), (53, 3)]),
        'hip': ('Hip', 'F', [(42, 4)]),
        'nova': ('Nova', 'F', [(47, 4)]),
        'hop': ('Hop', 'F', [(48, 4)]),
    },
}[GAME]


def dancers():
    """{key: dict(label, sex, nn, variant, stem, index)} in cast order."""
    out = {}
    for stem, (label, sex, costumes) in CASTS.items():
        n = 0
        for nn, variants in costumes:
            for v in range(1, variants + 1):
                n += 1
                out['%s%s%02d' % (GAME, stem.replace('.', ''), n)] = dict(
                    label='%s %d' % (label, n), sex=sex, nn=nn, variant=v, stem=stem, index=len(out))
    return out


DANCERS_TABLE = dancers()


def bone_name(n):
    return n.replace('*', '_')


def chr_path(nn, k):
    return os.path.join(CHR_DIR, 'CHR%02d%d.bin' % (nn, k))


# ---------------------------------------------------------------------------------------------
# the choreography library
# ---------------------------------------------------------------------------------------------
def song_files():
    """[(song tag, MOT path, SSQ path)] of the songs' choreographies (no lesson files)."""
    mdir = os.path.join(DISC, 'motion')
    out = []
    for f in sorted(os.listdir(mdir)):
        if not (f.startswith('MOT010_SSQ') and f.endswith('.bin')):
            continue
        tag = f[len('MOT010_SSQ'):-4]           # '001' / '001J'
        if GAME == 'hp2':
            ssq = os.path.join(DISC, 'ssq', 'MU_DDR_%s.ss9' % tag)
        else:
            ssq = os.path.join(DISC, 'ssq', 'Jss9' if tag.endswith('J') else 'ss9', 'MU_DDR_%s.ss9' % tag)
        out.append((tag, os.path.join(mdir, f), ssq))
    return out


_LIBRARY = None


def library(body, joints):
    """[clip dict(name, pieces [(motion, frames, bars)], bars)]: every song's choreography
    chained into takes, cut into ~8-bar clips, duplicates (same piece bytes) dropped."""
    global _LIBRARY
    if _LIBRARY is not None:
        return _LIBRARY
    seen, clips = set(), []
    total_bars = 0.0
    for tag, mot, ssq in song_files():
        blobs = [z for _p, _n, z in Z.members(open(mot, 'rb').read(), 'zab')]
        motions = [Z.parse_zab(z) for z in blobs]
        lengths = [m['length'] for m in motions]
        if not motions or min(lengths) > 250:       # the 3-clip demo / showcase files
            print('LIBRARY skip %s (%d clips of %s frames)' % (tag, len(lengths), sorted(set(lengths))))
            continue
        bpm = Z.dominant_bpm(Z.ssq_tempo(open(ssq, 'rb').read())) if os.path.exists(ssq) else 14400.0 / float(np.median(lengths))
        hashes = [hashlib.md5(z).hexdigest() for z in blobs]
        for chain in Z.chain_motions(body, list(enumerate(motions)), joints):
            pieces = [(motions[i], lengths[i], Z.clip_bars(lengths[i], bpm), hashes[i]) for i in chain]
            for ci, run in enumerate(Z.chunk_take([(p[3], p[2]) for p in pieces])):
                ps = [pieces[k] for k in run]
                bars = sum(p[2] for p in ps)
                key = tuple(p[3] for p in ps)
                if bars < 3 or key in seen:
                    continue
                seen.add(key)
                # a clip that never moves (a hold pose) is no dance
                w0 = Z.pose_positions(body, ps[0][0], joints, 0.0)
                moving = any(np.abs(Z.pose_positions(body, p[0], joints, p[1] * 0.5) - w0).max() > 0.3 for p in ps)
                if not moving:
                    continue
                clips.append(dict(name='h%ss%s_%03d' % (GAME[2], tag.lower(), chain[0] + run[0]),
                                  pieces=[(p[0], p[1], p[2]) for p in ps], bars=bars, bpm=bpm))
                total_bars += bars
    print('LIBRARY %d clips, %.0f bars from %d songs' % (len(clips), total_bars, len(song_files())))
    _LIBRARY = clips
    return clips


_HANDS = None


def hand_of(index, body, joints):
    global _HANDS
    if _HANDS is None:
        lib = library(body, joints)
        _HANDS = Z.deal_rotating(range(len(lib)), len(DANCERS_TABLE), PER_DANCER, seed=SEED)
    lib = library(body, joints)
    return [lib[i] for i in sorted(_HANDS[index])]


def clip_worlds(body, clip, joints, frames):
    """(frames x joints x 4 x 4) ROW-vector game worlds of one clip, retimed so a bar lasts 120
    output frames (each piece stretched onto its own bars)."""
    out = np.empty((len(frames), len(joints), 4, 4))
    f = np.asarray(frames, dtype=np.float64)
    starts = np.cumsum([0.0] + [120.0 * p[2] for p in clip['pieces']])
    piece = np.clip(np.searchsorted(starts, f, side='right') - 1, 0, len(clip['pieces']) - 1)
    for k, (mo, L, bars) in enumerate(clip['pieces']):
        sel = np.nonzero(piece == k)[0]
        if not len(sel):
            continue
        src = np.minimum((f[sel] - starts[k]) * L / (120.0 * bars), float(L))
        out[sel] = Z.joint_worlds(body, mo, joints, src)
    return Z.game_row(out)


# ---------------------------------------------------------------------------------------------
# model
# ---------------------------------------------------------------------------------------------
def load_costume(entry):
    zm = Z.members(open(chr_path(entry['nn'], 0), 'rb').read(), 'zmb')
    body = Z.parse_zmb(zm[0][2])
    head = Z.parse_zmb(zm[1][2]) if len(zm) > 1 else None
    tp = Z.members(open(chr_path(entry['nn'], entry['variant']), 'rb').read(), 'tpl')
    body_tex = Z.tpl_images(tp[0][2])
    head_tex = Z.tpl_images(tp[1][2]) if len(tp) > 1 else []
    return body, head, body_tex, head_tex


def accessories(body, head, variant):
    """[(attach joint, ZMB model, TPL images)] of a costume: accessory/<Joint>_<costume code>[_*].bin
    (the code is the body's texture prefix: `n_05` / `c_03` / `s_03` in FuruFuru Party, `HP2A_05`
    in MUSIC FIT) -- rings, fans, a headlamp, rigid on the joint; colour variant k uses the k-th TPL
    (the last when there are fewer). The Wii-Remote props (`*_wii_con`) are left out."""
    import re
    names = ' '.join(body['textures'] + (head['textures'] if head else []))
    m = re.search(r'(HP\d[A-Z]_\d\d)', names, re.I) or re.search(r'(?:tex_)?([ncsm]_\d\d)', names)
    if not m:
        return []
    code = m.group(1).lower()
    adir = os.path.join(DISC, 'accessory')
    out = []
    for f in sorted(os.listdir(adir)) if os.path.isdir(adir) else []:
        stem = f[:-4].lower() if f.lower().endswith('.bin') else None
        if not stem or 'wii_con' in stem:
            continue
        for joint in [n['name'] for n in body['nodes']]:
            if stem.startswith(joint.lower() + '_' + code) and (len(stem) == len(joint) + 1 + len(code) or
                                                              stem[len(joint) + 1 + len(code)] == '_'):
                blob = open(os.path.join(adir, f), 'rb').read()
                zm = Z.members(blob, 'zmb')
                tps = Z.members(blob, 'tpl')
                if zm and tps:
                    out.append((joint, Z.parse_zmb(zm[0][2]), Z.tpl_images(tps[min(variant, len(tps)) - 1][2]), f))
                break
    return out


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
    h, w = rgba.shape[:2]
    nh, nw = 1 << max(0, int(round(np.log2(h)))), 1 << max(0, int(round(np.log2(w))))
    if (nh, nw) == (h, w):
        return rgba
    ys = (np.arange(nh) * h // nh).clip(0, h - 1)
    xs = (np.arange(nw) * w // nw).clip(0, w - 1)
    return rgba[ys][:, xs]


def texture_png(stem, rgba):
    out = os.path.join(tempfile.gettempdir(), 'hottest_party_%s_textures' % GAME, stem + '.png')
    os.makedirs(os.path.dirname(out), exist_ok=True)
    rgba = np.ascontiguousarray(pow2_rgba(rgba))
    W.write_png(out, rgba.shape[1], rgba.shape[0], rgba.tobytes())
    return out


def costume_pieces(body, head, body_tex, head_tex, names, acc=()):
    """[(piece, picture RGBA, uv set, two_sided, blended, tag)] of a costume: every submesh with a
    texture; an overlay pass (eyes / mouth) gets its baked UV-1 picture; accessories ride their
    joint. Additive passes (an accessory headlamp's glow) are left out: World's dancer shaders
    have no additive mode."""
    rest = Z.rest_worlds(body)
    mii = rest[body['by_name'][Z.ATTACH_NODE]]
    out = []
    sources = [(body, body_tex, Z.model_pieces(body, names), 'b')]
    if head is not None:
        sources.append((head, head_tex, Z.model_pieces(head, names, frame=mii, default_joint=Z.ATTACH_NODE), 'h'))
    for k, (joint, model, tex, _f) in enumerate(acc):
        sources.append((model, tex, Z.model_pieces(model, names, frame=rest[body['by_name'][joint]],
                                                   default_joint=joint), 'a%d' % k))
    for model, tex, pieces, prefix in sources:
        for pc in pieces:
            mt = model['materials'][pc['material']]
            if mt['flags'][3] != 0:          # an effect pass (environment map): not drawn as a mesh
                continue
            if Z.material_mode(mt)[0] == Z.BLEND_ADD:
                print('  SKIP additive %s sub %d (material %d)' % (pc['node'], pc['sub'], pc['material']))
                continue
            if not mt['textures'] or mt['textures'][0] >= len(tex):
                print('  WARN %s sub %d: material %d has no texture in the TPL (%s / %d)' % (
                    pc['node'], pc['sub'], pc['material'], mt['textures'], len(tex)))
                continue
            blend, soft, two, _lit = Z.material_mode(mt)
            base = tex[mt['textures'][0]]
            lay = Z.material_layer(model, pc['material'])
            if lay is not None and pc['flags'] & 0x10000 and lay['textures'][0] < len(tex):
                pic = Z.bake_overlay(base, tex[lay['textures'][0]], pc)
                out.append((pc, pic, 1, two, False, '%sov%d' % (prefix, lay['textures'][0])))
            else:
                out.append((pc, base, 0, two, blend == Z.BLEND_ALPHA and soft, '%s%d' % (prefix, mt['textures'][0])))
    return out


def build_mesh(key, pieces, arm):
    """ONE skinned mesh; one material slot per distinct (picture, two-sided, blended)."""
    P_, N_, UV_, C_, W_, T_, M_ = [], [], [], [], [], [], []
    slot_key, slots = {}, []
    for pc, pic, uv_set, two, blended, tag in pieces:
        pos, nrm, uv, col, wts, tris = Z.weld(pc, uv_set)
        if not len(tris):
            continue
        tris, _f = H.consistent_winding(pos, nrm, tris)
        digest = hashlib.md5(np.ascontiguousarray(pic).tobytes()).hexdigest()[:8]
        sk = (digest, two, blended)
        if sk not in slot_key:
            slot_key[sk] = len(slots)
            slots.append((pic, two, blended, tag))
        base = len(P_)
        P_.extend(pos)
        N_.extend(nrm)
        UV_.extend(uv)
        C_.extend(col)
        W_.extend(wts)
        T_.extend((tris + base).tolist())
        M_.extend([slot_key[sk]] * len(tris))
    me = bpy.data.meshes.new(key + '_body')
    me.from_pydata([tuple(convert.vec_to_blender(p)) for p in P_], [], T_)
    me.update()
    lay = me.uv_layers.new(name='UVMap')
    loops_v = np.zeros(len(me.loops), dtype=np.int64)
    me.loops.foreach_get('vertex_index', loops_v)
    luv = np.array(UV_)[loops_v].copy()
    luv[:, 1] = 1.0 - luv[:, 1]
    lay.data.foreach_set('uv', luv.astype(np.float32).ravel())
    exact = me.attributes.new('ddr_normal', 'FLOAT_VECTOR', 'POINT')
    exact.data.foreach_set('vector', np.array([tuple(convert.vec_to_blender(n)) for n in N_]).ravel())
    ob = bpy.data.objects.new(key + '_body', me)
    bpy.context.scene.collection.objects.link(ob)
    ob.parent = arm
    groups = {n: ob.vertex_groups.new(name=n) for n in arm['ddr_bone_order']}
    for vi, ws in enumerate(W_):
        acc = {}
        for joint, w in ws:
            acc[bone_name(joint)] = acc.get(bone_name(joint), 0.0) + w
        for j, w in acc.items():
            groups[j].add([vi], w, 'ADD')
    mod = ob.modifiers.new('Armature', 'ARMATURE')
    mod.object = arm
    c_attr = P.white_color_attribute(ob)
    c_attr.data.foreach_set('color', np.array(C_, dtype=np.float32)[loops_v].ravel())
    for i, (pic, two, blended, tag) in enumerate(slots):
        stem = '%s_%s' % (key, tag)
        image = P.load_texture(stem, texture_png(stem, pic))
        mat = P.make_material(stem, image, two_sided=two, shader=SHADER)
        if blended:
            mat.surface_render_method = 'BLENDED'
        me.materials.append(mat)
    if len(me.materials) > 1:
        me.polygons.foreach_set('material_index', np.array(M_, dtype=np.int32))
        me.update()
    return ob, len(slots)


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
    body, head, body_tex, head_tex = load_costume(entry)
    motion_bones = Z.parse_zab(Z.members(open(song_files()[0][1], 'rb').read(), 'zab')[0][2])['order']
    joints = Z.rig_joints(body, keep=motion_bones)
    jnames = [n for n, _p in joints]
    binds = Z.game_bind_matrices(body, joints)
    arm = build_armature(key, joints, binds)
    acc = accessories(body, head, entry['variant'])
    pieces = costume_pieces(body, head, body_tex, head_tex, set(jnames), acc)
    _ob, nslots = build_mesh(key, pieces, arm)
    bpy.context.view_layer.update()
    print('MODEL %s (CHR%02d%d): %d joints, %d pieces, %d material slots, accessories %s' % (
        key, entry['nn'], entry['variant'], len(joints), len(pieces), nslots, [a[3] for a in acc]))

    out_dir = os.path.join(OUT_BASE, label)
    bname = 'pl_' + key
    body_dir = os.path.join(out_dir, bname)
    motion_dir = os.path.join(body_dir, 'motion')
    os.makedirs(motion_dir, exist_ok=True)
    for d, ext in ((body_dir, '.dds'), (motion_dir, '.anm')):
        for stale in os.listdir(d):
            if stale.endswith(ext):
                os.remove(os.path.join(d, stale))
    rep = export_character.export_character(out_dir, arm, key=key, write_textures=True, write_rlist=False)
    print('EXPORT', key, rep['body_spec'], [os.path.relpath(w, out_dir) for w in rep['written']])

    m, file_names, parents, file_binds = exported_rig(body_dir, bname)
    zan_of = {bone_name(n): n for n in jnames}
    assert sorted(file_names) == sorted(zan_of), (file_names, jnames)
    idents = [b['identity'] for b in m['bones']]
    assert len(set(idents)) == len(idents), 'bone identity collision'
    assert all(len(me['palette']) <= 52 for me in m['meshes'])
    assert len(file_names) <= 64, 'more posed bones than the frame board holds'
    assert K.write_model(K.model_to_spec(m)) == open(os.path.join(body_dir, bname + '.model'), 'rb').read()
    aliases = add_role_aliases(os.path.join(body_dir, bname + '.b2it'), file_names)
    assert set(aliases) == set(ROLE_ALIASES), aliases

    names = [zan_of[n] for n in file_names]
    game_binds = {n: binds[n] for n in names}
    hand = hand_of(entry['index'], body, names)
    total = 0
    for clip in hand:
        n_out = int(round(clip['bars'] * 120))
        frames = list(range(0, n_out + 1, 2))
        worlds = clip_worlds(body, clip, names, frames)
        shift = None
        if ROOT_MODE == 'recentre':
            hips = worlds[:, names.index('Hips'), 3]
            shift = ((hips[:, 0].min() + hips[:, 0].max()) / 2, (hips[:, 2].min() + hips[:, 2].max()) / 2)
        spec, wq = H.worlds_to_anm_spec(worlds, names, parents, file_binds, game_binds, frames, frames[-1], shift)
        data = A.write_anm(spec)
        err = check_clip(data, wq, parents, frames)
        assert err < 1e-3, '%s: joint error %.5f m' % (clip['name'], err)
        open(os.path.join(motion_dir, clip['name'] + '.anm'), 'wb').write(data)
        total += len(data)
        print('CLIP %s %2d piece(s) %5.1f bars (song %3.0f BPM) -> %5d frames @60, %7d bytes, max joint err %.2e m' % (
            clip['name'], len(clip['pieces']), clip['bars'], clip['bpm'], frames[-1], len(data), err))
    print('CLIPS %s: %d clips, %.1f bars, %.2f MB' % (key, len(hand), sum(c['bars'] for c in hand), total / 1e6))

    sidecar = os.path.join(out_dir, 'chara_resources.rlist.txt')
    with open(sidecar, 'w') as f:
        f.write('# %s "%s" (CHR%02d0 + CHR%02d%d), ported with its own rig and choreography\n' % (
            TITLE, label, entry['nn'], entry['nn'], entry['variant']))
        f.write('# (tools/blender_ddr_addon/examples/port_character_hottest2.py GAME=%s; clips %s)\n' % (
            GAME, ' '.join(c['name'] for c in hand)))
        f.write('%s, pl, %s, A, 1.0, %s, 0.0\n' % (key, sex, export_character.fmt_num(SHADOW[sex])))
    if PREVIEW:
        preview(out_dir, key, max(hand, key=lambda c: c['bars'])['name'] if hand else None)
    return out_dir


def preview(out_dir, key, clip):
    """Round trip through the GAME formats: re-import the export + one exported clip, render."""
    P.fresh_scene()
    bname = 'pl_' + key
    arm, _meshes, _parts, _info = import_character.load_character(
        os.path.join(out_dir, bname, bname + '.model'), import_textures=True)
    P.studio()
    os.makedirs(PREVIEW_DIR, exist_ok=True)
    P.render_camera(os.path.join(PREVIEW_DIR, '%s_rest.png' % key), Vector((0.0, -4.5, 0.95)),
                    Vector((0.0, 0.0, 0.95)), scale=2.3, res=(600, 800))
    P.render_camera(os.path.join(PREVIEW_DIR, '%s_face.png' % key), Vector((0.0, -4.5, 1.55)),
                    Vector((0.0, 0.0, 1.55)), scale=0.5, res=(500, 500))
    if clip:
        import_anm.load_anm(os.path.join(out_dir, bname, 'motion', clip + '.anm'), arm)
        for f in (0, 240, 480, 720):
            bpy.context.scene.frame_set(f)
            P.render_camera(os.path.join(PREVIEW_DIR, '%s_%s_f%04d.png' % (key, clip, f)),
                            Vector((0.0, -6.0, 1.0)), Vector((0.0, 0.0, 1.0)), scale=4.0, res=(600, 600))


if __name__ == '__main__':
    want = os.environ.get('DANCERS', next(iter(DANCERS_TABLE)))
    if want == 'all':
        todo = list(DANCERS_TABLE)
    else:
        todo = []
        for w in (s.strip().lower() for s in want.split(',') if s.strip()):
            todo += [k for k, e in DANCERS_TABLE.items() if e['stem'] == w] if w in CASTS else [w]
    unknown = [k for k in todo if k not in DANCERS_TABLE]
    if unknown:
        sys.exit('unknown DANCERS %s (have %s)' % (unknown, ' '.join(DANCERS_TABLE)))
    for k in todo:
        port(k)
    print('DONE')
