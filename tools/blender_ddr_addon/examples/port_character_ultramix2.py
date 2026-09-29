"""EXAMPLE / SHIPPED PORT: the six DDR ULTRAMIX 2 / Dancing Stage Unleashed 2 (Xbox) dancers WITH
THEIR OWN RIGS AND THEIR OWN CLIPS -- the DSU1 path of port_character_ultramix.py (design
.agents/planning/2026-09-28-dsu-dancer-port/design.md), adapted to DSU2's data (RE notes:
docs/dancing_stage_unleashed_dancers_port_feasibility.md section 11).

What differs from DSU1, and what this script does about it:
  * the dancer table is data, not code: `default_model.csv` (TYPE, NAME, MODEL DATA, PLATE TEXTURE,
    NORMAL/BLINK P1..P4, PREFER ANIM GROUP, ANIM GROUP 2, ANIM GROUP 3). The body texture is the
    row's NORMAL P1 (the `.ddm`'s own texture field is a stale tool leftover); the other costumes and
    the blink textures are not ported (one texture per dancer key in World);
  * `animations.csv` has no DANCER column -- rows carry a GROUP (male / woman / girl / unisex) and a
    dancer draws from its three groups (DSU2 FUN_0007d0a0: rand % 3, then a random clip of that
    group). The dancer's motion pool is the UNION of its groups (World shuffles it uniformly);
  * DSU2 plays a clip whole, frame 0 .. n-1 (FUN_000af2a0), so clips convert with loop_in = 0;
  * every model omits some ancestor joints (afro has no `root`, robo has no `Sternum`/`Clav_*1`,
    ...). All six bind skeletons are the same rig, and every clip carries every track, so the
    missing joints (plus `Toe_L`/`Toe_R`, the floor-shadow role bones) are added as UNWEIGHTED
    helper bones, their binds taken from a sibling model that has them.

Inputs (environment): DSU2_DIR (the extracted x_data folder, default ~/Desktop/dsu2/extracted_full),
DANCERS (default all six: afro,lady,emi,rage,robo,maid -- TYPE column values), OUT_BASE (default
<repo>/data_mods/custom_models/dancers), PREVIEW (1 = also render Workbench previews of the
re-imported export into PREVIEW_DIR, default the system temp dir -- never into the repo).
Run: /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
       --python tools/blender_ddr_addon/examples/port_character_ultramix2.py
"""
import copy
import csv
import os
import sys
import tempfile

import bpy
import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import port_character_ultramix as U1  # noqa: E402  (the DSU1 port: rig/mesh builders, aliases, preview)
from port_character_ultramix import A, K, P, U, export_character  # noqa: E402

DSU2_DIR = os.path.expanduser(os.environ.get('DSU2_DIR', '~/Desktop/dsu2/extracted_full'))
OUT_BASE = U1.OUT_BASE
PREVIEW = U1.PREVIEW

# DSU2 TYPE -> (folder / row label (<= 15 chars), key, sidecar sex, shadow scale, body texture stem)
DANCERS = {
    'afro': ('UMX2 Afro', 'umx2afro00', 'M', 0.8, 'umx2afro_al'),
    'lady': ('UMX2 Lady', 'umx2lady00', 'F', 0.75, 'umx2lady_al'),
    'emi': ('UMX2 Emi', 'umx2emi00', 'F', 0.75, 'umx2emi_al'),
    'rage': ('UMX2 Rage', 'umx2rage00', 'M', 0.8, 'umx2rage_al'),
    'robo': ('UMX2 Konsento03', 'umx2robo00', 'M', 0.8, 'umx2robo_al'),
    'maid': ('UMX2 Maid-Zukin', 'umx2maid00', 'F', 0.75, 'umx2maid_al'),
}
# Joints every ported rig must have besides the ancestors of its own bones: the World role bones'
# DSU stand-ins (port_character_ultramix.ROLE_ALIASES) that are animated by every DSU2 clip.
ROLE_JOINTS = ('root', 'Sternum', 'Toe_L', 'Toe_R', 'Head')
PLAY_FROM = 0  # DSU2 plays [0, n-1]; DSU1 played [15, n-15]


def csv_rows(name):
    with open(os.path.join(DSU2_DIR, name), newline='', encoding='latin1') as f:
        return [{k.strip(): (v or '').strip() for k, v in r.items()} for r in csv.DictReader(f)]


def dancer_row(kind):
    for r in csv_rows('default_model.csv'):
        if r['TYPE'] == kind:
            return r
    raise KeyError('default_model.csv has no TYPE %r' % kind)


def clip_pool(row):
    groups = {row['PREFER ANIM GROUP'], row['ANIM GROUP 2'], row['ANIM GROUP 3']} - {''}
    return sorted(r['NAME'] for r in csv_rows('animations.csv') if r['GROUP'] in groups), sorted(groups)


def all_models():
    return {r['TYPE']: U.parse_ddm(open(os.path.join(DSU2_DIR, r['MODEL DATA']), 'rb').read())
            for r in csv_rows('default_model.csv')}


def complete_rig(model, siblings):
    """`model` plus unweighted helper bones for every missing ancestor / role joint, each bind
    (inverse_bind, in the model's own DSU bind space) copied from the first sibling that has it."""
    have = {b['name'] for b in model['bones']}
    need = set(ROLE_JOINTS)
    for n in have:
        p = U.HIERARCHY[n]
        while p is not None:
            need.add(p)
            p = U.HIERARCHY[p]
    out = copy.copy(model)
    out['bones'] = list(model['bones'])
    added = []
    for n in sorted(need - have):
        donor = next((b for s in siblings for b in s['bones'] if b['name'] == n), None)
        if donor is None:
            raise KeyError('no DSU2 model carries joint %r' % n)
        # vs_register only keys the vertex -> bone map; a helper has no vertices.
        out['bones'].append(dict(index=len(out['bones']), name=n, inverse_bind=donor['inverse_bind'],
                                 vs_register=-1000 - len(added)))
        added.append(n)
    return out, added


def check_clip(model, clip, names, parents, binds, anm_bytes):
    worlds, frames = U.clip_game_worlds(model, clip, names, binds, loop_in=PLAY_FROM)
    parsed = A.parse_anm(anm_bytes)
    worst = 0.0
    for k in sorted({0, 1, len(frames) // 2, len(frames) - 1}):
        pose = A.evaluate_pose(parsed, 2 * k, parents)
        for i in range(len(names)):
            w = np.array(pose[i]['world'], dtype=float).reshape(4, 4)
            worst = max(worst, float(np.abs(w[3, :3] - worlds[k][i][3, :3]).max()))
    return worst, len(frames)


def texture_source(stem):
    """Decode the rip's DXT1 `.dds` (Blender reads DDS) to a temp PNG, so the exporter writes its
    usual A8R8G8B8 + mips DDS -- the proven World path -- instead of copying a mip-less DXT1."""
    img = bpy.data.images.load(os.path.join(DSU2_DIR, stem + '.dds'))
    assert img.pixels[0] is not None  # images load lazily; touching the pixels decodes it
    out = os.path.join(tempfile.gettempdir(), 'ultramix2_port_textures', stem + '.png')
    os.makedirs(os.path.dirname(out), exist_ok=True)
    img.filepath_raw = out
    img.file_format = 'PNG'
    img.save()
    bpy.data.images.remove(img)
    return out


def port(kind, models):
    label, key, sex, shadow, tex_stem = DANCERS[kind]
    row = dancer_row(kind)
    clips, groups = clip_pool(row)
    P.fresh_scene()
    model, added = complete_rig(models[kind], [m for k, m in models.items() if k != kind])
    names = U.hierarchy_order([b['name'] for b in model['bones']])
    binds = U.game_bind_matrices(model)
    arm = U1.build_armature(key, names, binds)
    U1.build_mesh(key, model, arm, tex_stem, tex_src=texture_source(row['NORMAL P1']))
    bpy.context.view_layer.update()
    print('RIG %s (%s): %d bones, helpers added %s; texture %s; groups %s -> %d clips' % (
        key, row['NAME'], len(names), added, row['NORMAL P1'], groups, len(clips)))

    out_dir = os.path.join(OUT_BASE, label)
    body = 'pl_' + key
    body_dir = os.path.join(out_dir, body)
    motion_dir = os.path.join(body_dir, 'motion')
    os.makedirs(motion_dir, exist_ok=True)
    for stale in os.listdir(motion_dir):
        if stale.endswith('.anm'):
            os.remove(os.path.join(motion_dir, stale))
    rep = export_character.export_character(out_dir, arm, key=key, write_textures=True, write_rlist=False)
    print('EXPORT', key, rep['body_spec'], [os.path.relpath(w, out_dir) for w in rep['written']])

    m, file_names, parents, file_binds = U1.exported_rig(body_dir, body)
    assert sorted(file_names) == sorted(names), (file_names, names)
    idents = [b['identity'] for b in m['bones']]
    assert len(set(idents)) == len(idents), 'bone identity collision'
    assert all(len(me['palette']) <= 52 for me in m['meshes'])
    assert len(file_names) <= 64, 'more posed bones than the frame board holds'
    assert K.write_model(K.model_to_spec(m)) == open(os.path.join(body_dir, body + '.model'), 'rb').read()
    aliases = U1.add_role_aliases(os.path.join(body_dir, body + '.b2it'), file_names)
    assert set(aliases) == set(U1.ROLE_ALIASES), aliases
    print('B2IT role aliases', aliases)

    for name in clips:
        stem = os.path.splitext(name)[0]
        clip = U.parse_ani(open(os.path.join(DSU2_DIR, name), 'rb').read())
        spec, _ = U.ani_to_anm_spec(model, clip, file_names, parents, file_binds, loop_in=PLAY_FROM)
        data = A.write_anm(spec)
        err, n = check_clip(model, clip, file_names, parents, file_binds, data)
        assert err < 1e-3, '%s: joint error %.5f m' % (name, err)
        open(os.path.join(motion_dir, stem + '.anm'), 'wb').write(data)
        print('CLIP %-18s %4d keys -> %4d frames @60, %6d bytes, max joint err %.2e m' % (
            stem, n, spec['frame_count'], len(data), err))

    sidecar = os.path.join(out_dir, 'chara_resources.rlist.txt')
    with open(sidecar, 'w') as f:
        f.write('# DDR ULTRAMIX 2 / Dancing Stage Unleashed 2 (Xbox) "%s", ported with its own rig and clips\n'
                % row['NAME'])
        f.write('# (tools/blender_ddr_addon/examples/port_character_ultramix2.py; anim groups %s)\n'
                % ', '.join(groups))
        f.write('%s, pl, %s, A, 1.0, %s, 0.0\n' % (key, sex, export_character.fmt_num(shadow)))
    print('SIDECAR', sidecar)
    if PREVIEW:
        U1.preview(out_dir, key, clips[0])


models = all_models()
for d in [s.strip() for s in os.environ.get('DANCERS', ','.join(DANCERS)).split(',') if s.strip()]:
    port(d, models)
print('DONE')
