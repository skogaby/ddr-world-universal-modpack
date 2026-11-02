"""EXAMPLE / SHIPPED PORT: the 25 DanceDanceRevolution Universe (Xbox 360, NTSC-U) dancers WITH
THEIR OWN RIGS AND THEIR OWN CLIPS -- the DSU recipe of port_character_ultramix.py /
port_character_ultramix4.py, adapted to Universe's data (RE notes:
docs/dancing_stage_unleashed_dancers_port_feasibility.md section 14).

Universe is KCEH's K3D engine again, and its data stays little-endian on the 360. What differs
from DDR ULTRAMIX 4, and what this script does about it:
  * the `.ddm` revision 0x112 now splits a material into 1..7 PALETTE GROUPS (one 30-bone palette
    and index range each; UMX4 always had one) and pads an odd index count to 4 bytes
    (ultramix_k3d_dump._parse_ddm4 reads both);
  * HD rigs: the male skeleton has 87 joints and the female 113 (fingers, forearm / leg `*Roll`,
    4-joint hair chains, tie, breasts, skirt chains; ultramix_k3d_dump.HIERARCHY_UNIVERSE). Every
    model carries its gender's whole skeleton, so no helper bones. That is more than any earlier
    port (<= 59): the DLL's pose buffers follow the rig (frame board 256 bones per instance) and
    the exporter splits meshes into 52-bone palette blocks;
  * every costume is its OWN model (`z_<name>_rig_a..d.ddm`), so each becomes its own dancer:
    `<Name> 1..4` (keys `univ<type>0<n>`). Queen's four costumes are one model and one texture
    set, so she ships once;
  * `<COSTUME>.csv` = SHADERS, then SHADERSn / TEXTURESn / RENDERn per costume, FACE. Material
    names match a SHADERSn cell exactly or as a `_<cell>` suffix (Emi 4's `cow_model_*`), plus one
    CSV typo (`shoessshader`). A named texture missing from the disc falls back to the `.ddm`'s
    own texture field (Lady 2's chain and shoes), and so does a material without a CSV row whose
    own texture is on the disc (Emi 4's buttons). Unlisted untextured `lambert1` materials (2..22
    stray triangles) are dropped. Materials sharing a texture become one World material
    (`uv<type><n>_<texture>`);
  * RENDER `toon` / `toonSphere` -> `mdl_ch_constant_vc`; `sphere` (chrome / gold / enamel env
    maps) gets rest-pose matcap UVs like UMX4; `semitrans` (Honey 2's and Queen's sunglass lenses,
    DXT1 with no alpha -- Universe's shader supplies it) goes to an alpha-blended slot with
    COLOR0 alpha GLASS_ALPHA;
  * the clips are 15 Hz like UMX4's (4:1 decimations of World's own 120 BPM takes: `match` at
    factor 4 finds the World take for 31 of the 34 listed clips, r 0.78-0.96), so the keys land
    on every 4th frame of the 60 fps timeline (U.FRAME_STEP_UMX4). The clip pool is the gender's
    `animations.csv` group plus `unisex`, played whole from frame 0.
Not ported: the FACE swaps (blink / smile / sad), the toon ramp and outline, the sphere maps'
view dependence, the 20 generic back-up dancers (`z_b[fm]_body` + `z_b[fm]_head*`; maintainer
decision 2026-11-01).

Inputs (environment): UNIVERSE_DIR (the extracted x_data folder, default
~/Desktop/ddr_universe/extracted; scripts/xdvdfs_extract.py then
`scripts/extract_ultramix_data.py universe_us`), DANCERS (default all seven:
emi,rage,lady,afro,honey,b,queen -- TYPE column values; `emi:2` = one costume), OUT_BASE (default
<repo>/data_mods/custom_models/dancers), SOURCE (default `UNIVERSE`), GLASS_ALPHA (default 0.5),
PREVIEW (1 = also render Workbench previews of the re-imported export into PREVIEW_DIR, default
the system temp dir -- never into the repo).
Run: /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
       --python tools/blender_ddr_addon/examples/port_character_universe.py
"""
import csv
import os
import sys
import tempfile

import bpy
import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import port_character_ultramix as U1  # noqa: E402  (the DSU1 port: rig builder, aliases, preview)
import port_character_ultramix3 as U3  # noqa: E402  (clip check, floor probe, genders)
from port_character_ultramix import A, K, P, U, convert, export_character  # noqa: E402

UNIVERSE_DIR = os.path.expanduser(os.environ.get('UNIVERSE_DIR', '~/Desktop/ddr_universe/extracted'))
OUT_BASE = U1.OUT_BASE
SOURCE = os.environ.get('SOURCE', 'UNIVERSE')
PREVIEW = U1.PREVIEW
PREVIEW_DIR = os.environ.get('PREVIEW_DIR') or os.path.join(tempfile.gettempdir(), 'universe_port_previews')
GLASS_ALPHA = float(os.environ.get('GLASS_ALPHA', '0.5'))

# TYPE -> label stem (the folder is `<stem> <costume>`, or just `<stem>` with one costume).
DANCERS = {
    'emi': 'Emi',
    'rage': 'Rage',
    'lady': 'Lady',
    'afro': 'Afro',
    'honey': 'Honey',
    'b': 'B',
    'queen': 'Queen',
}
GENDERS = U3.GENDERS
ROLE_ALIASES = {'LeftToeBase': 'leftFootIndex1', 'RightToeBase': 'rightFootIndex1'}
# Cells of the costume CSVs that misspell the `.ddm` material they mean.
CSV_TYPOS = {'shoessshader': 'shoesshader'}
PLAY_FROM = 0
SHADER = U1.SHADER
MAX_POSED_BONES = 256  # src/services/scene3d/frame_board.rs MAX_BONES


def rip_path(name):
    """`name` in the rip, matched case-insensitively (the CSVs and the TOC disagree on case:
    `Z_F_hiphop01.ani` is `Z_F_hipHop01.ani`, `z_emi_rig_a.ddm` is `Z_Emi_Rig_A.ddm`)."""
    if not _RIP_FILES:
        _RIP_FILES.update({f.lower(): f for f in os.listdir(UNIVERSE_DIR)})
    return os.path.join(UNIVERSE_DIR, _RIP_FILES[name.lower()])


_RIP_FILES = {}


def texture_file(stem):
    """The rip's texture for `stem`, or None when the disc does not carry it."""
    rip_path('animations.csv')  # fills the index
    return next((rip_path(stem + ext) for ext in ('.dds', '.tga', '.bmp')
                 if (stem + ext).lower() in _RIP_FILES), None)


def csv_rows(name):
    with open(rip_path(name), newline='', encoding='latin1') as f:
        return [{(k or '').strip(): (v or '').strip() for k, v in r.items()} for r in csv.DictReader(f)]


def dancer_row(kind):
    for r in csv_rows('z_default_models.csv'):
        if r['TYPE'] == kind:
            return r
    raise KeyError('z_default_models.csv has no TYPE %r' % kind)


def clip_pool(row):
    column = GENDERS[row['GENDER']][2]
    groups = {row['GENDER'], 'unisex'}
    clips = [r[column] for r in csv_rows('animations.csv') if r['GROUP'] in groups and r[column]]
    return sorted(clips), sorted(groups)


def costumes(row):
    """[(costume number, model file, {csv shader name (lower): (texture, render)})], costumes that
    repeat an earlier one's model and table dropped (Queen)."""
    with open(rip_path(row['COSTUME'] + '.csv'), newline='', encoding='latin1') as f:
        rows = [[c.strip() for c in r] for r in csv.reader(f)]
    head = rows[0]
    assert rows[1][0] == '<RIG/PLATE>', rows[1]
    out, seen = [], []
    for n in range(1, 5):
        col = head.index('TEXTURES%d' % n)
        assert head[col - 1] == 'SHADERS%d' % n and head[col + 1] == 'RENDER%d' % n, head
        table = {}
        for r in rows[2:]:
            if len(r) > col + 1 and r[col - 1]:
                name = r[col - 1].lower()
                table[CSV_TYPOS.get(name, name)] = (r[col], r[col + 1])
        ident = (rows[1][col].lower(), sorted(table.items()))
        if ident in seen:
            continue
        seen.append(ident)
        out.append((n, rows[1][col], table))
    return out


def plan_slots(model, table, key):
    """Per `.ddm` material: (World slot index or None = dropped, render). World slots are the
    distinct (texture, translucent) pairs in first-use order. Returns (slots, plan, dropped) with
    slots = [(world stem, rip texture file, translucent)]."""
    prefix = key.replace('univ', 'uv', 1).replace('0', '', 1)  # univemi01 -> uvemi1
    slots, plan, dropped = [], [], []
    for mt in model['materials']:
        name = mt['name'].lower()
        cell = next((c for c in table if name == c or name.endswith('_' + c)), None)
        if cell is None:
            if not mt['texture'] or texture_file(mt['texture']) is None:
                plan.append((None, None))
                dropped.append('%s (%d tris, untextured)' % (mt['name'], mt['triangles']))
                continue
            # Emi 4's buttons: no CSV row, but a texture of their own on the disc
            print('TEXTURE %s: no costume CSV row -> the .ddm\'s %s' % (mt['name'], mt['texture']))
            tex, render = mt['texture'], 'toon'
        else:
            tex, render = table[cell]
        if not tex or render == 'hidden':
            plan.append((None, render))
            dropped.append('%s (hidden)' % mt['name'])
            continue
        src = texture_file(tex)
        if src is None:
            src = texture_file(mt['texture'])
            assert src is not None, (mt['name'], tex, mt['texture'])
            print('TEXTURE %s: %s is not on the disc -> the .ddm\'s %s' % (mt['name'], tex, mt['texture']))
            tex = mt['texture']
        rest = tex.lower()
        for p in ('z_', key[4:-2] + '_'):
            if rest.startswith(p):
                rest = rest[len(p):]
        slot = (prefix + '_' + rest, src, render == 'semitrans')
        if slot not in slots:
            slots.append(slot)
        plan.append((slots.index(slot), render))
    keys = [K.texture_registry_key(s) for s, _, _ in slots]
    assert len(set(keys)) == len({s for s, _, _ in slots}), 'texture stems collide: %r' % slots
    return slots, plan, dropped


def texture_png(stem, src):
    """The rip's DXT1 texture decoded to a temp PNG (the exporter then writes its usual A8R8G8B8 +
    mips DDS -- the proven World path)."""
    img = bpy.data.images.load(src)
    assert img.pixels[0] is not None  # images load lazily; touching the pixels decodes it
    out = os.path.join(tempfile.gettempdir(), 'universe_port_textures', stem + '.png')
    os.makedirs(os.path.dirname(out), exist_ok=True)
    img.filepath_raw = out
    img.file_format = 'PNG'
    img.save()
    bpy.data.images.remove(img)
    return out


def build_mesh(key, model, arm, slots, plan):
    """The costume's triangles only (unreferenced vertices dropped -- no vertex is shared between
    palette groups), one Blender material per World slot, sphere materials on matcap UVs,
    translucent slots blended with COLOR0 alpha GLASS_ALPHA."""
    pos, nrm, uv, weights, tris = U.game_mesh(model)
    uv = uv.copy()
    keep, slot_of_tri = [], []
    for mt, (slot, render) in zip(model['materials'], plan):
        rng = range(mt['first_index'] // 3, mt['first_index'] // 3 + mt['triangles'])
        if slot is None:
            continue
        if render == 'sphere':
            vs = np.unique(tris[list(rng)])
            uv[vs, 0] = 0.5 + 0.5 * nrm[vs, 0]
            uv[vs, 1] = 0.5 - 0.5 * nrm[vs, 1]
        keep.extend(rng)
        slot_of_tri.extend([slot] * len(rng))
    tris = tris[keep]
    used = np.unique(tris)
    remap = np.full(len(pos), -1, dtype=np.int64)
    remap[used] = np.arange(len(used))
    tris = remap[tris]
    pos, nrm, uv = pos[used], nrm[used], uv[used]
    weights = [weights[i] for i in used]

    me = bpy.data.meshes.new(key + '_body')
    me.from_pydata([tuple(convert.vec_to_blender(p)) for p in pos], [], tris.tolist())
    me.update()
    assert len(me.polygons) == len(tris), 'Blender dropped degenerate triangles'
    lay = me.uv_layers.new(name='UVMap')
    loops_v = np.zeros(len(me.loops), dtype=np.int64)
    me.loops.foreach_get('vertex_index', loops_v)
    luv = uv[loops_v].copy()
    luv[:, 1] = 1.0 - luv[:, 1]  # D3D v-down -> Blender v-up (the exporter flips back)
    lay.data.foreach_set('uv', luv.astype(np.float32).ravel())
    exact = me.attributes.new('ddr_normal', 'FLOAT_VECTOR', 'POINT')
    exact.data.foreach_set('vector', np.array([tuple(convert.vec_to_blender(n)) for n in nrm]).ravel())
    ob = bpy.data.objects.new(key + '_body', me)
    bpy.context.scene.collection.objects.link(ob)
    ob.parent = arm
    groups = {n: ob.vertex_groups.new(name=n) for n in arm['ddr_bone_order']}
    for vi, ws in enumerate(weights):
        for bone, w in ws:
            if w > 0:
                groups[bone].add([vi], w, 'REPLACE')
    mod = ob.modifiers.new('Armature', 'ARMATURE')
    mod.object = arm
    colour = P.white_color_attribute(ob)
    for stem, src, translucent in slots:
        mat = P.make_material(key + '_' + stem, P.load_texture(stem, texture_png(stem, src)),
                              two_sided=False, shader=SHADER)
        if translucent:
            mat.surface_render_method = 'BLENDED'
        me.materials.append(mat)
    slot_of_tri = np.array(slot_of_tri, dtype=np.int32)
    me.polygons.foreach_set('material_index', slot_of_tri)
    glass = [i for i, (_, _, translucent) in enumerate(slots) if translucent]
    if glass:
        loop_tri = np.repeat(np.arange(len(tris)), 3)  # from_pydata keeps 3 loops per triangle in order
        rgba = np.ones((len(me.loops), 4), dtype=np.float32)
        rgba[np.isin(slot_of_tri[loop_tri], glass), 3] = GLASS_ALPHA  # alpha is stored linearly
        colour.data.foreach_set('color', rgba.ravel())
    me.update()
    return ob, len(tris), len(used)


def port(kind, only=None):
    stem = DANCERS[kind]
    row = dancer_row(kind)
    sex, shadow, _ = GENDERS[row['GENDER']]
    clips, groups = clip_pool(row)
    sets = costumes(row)
    for number, model_file, table in sets:
        if only is not None and number != only:
            continue
        label = stem if len(sets) == 1 else '%s %d' % (stem, number)
        key = 'univ%s%02d' % (kind, number)
        port_costume(kind, row, label, key, number, model_file, table, sex, shadow, clips, groups)


def port_costume(kind, row, label, key, number, model_file, table, sex, shadow, clips, groups):
    model = U.parse_ddm(open(rip_path(model_file), 'rb').read())
    assert model.get('revision') == 4, model_file
    slots, plan, dropped = plan_slots(model, table, key)
    sphere = [mt['name'] for mt, (s, r) in zip(model['materials'], plan) if s is not None and r == 'sphere']
    P.fresh_scene()
    names = U.hierarchy_order([b['name'] for b in model['bones']], model['hierarchy'])
    binds = U.game_bind_matrices(model)
    arm = U1.build_armature(key, names, binds, model['hierarchy'])
    _, n_tris, n_verts = build_mesh(key, model, arm, slots, plan)
    bpy.context.view_layer.update()
    print('RIG %s (%s costume %d, %s, %s): %d bones, Hips at %.3f m; %d of %d materials -> %d slots %s; '
          'dropped %s; sphere %s; %d tris, %d verts; groups %s -> %d clips' % (
              key, row['NAME'], number, row['GENDER'], model_file, len(names), binds['Hips'][3, 1],
              len(model['materials']) - len(dropped), len(model['materials']), len(slots),
              [s + (' (blended)' if t else '') for s, _, t in slots], dropped, sphere, n_tris, n_verts,
              groups, len(clips)))

    out_dir = os.path.join(OUT_BASE, SOURCE, label)
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

    m, file_names, parents, file_binds = U1.exported_rig(body_dir, body, ROLE_ALIASES)
    assert sorted(file_names) == sorted(names), (file_names, names)
    idents = [b['identity'] for b in m['bones']]
    assert len(set(idents)) == len(idents), 'bone identity collision'
    assert all(len(me['palette']) <= 52 for me in m['meshes']), [len(me['palette']) for me in m['meshes']]
    assert len(file_names) <= MAX_POSED_BONES, 'more posed bones than the frame board holds'
    assert K.write_model(K.model_to_spec(m)) == open(os.path.join(body_dir, body + '.model'), 'rb').read()
    aliases = U1.add_role_aliases(os.path.join(body_dir, body + '.b2it'), file_names, ROLE_ALIASES)
    assert set(aliases) == set(ROLE_ALIASES), aliases
    for role in ('Hips', 'Spine2', 'Head'):
        assert role in file_names, role
    print('B2IT role aliases', aliases, 'meshes', len(m['meshes']), 'palettes', [len(me['palette']) for me in m['meshes']])

    low = []
    for name in clips:
        path = rip_path(name)
        clip_stem = os.path.splitext(os.path.basename(path))[0]
        clip = U.parse_ani(open(path, 'rb').read())
        spec, _ = U.ani_to_anm_spec(model, clip, file_names, parents, file_binds, loop_in=PLAY_FROM,
                                    frame_step=U.FRAME_STEP_UMX4)
        data = A.write_anm(spec)
        err, n = U3.check_clip(model, clip, file_names, parents, file_binds, data, frame_step=U.FRAME_STEP_UMX4)
        assert err < 1e-3, '%s: joint error %.5f m' % (name, err)
        open(os.path.join(motion_dir, clip_stem + '.anm'), 'wb').write(data)
        low.append(U3.lowest_point(model, clip))
        print('CLIP %-18s %4d keys -> %4d frames @60, %6d bytes, max joint err %.2e m, lowest %+.3f m' % (
            clip_stem, n, spec['frame_count'], len(data), err, low[-1]))
    print('FLOOR %s lowest skinned vertex over all clips: min %+.3f m, median %+.3f m' % (
        key, min(low), float(np.median(low))))

    sidecar = os.path.join(out_dir, 'chara_resources.rlist.txt')
    with open(sidecar, 'w') as f:
        f.write('# DanceDanceRevolution Universe (Xbox 360) "%s" costume %d, ported with its own rig and clips\n'
                % (row['NAME'], number))
        f.write('# (tools/blender_ddr_addon/examples/port_character_universe.py; %s clips, groups %s)\n'
                % (row['GENDER'], ', '.join(groups)))
        f.write('%s, pl, %s, A, 1.0, %s, 0.0\n' % (key, sex, export_character.fmt_num(shadow)))
    print('SIDECAR', sidecar)
    if PREVIEW:
        U1.PREVIEW_DIR = PREVIEW_DIR
        U1.preview(out_dir, key, os.path.basename(rip_path(clips[0])))


if __name__ == '__main__':
    for item in [s.strip() for s in os.environ.get('DANCERS', ','.join(DANCERS)).split(',') if s.strip()]:
        kind, _, number = item.partition(':')
        port(kind, int(number) if number else None)
    print('DONE')
