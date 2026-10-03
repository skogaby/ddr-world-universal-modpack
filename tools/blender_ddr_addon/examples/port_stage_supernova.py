"""EXAMPLE / PORT: the DanceDanceRevolution SuperNova (PS2, JP 2006) 3D stages -- and SuperNova 2's
(JP 2008) one new one -- as Background Dancers custom stages, fed by the TZM decoders in
scripts/tzm_dump.py (formats + RE: docs/ps2_ddr_filedata_research.md §7.4).

A SuperNova stage pack is an XSI scene: objects grouped under blend-layer roots (`dec` opaque,
`add` additive, `sub` subtractive, `glo` glow = base texture + additive glow texture, `ble` alpha),
each object owning coloured strip meshes (format 0x152), a `stageNNN` MOTION record with SRT
tracks on the animated objects (a 4 s / 8-beat loop at 60 fps, 8 s at 29.97) and a `cameraNNN`
record. World's own stage parts use the same layer names (`gm_dawnstreet00_{dec,ble,glo}`), so
the port maps one SuperNova layer to one World part with the stock flag conventions:

  layer  part        mesh flags                      World meaning
  dec    dec / bg    0x0001                          opaque, two-sided (`bg` = the skydome subtree)
  ble    ble  (:-1)  0x02C1                          alpha blend, z-write, two-sided
  add    add         0x06C1 + flags2 4               additive, no z-write, two-sided
  sub    sub         0x06C1 + flags2 8               subtractive
  glo    glo         base copy 0x0001 (`_t` sheet) + additive copy 0x06C1/4 (`_g` sheet)

Every mesh is two-sided because the PS2 GS never culled. Per part:
  1. bake each mesh into game space (metres, tzm_dump.GAME_SCALE, the TZM frame already faces
     +Z) through its object's rest world transform, keep the file's normals (`ddr_normal`), UVs
     and RGBA vertex colours (the `_vc` shader multiplies COLOR0);
  2. rig: a `root` bone plus one FLAT bone per animated object (its deepest animated ancestor is
     the mesh's anchor; the static sub-chain is baked into the vertices), bind = the rigid part of
     the anchor's rest world; meshes are rigidly weighted to their anchor. Flat bones side-step
     World's segment-scale compensation;
  3. export `gm_<key>_<part>.model` with the add-on, then `gm_<key>_<part>_play_loop.anm` (loop
     bit set) from the record's SRT tracks: per bone per key q / t / (scale relative to the bind
     scale, only when it moves), keys every 60/fps frames plus a wrap key; each clip is checked
     against the TZM object worlds (< 1 mm);
  4. `gm_<key>_<part>_play_loop.sanm` from the record's material fcurves (tzm_dump.material_animation;
     RE §7.4): kind-8 tracks per record frame (+ wrap key) on parameter floats 2 / 3 = m_vTexAnime
     offU / offV (the kind-503 texture translation — SuperNova adds it to the same GS-native uv the
     .model keeps), 4..6 = vConstatntColor rgb (the kind-504 colour on the base pass, the kind-1302
     glow strength on the `_g` additive copy); those materials get the `mdl_ch_constant_c_vc` shader
     with their frame-0 values seeded. The hook DLL evaluates the clip on the stage clock;
  5. the camera record: `Camera_001..010` -> `camera/<key>_st01..10.camanm`, `Camera_neu` ->
     `<key>_non01`, the shared `stage_chara_camera.TZM` close-ups -> `_non02..11`
     (tzm_dump.camera_to_camanm_spec: 60 fps keys, cm, look-at orientation, the FOV through the
     inverse of the game's projection keeping SuperNova's vertical extent -- see FOV_KEEP);
  6. (FOOTPANEL=1 only) the SuperNova foot panel (`model/footpanel.TZM`, the unlit `ftpnl` mesh)
     as part `footpanel` -- off by default: stock World stages carry a `footpanel` part only on the
     lesson-only `boom00`, and the shipped SuperNova stages dropped theirs;
  7. sidecar `map_resources.rlist.txt`: `<key>, 000000, 000000, bg:-2, dec, glo, add, sub, ble:-1`
     (present parts only).
`stage011..020`'s recoloured twins are ported as their own stages. Not ported: the `_conf.PTF`
lighting, the material diffuse colour.

SuperNova 2 (GAME=sn2) ships the SAME twenty stage packs byte for byte (+ `stage_chara_camera`,
`footpanel`); its only new stage is `system_bg002` (grid + light beams + stars, an 8 s loop),
so `STAGES=all` there means just that one, keyed `snsystembg002` beside SuperNova's
`snsystembg001`.

Inputs (environment):
  GAME           sn (default: SuperNova) | sn2 (SuperNova 2) -- the extraction, stage list, staging folder
  SN_DIR         the extraction (scripts/extract_ps2_ddr_data.py extract supernova_jp | supernova2_jp ...),
                 default ~/Desktop/PS2 DDR ISOs/Dance Dance Revolution SuperNova[ 2] (Japan)/extracted_full
  STAGES         comma list (sn: stage001 .. stage020, system_bg001; sn2: system_bg002; default the
                 first), or 'all'
  OUT_BASE       default ~/Desktop/SuperNova Stages | ~/Desktop/SuperNova 2 Stages (one folder per
                 stage, `Stage 01` / `System BG 1` / `System BG 2`)
  FOOTPANEL      1 = also write the `gm_<key>_footpanel` part (default 0)
  PREVIEW        1 = render Workbench previews of the RE-IMPORTED parts into PREVIEW_DIR, plus the
                 stage + PREVIEW_DANCER through the written .camanm clips
  CHARA_CAMERAS  1 (default) adds the dancer close-ups as `_non02..11`
  FOV_KEEP       vertical (default) | horizontal: which extent of SuperNova's 4:3 frame survives 16:9
Run: GAME=sn2 STAGES=all /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
       --python tools/blender_ddr_addon/examples/port_stage_supernova.py
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
import tzm_dump as Z  # noqa: E402
from blender_ddr_addon import convert, export_model, import_anm, import_model  # noqa: E402
from blender_ddr_addon.codec import anm as A  # noqa: E402
from blender_ddr_addon.codec import ktmdl as K  # noqa: E402

GAMES = {
    'sn': dict(title='DDR SuperNova (PS2)', prefix='sn',
               dir='~/Desktop/PS2 DDR ISOs/Dance Dance Revolution SuperNova (Japan)/extracted_full',
               out='~/Desktop/SuperNova Stages',
               stages=['stage%03d' % i for i in range(1, 21)] + ['system_bg001']),
    'sn2': dict(title='DDR SuperNova 2 (PS2)', prefix='sn',
                dir='~/Desktop/PS2 DDR ISOs/Dance Dance Revolution SuperNova 2 (Japan)/extracted_full',
                out='~/Desktop/SuperNova 2 Stages',
                stages=['system_bg002']),   # stage001..020 + system_bg001 are SuperNova's packs byte for byte
    # DDR X: six stages; `stage001` ships its eighteen beat-pulsing speakers in a separate
    # `stage001_speaker.TZM` the 1P game overlays, and `stage001_2play.TZM` is exactly the two merged
    # (same cameras), so that pack IS Stage 01. The other `_2play` packs are the reduced 2P dressings
    # (002 / 003) or a stub (005) and `stage006` carries the 10th-anniversary logo X2 blanked. DDR X2
    # ships these six packs again (stage006 minus the logo) and SuperNova 2's `system_bg002` byte
    # for byte -- nothing of its own, so there is no GAME=x2 here.
    'x': dict(title='DDR X (PS2)', prefix='x',
              dir='~/Desktop/PS2 DDR ISOs/Dance Dance Revolution X (Japan)/extracted_full',
              out='~/Desktop/DDR X Stages',
              stages=['stage001', 'stage002', 'stage003', 'stage004', 'stage005', 'stage006'],
              packs={'stage001': 'stage001_2play'},
              screens='Render', drop={'RenderBIGTV2'}),
}
GAME = os.environ.get('GAME', 'sn')
if GAME not in GAMES:
    sys.exit('GAME must be one of %s' % ', '.join(GAMES))
CFG = GAMES[GAME]
SN_DIR = os.path.expanduser(os.environ.get('SN_DIR', CFG['dir']))
MODEL_DIR = os.path.join(SN_DIR, 'files', 'IMAGE', 'model')
OUT_BASE = os.path.expanduser(os.environ.get('OUT_BASE', CFG['out']))
FOOTPANEL = os.environ.get('FOOTPANEL', '0') == '1'
PREVIEW = os.environ.get('PREVIEW', '0') == '1'
PREVIEW_DIR = os.environ.get('PREVIEW_DIR') or os.path.join(tempfile.gettempdir(), 'supernova_stage_previews')
# a ported dancer to stand at the origin in the camera previews (the add-on's export folder
# layout: <Character>/pl_<key>/pl_<key>.model): the game's own Afro when shipped, else SuperNova's;
# '' = stage only
_DANCERS = os.path.join(REPO, 'data_mods', 'custom_models', 'dancers')
_PREVIEW_DANCERS = [os.path.join(_DANCERS, 'DDR X + X2', 'Afro 1', 'pl_xafro01', 'pl_xafro01.model')] if GAME == 'x' else []
_PREVIEW_DANCERS.append(os.path.join(_DANCERS, 'DDR SUPRNVA 1+2', 'Afro 1', 'pl_snafro00', 'pl_snafro00.model'))
PREVIEW_DANCER = os.path.expanduser(os.environ.get(
    'PREVIEW_DANCER', next((p for p in _PREVIEW_DANCERS if os.path.exists(p)), _PREVIEW_DANCERS[-1])))

STAGES = CFG['stages']
# World mesh flags per SuperNova blend layer: (flags, flags2)
LAYER_FLAGS = {
    'dec': (0x0001, 0), 'bg': (0x0001, 0),
    'ble': (0x02C1, 0),
    'add': (0x06C1, 4),
    'sub': (0x06C1, 8),
    'glo': (0x0001, 0),      # the base copy; the glow copy takes ADDITIVE
}
ADDITIVE = (0x06C1, 4)
PART_ORDER = ['bg', 'dec', 'glo', 'add', 'sub', 'ble', 'footpanel']
PART_PRIORITY = {'bg': -2, 'ble': -1}
SKY_NAMES = {'bg', 'haikei', 'sky', 'sphere'}
FOOTPANEL_TEX = 'snfootpanel'
# cameras: the ten numbered stage cameras -> the MAIN list, `Camera_neu` + the shared dancer
# close-ups -> the `_non` cut-aways the DLL shows at dance cuts
CHARA_CAMERA_PACK = 'stage_chara_camera.TZM'
CHARA_CAMERAS = os.environ.get('CHARA_CAMERAS', '1') == '1'
# SuperNova's kind-7 FOV is XSI's HORIZONTAL angle of its 4:3 frame; 'vertical' keeps that
# frame's vertical extent on World's 16:9 output (more visible at the sides), 'horizontal'
# keeps the horizontal extent (top / bottom cropped)
FOV_KEEP = os.environ.get('FOV_KEEP', 'vertical')
# Stage SCREENS (DDR X's `Render*` objects: the TVs the game drew its render-to-texture sub-monitor
# feed on): their material is textured `offscreen1`, the name World registers its 1280² movie
# render target under (tools/blender_ddr_addon/README.md "Stage screens"), so the song's movie
# plays on them. The movie is contain-fitted into that square -- a 16:9 one covers v 0.21875 ..
# 0.78125 -- and X's TVs sample a v band of their own 4:3-ish target (0.2 .. 0.8), so each screen's
# authored v range is remapped onto World's band; u is kept (left to right as seen, unmirrored).
SCREEN_TEXTURE = export_model.SCREEN_TEXTURE_KEY
SCREEN_BAND = (0.21875, 0.78125)
SCREEN_OBJECTS = CFG.get('screens')     # object-name prefix marking a screen; None = no screens
DROP_OBJECTS = CFG.get('drop', set())   # screen duplicates (X's coplanar `RenderBIGTV2` glass over `RenderBIGTV`)


def stage_label(stage):
    """The friendly folder = the options-row label (the SOURCE folder supplies the game prefix):
    `Stage 01` .. `Stage 20`, `System BG 1` / `System BG 2` (`System BG` where a game has one)."""
    if stage in CFG.get('labels', {}):
        return CFG['labels'][stage]
    return 'Stage %s' % stage[-2:] if stage.startswith('stage') else 'System BG %d' % int(stage[-3:])


def stage_key(stage):
    return CFG['prefix'] + re.sub(r'[^a-z0-9]', '', stage.lower())


def stage_pack(stage):
    """The TZM file stem a stage is read from (X's Stage 01 = `stage001_2play`)."""
    return CFG.get('packs', {}).get(stage, stage)


# ---------------------------------------------------------------------------
# source
# ---------------------------------------------------------------------------
def is_camera_record(rec):
    """A record is the camera set when it carries camera tracks (position / interest / FOV --
    tzm_dump.camera_tracks); SuperNova names them `cameraNNN` / `*_cam`, X `jx_stNNN_cam_FIX2`."""
    return bool(Z.camera_tracks(rec))


def load_stage(stage):
    chunks = Z.load_tzm(os.path.join(MODEL_DIR, 'stage', stage_pack(stage) + '.TZM'))
    d = dict(chunks)
    model = Z.parse_model(d['MODEL'])
    materials = Z.parse_materiallist(d['MATERIALLIST'])
    textures = Z.textures_of(chunks)
    records = Z.parse_motion(d['MOTION']) if 'MOTION' in d else []
    stage_rec = next((r for r in records if not is_camera_record(r)), None)
    cameras = [r for r in records if is_camera_record(r)]
    return dict(name=stage, model=model, materials=materials, textures=textures, record=stage_rec, cameras=cameras)


def load_chara_cameras():
    """The dancer close-up set every stage shares (`stage/stage_chara_camera.TZM`, 60 fps, ten
    `chara_Camera_0NN` clips aimed at the dancer's chest)."""
    path = os.path.join(MODEL_DIR, 'stage', CHARA_CAMERA_PACK)
    if not os.path.exists(path):
        return None
    d = dict(Z.load_tzm(path))
    return Z.parse_motion(d['MOTION'])[0] if 'MOTION' in d else None


def layer_and_part(model, oi):
    chain = Z.object_chain(model, oi)
    names = [model['objects'][i]['name'] for i in chain]
    layer = names[0] if names[0] in LAYER_FLAGS else 'dec'
    part = 'bg' if layer == 'dec' and any(n.lower() in SKY_NAMES for n in names) else layer
    return layer, part, chain


def is_screen(model, oi):
    """A render-target screen: an object (or ancestor) named with the game's screen prefix."""
    if not SCREEN_OBJECTS:
        return False
    return any(model['objects'][i]['name'].startswith(SCREEN_OBJECTS) for i in Z.object_chain(model, oi))


def is_dropped(model, oi):
    return any(model['objects'][i]['name'] in DROP_OBJECTS for i in Z.object_chain(model, oi))


def texture_png(key, tex):
    """(DDS stem, PNG path) of a stage texture: `<game prefix><stage number><texture name>`, alnum,
    <= 20 characters (`sn001haikei`, `x002jxst0020501`) -- unique within a stage."""
    stem = re.sub(r'[^a-z0-9]', '', CFG['prefix'] + key[-3:] + tex['name'].lower().replace('_png', ''))[:20]
    out = os.path.join(tempfile.gettempdir(), 'supernova_stage_textures', stem + '.png')
    os.makedirs(os.path.dirname(out), exist_ok=True)
    Z.P.write_png(out, tex['width'], tex['height'], tex['rgba'].tobytes())
    return stem, out


def rigid(m, m_unit):
    """(rotation + translation, per-axis scale) of a column-form world affine: the rotation from
    the scale-free composition of the same chain (`m_unit`, so a flattened prop with a zero scale
    still has a proper frame), the translation from the full matrix, the scale = column lengths."""
    scale = np.linalg.norm(m[:3, :3], axis=0)
    out = np.eye(4)
    out[:3, :3] = m_unit[:3, :3]
    out[:3, 3] = m[:3, 3]
    return out, scale


def game_col(m, s):
    """Column-form model-unit affine -> game metres (translation scaled, rotation kept)."""
    S, Si = np.diag([s, s, s, 1.0]), np.diag([1 / s, 1 / s, 1 / s, 1.0])
    return S @ m @ Si


# ---------------------------------------------------------------------------
# build
# ---------------------------------------------------------------------------
def sn_material_name(src, mat_rec):
    return next((k for k, v in src['materials'].items() if v is mat_rec), None)


def animated_params(anim_entry, tag):
    """The World parameter floats (index -> per-frame values) a SuperNova material's fcurves
    drive on one pass: the texture translation -> `m_vTexAnime.zw` (offU / offV, indices 2 / 3:
    SuperNova's texture matrix adds T to the GS-native uv the .model keeps, so the sign is the
    same); the texture-stage colour (504, the base pass) or the glow strength (1302, the `_g`
    additive pass) -> `vConstatntColor.rgb` (indices 4..6, needs a `_c` shader)."""
    chans = {}
    if anim_entry is None:
        return chans
    uv = anim_entry['uv_offset']
    if uv is not None:
        chans[2], chans[3] = uv[:, 0], uv[:, 1]
    if tag == 'base' and anim_entry['colour'] is not None:
        for i in range(3):
            chans[4 + i] = anim_entry['colour'][:, i]
    if tag == 'glow' and anim_entry['glow'] is not None:
        for i in range(3):
            chans[4 + i] = anim_entry['glow']
    return chans


CONSTANT_C_SHADER = 'mdl_ch_constant_c_vc'   # the skinned `_c` variant: PS applies vConstatntColor


def build_part(src, key, part, mesh_ids, worlds_rest, unit_rest, animated, scale, anim=None):
    """One World part: armature (root + flat anchor bones) and the mesh objects. Returns
    (arm, objects, bone names, anchor object index per bone, bone binds (col, game)). `anim` =
    tzm_dump.material_animation of the stage record: a material whose SuperNova material
    animates its colour / glow is exported with the `_c` shader and its frame-0 values seeded in
    `ddr_params` (the .sanm the DLL evaluates overwrites them every frame)."""
    model = src['model']
    anim = anim or {}
    anchors = []
    for k in mesh_ids:
        chain = Z.object_chain(model, model['mesh_object'][k])
        anchor = next((oi for oi in reversed(chain) if model['objects'][oi]['name'] in animated), None)
        anchors.append(anchor)
    bone_objs = sorted({a for a in anchors if a is not None})
    bone_names = ['root'] + ['%s.%d' % (model['objects'][oi]['name'], oi) for oi in bone_objs]
    binds = [np.eye(4)] + [game_col(rigid(worlds_rest[oi], unit_rest[oi])[0], scale) for oi in bone_objs]

    arm_data = bpy.data.armatures.new('%s_%s_rig' % (key, part))
    arm = bpy.data.objects.new('gm_%s_%s_arm' % (key, part), arm_data)
    bpy.context.scene.collection.objects.link(arm)
    bpy.context.view_layer.objects.active = arm
    bpy.ops.object.mode_set(mode='EDIT')
    ebs = {}
    for n, b in zip(bone_names, binds):
        eb = arm_data.edit_bones.new(n)
        eb.head = (0.0, 0.0, 0.0)
        eb.tail = (0.0, 0.04, 0.0)
        eb.matrix = convert.rowmat_to_blender([float(x) for x in b.T.reshape(16)])
        ebs[n] = eb
    for n in bone_names[1:]:
        ebs[n].parent = ebs['root']
    bpy.ops.object.mode_set(mode='OBJECT')
    arm['ddr_bone_order'] = bone_names

    objects = []
    screens = 0
    S = np.diag([scale, scale, scale, 1.0])
    for k, anchor in zip(mesh_ids, anchors):
        me_src = model['meshes'][k]
        oi = model['mesh_object'][k]
        obj_src = model['objects'][oi]
        if is_dropped(model, oi):
            continue
        screen = is_screen(model, oi)
        mat_rec = Z.material_for(src['materials'], me_src['material']) or {}
        tex_names = [t for t in mat_rec.get('textures', []) if t in src['textures']]
        layer, _part, _chain = layer_and_part(model, oi)
        passes = [(tex_names[0] if tex_names else None, LAYER_FLAGS[layer], 'base')]
        if layer == 'glo' and len(tex_names) > 1 and not screen:
            passes.append((tex_names[1], ADDITIVE, 'glow'))
        W = S @ worlds_rest[oi]
        pos = (np.c_[me_src['positions'], np.ones(me_src['count'])] @ W.T)[:, :3]
        nrm = me_src['normals'] @ np.linalg.pinv(worlds_rest[oi][:3, :3])
        nrm /= np.maximum(np.linalg.norm(nrm, axis=1, keepdims=True), 1e-12)
        tris, _ = Z.consistent_winding(pos, nrm, me_src['triangles'])
        if not len(tris):
            continue
        bone = 'root' if anchor is None else '%s.%d' % (model['objects'][anchor]['name'], anchor)
        src_uv = me_src['uv']
        if screen:
            # the authored v band of the game's render target -> World's 16:9 movie band
            v_lo, v_hi = float(src_uv[:, 1].min()), float(src_uv[:, 1].max())
            src_uv = src_uv.copy()
            src_uv[:, 1] = SCREEN_BAND[0] + (src_uv[:, 1] - v_lo) / max(v_hi - v_lo, 1e-6) * (SCREEN_BAND[1] - SCREEN_BAND[0])
            screens += 1
        for tex_name, (flags, flags2), tag in passes:
            name = 'gm_%s_%s_%02d_%s_%s' % (key, part, k, obj_src['name'], tag)
            me = bpy.data.meshes.new(name)
            me.from_pydata([tuple(convert.vec_to_blender(p)) for p in pos], [], tris.tolist())
            me.update()
            lay = me.uv_layers.new(name='UVMap')
            loops_v = np.zeros(len(me.loops), dtype=np.int64)
            me.loops.foreach_get('vertex_index', loops_v)
            luv = src_uv[loops_v].copy()
            luv[:, 1] = 1.0 - luv[:, 1]
            lay.data.foreach_set('uv', luv.astype(np.float32).ravel())
            exact = me.attributes.new('ddr_normal', 'FLOAT_VECTOR', 'POINT')
            exact.data.foreach_set('vector', np.array([tuple(convert.vec_to_blender(n)) for n in nrm]).ravel())
            ob = bpy.data.objects.new(name, me)
            bpy.context.scene.collection.objects.link(ob)
            ob.parent = arm
            groups = {n: ob.vertex_groups.new(name=n) for n in bone_names}
            groups[bone].add(list(range(me_src['count'])), 1.0, 'REPLACE')
            mod = ob.modifiers.new('Armature', 'ARMATURE')
            mod.object = arm
            col = P.white_color_attribute(ob)
            if me_src['colours'] is not None and not screen:   # a screen shows the movie at full strength
                rgba = me_src['colours'][loops_v].astype(np.float32)
                if tag == 'glow':
                    rgba[:, 3] = 1.0  # the glow copy adds at full strength
                col.data.foreach_set('color', rgba.ravel())
            if screen:
                # the image NAME is what matters (the exporter writes the 8x8 `offscreen1.dds` marker and
                # the game binds its movie render target); the pixels only dress the Blender preview
                stem, png = texture_png(key, src['textures'][tex_name]) if tex_name else (None, None)
                image = P.load_texture(SCREEN_TEXTURE, png) if png else P.palette_texture(SCREEN_TEXTURE, [(0.0, 0.0, 0.0)], size=8)
            elif tex_name:
                stem, png = texture_png(key, src['textures'][tex_name])
                image = P.load_texture(stem, png)
            else:
                image = P.palette_texture('%s%s_white' % (CFG['prefix'], key[-3:]), [(1.0, 1.0, 1.0)], size=8)
            mat = P.make_material(name, image, two_sided=True)
            if flags & 0x0040:
                mat.surface_render_method = 'BLENDED'
            sn_name = sn_material_name(src, mat_rec) if mat_rec and not screen else None
            if sn_name:
                mat['ddr_sn_material'] = sn_name
                mat['ddr_sn_pass'] = tag
                chans = animated_params(anim.get(sn_name), tag)
                if any(i in chans for i in (4, 5, 6)):
                    mat['ddr_shader'] = CONSTANT_C_SHADER
                    mat['ddr_params'] = [1.0, 1.0, float(chans.get(2, [0.0])[0]), float(chans.get(3, [0.0])[0]),
                                         float(chans[4][0]), float(chans[5][0]), float(chans[6][0]), 1.0,
                                         0.0, 0.0, 0.0, 0.0]
            me.materials.append(mat)
            ob['ddr_flags'] = flags
            ob['ddr_flags2'] = flags2
            objects.append(ob)
    if screens:
        print('  SCREENS %s: %d mesh(es) textured %s (v -> %.5f..%.5f)' % (part, screens, SCREEN_TEXTURE, *SCREEN_BAND))
    return arm, objects, bone_names, bone_objs, binds


def loop_spec(bone_objs, my_binds, file_binds, worlds_rest, unit_rest, worlds_frames, unit_frames, scale, fps):
    """A write_anm spec for the flat rig (root + children of root) from the anchors' world
    matrices per key: bone b's local (= world) is q(f) = the anchor's game-space rotation, t(f) =
    its translation in metres, s(f) = its scale relative to the rest scale a zero rest scale is
    baked into the vertices). The exporter must not have re-framed the binds (asserted, since a
    re-framing does not commute with a non-uniform scale). Returns (spec, expected row-form
    worlds per key, the 60 fps step between keys)."""
    n_f = len(worlds_frames)
    step = max(1, int(round(60.0 / fps))) if fps > 0 else 1
    times = [step * i for i in range(n_f)] + [step * n_f]  # + the wrap key (= key 0)
    tracks = [dict(kind=0x1C, target=0, keys=[(0.0, 0.0, 0.0, 1.0)]), dict(kind=0x1D, target=0, keys=[(0.0, 0.0, 0.0)])]
    expected = np.zeros((n_f, 1 + len(bone_objs), 4, 4))
    expected[:, 0] = np.eye(4)
    for b, oi in enumerate(bone_objs, start=1):
        s_rest = rigid(worlds_rest[oi], unit_rest[oi])[1]
        q_frame = np.asarray(file_binds[b]) @ np.linalg.inv(np.asarray(my_binds[b]))
        assert np.abs(q_frame - np.eye(4)).max() < 1e-4, 'bone %d: the exporter re-framed the bind' % b
        quats, trans, scales, prev = [], [], [], None
        for f in range(n_f):
            rig, sc = rigid(worlds_frames[f][oi], unit_frames[f][oi])
            g = game_col(rig, scale)
            rel = np.where(s_rest > 1e-9, sc / np.where(s_rest > 1e-9, s_rest, 1.0), 1.0)
            r_row = g[:3, :3].T
            world = np.eye(4)
            world[:3, :3] = np.diag(rel) @ r_row   # the evaluator's S . R (rows scaled)
            world[3, :3] = g[:3, 3]
            expected[f, b] = world
            qv = Z.rowmat_to_quat(r_row)
            if prev is not None and sum(a * c for a, c in zip(prev, qv)) < 0:
                qv = tuple(-c for c in qv)
            prev = qv
            quats.append(qv)
            trans.append(tuple(float(x) for x in g[:3, 3]))
            scales.append(tuple(float(x) for x in rel))
        for lst in (quats, trans, scales):
            lst.append(lst[0])
        tracks.append(dict(kind=0x1C, target=b, times=times, keys=quats))
        tracks.append(dict(kind=0x1D, target=b, times=times, keys=trans))
        if any(abs(c - 1.0) > 1e-4 for s_ in scales for c in s_):
            tracks.append(dict(kind=10, target=b, times=times, keys=scales))
    parents = [-1] + [0] * len(bone_objs)
    return dict(frame_count=times[-1], flag=1, hierarchy=parents, tracks=tracks), expected, step


def check_loop(anm_bytes, expected, step):
    """Worst deviation of the written loop from `expected` (row worlds per key), as
    (absolute error of the 3x3, translation error relative to the translation's magnitude)."""
    parsed = A.parse_anm(anm_bytes)
    n_f, n_b = expected.shape[:2]
    parents = [-1] + [0] * (n_b - 1)
    worst_r = worst_t = 0.0
    for f in sorted({0, 1, n_f // 3, n_f // 2, n_f - 1}):
        pose = A.evaluate_pose(parsed, step * f, parents)
        for b in range(n_b):
            w = np.array(pose[b]['world'], dtype=float).reshape(4, 4)
            worst_r = max(worst_r, float(np.abs(w[:3, :3] - expected[f, b][:3, :3]).max()))
            t_err = float(np.abs(w[3, :3] - expected[f, b][3, :3]).max())
            worst_t = max(worst_t, t_err / max(1.0, float(np.abs(expected[f, b][3, :3]).max())))
    return worst_r, worst_t


def material_anim_spec(objects, anim, step, n_frames):
    """A write_anm .sanm spec for one part: per KTMDL material (the exporter makes one per
    Blender material name) whose SuperNova material has fcurves, kind-8 tracks on the parameter
    indices of animated_params, one key per record frame at `step` (60 / fps) frames plus a wrap
    key: the frame-0 value for colour / glow (their fcurves are periodic), the linear
    extrapolation for a UV offset (a scroll of -2 wraps to 0 -- equal modulo the tiling -- and an
    interpolated wrap key would sweep through half a tile). None when nothing animates."""
    times = [step * i for i in range(n_frames)] + [step * n_frames]
    targets, tracks, seen = [], [], set()
    for ob in objects:
        for mat in ob.data.materials:
            sn = mat.get('ddr_sn_material')
            if not sn or sn not in anim or mat.name in seen:
                continue
            chans = animated_params(anim[sn], mat.get('ddr_sn_pass'))
            if not chans:
                continue
            seen.add(mat.name)
            slot = len(targets)
            shader = mat.get('ddr_shader') or 'mdl_ch_constant_vc'
            targets.append(dict(identity=K.pack_identity(mat.name), identity2=0, hash=K.fnv1(shader), flags=0x2000))
            for sub, vals in sorted(chans.items()):
                vals = [float(v) for v in vals]
                assert len(vals) == n_frames, (len(vals), n_frames)
                wrap = (2 * vals[-1] - vals[-2]) if (sub in (2, 3) and len(vals) > 1) else vals[0]
                tracks.append(dict(kind=8, target=slot, sub=sub, times=times, keys=[(v,) for v in vals + [wrap]]))
    if not tracks:
        return None
    return dict(frame_count=times[-1], flag=1, fps=60, material_tracks=tracks, material_targets=targets)


def check_material_anim(data, spec, anim_names):
    """Worst deviation of the written .sanm's samples (anm_dump.evaluate_materials, the DLL's
    rule) from the spec's keys at every key time."""
    parsed = A.parse_anm(data)
    worst = 0.0
    for t in spec['material_tracks']:
        for time, key in zip(t['times'][::max(1, len(t['times']) // 8)], t['keys'][::max(1, len(t['times']) // 8)]):
            got = A.evaluate_materials(parsed, float(time))[t['target']][t['sub']]
            worst = max(worst, abs(got - key[0]))
    return worst


def build_footpanel(key):
    """The SuperNova foot panel's unlit `ftpnl` mesh as a static part object."""
    chunks = Z.load_tzm(os.path.join(MODEL_DIR, 'footpanel.TZM'))
    d = dict(chunks)
    model = Z.parse_model(d['MODEL'])
    tex = next(iter(Z.textures_of(chunks).values()))
    k = next(k for k, oi in model['mesh_object'].items() if model['objects'][oi]['name'] == 'ftpnl')
    me_src = model['meshes'][k]
    pos, nrm = Z.mesh_bind_positions(model, k)
    pos = pos * Z.GAME_SCALE
    tris, _ = Z.consistent_winding(pos, nrm, me_src['triangles'])
    name = 'gm_%s_footpanel' % key
    me = bpy.data.meshes.new(name)
    me.from_pydata([tuple(convert.vec_to_blender(p)) for p in pos], [], tris.tolist())
    me.update()
    lay = me.uv_layers.new(name='UVMap')
    loops_v = np.zeros(len(me.loops), dtype=np.int64)
    me.loops.foreach_get('vertex_index', loops_v)
    luv = me_src['uv'][loops_v].copy()
    luv[:, 1] = 1.0 - luv[:, 1]
    lay.data.foreach_set('uv', luv.astype(np.float32).ravel())
    exact = me.attributes.new('ddr_normal', 'FLOAT_VECTOR', 'POINT')
    exact.data.foreach_set('vector', np.array([tuple(convert.vec_to_blender(n)) for n in nrm]).ravel())
    ob = bpy.data.objects.new(name, me)
    bpy.context.scene.collection.objects.link(ob)
    P.white_color_attribute(ob)
    out = os.path.join(tempfile.gettempdir(), 'supernova_stage_textures', FOOTPANEL_TEX + '.png')
    os.makedirs(os.path.dirname(out), exist_ok=True)
    Z.P.write_png(out, tex['width'], tex['height'], tex['rgba'].tobytes())
    me.materials.append(P.make_material(name, P.load_texture(FOOTPANEL_TEX, out), two_sided=False))
    ob['ddr_flags'] = 0
    ob['ddr_flags2'] = 0
    return ob


# ---------------------------------------------------------------------------
# cameras
# ---------------------------------------------------------------------------
def camera_plan(src, chara_rec):
    """[(camanm stem, record, camera name)]: `Camera_001..010` -> `<key>_st01..10` (the main
    rotation), `Camera_neu` -> `<key>_non01`, the shared `chara_Camera_001..010` -> `_non02..11`."""
    key = stage_key(src['name'])
    plan = []
    for rec in src['cameras']:
        names = sorted(Z.camera_tracks(rec))
        numbered = [n for n in names if re.fullmatch(r'Camera_\d+', n)]
        for i, n in enumerate(numbered, start=1):
            plan.append(('%s_st%02d' % (key, i), rec, n))
        non = [n for n in names if n not in numbered]
        for i, n in enumerate(non, start=1):
            plan.append(('%s_non%02d' % (key, i), rec, n))
    if chara_rec is not None and CHARA_CAMERAS:
        start = 1 + sum(1 for stem, _, _ in plan if '_non' in stem)
        for i, n in enumerate(sorted(Z.camera_tracks(chara_rec)), start=start):
            plan.append(('%s_non%02d' % (key, i), chara_rec, n))
    return plan


def check_camera(data, times, pos_m, aim_m):
    """Worst (position error in metres, view-direction error) of a written camanm against the
    look-at it was built from, sampled the way the DLL samples (anm_dump.sample_track)."""
    parsed = A.parse_anm(data)
    cam = next(c for c in parsed['chunks'] if c['type'] == 4)
    worst_p = worst_d = 0.0
    n = len(times)
    for i in sorted({0, 1, n // 3, n // 2, n - 1}):
        q = A.sample_track(data, cam['tracks'][0], float(times[i]))
        p = np.array(A.sample_track(data, cam['tracks'][1], float(times[i]))) * 0.01
        rows = np.array(A.quat_to_rowmat(q))
        want = aim_m[i] - pos_m[i]
        want /= max(np.linalg.norm(want), 1e-12)
        worst_p = max(worst_p, float(np.abs(p - pos_m[i]).max()))
        worst_d = max(worst_d, float(np.abs(-rows[2] - want).max()))
    return worst_p, worst_d


def export_cameras(src, set_dir, chara_rec):
    """Write every camera of the stage pack (+ the shared close-ups) as `camera/<stem>.camanm`
    and verify each against its source. Returns the stems written."""
    cam_dir = os.path.join(set_dir, 'camera')
    os.makedirs(cam_dir, exist_ok=True)
    for stale in os.listdir(cam_dir):
        if stale.lower().endswith('.camanm'):
            os.remove(os.path.join(cam_dir, stale))
    stems = []
    for stem, rec, name in camera_plan(src, chara_rec):
        spec, times, (pos_m, aim_m) = Z.camera_to_camanm_spec(rec, name, Z.GAME_SCALE, keep=FOV_KEEP)
        data = A.write_anm(spec)
        err_p, err_d = check_camera(data, times, pos_m, aim_m)
        assert err_p < 1e-3 and err_d < 1e-3, '%s %s: camera error pos %.4f m dir %.4f' % (src['name'], name, err_p, err_d)
        with open(os.path.join(cam_dir, stem + '.camanm'), 'wb') as f:
            f.write(data)
        stems.append(stem)
        fov = spec['camera'][2]['keys'][0][0]
        print('  CAMERA %-18s <- %-16s %s  %3d keys, %d frames (%.2f s), fov slot %.2f deg, err %.1e m / %.1e' % (
            stem, name, rec['name'], len(times), spec['frame_count'], spec['frame_count'] / 60.0, fov, err_p, err_d))
    return stems


# ---------------------------------------------------------------------------
# port
# ---------------------------------------------------------------------------
def port(stage):
    label, key = stage_label(stage), stage_key(stage)
    assert len(label.encode()) <= 15, label
    src = load_stage(stage)
    model, rec = src['model'], src['record']
    scale = Z.GAME_SCALE
    animated = Z.animated_names(rec) if rec else set()
    worlds_rest = Z.object_worlds(model)
    unit_rest = Z.object_worlds(model, unit_scale=True)
    n_keys = max((len(k) for k in Z.object_tracks(rec).values()), default=1) if rec else 1
    worlds_frames = [Z.object_worlds_at(model, rec, f) for f in range(n_keys)] if rec else [worlds_rest]
    unit_frames = [Z.object_worlds_at(model, rec, f, unit_scale=True) for f in range(n_keys)] if rec else [unit_rest]
    fps = rec['fps'] if rec else 60.0
    anim = Z.material_animation(rec) if rec else {}
    for name, e in sorted(anim.items()):
        print('  MATERIAL %-32s %s%s%s%s' % (
            name, 'uv ' if e['uv_offset'] is not None else '', 'colour ' if e['colour'] is not None else '',
            'glow ' if e['glow'] is not None else '', ('UNSUPPORTED %s' % e['unsupported']) if e['unsupported'] else ''))

    parts = {}
    for k, oi in sorted(model['mesh_object'].items()):
        _layer, part, _chain = layer_and_part(model, oi)
        parts.setdefault(part, []).append(k)
    print('STAGE %s: %d objects, %d meshes, %d animated objects, %d keys @ %g fps, parts %s' % (
        stage, len(model['objects']), len(model['meshes']), len(animated), n_keys, fps,
        {p: len(v) for p, v in parts.items()}))

    out_dir = os.path.join(OUT_BASE, label)
    set_dir = os.path.join(out_dir, 'mapset_' + key)
    os.makedirs(set_dir, exist_ok=True)
    written_parts = []
    for part in PART_ORDER:
        if part == 'footpanel':
            if not FOOTPANEL:
                continue
            P.fresh_scene()
            ob = build_footpanel(key)
            pdir = os.path.join(set_dir, 'gm_%s_footpanel' % key)
            written, spec = export_model.export_model(os.path.join(pdir, 'gm_%s_footpanel.model' % key), None, [ob], True)
            written_parts.append(part)
            print('  PART footpanel: %d meshes, %d bones' % (len(spec['meshes']), len(spec['bones'])))
            continue
        if part not in parts:
            continue
        P.fresh_scene()
        arm, objects, bone_names, bone_objs, binds = build_part(src, key, part, parts[part], worlds_rest,
                                                                unit_rest, animated, scale, anim)
        if not objects:
            continue
        bpy.context.view_layer.update()
        model_name = 'gm_%s_%s' % (key, part)
        pdir = os.path.join(set_dir, model_name)
        os.makedirs(pdir, exist_ok=True)
        for stale in os.listdir(pdir):
            if stale.endswith(('.dds', '.anm', '.sanm')):
                os.remove(os.path.join(pdir, stale))
        written, spec = export_model.export_model(os.path.join(pdir, model_name + '.model'), arm, objects, True)
        m = K.parse_model(open(os.path.join(pdir, model_name + '.model'), 'rb').read())
        assert K.write_model(K.model_to_spec(m)) == open(os.path.join(pdir, model_name + '.model'), 'rb').read()
        assert len(m['bones']) == len(bone_names), (len(m['bones']), bone_names)
        # bind frames as exported (Blender re-frames nothing for a proper rotation, but read them back)
        file_binds = [np.array(b['bind'], dtype=float).reshape(4, 4) for b in m['bones']]
        info = '  PART %-9s %2d meshes -> %2d KTMDL meshes, %2d bones, flags %s' % (
            part, len(parts[part]), len(spec['meshes']), len(spec['bones']),
            sorted({(hex(me['flags']), me.get('flags2', 0)) for me in m['meshes']}))
        if bone_objs:
            my_binds = [b.T for b in binds]  # row form
            lspec, expected, step = loop_spec(bone_objs, my_binds, file_binds, worlds_rest, unit_rest, worlds_frames, unit_frames, scale, fps)
            data = A.write_anm(lspec)
            err_r, err_t = check_loop(data, expected, step)
            assert err_r < 2e-3 and err_t < 2e-4, '%s %s: loop error rot %.5f trans %.5f' % (stage, part, err_r, err_t)
            open(os.path.join(pdir, model_name + '_play_loop.anm'), 'wb').write(data)
            info += ', loop %d frames (%d keys, %d animated bones, err %.1e / %.1e rel)' % (
                lspec['frame_count'], expected.shape[0], len(bone_objs), err_r, err_t)
        if anim and rec:
            step = max(1, int(round(60.0 / fps))) if fps > 0 else 1
            sspec = material_anim_spec(objects, anim, step, len(Z.record_frames(rec)))
            if sspec:
                sdata = A.write_anm(sspec)
                err_m = check_material_anim(sdata, sspec, set(anim))
                assert err_m < 1e-5, '%s %s: sanm error %.2e' % (stage, part, err_m)
                open(os.path.join(pdir, model_name + '_play_loop.sanm'), 'wb').write(sdata)
                # the .model must carry the identities the .sanm addresses
                idents = {mm['identity'] for mm in m['materials']}
                missing = [t for t in sspec['material_targets'] if t['identity'] not in idents]
                assert not missing, missing
                info += ', sanm %d frames (%d materials, %d tracks, subs %s)' % (
                    sspec['frame_count'], len(sspec['material_targets']), len(sspec['material_tracks']),
                    sorted({t['sub'] for t in sspec['material_tracks']}))
        print(info)
        written_parts.append(part)

    with open(os.path.join(out_dir, 'map_resources.rlist.txt'), 'w') as f:
        f.write('# %s %s, ported with its layers as parts and its object animation\n' % (CFG['title'], stage))
        f.write('# (tools/blender_ddr_addon/examples/port_stage_supernova.py GAME=%s)\n' % GAME)
        fields = ['%s:%d' % (p, PART_PRIORITY[p]) if p in PART_PRIORITY else p for p in written_parts]
        f.write('%s, 000000, 000000, %s\n' % (key, ', '.join(fields)))
    print('SIDECAR', os.path.join(out_dir, 'map_resources.rlist.txt'), fields)
    camera_stems = export_cameras(src, set_dir, load_chara_cameras()) if src['cameras'] else []
    if PREVIEW:
        preview(set_dir, key, written_parts, camera_stems)
    return out_dir


def preview(set_dir, key, parts, camera_stems=()):
    """Round trip through the GAME formats: re-import every exported part, render the stage from
    SuperNova's neutral camera spot ((0, 20, 30) looking at (0, 9.7, 1.8) stage units) and wide,
    then -- with a ported SuperNova dancer standing at the origin (PREVIEW_DANCER, a .model) --
    through the written `.camanm` clips exactly as the game will project them
    (import_anm.load_camanm applies the CameraNode's 16:9 recipe): the neutral shot at frame 0 and
    the first three main shots at their midpoint."""
    P.fresh_scene()
    for part in parts:
        name = 'gm_%s_%s' % (key, part)
        import_model.load_model(os.path.join(set_dir, name, name + '.model'), import_textures=True, with_armature=True)
    P.studio()
    os.makedirs(PREVIEW_DIR, exist_ok=True)
    s = Z.GAME_SCALE
    render_persp(os.path.join(PREVIEW_DIR, '%s_neu.png' % key), Vector((0.0, -30.0 * s, 20.0 * s)),
                 Vector((0.0, -1.8 * s, 9.7 * s)))
    render_persp(os.path.join(PREVIEW_DIR, '%s_wide.png' % key), Vector((0.0, -150.0 * s, 60.0 * s)),
                 Vector((0.0, 0.0, 10.0 * s)))
    if not camera_stems:
        return
    if PREVIEW_DANCER and os.path.exists(PREVIEW_DANCER):
        import_model.load_model(PREVIEW_DANCER, import_textures=True, with_armature=True)
    shots = [st for st in camera_stems if '_non01' in st] + [st for st in camera_stems if '_st' in st][:3]
    sc = bpy.context.scene
    for stem in shots:
        cam = import_anm.load_camanm(os.path.join(set_dir, 'camera', stem + '.camanm'))
        cam.data.clip_end = 1000.0
        sc.camera = cam
        sc.render.resolution_x, sc.render.resolution_y = 960, 540
        frame = 0 if '_non' in stem else sc.frame_end // 2
        sc.frame_set(frame)
        sc.render.filepath = os.path.join(PREVIEW_DIR, '%s_cam_%s_f%d.png' % (key, stem.split('_')[-1], frame))
        bpy.ops.render.render(write_still=True)


def render_persp(path, pos, target, lens=24.0, res=(960, 540)):
    sc = bpy.context.scene
    cam = bpy.data.objects.get('Preview camera')
    if not cam:
        cam = bpy.data.objects.new('Preview camera', bpy.data.cameras.new('Preview camera'))
        sc.collection.objects.link(cam)
    cam.location = pos
    cam.rotation_euler = (Vector(target) - Vector(pos)).to_track_quat('-Z', 'Y').to_euler()
    cam.data.type = 'PERSP'
    cam.data.lens = lens
    cam.data.clip_end = 1000.0
    sc.camera = cam
    sc.render.resolution_x, sc.render.resolution_y = res
    sc.render.filepath = path
    bpy.ops.render.render(write_still=True)


if __name__ == '__main__':
    want = os.environ.get('STAGES', STAGES[0])
    todo = STAGES if want == 'all' else [s.strip() for s in want.split(',') if s.strip()]
    for stage in todo:
        port(stage)
    print('DONE')
