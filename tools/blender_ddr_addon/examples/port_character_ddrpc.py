"""EXAMPLE / PORT: a Dance Dance Revolution for Windows (KCEA 2002, "DDR PC") polygon dancer --
a built-in character from the game's exe + data.bin, or a downloadable `character_dll/<name>.dll`
+ `.bin` pair -- WITH ITS OWN RIG AND THE GAME'S OWN BAKED CHOREOGRAPHY, as a Background Dancers
custom dancer (the Omnimix path of port_character_ultramix.py / port_character_sys573.py).
Decoders + RE: scripts/ddrpc_dancer_dump.py, docs/ddr_pc_dancers_research.md.

The PC build is the System 573 engine on Direct3D 8, with the 573 content baked down: one rigid
mesh per joint in Direct3D vertices (position, normal, diffuse, uv), BMP textures, and every
dance routine as per-frame 4x3 joint matrices (60 frames per measure). Per dancer:
  1. rig: `root` + the 16 PC joints (chest, head, hips, R/L upperarm, foot, forearm, shin, thigh,
     hand, neck) under the 573 hierarchy, binds = the sex's idle frame-0 joint matrices (the
     joint-local meshes are modelled in those rotated frames);
  2. ONE rigid-skinned mesh: the file's own normals (`ddr_normal`), the file's vertex diffuse as
     COLOR0 (the game lights COLOR1 and modulates the texture with it; SetMaterial is never
     called), `mdl_ch_constant_vc`, two-sided (the game draws dancers with D3DCULL_NONE). Texture
     = one atlas of every BMP at 2x nearest, the colour key (248, 0, 248) as alpha 0, plus a white
     cell for the untextured materials; a face material's frame 0 (no blink/mouth frames);
  3. export with the add-on, add the World ROLE-BONE ALIASES to the `.b2it`;
  4. convert the dancer's own playlist (8 routines for a male, 12 for a female: FUN_00402240's
     tables by `id & 7`) to `motion/<routine>.anm`: 120 World frames per measure, one key per PC
     frame (every 2nd World frame), each clip checked against the PC matrices (< 1 mm per joint);
  5. write the `chara_resources.rlist.txt` sidecar (the header's sex and model scale).

Inputs (environment):
  DDRPC_DIR   folder with DanceDanceRevolution.exe + data.bin (default ~/Desktop/ddrpc_work/game)
  DDRPC_DLL   folder with the downloadable <name>.dll + <name>.bin (default DDRPC_DIR/character_dll;
              '' = built-ins only)
  DANCERS     comma list of names (Rage, Johnny, ..., alex, zach, ...; default Rage), or 'all'
  ROOT_MODE   inplace (default: the PC's in-place motion) | recentre (hips x/z bbox centred)
  OUT_BASE    default <repo>/data_mods/custom_models/dancers/DDR WINDOWS 2K2
  PREVIEW     1 = also render Workbench previews of the RE-IMPORTED export into PREVIEW_DIR
              (default the system temp dir -- never into the repo)
Run: /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
       --python tools/blender_ddr_addon/examples/port_character_ddrpc.py
"""
import os
import re
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
import ddrpc_dancer_dump as D  # noqa: E402
from blender_ddr_addon import convert, export_character, import_anm, import_character  # noqa: E402
from blender_ddr_addon.codec import anm as A  # noqa: E402
from blender_ddr_addon.codec import ktmdl as K  # noqa: E402

DDRPC_DIR = os.path.expanduser(os.environ.get('DDRPC_DIR', '~/Desktop/ddrpc_work/game'))
DDRPC_DLL = os.environ.get('DDRPC_DLL', os.path.join(DDRPC_DIR, 'character_dll'))
OUT_BASE = os.environ.get('OUT_BASE', os.path.join(REPO, 'data_mods', 'custom_models', 'dancers', 'DDR WINDOWS 2K2'))
ROOT_MODE = os.environ.get('ROOT_MODE', 'inplace')
PREVIEW = os.environ.get('PREVIEW', '0') == '1'
PREVIEW_DIR = os.environ.get('PREVIEW_DIR') or os.path.join(tempfile.gettempdir(), 'ddrpc_port_previews')
SHADER = 'mdl_ch_constant_vc'

# downloadable characters: file stem -> menu label (the packs' own spelling)
DLC_LABELS = {'mrspanky': 'Mr Spanky', 'elliot': 'Elliot', 'sherry': 'Sherry'}
# model-key stems where the label's slug reads badly with the trailing `00`
KEY_STEMS = {'Robo2000': 'robo2k'}


def display_name(ch) -> str:
    n = str(ch['name'])
    if ch['source'] == 'dll':
        return DLC_LABELS.get(n.lower(), n[:1].upper() + n[1:])
    return n


def names_for(ch):
    """Folder label (<= 15 bytes, Windows-legal) and model key `ddrpc<name>00`."""
    label = display_name(ch)
    key = 'ddrpc' + KEY_STEMS.get(label, re.sub(r'[^a-z0-9]', '', label.lower())) + '00'
    assert len(label.encode()) <= 15, label  # the options row's SSO budget (catalog::MAX_LABEL_BYTES)
    assert not re.search(r'[<>:"/\\|?*]|[. ]$', label), label
    return label, key


def build_armature(key, rest_t):
    binds = D.world_binds(rest_t)
    arm_data = bpy.data.armatures.new(key + '_rig')
    arm = bpy.data.objects.new(key + '_Armature', arm_data)
    bpy.context.scene.collection.objects.link(arm)
    bpy.context.view_layer.objects.active = arm
    bpy.ops.object.mode_set(mode='EDIT')
    ebs = {}
    for name in D.BONE_NAMES:
        eb = arm_data.edit_bones.new(name)
        eb.head = (0.0, 0.0, 0.0)
        eb.tail = (0.0, 0.04, 0.0)
        eb.matrix = convert.rowmat_to_blender([float(x) for x in binds[name].reshape(16)])
        ebs[name] = eb
    for name, p in zip(D.BONE_NAMES, D.BONE_PARENTS):
        if p >= 0:
            ebs[name].parent = ebs[D.BONE_NAMES[p]]
    bpy.ops.object.mode_set(mode='OBJECT')
    arm['ddr_bone_order'] = list(D.BONE_NAMES)
    arm['ddr_chara_key'] = key
    return arm


def atlas_image(key, atlas):
    out = os.path.join(tempfile.gettempdir(), 'ddrpc_port_textures', key + '.png')
    os.makedirs(os.path.dirname(out), exist_ok=True)
    D.write_png(out, atlas)
    return out


def color_attribute(ob, loop_rgba):
    """ONE BYTE_COLOR attribute carrying the source D3DCOLOR bytes verbatim: written through
    `color_srgb`, which stores the bytes as given (learnings 2026-10-04: `color` re-encodes)."""
    me = ob.data
    for ca in list(me.color_attributes):
        me.color_attributes.remove(ca)
    col = me.color_attributes.new('Col', 'BYTE_COLOR', 'CORNER')
    col.data.foreach_set('color_srgb', (loop_rgba.astype(np.float32) / 255.0).ravel())
    me.color_attributes.active_color = col
    me.color_attributes.active = col
    return col


def build_mesh(key, ch, rest_t, arm):
    pos, nrm, uv, bone, col, tris, atlas = D.world_mesh(ch, rest_t)
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
    by_bone = {}
    for vi, b in enumerate(bone):
        by_bone.setdefault(b, []).append(vi)
    for b, vs in by_bone.items():
        groups[b].add(vs, 1.0, 'REPLACE')
    mod = ob.modifiers.new('Armature', 'ARMATURE')
    mod.object = arm
    color_attribute(ob, col[loops_v])
    me.materials.append(P.make_material(key + '_body', P.load_texture(key, atlas_image(key, atlas)),
                                        two_sided=True, shader=SHADER))
    return ob, col


def add_role_aliases(b2it_path, names):
    entries = K.parse_b2it(open(b2it_path, 'rb').read())
    have = {n for n, _ in entries}
    for role, joint in D.WORLD_ROLE_ALIASES.items():
        if role not in have and joint in names:
            entries.append((role, names.index(joint)))
    open(b2it_path, 'wb').write(K.write_b2it(entries))
    return sorted(n for n, _ in entries if n in D.WORLD_ROLE_ALIASES)


def exported_rig(body_dir, body):
    m = K.parse_model(open(os.path.join(body_dir, body + '.model'), 'rb').read())
    table = K.parse_b2it(open(os.path.join(body_dir, body + '.b2it'), 'rb').read())
    by_index = {i: n for n, i in table if n not in D.WORLD_ROLE_ALIASES}
    names = [by_index[i] for i in range(len(m['bones']))]
    parents = [b['parent'] for b in m['bones']]
    binds = [np.array(b['bind'], dtype=float).reshape(4, 4) for b in m['bones']]
    return m, names, parents, binds


def check_clip(anm_bytes, frames, W, parents):
    """Max joint-position error (m) of the written .anm against the PC pose."""
    parsed = A.parse_anm(anm_bytes)
    worst = 0.0
    picks = sorted({0, 1, len(frames) // 3, len(frames) // 2, len(frames) - 2, len(frames) - 1})
    for k in picks:
        pose = A.evaluate_pose(parsed, frames[k], parents)
        for i in range(len(parents)):
            w = np.array(pose[i]['world'], dtype=float).reshape(4, 4)
            worst = max(worst, float(np.abs(w[3, :3] - W[k][i][3, :3]).max()))
    return worst


def port(game, ch):
    label, key = names_for(ch)
    source = ('Dance Dance Revolution (Windows, KCEA 2002) %s "%s" (id %d)'
              % ('downloadable character' if ch['source'] == 'dll' else 'built-in character', ch['name'], ch['id']))
    P.fresh_scene()
    rest_t = D.rest_matrices(game.routine('%s_normal' % ch['sex'])[0])
    arm = build_armature(key, rest_t)
    _ob, src_rgba = build_mesh(key, ch, rest_t, arm)
    bpy.context.view_layer.update()

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
    assert sorted(file_names) == sorted(D.BONE_NAMES), file_names
    idents = [b['identity'] for b in m['bones']]
    assert len(set(idents)) == len(idents), 'bone identity collision'
    assert all(len(me['palette']) <= 52 for me in m['meshes'])
    assert K.write_model(K.model_to_spec(m)) == open(os.path.join(body_dir, body + '.model'), 'rb').read()
    aliases = add_role_aliases(os.path.join(body_dir, body + '.b2it'), file_names)
    assert set(aliases) == set(D.WORLD_ROLE_ALIASES), aliases
    print('B2IT role aliases', aliases)

    src_binds = D.world_binds(rest_t)
    clip_names = []
    for rname in game.playlist(ch):
        clips = game.routine(rname)
        frames, worlds = D.routine_worlds(clips, ROOT_MODE)
        W = D.retarget_worlds(worlds, file_names, src_binds, file_binds)
        spec = D.worlds_to_anm_spec(frames, W, parents)
        data = A.write_anm(spec)
        err = check_clip(data, frames, W, parents)
        assert err < 1e-3, '%s: joint error %.5f m' % (rname, err)
        short = re.sub(r'^(MF|M|F)_', '', rname)
        open(os.path.join(motion_dir, short + '.anm'), 'wb').write(data)
        clip_names.append(short)
        print('CLIP %-9s %2d measures -> %4d frames @60, %6d bytes, max joint err %.2e m' % (
            short, len(clips), spec['frame_count'], len(data), err))

    sidecar = os.path.join(out_dir, 'chara_resources.rlist.txt')
    with open(sidecar, 'w') as f:
        f.write('# %s, ported with its own rig and the game\'s baked choreography\n' % source)
        f.write('# (tools/blender_ddr_addon/examples/port_character_ddrpc.py; ROOT_MODE=%s; routines %s)\n'
                % (ROOT_MODE, ' '.join(clip_names)))
        f.write('%s, pl, %s, A, %s, %s, 0.0\n' % (key, ch['sex'], export_character.fmt_num(round(ch['scale'], 3)),
                                                 export_character.fmt_num(0.75 if ch['sex'] == 'F' else 0.8)))
    print('SIDECAR', sidecar)
    if PREVIEW:
        if clip_names:
            preview(out_dir, key, clip_names[0])
    return out_dir


def preview(out_dir, key, clip: str):
    """Round trip through the GAME formats: re-import the export + one exported clip, render with
    a texture x COLOR0 emission look (Workbench TEXTURE ignores vertex colours)."""
    P.fresh_scene()
    body = 'pl_' + key
    arm, _meshes, _parts, _info = import_character.load_character(
        os.path.join(out_dir, body, body + '.model'), import_textures=True)
    P.studio()
    os.makedirs(PREVIEW_DIR, exist_ok=True)
    P.render_camera(os.path.join(PREVIEW_DIR, '%s_rest.png' % key), Vector((0.0, -4.5, 0.95)),
                    Vector((0.0, 0.0, 0.95)), scale=2.3, res=(600, 800))
    P.render_camera(os.path.join(PREVIEW_DIR, '%s_rest_side.png' % key), Vector((4.5, 0.0, 0.95)),
                    Vector((0.0, 0.0, 0.95)), scale=2.3, res=(600, 800))
    import_anm.load_anm(os.path.join(out_dir, body, 'motion', clip + '.anm'), arm)
    for f in (60, 360, 720):
        bpy.context.scene.frame_set(f)
        P.render_camera(os.path.join(PREVIEW_DIR, '%s_%s_f%04d.png' % (key, clip, f)),
                        Vector((0.0, -6.0, 1.0)), Vector((0.0, 0.0, 1.0)), scale=4.0, res=(600, 600))


if __name__ == '__main__':
    game = D.Game(DDRPC_DIR)
    everyone = D.dancers(game, DDRPC_DLL if DDRPC_DLL and os.path.isdir(DDRPC_DLL) else None)
    want = os.environ.get('DANCERS', 'Rage')
    if want == 'all':
        todo = everyone
    else:
        names = [s.strip().lower() for s in want.split(',') if s.strip()]
        todo = [ch for ch in everyone if ch['name'].lower() in names]
        missing = set(names) - {ch['name'].lower() for ch in todo}
        assert not missing, 'unknown dancers %s' % sorted(missing)
    for ch in todo:
        port(game, ch)
    print('DONE')
