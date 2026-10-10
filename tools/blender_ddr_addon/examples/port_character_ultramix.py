"""EXAMPLE / SHIPPED PORT: a DDR ULTRAMIX / Dancing Stage Unleashed (Xbox) dancer WITH ITS OWN RIG
AND ITS OWN CLIPS — no donor rig, no weight retarget, no pose conform (the Omnimix path, design
.agents/planning/2026-09-28-dsu-dancer-port/design.md; formats + RE in
docs/dancing_stage_unleashed_dancers_port_feasibility.md).

What it does, per dancer (afro / lady):
  1. reads the rip's `<dancer>.ddm` with scripts/ultramix_k3d_dump.py and rebuilds the K3D rig in
     Blender from its bind matrices (parent table: ultramix_k3d_dump.HIERARCHY), in World's game
     space (Y-up metres, facing +Z, 0.1026 m per DSU unit, rest soles on y = 0, Z-mirrored);
  2. builds the skinned mesh (2 influences, the file's own normals via `ddr_normal`, white COLOR0,
     `mdl_ch_constant_vc` like every shipped port) and exports it with the add-on;
  3. adds World ROLE-BONE ALIASES to the body `.b2it` (Hips / Spine2 / Left|RightToeBase -> the DSU
     joints; Head exists) — the DLL finds the shadow, Big Head and part bones by name there;
  4. converts every `animations.csv` clip of that dancer to `motion/<clip>.anm` against the EXPORTED
     bind frames (ultramix_k3d_dump.ani_to_anm_spec: DSU's played window [15, n-15], 30 Hz keys every
     2nd frame of a 60 fps clip), and checks each clip's joint positions against the DSU world
     transforms (< 1 mm);
  5. writes the `chara_resources.rlist.txt` sidecar.

The folder it writes is loaded by the Background Dancers mod as a custom dancer that plays its OWN
pool (`motion/`), instead of a sex pool.

Inputs (environment): DSU_DIR (the extracted x_data folder, default ~/Desktop/dancing_stage_unleashed/
extracted_full), DANCERS (default "afro,lady"), OUT_BASE (default <repo>/data_mods/custom_models/dancers),
PREVIEW (1 = also render Workbench previews of the re-imported export into PREVIEW_DIR, default the
system temp dir — never into the repo).
Run: /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
       --python tools/blender_ddr_addon/examples/port_character_ultramix.py
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
import ultramix_k3d_dump as U  # noqa: E402
from blender_ddr_addon import convert, export_character, import_anm, import_character  # noqa: E402
from blender_ddr_addon.codec import anm as A  # noqa: E402
from blender_ddr_addon.codec import ktmdl as K  # noqa: E402

DSU_DIR = os.path.expanduser(os.environ.get('DSU_DIR', '~/Desktop/dancing_stage_unleashed/extracted_full'))
OUT_BASE = os.environ.get('OUT_BASE', os.path.join(REPO, 'data_mods', 'custom_models', 'dancers'))
PREVIEW = os.environ.get('PREVIEW', '0') == '1'
PREVIEW_DIR = os.environ.get('PREVIEW_DIR') or os.path.join(tempfile.gettempdir(), 'ultramix_port_previews')

# dancer -> (folder / label, key, sidecar sex, shadow scale, body texture stem)
DANCERS = {
    'afro': ('Ultramix Afro', 'umxafro00', 'M', 0.8, 'umxafro_al'),
    'lady': ('Ultramix Lady', 'umxlady00', 'F', 0.75, 'umxlady_al'),
}
# World role bone -> the DSU joint playing it (design D2). `Head` is named alike.
ROLE_ALIASES = {'Hips': 'root', 'Spine2': 'Sternum', 'LeftToeBase': 'Toe_L', 'RightToeBase': 'Toe_R'}
SHADER = 'mdl_ch_constant_vc'


def clip_rows(dancer):
    with open(os.path.join(DSU_DIR, 'animations.csv'), newline='', encoding='latin1') as f:
        return [r['NAME'] for r in csv.DictReader(f) if r['DANCER'].strip() == dancer]


def build_armature(key, names, binds, hierarchy=None):
    """`hierarchy`: the parent table (default ultramix_k3d_dump.HIERARCHY; UMX4 passes the
    model's own)."""
    hierarchy = U.HIERARCHY if hierarchy is None else hierarchy
    arm_data = bpy.data.armatures.new(key + '_rig')
    arm = bpy.data.objects.new(key + '_Armature', arm_data)
    bpy.context.scene.collection.objects.link(arm)
    bpy.context.view_layer.objects.active = arm
    bpy.ops.object.mode_set(mode='EDIT')
    ebs = {}
    for n in names:
        eb = arm_data.edit_bones.new(n)
        eb.head = (0.0, 0.0, 0.0)
        eb.tail = (0.0, 0.04, 0.0)
        eb.matrix = convert.rowmat_to_blender([float(x) for x in binds[n].reshape(16)])
        ebs[n] = eb
    for n in names:
        p = hierarchy[n]
        if p is not None:
            ebs[n].parent = ebs[p]
    bpy.ops.object.mode_set(mode='OBJECT')
    arm['ddr_bone_order'] = list(names)
    arm['ddr_chara_key'] = key
    return arm


def build_mesh(key, model, arm, tex_stem, tex_src=None):
    """`tex_src`: the body texture image to load (default: the rip's `<ddm texture>.tga`)."""
    pos, nrm, uv, weights, tris = U.game_mesh(model)
    me = bpy.data.meshes.new(key + '_body')
    me.from_pydata([tuple(convert.vec_to_blender(p)) for p in pos], [], tris.tolist())
    me.update()
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
                groups[bone].add([vi], w, 'ADD')
    mod = ob.modifiers.new('Armature', 'ARMATURE')
    mod.object = arm
    P.white_color_attribute(ob)
    src = tex_src or os.path.join(DSU_DIR, model['texture'] + '.tga')
    me.materials.append(P.make_material(key + '_body', P.load_texture(tex_stem, src), two_sided=False, shader=SHADER))
    return ob


def add_role_aliases(b2it_path, names, aliases=ROLE_ALIASES):
    entries = K.parse_b2it(open(b2it_path, 'rb').read())
    have = {n for n, _ in entries}
    for role, joint in aliases.items():
        if role not in have and joint in names:
            entries.append((role, names.index(joint)))
    open(b2it_path, 'wb').write(K.write_b2it(entries))
    return sorted(n for n, _ in entries if n in aliases)


def exported_rig(body_dir, body, aliases=ROLE_ALIASES):
    m = K.parse_model(open(os.path.join(body_dir, body + '.model'), 'rb').read())
    table = K.parse_b2it(open(os.path.join(body_dir, body + '.b2it'), 'rb').read())
    by_index = {}
    for n, i in table:
        if n not in aliases:
            by_index[i] = n
    names = [by_index[i] for i in range(len(m['bones']))]
    parents = [b['parent'] for b in m['bones']]
    binds = [np.array(b['bind'], dtype=float).reshape(4, 4) for b in m['bones']]
    return m, names, parents, binds


def check_clip(model, clip, names, parents, binds, anm_bytes):
    worlds, frames = U.clip_game_worlds(model, clip, names, binds)
    parsed = A.parse_anm(anm_bytes)
    worst = 0.0
    for k in sorted({0, 1, len(frames) // 2, len(frames) - 1}):
        pose = A.evaluate_pose(parsed, 2 * k, parents)
        for i in range(len(names)):
            w = np.array(pose[i]['world'], dtype=float).reshape(4, 4)
            worst = max(worst, float(np.abs(w[3, :3] - worlds[k][i][3, :3]).max()))
    return worst, len(frames)


def port(dancer):
    label, key, sex, shadow, tex_stem = DANCERS[dancer]
    P.fresh_scene()
    model = U.parse_ddm(open(os.path.join(DSU_DIR, dancer + '.ddm'), 'rb').read())
    names = U.hierarchy_order([b['name'] for b in model['bones']])
    binds = U.game_bind_matrices(model)
    arm = build_armature(key, names, binds)
    build_mesh(key, model, arm, tex_stem)
    bpy.context.view_layer.update()

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

    m, file_names, parents, file_binds = exported_rig(body_dir, body)
    assert sorted(file_names) == sorted(names), (file_names, names)
    idents = [b['identity'] for b in m['bones']]
    assert len(set(idents)) == len(idents), 'bone identity collision'
    assert all(len(me['palette']) <= 52 for me in m['meshes'])
    assert K.write_model(K.model_to_spec(m)) == open(os.path.join(body_dir, body + '.model'), 'rb').read()
    aliases = add_role_aliases(os.path.join(body_dir, body + '.b2it'), file_names)
    print('B2IT role aliases', aliases)

    for name in clip_rows(dancer):
        stem = os.path.splitext(name)[0]
        clip = U.parse_ani(open(os.path.join(DSU_DIR, name), 'rb').read())
        spec, _ = U.ani_to_anm_spec(model, clip, file_names, parents, file_binds)
        data = A.write_anm(spec)
        err, n = check_clip(model, clip, file_names, parents, file_binds, data)
        assert err < 1e-3, '%s: joint error %.5f m' % (name, err)
        open(os.path.join(motion_dir, stem + '.anm'), 'wb').write(data)
        print('CLIP %-18s %4d keys -> %4d frames @60, %6d bytes, max joint err %.2e m' % (
            stem, n, spec['frame_count'], len(data), err))

    sidecar = os.path.join(out_dir, 'chara_resources.rlist.txt')
    with open(sidecar, 'w') as f:
        f.write('# Dancing Stage Unleashed / DDR ULTRAMIX (Xbox) "%s", ported with its own rig and clips\n' % dancer)
        f.write('# (tools/blender_ddr_addon/examples/port_character_ultramix.py)\n')
        f.write('%s, pl, %s, A, 1.0, %s, 0.0\n' % (key, sex, export_character.fmt_num(shadow)))
    print('SIDECAR', sidecar)
    if PREVIEW:
        preview(out_dir, key, clip_rows(dancer)[0])


def preview(out_dir, key, first_clip):
    """Round trip through the GAME formats: re-import the export + one exported clip, render."""
    P.fresh_scene()
    body = 'pl_' + key
    arm, meshes, _parts, _info = import_character.load_character(
        os.path.join(out_dir, body, body + '.model'), import_textures=True)
    clip = os.path.join(out_dir, body, 'motion', os.path.splitext(first_clip)[0] + '.anm')
    import_anm.load_anm(clip, arm)
    P.studio()
    pdir = PREVIEW_DIR
    os.makedirs(pdir, exist_ok=True)
    for f in (0, 240, 600):
        bpy.context.scene.frame_set(f)
        P.render_camera(os.path.join(pdir, '%s_f%04d.png' % (key, f)), Vector((0.0, -4.5, 1.0)),
                        Vector((0.0, 0.0, 0.9)), scale=2.4)


if __name__ == '__main__':  # Blender runs --python scripts as __main__; port_character_ultramix2 imports this
    for d in [s.strip() for s in os.environ.get('DANCERS', 'afro,lady').split(',') if s.strip()]:
        port(d)
    print('DONE')
