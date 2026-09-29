"""EXAMPLE / PORT: a Konami System 573 DDR polygon dancer (3rdMIX PLUS / 4thMIX PLUS / 5thMIX)
WITH ITS OWN RIG AND ITS OWN CHOREOGRAPHY, as a Background Dancers custom dancer -- the Omnimix
path of the Ultramix ports (port_character_ultramix.py; design
.agents/planning/2026-09-28-dsu-dancer-port/design.md), fed by the 573 decoders
(scripts/sys573_dancer_dump.py; formats + RE: docs/sys573_dancers_research.md).

Per dancer:
  1. rebuild the rig in World game space (sys573_dancer_dump.world_bones / world_binds):
     `root` (the routine's travel), 16 joints, and one HELPER bone per switchable object
     (5 hand shapes per hand, 4 faces). The 573 draws one alternate per joint per frame; here
     every alternate is in the mesh and the .anm scales the hidden ones' helpers to 1e-3;
  2. build ONE rigid-skinned mesh (every object on one bone, the file's own normals via
     `ddr_normal`, white COLOR0, `mdl_ch_constant_vc`). Texture = a 512x256 atlas: the PSX
     page on the left, the flat-colour (untextured) polygons' colours as swatches on the right,
     upscaled 2x nearest-neighbour; the 573's transparent texels stay alpha 0 (alpha-tested);
  3. export with the add-on, add the World ROLE-BONE ALIASES to the `.b2it`
     (Hips / Spine2 / Head / Left|RightToeBase -> hips / chest / head / foot_L|R);
  4. convert every dance routine (16 of them, 13-14 measures; clip = one measure) to
     `motion/<routine>.anm`: 120 frames per measure (World runs its dance clock at 120 BPM and
     bpm_sync maps it onto the song), keys every 2nd frame, the root carrying the 573's
     measure-to-measure travel (ROOT_MODE, below). Each clip is checked against the 573 pose
     (< 1 mm per joint) and its helper scales against the 573 draw selection;
  5. write the `chara_resources.rlist.txt` sidecar.

Inputs (environment):
  SYS573_DIR  extracted mixes root (default ~/Desktop/ddr_573_extracted; one sub-folder per
              mix, as written by scripts/extract_sys573_data.py extract --out)
  DANCERS     comma list of <mix>/<chara> (default 3rdmix_plus/afro), or 'all'
  ROOT_MODE   recentre (default: the 573 path, centred on the dancer's mark) | travel | inplace
  OUT_BASE    default <repo>/data_mods/custom_models/dancers
  PREVIEW     1 = also render Workbench previews of the RE-IMPORTED export into PREVIEW_DIR
              (default the system temp dir -- never into the repo)
Run: /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
       --python tools/blender_ddr_addon/examples/port_character_sys573.py
"""
import glob
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
import sys573_dancer_dump as S  # noqa: E402
from blender_ddr_addon import convert, export_character, import_anm, import_character  # noqa: E402
from blender_ddr_addon.codec import anm as A  # noqa: E402
from blender_ddr_addon.codec import ktmdl as K  # noqa: E402

SYS573_DIR = os.path.expanduser(os.environ.get('SYS573_DIR', '~/Desktop/ddr_573_extracted'))
OUT_BASE = os.environ.get('OUT_BASE', os.path.join(REPO, 'data_mods', 'custom_models', 'dancers'))
ROOT_MODE = os.environ.get('ROOT_MODE', 'recentre')
PREVIEW = os.environ.get('PREVIEW', '0') == '1'
PREVIEW_DIR = os.environ.get('PREVIEW_DIR') or os.path.join(tempfile.gettempdir(), 'sys573_port_previews')
SHADER = 'mdl_ch_constant_vc'
TEX_UPSCALE = 2

# mix folder -> (label prefix, key prefix, mix digit)
MIXES = {'3rdmix_plus': ('3rdMIX', 'ddr3', '3'), '4thmix_plus': ('4thMIX', 'ddr4', '4'),
         '5thmix': ('5thMIX', 'ddr5', '5')}
# sidecar sex per character (shadow scale and the sex-pool fallback only; judged from the models)
FEMALE = {'janet', 'lady', 'onna', 'ringf', 'spacef', 'violet', 'zukin', 'kaeru', 'evil_b', 'iizf',
          'lady_b', 'meido', 'onee', 'onna_c', 'onna_d', 'spacef_b', 'yuni', 'charm5', 'emi5', 'girl',
          'hongkong', 'janet5', 'zukin5a'}


def display_name(chara, mix_digit):
    """`afro4` (4thMIX) -> 'Afro', `zukin5a` -> 'Zukin A', `spacef_b` -> 'Spacef B', `qp` -> 'QP':
    the label already names the mix, so a trailing mix-number marker is dropped."""
    import re
    m = re.match(r'^([a-z]+)%s([a-z]?)$' % mix_digit, chara)
    words = [m.group(1)] + ([m.group(2)] if m.group(2) else []) if m else chara.split('_')
    return ' '.join(w.upper() if len(w) <= 2 and len(words) == 1 else w.title() for w in words)


def names_for(mix, chara):
    """Folder label `<N>MIX <Name>` (the maintainer's naming, e.g. '3rdMIX Afro') and key."""
    label_pre, key_pre, digit = MIXES[mix]
    label = '%s %s' % (label_pre, display_name(chara, digit))
    key = key_pre + chara.replace('_', '') + '00'
    assert len(label.encode()) <= 15, label  # the options row's SSO budget (catalog::MAX_LABEL_BYTES)
    return label, key


def build_armature(key, ch):
    bones = S.world_bones(ch)
    binds = S.world_binds(ch)
    arm_data = bpy.data.armatures.new(key + '_rig')
    arm = bpy.data.objects.new(key + '_Armature', arm_data)
    bpy.context.scene.collection.objects.link(arm)
    bpy.context.view_layer.objects.active = arm
    bpy.ops.object.mode_set(mode='EDIT')
    ebs = {}
    for name, _parent, _j, _oi in bones:
        eb = arm_data.edit_bones.new(name)
        eb.head = (0.0, 0.0, 0.0)
        eb.tail = (0.0, 0.04, 0.0)
        eb.matrix = convert.rowmat_to_blender([float(x) for x in binds[name].reshape(16)])
        ebs[name] = eb
    for name, parent, _j, _oi in bones:
        if parent:
            ebs[name].parent = ebs[parent]
    bpy.ops.object.mode_set(mode='OBJECT')
    arm['ddr_bone_order'] = [b[0] for b in bones]
    arm['ddr_chara_key'] = key
    return arm


def atlas_png(key, ch):
    from PIL import Image  # Blender's Python may lack Pillow: fall back to bpy images
    img, _ = S.world_atlas(ch)
    img = np.repeat(np.repeat(img, TEX_UPSCALE, 0), TEX_UPSCALE, 1)
    out = os.path.join(tempfile.gettempdir(), 'sys573_port_textures', key + '.png')
    os.makedirs(os.path.dirname(out), exist_ok=True)
    Image.fromarray(img).save(out)
    return out


def atlas_image(key, ch):
    try:
        return atlas_png(key, ch)
    except ImportError:
        img, _ = S.world_atlas(ch)
        img = np.repeat(np.repeat(img, TEX_UPSCALE, 0), TEX_UPSCALE, 1)
        h, w = img.shape[:2]
        bimg = bpy.data.images.new(key + '_atlas', w, h, alpha=True)
        bimg.pixels.foreach_set((img[::-1].astype(np.float32) / 255.0).ravel())
        out = os.path.join(tempfile.gettempdir(), 'sys573_port_textures', key + '.png')
        os.makedirs(os.path.dirname(out), exist_ok=True)
        bimg.filepath_raw = out
        bimg.file_format = 'PNG'
        bimg.save()
        bpy.data.images.remove(bimg)
        return out


def build_mesh(key, ch, arm):
    pos, nrm, uv, bone, tris = S.world_mesh(ch)
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
    P.white_color_attribute(ob)
    me.materials.append(P.make_material(key + '_body', P.load_texture(key, atlas_image(key, ch)),
                                        two_sided=False, shader=SHADER))
    return ob


def add_role_aliases(b2it_path, names):
    entries = K.parse_b2it(open(b2it_path, 'rb').read())
    have = {n for n, _ in entries}
    for role, joint in S.WORLD_ROLE_ALIASES.items():
        if role not in have and joint in names:
            entries.append((role, names.index(joint)))
    open(b2it_path, 'wb').write(K.write_b2it(entries))
    return sorted(n for n, _ in entries if n in S.WORLD_ROLE_ALIASES)


def exported_rig(body_dir, body):
    m = K.parse_model(open(os.path.join(body_dir, body + '.model'), 'rb').read())
    table = K.parse_b2it(open(os.path.join(body_dir, body + '.b2it'), 'rb').read())
    by_index = {i: n for n, i in table if n not in S.WORLD_ROLE_ALIASES}
    names = [by_index[i] for i in range(len(m['bones']))]
    parents = [b['parent'] for b in m['bones']]
    binds = [np.array(b['bind'], dtype=float).reshape(4, 4) for b in m['bones']]
    return m, names, parents, binds


def check_clip(anm_bytes, frames, worlds, visible, names, parents):
    """Max joint-position error (m) of the written .anm against the 573 pose, and the number of
    sampled frames where a helper's evaluated scale disagrees with the 573 draw selection."""
    parsed = A.parse_anm(anm_bytes)
    worst, bad = 0.0, 0
    picks = sorted({0, 1, len(frames) // 3, len(frames) // 2, len(frames) - 2, len(frames) - 1})
    for k in picks:
        pose = A.evaluate_pose(parsed, frames[k], parents)
        for i, n in enumerate(names):
            w = np.array(pose[i]['world'], dtype=float).reshape(4, 4)
            worst = max(worst, float(np.abs(w[3, :3] - worlds[k][i][3, :3]).max()))
            if n in visible:
                on = pose[i]['s'][0] > 0.5
                bad += on != visible[n][k]
    return worst, bad


def routines_of(motion_dir):
    out = {}
    for path in sorted(glob.glob(os.path.join(motion_dir, '*', '*.cmm'))):
        if os.path.basename(path).startswith(('inst', 'normal')):
            continue  # instructor rig; the 1-measure idle clips are not dances
        clips = S.load_motion(path)
        for rname, names in S.routines(clips).items():
            if len(names) > 1:
                out[rname] = (clips, names)
    return out


def port(mix, chara):
    label, key = names_for(mix, chara)
    chara_dir = os.path.join(SYS573_DIR, mix, 'data', 'chara')
    ch = S.load_character(chara_dir, chara)
    P.fresh_scene()
    arm = build_armature(key, ch)
    build_mesh(key, ch, arm)
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
    assert sorted(file_names) == sorted(arm['ddr_bone_order']), file_names
    idents = [b['identity'] for b in m['bones']]
    assert len(set(idents)) == len(idents), 'bone identity collision'
    assert all(len(me['palette']) <= 52 for me in m['meshes'])
    assert len(file_names) <= 64, 'more posed bones than the frame board holds'
    assert K.write_model(K.model_to_spec(m)) == open(os.path.join(body_dir, body + '.model'), 'rb').read()
    aliases = add_role_aliases(os.path.join(body_dir, body + '.b2it'), file_names)
    assert set(aliases) == set(S.WORLD_ROLE_ALIASES), aliases
    print('B2IT role aliases', aliases)

    clip_names = []
    for rname, (clips, names) in sorted(routines_of(os.path.join(SYS573_DIR, mix, 'data', 'motion')).items()):
        samples = S.routine_samples(clips, names, ch['rest'], ROOT_MODE)
        spec, frames, worlds, visible = S.routine_to_anm_spec(ch, samples, file_names, parents, file_binds)
        data = A.write_anm(spec)
        err, bad = check_clip(data, frames, worlds, visible, file_names, parents)
        assert err < 1e-3, '%s: joint error %.5f m' % (rname, err)
        assert bad == 0, '%s: %d helper visibility mismatches' % (rname, bad)
        open(os.path.join(motion_dir, rname + '.anm'), 'wb').write(data)
        clip_names.append(rname)
        print('CLIP %-9s %2d measures -> %4d frames @60, %6d bytes, max joint err %.2e m' % (
            rname, len(names), spec['frame_count'], len(data), err))

    sidecar = os.path.join(out_dir, 'chara_resources.rlist.txt')
    sex = 'F' if chara in FEMALE else 'M'
    with open(sidecar, 'w') as f:
        f.write('# DDR %s (Konami System 573) "%s", ported with its own rig and choreography\n' % (mix, chara))
        f.write('# (tools/blender_ddr_addon/examples/port_character_sys573.py; ROOT_MODE=%s)\n' % ROOT_MODE)
        f.write('%s, pl, %s, A, 1.0, %s, 0.0\n' % (key, sex, export_character.fmt_num(0.75 if sex == 'F' else 0.8)))
    print('SIDECAR', sidecar)
    if PREVIEW:
        preview(out_dir, key, clip_names[0] if clip_names else None)
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
    if clip:
        import_anm.load_anm(os.path.join(out_dir, body, 'motion', clip + '.anm'), arm)
        for f in (60, 360, 720):
            bpy.context.scene.frame_set(f)
            P.render_camera(os.path.join(PREVIEW_DIR, '%s_%s_f%04d.png' % (key, clip, f)),
                            Vector((0.0, -6.0, 1.0)), Vector((0.0, 0.0, 1.0)), scale=4.0, res=(600, 600))


def all_dancers():
    out = []
    for mix in MIXES:
        for d in sorted(glob.glob(os.path.join(SYS573_DIR, mix, 'data', 'chara', '*', '*.cmd'))):
            chara = os.path.basename(os.path.dirname(d))
            with open(d, 'rb') as f:
                head = f.read(12)
            if head[:8] == bytes(8) and head[8] == 0x1C:  # skip stale table entries (not models)
                out.append((mix, chara))
    return out


if __name__ == '__main__':
    want = os.environ.get('DANCERS', '3rdmix_plus/afro')
    todo = all_dancers() if want == 'all' else [tuple(s.strip().split('/')) for s in want.split(',') if s.strip()]
    for mix, chara in todo:
        port(mix, chara)
    print('DONE')
