"""EXAMPLE / SHIPPED PORT: the eight Dancing Stage Unleashed 3 (Xbox, PAL) dancers WITH THEIR OWN
RIGS AND THEIR OWN CLIPS -- the DSU2 recipe of port_character_ultramix2.py (design
.agents/planning/2026-09-28-dsu-dancer-port/design.md), adapted to DSU3's data (RE notes:
docs/dancing_stage_unleashed_dancers_port_feasibility.md section 12).

What differs from DSU2, and what this script does about it:
  * a new `.ddm` revision with 4 materials per dancer (cloth / face / pants / shoes, each a
    contiguous triangle range; ultramix_k3d_dump.parse_ddm reads both revisions). Each material
    becomes its own World material and texture. The `.ddm`'s texture names are stale tool
    leftovers (afro's slots name the `_b` costume); the textures are the COSTUME1 column of the
    dancer's `<COSTUME>.csv` (loader DSU3 FUN_00017db0), one `name:shader` row per material in
    slot order. The P2-P4 costumes and the `_eye` blink twins of the face texture are not ported;
  * a new skeleton with Maya-style joint names (`M_Root`, `L_Knee`, `R_knee`, ...;
    ultramix_k3d_dump.HIERARCHY) and no toe joints. The World role bones map to M_Root / M_Chest
    / M_Head / L_Ankle / R_Ankle through `.b2it` aliases;
  * two clip families: `M_*.ani` for GENDER male and `F_*.ani` for woman / girl.
    `animations.csv` is FEMALENAME, MALENAME, SPEED, SKIP, GROUP, and the dancer's own column is
    picked by gender (FUN_0015b490). DSU3 picks the gender's group or `unisex` 50/50 per clip;
    World plays the UNION of those two groups (uniform shuffle);
  * DSU3's dance set-up plays a clip from frame 0 (FUN_001f6040, start 0 from FUN_000bde70), and
    no caller trims the end, so clips convert with loop_in = 0, as in DSU2;
  * bind origins differ per model (e.g. rage and b sit 8.7 units below afro), so a missing
    ancestor joint is added as an UNWEIGHTED helper bone whose bind is copied from a sibling
    model with the same bind rotations, shifted by the two rigs' translation offset.

Inputs (environment): DSU3_DIR (the extracted x_data folder, default ~/Desktop/dsu3/extracted_full),
DANCERS (default all eight: afro,lady,emi,rage,robo,maid,b,hney -- TYPE column values), OUT_BASE
(default <repo>/data_mods/custom_models/dancers), PREVIEW (1 = also render Workbench previews of
the re-imported export into PREVIEW_DIR, default the system temp dir -- never into the repo).
Run: /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
       --python tools/blender_ddr_addon/examples/port_character_ultramix3.py
"""
import copy
import csv
import os
import re
import sys
import tempfile

import bpy
import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import port_character_ultramix as U1  # noqa: E402  (the DSU1 port: rig builder, aliases, preview)
from port_character_ultramix import A, K, P, U, export_character  # noqa: E402

DSU3_DIR = os.path.expanduser(os.environ.get('DSU3_DIR', '~/Desktop/dsu3/extracted_full'))
OUT_BASE = U1.OUT_BASE
PREVIEW = U1.PREVIEW

# DSU3 TYPE -> (folder / row label (<= 15 chars), key)
DANCERS = {
    'afro': ('UMX3 Afro', 'umx3afro00'),
    'lady': ('UMX3 Lady', 'umx3lady00'),
    'emi': ('UMX3 Emi', 'umx3emi00'),
    'rage': ('UMX3 Rage', 'umx3rage00'),
    'robo': ('UMX3 Konsento', 'umx3robo00'),
    'maid': ('UMX3 Maid-Zukin', 'umx3maid00'),
    'b': ('UMX3 B', 'umx3b00'),  # the game's NAME is "B'"; the apostrophe is kept out of the path
    'hney': ('UMX3 Honey', 'umx3honey00'),
}
# GENDER -> (sidecar sex, shadow scale, animations.csv column)
GENDERS = {
    'male': ('M', 0.8, 'MALENAME'),
    'woman': ('F', 0.75, 'FEMALENAME'),
    'girl': ('F', 0.75, 'FEMALENAME'),
}
# World role bone -> the DSU3 joint playing it (design D2). There are no toe joints; the ankles
# stand in (the shadow only uses their x/z).
ROLE_ALIASES = {'Hips': 'M_Root', 'Spine2': 'M_Chest', 'Head': 'M_Head',
                'LeftToeBase': 'L_Ankle', 'RightToeBase': 'R_Ankle'}
PLAY_FROM = 0  # DSU3 plays [0, n-1] like DSU2
SHADER = U1.SHADER
# A costume row's part (after the dancer prefix) must match the `.ddm` material slot's part; a
# face slot is named after its blink texture in some models (afro's `afro_eye_b`).
PART_ALIASES = {'eye': 'face'}


def csv_rows(name):
    with open(os.path.join(DSU3_DIR, name), newline='', encoding='latin1') as f:
        return [{k.strip(): (v or '').strip() for k, v in r.items()} for r in csv.DictReader(f)]


def dancer_row(kind):
    for r in csv_rows('default_model.csv'):
        if r['TYPE'] == kind:
            return r
    raise KeyError('default_model.csv has no TYPE %r' % kind)


def clip_pool(row):
    """The dancer's clips: its gender's column, rows of its gender's group or `unisex`."""
    column = GENDERS[row['GENDER']][2]
    groups = {row['GENDER'], 'unisex'}
    clips = [r[column] for r in csv_rows('animations.csv')
             if r['GROUP'] in groups and r[column] not in ('', '<empty>')]
    return sorted(clips), sorted(groups)


def slot_part(texture_name):
    """'E:MAYA_...<CR>rage_face' / 'afro_eye_b' / 'zukin_cloth' -> 'face' / 'face' / 'cloth'. Rage's
    slots carry a Maya source path whose `\\r` separator the tool turned into a real CR."""
    stem = re.split(r'[\\/\r]', texture_name)[-1]
    part = stem.split('_')[1] if '_' in stem else stem
    return PART_ALIASES.get(part, part)


def costume_textures(row, model):
    """COSTUME1 texture stem per `.ddm` material slot, checked against the slot names."""
    rows = csv_rows(row['COSTUME'] + '.csv')
    stems = [r['COSTUME1'].split(':')[0] for r in rows]
    assert len(stems) == len(model['materials']), (row['TYPE'], stems, model['materials'])
    for stem, mt in zip(stems, model['materials']):
        assert slot_part(stem) == slot_part(mt['texture']), (stem, mt['texture'])
    return stems


def all_models():
    return {r['TYPE']: U.parse_ddm(open(os.path.join(DSU3_DIR, r['MODEL DATA']), 'rb').read())
            for r in csv_rows('default_model.csv')}


def binds_of(model):
    return {b['name']: np.linalg.inv(np.array(b['inverse_bind']).reshape(4, 4)) for b in model['bones']}


def complete_rig(model, siblings):
    """`model` plus unweighted helper bones for every missing ancestor / role joint. A helper's
    bind comes from the first sibling whose shared bones have the same bind rotations (<= 1e-3)
    and one common translation offset (residual <= 1e-2 units), shifted by that offset."""
    have = {b['name'] for b in model['bones']}
    need = set(ROLE_ALIASES.values())
    for n in have:
        p = U.HIERARCHY[n]
        while p is not None:
            need.add(p)
            p = U.HIERARCHY[p]
    mine = binds_of(model)
    out = copy.copy(model)
    out['bones'] = list(model['bones'])
    added = []
    for n in sorted(need - have):
        bind = None
        for s in siblings:
            theirs = binds_of(s)
            shared = sorted(have & set(theirs))
            if n not in theirs or not shared:
                continue
            if max(np.abs(mine[k][:3, :3] - theirs[k][:3, :3]).max() for k in shared) > 1e-3:
                continue
            offs = np.array([mine[k][3, :3] - theirs[k][3, :3] for k in shared])
            if np.abs(offs - offs.mean(0)).max() > 1e-2:
                continue
            bind = theirs[n].copy()
            bind[3, :3] += offs.mean(0)
            break
        if bind is None:
            raise KeyError('no DSU3 model with a matching bind carries joint %r' % n)
        # vs_register only keys the vertex -> bone map; a helper has no vertices.
        out['bones'].append(dict(index=len(out['bones']), name=n,
                                 inverse_bind=tuple(np.linalg.inv(bind).reshape(16)),
                                 vs_register=-1000 - len(added)))
        added.append(n)
    return out, added


def texture_source(stem):
    """Decode the rip's DXT1 `.dds` (Blender reads DDS) to a temp PNG, so the exporter writes its
    usual A8R8G8B8 + mips DDS -- the proven World path -- instead of copying a mip-less DXT1."""
    img = bpy.data.images.load(os.path.join(DSU3_DIR, stem + '.dds'))
    assert img.pixels[0] is not None  # images load lazily; touching the pixels decodes it
    out = os.path.join(tempfile.gettempdir(), 'ultramix3_port_textures', stem + '.png')
    os.makedirs(os.path.dirname(out), exist_ok=True)
    img.filepath_raw = out
    img.file_format = 'PNG'
    img.save()
    bpy.data.images.remove(img)
    return out


def build_mesh(key, model, arm, tex_stems):
    """U1.build_mesh with one World material per `.ddm` material: triangle i becomes polygon i, so
    each material's triangle range sets its polygons' material_index. World texture stems are the
    rip's names prefixed `umx3` (unique next to the DSU1/DSU2 ports)."""
    world = ['umx3' + s for s in tex_stems]
    ob = U1.build_mesh(key, model, arm, world[0], tex_src=texture_source(tex_stems[0]))
    me = ob.data
    me.materials.clear()
    for stem, world_stem in zip(tex_stems, world):
        me.materials.append(P.make_material(key + '_' + stem, P.load_texture(world_stem, texture_source(stem)),
                                            two_sided=False, shader=SHADER))
    idx = np.zeros(len(me.polygons), dtype=np.int32)
    for slot, mt in enumerate(model['materials']):
        first = mt['first_index'] // 3
        idx[first:first + mt['triangles']] = slot
    me.polygons.foreach_set('material_index', idx)
    me.update()
    return ob


def check_clip(model, clip, names, parents, binds, anm_bytes, frame_step=U.FRAME_STEP_DSU):
    worlds, frames = U.clip_game_worlds(model, clip, names, binds, loop_in=PLAY_FROM)
    parsed = A.parse_anm(anm_bytes)
    worst = 0.0
    for k in sorted({0, 1, len(frames) // 2, len(frames) - 1}):
        pose = A.evaluate_pose(parsed, frame_step * k, parents)
        for i in range(len(names)):
            w = np.array(pose[i]['world'], dtype=float).reshape(4, 4)
            worst = max(worst, float(np.abs(w[3, :3] - worlds[k][i][3, :3]).max()))
    return worst, len(frames)


def lowest_point(model, clip):
    """Lowest skinned vertex over the clip, metres above World's floor (DSU's floor is y = 0)."""
    return min(float(U.skin_pose(model, clip, f)[0][:, 1].min())
               for f in range(0, clip['frame_count'], 10)) * U.GAME_SCALE


def port(kind, models):
    label, key = DANCERS[kind]
    row = dancer_row(kind)
    sex, shadow, _ = GENDERS[row['GENDER']]
    clips, groups = clip_pool(row)
    P.fresh_scene()
    same_gender = [models[k] for k in models if k != kind and dancer_row(k)['GENDER'] == row['GENDER']]
    others = [models[k] for k in models if k != kind and dancer_row(k)['GENDER'] != row['GENDER']]
    model, added = complete_rig(models[kind], same_gender + others)
    tex_stems = costume_textures(row, model)
    names = U.hierarchy_order([b['name'] for b in model['bones']])
    binds = U.game_bind_matrices(model)
    arm = U1.build_armature(key, names, binds)
    build_mesh(key, model, arm, tex_stems)
    bpy.context.view_layer.update()
    print('RIG %s (%s, %s): %d bones, helpers added %s; textures %s; groups %s -> %d clips' % (
        key, row['NAME'], row['GENDER'], len(names), added, tex_stems, groups, len(clips)))

    out_dir = os.path.join(OUT_BASE, label)
    body = 'pl_' + key
    body_dir = os.path.join(out_dir, body)
    motion_dir = os.path.join(body_dir, 'motion')
    os.makedirs(motion_dir, exist_ok=True)
    for stale in os.listdir(body_dir):
        if stale.endswith('.dds'):
            os.remove(os.path.join(body_dir, stale))
    for stale in os.listdir(motion_dir):
        if stale.endswith('.anm'):
            os.remove(os.path.join(motion_dir, stale))
    rep = export_character.export_character(out_dir, arm, key=key, write_textures=True, write_rlist=False)
    print('EXPORT', key, rep['body_spec'], [os.path.relpath(w, out_dir) for w in rep['written']])

    m, file_names, parents, file_binds = U1.exported_rig(body_dir, body, ROLE_ALIASES)
    assert sorted(file_names) == sorted(names), (file_names, names)
    idents = [b['identity'] for b in m['bones']]
    assert len(set(idents)) == len(idents), 'bone identity collision'
    assert all(len(me['palette']) <= 52 for me in m['meshes'])
    assert len(m['meshes']) == len(tex_stems), 'one KTMDL mesh per DSU3 material'
    assert len(file_names) <= 64, 'more posed bones than the frame board holds'
    assert K.write_model(K.model_to_spec(m)) == open(os.path.join(body_dir, body + '.model'), 'rb').read()
    aliases = U1.add_role_aliases(os.path.join(body_dir, body + '.b2it'), file_names, ROLE_ALIASES)
    assert set(aliases) == set(ROLE_ALIASES), aliases
    print('B2IT role aliases', aliases)

    low = []
    for name in clips:
        stem = os.path.splitext(name)[0]
        clip = U.parse_ani(open(os.path.join(DSU3_DIR, name), 'rb').read())
        spec, _ = U.ani_to_anm_spec(model, clip, file_names, parents, file_binds, loop_in=PLAY_FROM)
        data = A.write_anm(spec)
        err, n = check_clip(model, clip, file_names, parents, file_binds, data)
        assert err < 1e-3, '%s: joint error %.5f m' % (name, err)
        open(os.path.join(motion_dir, stem + '.anm'), 'wb').write(data)
        low.append(lowest_point(models[kind], clip))
        print('CLIP %-18s %4d keys -> %4d frames @60, %6d bytes, max joint err %.2e m, lowest %+.3f m' % (
            stem, n, spec['frame_count'], len(data), err, low[-1]))
    print('FLOOR %s lowest skinned vertex over all clips: min %+.3f m, median %+.3f m' % (
        key, min(low), float(np.median(low))))

    sidecar = os.path.join(out_dir, 'chara_resources.rlist.txt')
    with open(sidecar, 'w') as f:
        f.write('# Dancing Stage Unleashed 3 (Xbox) "%s", ported with its own rig and clips\n' % row['NAME'])
        f.write('# (tools/blender_ddr_addon/examples/port_character_ultramix3.py; %s clips, groups %s)\n'
                % (row['GENDER'], ', '.join(groups)))
        f.write('%s, pl, %s, A, 1.0, %s, 0.0\n' % (key, sex, export_character.fmt_num(shadow)))
    print('SIDECAR', sidecar)
    if PREVIEW:
        U1.preview(out_dir, key, clips[0])


if __name__ == '__main__':
    models = all_models()
    for d in [s.strip() for s in os.environ.get('DANCERS', ','.join(DANCERS)).split(',') if s.strip()]:
        port(d, models)
    print('DONE')
