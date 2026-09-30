"""EXAMPLE / PORT: the DanceDanceRevolution SuperNova (PS2, JP 2006) polygon dancers WITH THEIR
OWN RIG AND THEIR OWN CHOREOGRAPHY, as Background Dancers custom dancers -- the Omnimix path of
the Ultramix / System 573 / STRIKE ports (port_character_ultramix.py), fed by the TZM decoders
in scripts/tzm_dump.py (formats + RE: docs/ps2_ddr_filedata_research.md §7.4).

SuperNova's engine is new (XSI exports: skinned strip meshes with up to three weights per
vertex on a 22-joint HumanIK-named skeleton, 30 Hz quaternion clips), not the 573's. Per dancer:
  1. rebuild the rig from the MODEL chunk's bone list in World game space (Y-up metres, facing
     +Z, left at +X -- the TZM frame's own handedness, so only a scale applies:
     tzm_dump.GAME_SCALE puts the Hip at World's 0.97 m; BABYLON's `SCALE` node (0.6) is folded
     into that scale and dropped from the rig);
  2. build ONE skinned mesh from every mesh of every object (object transforms applied -- AFRO's
     muffler is authored in its own frame), the file's own normals via `ddr_normal`, white
     COLOR0 (a mesh with vertex colours keeps them: GUS's glasses are 60 % alpha and go to a
     second, alpha-blended material slot), strip winding made consistent with the normals;
     texture = the pack's 512² CLUT sheet (alpha 0x80 -> opaque), exported with the add-on;
  3. add World ROLE-BONE ALIASES to the body `.b2it` (Hips / Left|RightToeBase -> Hip /
     Left|RightToes; Spine2 and Head are named alike);
  4. convert the character's OWN routine list (the ELF character table, SLPM_666.09
     0x3A5260) to `motion/<clip>.anm` against the EXPORTED bind frames (tzm_dump.clip_to_anm_spec:
     30 Hz keys every 2nd frame of the 60 fps timeline; the clips are authored at 120 BPM like
     World's), each checked against the TZM pose (< 1 mm per joint). The 4 s `*_NE_01` idles are
     left out of the dance pool;
  5. write the `chara_resources.rlist.txt` sidecar (sex from the same table).

Inputs (environment):
  SN_DIR      the extraction (scripts/extract_ps2_ddr_data.py extract supernova_jp ...),
              default ~/Desktop/PS2 DDR ISOs/Dance Dance Revolution SuperNova (Japan)/extracted_full
  DANCERS     comma list of skin names (afro babylon emi gus jenny rage robozukin ruby; default afro),
              or 'all'
  OUT_BASE    default ~/Desktop/SuperNova Dancers (one folder per character, named after it;
              NOT data_mods/custom_models while that layout is being reworked)
  PREVIEW     1 = also render Workbench previews of the RE-IMPORTED export into PREVIEW_DIR
              (default the system temp dir -- never into the repo)
Run: /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
       --python tools/blender_ddr_addon/examples/port_character_supernova.py
"""
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
import tzm_dump as Z  # noqa: E402
from blender_ddr_addon import convert, export_character, import_anm, import_character  # noqa: E402
from blender_ddr_addon.codec import anm as A  # noqa: E402
from blender_ddr_addon.codec import ktmdl as K  # noqa: E402

SN_DIR = os.path.expanduser(os.environ.get(
    'SN_DIR', '~/Desktop/PS2 DDR ISOs/Dance Dance Revolution SuperNova (Japan)/extracted_full'))
MODEL_DIR = os.path.join(SN_DIR, 'files', 'IMAGE', 'model')
OUT_BASE = os.path.expanduser(os.environ.get('OUT_BASE', '~/Desktop/SuperNova Dancers'))
PREVIEW = os.environ.get('PREVIEW', '0') == '1'
PREVIEW_DIR = os.environ.get('PREVIEW_DIR') or os.path.join(tempfile.gettempdir(), 'supernova_port_previews')
SHADER = 'mdl_ch_constant_vc'

# The ELF character table (SLPM_666.09 0x3A5260, 0x60 bytes per record: name, 1, IMAGE file
# index, "globalSRT", the motion index list terminated by 30, ..., sex at +0x54). Motion names
# index the list at 0x3A5B8D (FF_NE_01 .. FF_SF_03 = 0..13, MM_NE_01 .. MM_SF_03 = 14..28).
# skin -> (folder / label, key, sex, the character's routines minus its NE idle)
CHARACTERS = {
    'afro': ('Afro', 'snafro00', 'M', ['MM_JA_01', 'MM_JA_02', 'MM_SF_01', 'MM_SF_02', 'MM_SF_03']),
    'emi': ('Emi', 'snemi00', 'F', ['FF_HH_02', 'FF_HT_01', 'FF_HT_02', 'FF_SF_01', 'FF_SF_02', 'FF_SF_03']),
    'babylon': ('Baby-Lon', 'snbabylon00', 'M',
                ['MM_HT_01', 'MM_HT_02', 'MM_JA_01', 'MM_JA_02', 'MM_SF_01', 'MM_SF_02', 'MM_SF_03']),
    'robozukin': ('Robo-Zukin', 'snrobozukin00', 'F',
                  ['FF_HT_01', 'FF_HT_02', 'FF_JA_02', 'FF_SF_01', 'FF_SF_02', 'FF_SF_03']),
    'rage': ('Rage', 'snrage00', 'M', ['MM_BR_01', 'MM_BR_02', 'MM_BR_03', 'MM_HH_01', 'MM_HH_02', 'MM_HT_01',
                                       'MM_HT_02', 'MM_HT_03', 'MM_HT_04']),
    'jenny': ('Jenny', 'snjenny00', 'F', ['FF_BR_01', 'FF_BR_02', 'FF_HH_01', 'FF_HH_02', 'FF_HH_03']),
    'gus': ('Gus', 'sngus00', 'M', ['MM_HT_01', 'MM_HT_02', 'MM_HT_03', 'MM_HT_04']),
    'ruby': ('Ruby', 'snruby00', 'F', ['FF_BR_01', 'FF_BR_02', 'FF_HH_03', 'FF_JA_01']),
}
# World role bone -> the SuperNova joint playing it (`Spine2` and `Head` are named alike).
ROLE_ALIASES = {'Hips': 'Hip', 'LeftToeBase': 'LeftToes', 'RightToeBase': 'RightToes'}


def load_skin(skin):
    chunks = Z.load_tzm(os.path.join(MODEL_DIR, 'chara', 'skin', skin + '.TZM'))
    model = Z.parse_model(dict(chunks)['MODEL'])
    tex = next(iter(Z.textures_of(chunks).values()))
    return model, tex


def load_clip(name):
    return Z.parse_motion(dict(Z.load_tzm(os.path.join(MODEL_DIR, 'chara', 'motion', name + '.TZM')))['MOTION'])[0]


def build_armature(key, rig, binds):
    arm_data = bpy.data.armatures.new(key + '_rig')
    arm = bpy.data.objects.new(key + '_Armature', arm_data)
    bpy.context.scene.collection.objects.link(arm)
    bpy.context.view_layer.objects.active = arm
    bpy.ops.object.mode_set(mode='EDIT')
    ebs = {}
    for n, _p in rig:
        eb = arm_data.edit_bones.new(n)
        eb.head = (0.0, 0.0, 0.0)
        eb.tail = (0.0, 0.04, 0.0)
        eb.matrix = convert.rowmat_to_blender([float(x) for x in np.asarray(binds[n]).reshape(16)])
        ebs[n] = eb
    for n, p in rig:
        if p is not None:
            ebs[n].parent = ebs[p]
    bpy.ops.object.mode_set(mode='OBJECT')
    arm['ddr_bone_order'] = [n for n, _ in rig]
    arm['ddr_chara_key'] = key
    return arm


def texture_png(key, tex):
    out = os.path.join(tempfile.gettempdir(), 'supernova_port_textures', key + '.png')
    os.makedirs(os.path.dirname(out), exist_ok=True)
    Z.P.write_png(out, tex['width'], tex['height'], tex['rgba'].tobytes())
    return out


def build_mesh(key, model, tex, arm):
    pos, nrm, uv, col, weights, tris, src_mesh = Z.game_mesh(model)
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
        for bone, w in ws:
            if w > 0:
                groups[bone].add([vi], w, 'ADD')
    mod = ob.modifiers.new('Armature', 'ARMATURE')
    mod.object = arm
    colour = P.white_color_attribute(ob)
    image = P.load_texture(key + '_tex', texture_png(key, tex))
    me.materials.append(P.make_material(key + '_body', image, two_sided=False, shader=SHADER))
    # vertex colours: a mesh with its own (GUS's 60 % alpha glasses) keeps them and, when
    # translucent, moves to an alpha-blended second slot
    translucent = set()
    if col is not None:
        rgba = col[loops_v].astype(np.float32)
        colour.data.foreach_set('color', rgba.ravel())
        translucent = {int(k) for k in np.unique(src_mesh) if model['meshes'][k]['colours'] is not None
                       and float(model['meshes'][k]['colours'][:, 3].min()) < 0.999}
    if translucent:
        blend = P.make_material(key + '_blend', image, two_sided=False, shader=SHADER)
        blend.surface_render_method = 'BLENDED'
        me.materials.append(blend)
        mat_idx = np.where(np.isin(src_mesh, list(translucent)), 1, 0).astype(np.int32)
        me.polygons.foreach_set('material_index', mat_idx)
        me.update()
    return ob, sorted(translucent)


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


def check_clip(anm_bytes, worlds, parents, step):
    """Max joint-position error (m) of the written .anm against the TZM pose at a few keys."""
    parsed = A.parse_anm(anm_bytes)
    n = worlds.shape[0]
    worst = 0.0
    for k in sorted({0, 1, n // 3, n // 2, n - 2, n - 1}):
        pose = A.evaluate_pose(parsed, step * k, parents)
        for i in range(worlds.shape[1]):
            w = np.array(pose[i]['world'], dtype=float).reshape(4, 4)
            worst = max(worst, float(np.abs(w[3, :3] - worlds[k][i][3, :3]).max()))
    return worst


def port(skin):
    label, key, sex, routines = CHARACTERS[skin]
    assert len(label.encode()) <= 15, label  # the options row's SSO budget (catalog::MAX_LABEL_BYTES)
    P.fresh_scene()
    model, tex = load_skin(skin)
    rig, _index = Z.rig_bones(model)
    binds = Z.game_bind_matrices(model)
    arm = build_armature(key, rig, binds)
    _ob, translucent = build_mesh(key, model, tex, arm)
    bpy.context.view_layer.update()
    print('MODEL %s: %d bones, %d meshes (%d translucent), %d vertices, scale %.4f m/unit' % (
        skin, len(rig), len(model['meshes']), len(translucent), sum(m['count'] for m in model['meshes']),
        Z.unit_scale(model)))

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
    assert sorted(file_names) == sorted(n for n, _ in rig), (file_names, rig)
    idents = [b['identity'] for b in m['bones']]
    assert len(set(idents)) == len(idents), 'bone identity collision'
    assert all(len(me['palette']) <= 52 for me in m['meshes'])
    assert len(file_names) <= 64, 'more posed bones than the frame board holds'
    assert K.write_model(K.model_to_spec(m)) == open(os.path.join(body_dir, body + '.model'), 'rb').read()
    aliases = add_role_aliases(os.path.join(body_dir, body + '.b2it'), file_names)
    assert set(aliases) == set(ROLE_ALIASES), aliases
    print('B2IT role aliases', aliases)

    for name in routines:
        rec = load_clip(name)
        spec, worlds = Z.clip_to_anm_spec(model, rec, file_names, parents, file_binds)
        data = A.write_anm(spec)
        step = spec['tracks'][0].get('times', [0, 2])[1] if len(spec['tracks'][0].get('times', [])) > 1 else 2
        err = check_clip(data, worlds, parents, step)
        assert err < 1e-3, '%s: joint error %.5f m' % (name, err)
        open(os.path.join(motion_dir, name + '.anm'), 'wb').write(data)
        print('CLIP %-9s %4d keys @%g Hz -> %4d frames @60, %6d bytes, max joint err %.2e m' % (
            name, worlds.shape[0], rec['fps'], spec['frame_count'], len(data), err))

    sidecar = os.path.join(out_dir, 'chara_resources.rlist.txt')
    with open(sidecar, 'w') as f:
        f.write('# DDR SuperNova (PS2) "%s", ported with its own rig and choreography\n' % label)
        f.write('# (tools/blender_ddr_addon/examples/port_character_supernova.py; routines %s)\n' % ' '.join(routines))
        f.write('%s, pl, %s, A, 1.0, %s, 0.0\n' % (key, sex, export_character.fmt_num(0.75 if sex == 'F' else 0.8)))
    print('SIDECAR', sidecar)
    if PREVIEW:
        preview(out_dir, key, routines[0])
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
    for f in (0, 300, 600, 900):
        bpy.context.scene.frame_set(f)
        P.render_camera(os.path.join(PREVIEW_DIR, '%s_%s_f%04d.png' % (key, clip, f)),
                        Vector((0.0, -6.0, 1.0)), Vector((0.0, 0.0, 1.0)), scale=4.0, res=(600, 600))


if __name__ == '__main__':
    want = os.environ.get('DANCERS', 'afro')
    todo = list(CHARACTERS) if want == 'all' else [s.strip().lower() for s in want.split(',') if s.strip()]
    for skin in todo:
        port(skin)
    print('DONE')
