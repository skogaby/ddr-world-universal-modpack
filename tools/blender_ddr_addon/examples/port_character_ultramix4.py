"""EXAMPLE / SHIPPED PORT: the ten DDR ULTRAMIX 4 (Xbox, NTSC-U) dancers WITH THEIR OWN RIGS AND
THEIR OWN CLIPS -- the DSU recipe of port_character_ultramix.py / port_character_ultramix3.py,
adapted to UMX4's data (RE notes: docs/dancing_stage_unleashed_dancers_port_feasibility.md section 13).

What differs from DSU3, and what this script does about it:
  * a new `.ddm` revision (u32 0x112 at 0x04; ultramix_k3d_dump._parse_ddm4): 6..22 materials per
    dancer, each with its own bone palette, and 64-byte vertices with 4 influences. The bind space
    is already the clip space (Y-up, facing -Z), so no Z-up -> Y-up turn;
  * HumanIK-style joint names (`Hips`, `Spine2`, `leftUpLeg`, ...; ultramix_k3d_dump.HIERARCHY_UMX4).
    Every model carries its gender's whole skeleton (male 32 joints, female 57), so no helper bones
    are needed. World role bones: `Hips` / `Spine2` / `Head` exist; Left/RightToeBase alias
    `left/rightFootIndex1`;
  * a `.ddm` holds the geometry of ALL FOUR costumes. `<COSTUME>.csv` (SHADERS, TEXTURES1, RENDER1,
    ..., FACE) names each material's texture and render style per costume; a costume leaves a
    material's cell empty (or RENDER `hidden`) to hide it. The port builds COSTUME 1: those
    materials' triangles are dropped. Materials sharing a texture are merged into one World
    material (`umx4<texture>`, the rip's `x_` dropped). RENDER `toon` / `specular` /
    `specularTrans` become `mdl_ch_constant_vc` like every Ultramix port; `sphere` materials (chrome
    env maps: robo's pipes, the glasses) get rest-pose matcap UVs from their normals
    (u = 0.5 + 0.5 n.x, v = 0.5 - 0.5 n.y seen from the front) so the env texture reads as metal;
  * the dancer table is `x_default_models.csv` (TYPE, NAME, HIDDEN, COSTUME, GENDER; the model file
    is the costume CSV's `<RIG/PLATE>` row); `animations.csv` is DSU3's (NUM column added). The clip
    pool is the gender's group plus `unisex`, played whole from frame 0;
  * the clips are 15 Hz, not DSU's 30 Hz: 26 of the 34 are 4:1 decimations of World's own 120 BPM
    takes (root-height correlation >= 0.85 at 4x, <= 0.75 at 2x; RE note section 13), so the keys
    land on every 4th frame of the 60 fps timeline (U.FRAME_STEP_UMX4). A 300-key clip is ~20 s.
Not ported: the costumes 2-4, the FACE blink / smile / wink swaps, the toon ramp and outline, the
sphere maps' view dependence, `specularTrans` translucency (one sleeve on Charmy).

Ships into ONE source folder with the earlier Ultramix ports: `data_mods/custom_models/dancers/
ULTRAMIX 1-4/<label>/` (returning characters are numbered after their UMX1-3 models).

Inputs (environment): UMX4_DIR (the extracted x_data folder, default
~/Desktop/DDR ISOs/ultramix_4/extracted_full; `scripts/extract_ultramix_data.py ultramix4_us`),
DANCERS (default all ten: emi,rage,lady,afro,honey,b,charmy,astro,zukin,robo -- TYPE column values),
OUT_BASE (default <repo>/data_mods/custom_models/dancers), SOURCE (default `ULTRAMIX 1-4`), PREVIEW
(1 = also render Workbench previews of the re-imported export into PREVIEW_DIR, default the system
temp dir -- never into the repo).
Run: /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
       --python tools/blender_ddr_addon/examples/port_character_ultramix4.py
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
import port_character_ultramix3 as U3  # noqa: E402  (clip check, floor probe)
from port_character_ultramix import A, K, P, U, convert, export_character  # noqa: E402

UMX4_DIR = os.path.expanduser(os.environ.get('UMX4_DIR', '~/Desktop/DDR ISOs/ultramix_4/extracted_full'))
OUT_BASE = U1.OUT_BASE
SOURCE = os.environ.get('SOURCE', 'ULTRAMIX 1-4')
PREVIEW = U1.PREVIEW

# UMX4 TYPE -> (folder / row label (<= 15 bytes), key). The TYPE ids are recycled slots: `emi` is
# Yuni, `rage` Akira and `afro` Boldo -- new characters. Lady, Honey, B', Zukin and Robo return as
# NEW models (new meshes, rigs and textures), numbered after their UMX1-3 ports in the same source.
DANCERS = {
    'emi': ('Yuni', 'umx4yuni00'),
    'rage': ('Akira', 'umx4akira00'),
    'lady': ('Lady 4', 'umx4lady00'),
    'afro': ('Boldo', 'umx4boldo00'),
    'honey': ('Honey 2', 'umx4honey00'),
    'b': ('B 2', 'umx4b00'),
    'charmy': ('Charmy', 'umx4charmy00'),
    'astro': ('Astro', 'umx4astro00'),
    'zukin': ('Maid-Zukin 3', 'umx4zukin00'),
    'robo': ('Konsento 2', 'umx4robo00'),
}
GENDERS = U3.GENDERS
ROLE_ALIASES = {'LeftToeBase': 'leftFootIndex1', 'RightToeBase': 'rightFootIndex1'}
PLAY_FROM = 0
SHADER = U1.SHADER
COSTUME = '1'


def rip_path(name):
    """`name` in the rip, matched case-insensitively (the CSVs and the TOC disagree on case:
    `X_F_hiphop01.ani` is `X_F_hipHop01.ani`, `x_akira.ddm` is `x_Akira.ddm`)."""
    if not _RIP_FILES:
        _RIP_FILES.update({f.lower(): f for f in os.listdir(UMX4_DIR)})
    return os.path.join(UMX4_DIR, _RIP_FILES[name.lower()])


_RIP_FILES = {}


def csv_rows(name):
    with open(rip_path(name), newline='', encoding='latin1') as f:
        return [{(k or '').strip(): (v or '').strip() for k, v in r.items()} for r in csv.DictReader(f)]


def dancer_row(kind):
    for r in csv_rows('x_default_models.csv'):
        if r['TYPE'] == kind:
            return r
    raise KeyError('x_default_models.csv has no TYPE %r' % kind)


def clip_pool(row):
    column = GENDERS[row['GENDER']][2]
    groups = {row['GENDER'], 'unisex'}
    clips = [r[column] for r in csv_rows('animations.csv') if r['GROUP'] in groups and r[column]]
    return sorted(clips), sorted(groups)


def costume(row):
    """(model file, {material name: (texture stem, render)}) for COSTUME."""
    rows = csv_rows(row['COSTUME'] + '.csv')
    assert rows[0]['SHADERS'] == '<RIG/PLATE>', rows[0]
    table = {r['SHADERS']: (r['TEXTURES' + COSTUME], r['RENDER' + COSTUME]) for r in rows[1:]}
    return rows[0]['TEXTURES' + COSTUME], table


def texture_source(stem):
    """The rip's DXT1 `.dds` as a temp PNG (the exporter then writes its usual A8R8G8B8 + mips)."""
    img = bpy.data.images.load(rip_path(stem + '.dds'))
    assert img.pixels[0] is not None  # images load lazily; touching the pixels decodes it
    out = os.path.join(tempfile.gettempdir(), 'ultramix4_port_textures', stem + '.png')
    os.makedirs(os.path.dirname(out), exist_ok=True)
    img.filepath_raw = out
    img.file_format = 'PNG'
    img.save()
    bpy.data.images.remove(img)
    return out


def world_stem(stem):
    return 'umx4' + (stem[2:] if stem.lower().startswith('x_') else stem)


def plan_slots(model, table):
    """Per `.ddm` material: (World slot index or None = hidden, render). World slots are the
    distinct COSTUME textures in first-use order."""
    stems, plan = [], []
    for mt in model['materials']:
        if mt['name'] not in table:
            raise KeyError('costume CSV has no row for material %r' % mt['name'])
        tex, render = table[mt['name']]
        if not tex or render == 'hidden':
            plan.append((None, render))
            continue
        if tex not in stems:
            stems.append(tex)
        plan.append((stems.index(tex), render))
    return stems, plan


def build_mesh(key, model, arm, stems, plan):
    """The costume's triangles only (unreferenced vertices dropped -- UMX4 shares no vertex between
    materials), one Blender material per World slot, sphere materials on matcap UVs."""
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
    P.white_color_attribute(ob)
    for stem in stems:
        me.materials.append(P.make_material(key + '_' + stem, P.load_texture(world_stem(stem), texture_source(stem)),
                                            two_sided=False, shader=SHADER))
    me.polygons.foreach_set('material_index', np.array(slot_of_tri, dtype=np.int32))
    me.update()
    return ob, len(tris), len(used)


def port(kind):
    label, key = DANCERS[kind]
    row = dancer_row(kind)
    sex, shadow, _ = GENDERS[row['GENDER']]
    clips, groups = clip_pool(row)
    model_file, table = costume(row)
    model = U.parse_ddm(open(rip_path(model_file), 'rb').read())
    assert model.get('revision') == 4, model_file
    stems, plan = plan_slots(model, table)
    hidden = [mt['name'] for mt, (s, _) in zip(model['materials'], plan) if s is None]
    sphere = [mt['name'] for mt, (s, r) in zip(model['materials'], plan) if s is not None and r == 'sphere']
    P.fresh_scene()
    names = U.hierarchy_order([b['name'] for b in model['bones']], model['hierarchy'])
    binds = U.game_bind_matrices(model)
    arm = U1.build_armature(key, names, binds, model['hierarchy'])
    _, n_tris, n_verts = build_mesh(key, model, arm, stems, plan)
    bpy.context.view_layer.update()
    print('RIG %s (%s, %s, %s): %d bones; %d of %d materials -> %d textures %s; hidden %s; sphere %s; '
          '%d tris, %d verts; groups %s -> %d clips' % (
              key, row['NAME'], row['GENDER'], model_file, len(names), len(model['materials']) - len(hidden),
              len(model['materials']), len(stems), stems, hidden, sphere, n_tris, n_verts, groups, len(clips)))

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
    assert len(m['meshes']) == len(stems), 'one KTMDL mesh per World texture'
    assert len(file_names) <= 64, 'more posed bones than the frame board holds'
    assert K.write_model(K.model_to_spec(m)) == open(os.path.join(body_dir, body + '.model'), 'rb').read()
    aliases = U1.add_role_aliases(os.path.join(body_dir, body + '.b2it'), file_names, ROLE_ALIASES)
    assert set(aliases) == set(ROLE_ALIASES), aliases
    for role in ('Hips', 'Spine2', 'Head'):
        assert role in file_names, role
    print('B2IT role aliases', aliases, 'palettes', [len(me['palette']) for me in m['meshes']])

    low = []
    for name in clips:
        path = rip_path(name)
        stem = os.path.splitext(os.path.basename(path))[0]
        clip = U.parse_ani(open(path, 'rb').read())
        spec, _ = U.ani_to_anm_spec(model, clip, file_names, parents, file_binds, loop_in=PLAY_FROM,
                                    frame_step=U.FRAME_STEP_UMX4)
        data = A.write_anm(spec)
        err, n = U3.check_clip(model, clip, file_names, parents, file_binds, data, frame_step=U.FRAME_STEP_UMX4)
        assert err < 1e-3, '%s: joint error %.5f m' % (name, err)
        open(os.path.join(motion_dir, stem + '.anm'), 'wb').write(data)
        low.append(U3.lowest_point(model, clip))
        print('CLIP %-18s %4d keys -> %4d frames @60, %6d bytes, max joint err %.2e m, lowest %+.3f m' % (
            stem, n, spec['frame_count'], len(data), err, low[-1]))
    print('FLOOR %s lowest skinned vertex over all clips: min %+.3f m, median %+.3f m' % (
        key, min(low), float(np.median(low))))

    sidecar = os.path.join(out_dir, 'chara_resources.rlist.txt')
    with open(sidecar, 'w') as f:
        f.write('# DDR ULTRAMIX 4 (Xbox) "%s", ported with its own rig and clips\n' % row['NAME'])
        f.write('# (tools/blender_ddr_addon/examples/port_character_ultramix4.py; costume %s, %s clips, groups %s)\n'
                % (COSTUME, row['GENDER'], ', '.join(groups)))
        f.write('%s, pl, %s, A, 1.0, %s, 0.0\n' % (key, sex, export_character.fmt_num(shadow)))
    print('SIDECAR', sidecar)
    if PREVIEW:
        U1.preview(out_dir, key, os.path.basename(rip_path(clips[0])))


if __name__ == '__main__':
    for d in [s.strip() for s in os.environ.get('DANCERS', ','.join(DANCERS)).split(',') if s.strip()]:
        port(d)
    print('DONE')
