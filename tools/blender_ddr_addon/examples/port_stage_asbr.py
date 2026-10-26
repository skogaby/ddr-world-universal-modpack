"""EXAMPLE: port a JoJo's Bizarre Adventure: All Star Battle R (PS4) stage into a World Background
Dancers stage (`data_mods/custom_models/stages/<Source>/<Label>/mapset_<key>/…`).

Written 2026-10-25 for the 14 ASBR arenas (+ the 7 HIGH_SPEC variants, which are the same scenes with
more detail — `SPEC=high` picks them where they exist). Reads the CPK extraction directly through
scripts/xfbin_dump.py; the format notes are docs/jojo_eoh_research.md (ASBR shares EoH's NUCC / NUD
/ NTP3 files; a stage is a flat list of clumps — buildings, floor, props, trees, crowd planes, a
skydome — each a static object tree in centimetres, Z-up, plus effect / gimmick / NPC clumps).

Inputs (environment):
  ASBR_DIR     the CPK extraction root (<dir>/stage/data/stage/sdNNa.xfbin, <dir>/platform/.../HIGH_SPEC/)
  STAGES       `all` or a comma list of stage ids (sd04a, sa07a, ...); default sd04a
  SPEC         `high` (default; the PS4 Pro build where one exists) or `base`
  OUT_BASE     where <Label>/ work dirs go (default ~/Desktop/ASBR Stages); each has export/ to ship
  PREVIEW=1    Workbench renders from the DDR front / back / top
  FACING       the side of the SOURCE arena the fighting camera sits on: `-x` (default; the fighters
               stand at (±10, ∓100) facing each other along Y, the crowd planes at +X), `+x`, `-y`, `+y`.
               That side is turned to the DDR camera's (Blender -Y).
  SCALE        metres per source cm (default 0.007 — 0.7 x true size)
  SKY_SPEED    playback rate of the sky / cloud clips (default 0.1: the source's 30 Hz clouds raced in-game)
  ANIMATE      1 (default) = the looping clips below; 0 = the static port
Run: ASBR_DIR=~/Desktop/ASBR/cpk STAGES=all PREVIEW=1 /Applications/Blender.app/Contents/MacOS/Blender -b \
       --factory-startup --python tools/blender_ddr_addon/examples/port_stage_asbr.py

What becomes what:
  * every model of every kept clump (not `*bod1` NPCs, `_eff_` effects, `gimmick*`, `*hit`) is baked
    to world space through its coord chain and joined per clump; the skydome clump (`<id>bac`) is
    the `bg` part (`:-2`), opaque materials (blend ONE/ZERO) the `dec` part, alpha-blended ones
    (SRC_ALPHA) the `ble` part (`:-1`), additive ones (SRC_ALPHA/ONE) the `add` part.
  * materials: the first non-lightmap / non-normal-map sheet of the NUD material chunk is the
    diffuse (`_t01`, `_d`; the `*_shadowsmap` lightmaps on UV set 1 and the `_nm` normal maps are
    not used — World's shaders are unlit); NTP3 -> DDS capped at 1024; stems `jj<id><sheet>`.
    NUD cull 0 -> two-sided; vertex colours are kept (`_vc` shader). The material chunk's UV
    transform of the picked sheet (`uv * scale + offset`: tiling, flip-book cells) becomes World's
    m_vTexAnime (params 0..3 = 1 / scale, offset); a clamped sky axis bakes it into the uvs.
  * env-mapped water (cube map + normal + height sheets): its normal map shaded under a fixed light
    into a tint sheet (`water_sheet`), scrolled like the source's first normal layer.
  * animation (ANIMATE=1): the stage's LOOPING nuccChunkAnm clips that are not gimmick / cut-in /
    hazard / route clips (`ANM_SKIP`). Material clips' UV offsets (params 2 / 3) -> the part's
    `_play_loop.sanm` (flip-book axes stepped, scrolls interpolated); coord clips -> a flat rig
    (`root` + one bone per animated anchor) and `_play_loop.anm` (30 Hz source keys every 2nd
    World frame, decimated, checked against the source worlds). A part's clip is the lcm of its
    periods (CLIP_CAP). `KEEP` brings back clumps the static rules drop for their animation
    (crowd flip-books, flames); `INSTANCES` effect clumps a clip places (Morioh Town's smoke
    columns); `LAPS` ports a gimmick that runs laps of the arena (Battlefield's
    chariot) as parts `lap`, `lap2`, ... (a part holds at most 63 animated anchors).
  * cameras: the stage borrows a stock camera set (`custom_content.rs` default_camera_row), or
    Griffin House's (`CAMERA_FROM`).
  * sidecar `map_resources.rlist.txt`: `<key>, 000000, 000000, bg:-2, dec, lap, add, ble:-1` (parts present).
"""
import glob
import math
import os
import re
import shutil
import sys

import bpy
import numpy as np
from mathutils import Matrix, Vector

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.normpath(os.path.join(HERE, '..', '..', '..'))
sys.path.insert(0, HERE)
sys.path.insert(0, os.path.join(REPO, 'scripts'))
import port_lib as P  # noqa: E402  (registers the add-on)
import xfbin_dump as X  # noqa: E402
from blender_ddr_addon import convert, export_model  # noqa: E402
from blender_ddr_addon.codec import anm as A  # noqa: E402
from blender_ddr_addon.codec import ktmdl as K  # noqa: E402

ASBR = os.path.expanduser(os.environ['ASBR_DIR'])
OUT_BASE = os.path.expanduser(os.environ.get('OUT_BASE', '~/Desktop/ASBR Stages'))
PREVIEW = os.environ.get('PREVIEW', '0') == '1'
SPEC = os.environ.get('SPEC', 'high')
FACING = os.environ.get('FACING', '-x')
FACING_ROT = {'-y': 0.0, '-x': 90.0, '+y': 180.0, '+x': 270.0}
SCALE = float(os.environ.get('SCALE', '0.007'))    # metres per source cm; 0.7 x true scale (in-game: 1:1 dwarfed the dancer, 0.85 still read large)
SKY_SPEED = float(os.environ.get('SKY_SPEED', '0.1'))
ANIMATE = os.environ.get('ANIMATE', '1') == '1'    # 0 = the static port (no loops, no UV transforms from the clips)
SOURCE = 'JOJOS BZRRE ADV'

# stage id -> label (<= 15 bytes; the game's stage names). Rome Streets (sd05b) is left out: its trains
# are modelled on the camera side only and the orbiting DDR cameras show the open backs.
STAGES = {
    'sd01a': 'Dio Castle', 'sd02a': 'Battlefield', 'sd03a': 'El Cairo City', 'sd03b': 'DIO Mansion',
    'sd04a': 'Morioh Town', 'sd04b': 'Kira Estate', 'sd05a': 'Naples Station',
    'sd06b': 'Green Dolphin', 'sd06c': 'Kennedy Space', 'sd07a': 'Philly Coast', 'sd08a': 'Wall Eyes',
    'sa03e': 'Cairo Bridge', 'sa04b': 'Cape Boingy', 'sa05a': 'Colosseum', 'sa07a': 'Rocky Mtn Vilg',
}
# stages whose own camera set is borrowed from a shipped custom stage (interiors / tight rock
# formations where the stock orbit cameras leave the room): key -> camera folder to copy
# far clip (m) for a borrowed set whose own 200 m hides the stage's sky dome (Wall Eyes: ~760 m)
CAMERA_FAR = {'sd08a': 1500.0}
CAMERA_FROM_SCALE = 0.0085     # the SCALE the borrowed shots were validated at in-game (2026-10-25)
CAMERA_FROM = {
    'sd04b': os.path.join(REPO, 'data_mods', 'custom_models', 'stages', 'Custom', 'Griffin House', 'mapset_griffin00', 'camera'),
    'sd08a': os.path.join(REPO, 'data_mods', 'custom_models', 'stages', 'Custom', 'Griffin House', 'mapset_griffin00', 'camera'),
}
# clumps never ported, by name: NPC bodies (`bod1/2`, `body01`, `trall` rigs, `_bone`, `_skin`,
# `_pos_f` poses, `_shadow`), stage gimmicks (`gim_`, `gimmick`, `gimstone`, `gimfen`, `gimcar`;
# the sound-effect text decals `gion`, `concentline`), effects / particles / light helpers /
# collision, LOD twins (`_low`), the special-finish dressings (`_sf00`), the frog-rain state
# (`frg_`), the "sleep"/fade quads. Gimmick props that animate in the source (Wamuu's chariot,
# the road roller, the Naples cars) come back with the animation pass.
SKIP_CLUMP = re.compile(r'bod\d|body\d|trall|_bone$|_skin$|_pos_f|shadow|gim_|gimmick|gimstone|gimfen|gimcar|gion|concentline'
                        r'|_eff|eff\d|particle|ptcl|hit\d*$|_low$|_sf\d\d|frg_|fadein|disolve|morph|piece|_light\d|light\d\d'
                        r'|signal|spark|hilight|carlight|bgsmoke|dust|shine|wave\d|wtrfall|hook|string|chain|rack|note$|reaf|rock$|cup\d'
                        r'|newspaper|mob_art|mob\d\d$|root$|bucket|_flag$|carroute|accessories|flyobj')
# explicit per-stage drops (clump names) beyond the pattern: stacked colour variants of one parked
# car (the red/silver flicker), duplicate chandeliers, props parked at the arena centre
DROP = {
    'sd01a': {'sd01achandelier02', 'sd01achandelier_nochain00', 'sd01afire00', 'sd01afire03'},
    'sd03a': {'sd03acar02_red', 'sd03acar02_white', 'sd03acar05_green', 'sd03acar02_black', 'sd03acar05_blue'},
    'sd05a': {'sd05acar00_red', 'sd05acar00_white', 'sd05acar00_blue', 'sd05acar01', 'sd05acar02'},
    'sd06c': {'d06c_bucket00'},
}
# a clump centred within NEAR_M of the arena centre and smaller than NEAR_EXTENT_M across is a
# parked gimmick prop (chariot, bulldozer, wheelchair, book, barrel, dinosaur...): dropped. The
# floor and the room shells are wider than that.
NEAR_M, NEAR_EXTENT_M = 4.0, 15.0
# any clump with this few vertices is a light / effect marker
MARKER_VERTS = 12

# World mesh flags per part: (flags, flags2)
PART_FLAGS = {'bg': (0x0001, 0), 'sky_bg': (0x0001, 0), 'dec': (0x0000, 0), 'ble': (0x02C1, 0), 'add': (0x06C1, 4)}
PART_PRIO = {'bg': ':-2', 'sky_bg': ':-2', 'ble': ':-1'}
PART_ORDER = ['bg', 'sky_bg', 'dec', 'lap', 'add', 'ble']
AUX_TEX = ('shadowsmap', '_nm', 'cubemap', 'seacube', 'haching', 'ecmn_', 'normal', '_n$', 'falloff', 'mask', 'wtrhight', r'hight\d')
# meshes dropped by name inside a kept clump: lens flares (`fler`: ZERO/ONE or additive sky quads),
# gimmick-prop meshes stored in a scenery clump, route splines, effect sub-meshes
MESH_SKIP = re.compile(r'fler|flare|bucket|_eff_|carroot|route')
# flat colour for the reflective water planes (their only sheets are a cube map, a normal map and a
# height map -- an env-mapped shader World cannot run); per stage, else WATER_DEFAULT
WATER_DEFAULT = (72, 104, 132)
WATER = {'sa03e': (36, 48, 70), 'sd06c': (48, 128, 168), 'sd07a': (64, 112, 152), 'sa07a': (64, 112, 152),
         'sa04b': (92, 108, 112), 'sd08a': (60, 100, 130), 'sd01a': (40, 60, 90)}
BG_CLUMP = re.compile(r'(bac|sky|back)\d*$')
# sky clumps whose name does not say so: Battlefield's skydome + cloud rings (`sd02a01`; as `dec`
# its sky gradient wrapped past v = 1 into a hard band, 2026-10-26)
BG_EXTRA = {'sd02a': {'sd02a01'}}
# water / sea animation in a sky clump keeps the source speed (SKY_SPEED is for clouds)
NOT_SKY = re.compile(r'sea|water|wtr|wave|river')
# looping clips whose COORD curves are not ported (their material curves are): Philly Coast's shore
# sea (`sd07aeff_sea00`) holds the near-shore sea bones 0.2..1.3 m below the mesh's bind, which
# opened gaps between the beach and the water (the sky showed through, 2026-10-26)
COORD_SKIP_CLIPS = {'sd07a': {'sd07aeff_sea00'}}
# clumps brought back for their animation, past SKIP_CLUMP / DROP / the marker and centre rules:
# Dio's Castle's crowd billboards (`planemob`, 16-cell flip-books) and fireplace / balcony flame
# quads, Battlefield's torch flames
KEEP = {'sd01a': {'sd01aplanemob00', 'sd01afire01', 'sd01afire02'}, 'sd02a': {'sd02aeff00'}}
# meshes dropped by name inside a KEPT clump, per stage: Battlefield's torch heat haze (a normal-map
# distortion quad -- World has no refraction pass)
MESH_DROP = {'sd02a': {'sd02aeff00_02'}}
# a gimmick that runs LAPS of the arena while the stage plays, ported as an ambient loop: the
# clumps an in-place clip animates (`rig`: Battlefield's two horses galloping, the chariot, its
# rider -- one template horse clump instanced twice) carried along a route clip's dummy (`route`:
# one closed lap of the arena rim, 330 frames). The gimmick's other states (the idle at the
# centre, the run across the arena, the entry route) are not ported.
LAPS = {'sd02a': dict(rig='sd02agimkyu01_run01', route='sd02aroute01')}
# effect clumps a looping clip PLACES as an instance (the clump itself sits at the origin; the
# clip's instance coords put it in the scene), ported static at the clip's frame-0 pose with
# their material loop, present the whole song: Morioh Town's three smoke columns rising from the
# burning house (`sd04aeff` instances `sd04a_eff_bigsmoke003..005`; there are no flame sheets)
INSTANCES = {'sd04a': {'sd04aeff': ['sd04a_eff_bigsmoke00', 'sd04a_eff_bigsmoke01', 'sd04a_eff_bigsmoke02']}}

# ---------------------------------------------------------------------------------------------
# animation: the stage's LOOPING nuccChunkAnm clips (docs/jojo_eoh_research.md §3 "Anm") ->
# per-part object loops (`_play_loop.anm`, a flat rig) and material loops (`_play_loop.sanm`)
# ---------------------------------------------------------------------------------------------
# material chunk -> the clip target that animates it, where the clip names a material the file does
# not have: Kira House Fire's sea clip (`sa04beff_sea00`, reused from sa06b) drives
# `sa06b_sea_static00_sea00_01`, the file's sea material is `..._sea00_m01`
MAT_ALIAS = {'sa04b': {'sa06b_sea_static00_sea00_m01': 'sa06b_sea_static00_sea00_01'}}
STEP = 2                   # World 60 Hz frames per source frame (nuccChunkAnm plays at 30 Hz)
# non-ambient clips: gimmick states, cut-ins, hazard markers, routes, intro demos
ANM_SKIP = re.compile(r'gim|cut_in|danger|area|route|start_demo')
CLIP_CAP = 32000           # a part's clip = the lcm of its periods (STEP x this <= the u16 key times)
MAX_ANCHORS = 63           # frame board: 64 bones per instance, `root` included
MAX_MAT_PARAMS = 48        # scene3d::frame_board::MAX_MAT_PARAMS
ANIM_SHADER = 'mdl_ch_constant_vc'   # what the exporter gives a rigged part's coloured meshes


class StageAnim:
    """The looping clips' targets: `mat[material chunk name]` and `coord[(clump, coord name)]`
    -> (AnmChunk, AnmEntry), the first looping clip naming a target winning. Instanced coords
    (a reference name that is not its chunk's own name: the effect clips stamp one template coord
    many times) are not followed -- the port bakes each clump once."""

    def __init__(self, x, sid=None):
        self.mat, self.coord = {}, {}
        skip_coords = COORD_SKIP_CLIPS.get(sid, set())
        for a in x.by_kind('nuccChunkAnm'):
            if not a.loop or ANM_SKIP.search(a.name) or a.frames < 2:
                continue
            for e in a.entries:
                t = e.target
                if not isinstance(t, X.ChunkRef):
                    continue
                if e.kind == X.ANM_MATERIAL:
                    # by CHUNK where the reference resolves: Wall Eyes has two material chunks
                    # named `sd08a_wall00_t01` (the wet-sheen overlay the clip drives, and the
                    # rock wall), and a name match scrolled the rock (2026-10-26)
                    self.mat.setdefault(id(t.chunk) if t.chunk is not None else t.target, (a, e))
                elif e.kind == X.ANM_COORD and a.name not in skip_coords and t.name == t.target and t.chunk is not None and t.chunk.clump is not None:
                    self.coord.setdefault((t.chunk.clump.name, t.target), (a, e))
        self.alias = {mine: self.mat[theirs] for mine, theirs in MAT_ALIAS.get(sid, {}).items() if theirs in self.mat}

    def mat_entry(self, matc):
        """(clip, entry) animating a material chunk (by identity; a clip naming a material the
        file lacks reaches it through MAT_ALIAS)."""
        return self.mat.get(id(matc)) or self.alias.get(matc.name)

    def coord_entry(self, cl, c):
        return self.coord.get((cl.name, c.name))

    def animated_coords(self, cl):
        return {c for c in cl.coords if (ae := self.coord_entry(cl, c)) is not None and ae[1].animated()}

    def worlds_at(self, cl, f):
        """{coord: row-major 4x4} of a clump at source frame `f` (each animated coord at `f` modulo
        its own clip; coords without an entry at rest)."""
        W = {}
        for c in cl.coords:
            ae = self.coord_entry(cl, c)
            if ae is None:
                L = X.trs_matrix(c.position, c.rotation, c.scale)
            else:
                a, e = ae
                L = e.local_matrix(math.fmod(f, a.frames) if f > 0 else 0, c)
            W[c] = X.mat_mul(W[c.parent], L) if c.parent is not None else L
        return W


def clump_anchor(anim, cl, a, cache, speed=1.0):
    """A flat bone of the stage's own loops: anchor `a` of clump `cl` -- rest world, world at a
    source frame of the PORTED clip (the clump's clips played at `speed`: frame f samples the source
    at f * speed), the periods involved (in ported frames) and `stride`, the ported frames between
    keys (one key per source frame)."""
    def at(f):
        k = (id(cl), f * speed)
        if k not in cache:
            cache[k] = anim.worlds_at(cl, f * speed)
        return cache[k][a]
    return dict(rest=cl.world_matrices()[a], at=at, speed=speed, stride=max(1, int(round(1.0 / speed))),
                periods=[anim.coord_entry(cl, c)[0].frames / speed for c in anim.animated_coords(cl)])


def anchor_of(c, animated):
    """The deepest animated coord on `c`'s chain to the clump root (None: static)."""
    while c is not None:
        if c in animated:
            return c
        c = c.parent
    return None


def uv_transform(matc, slot, anim):
    """(scale u, scale v, offset u, offset v, animated offset curves {axis: AnmCurve}, the clip) of
    texture UV transform `slot` of a material chunk: the NUCC shaders sample `uv * scale + offset`
    (a 1/8 x 1/4 scale with offsets stepping by 1/8 / 1/4 is a flip-book cell; sd01afire03's
    static (0.75, 0.25, 0.25, 0.25) = cell 3 of row 1). The material chunk's floats hold the
    static transform (format bit `slot`), a looping clip's material curves (2 slot, 2 slot + 1
    offsets; 8 + 2 slot, 9 + 2 slot scales) override it while the stage plays."""
    su = sv = 1.0
    ou = ov = 0.0
    if matc is not None and getattr(matc, 'format', 0) & (1 << slot):
        k = 4 * bin(matc.format & ((1 << slot) - 1)).count('1')
        if k + 4 <= len(matc.floats):
            ou, ov, su, sv = matc.floats[k:k + 4]
    curves, clip = {}, None
    ae = anim.mat_entry(matc) if (matc is not None and anim is not None) else None
    if ae is not None and slot < 2:
        clip, e = ae
        for axis in (0, 1):
            c = e.curves.get(2 * slot + axis)
            if c is not None:
                if axis == 0:
                    ou = c.keys[0][1][0]
                else:
                    ov = c.keys[0][1][0]
                if len(set(round(v[0], 6) for _, v in c.keys)) > 1:
                    curves[axis] = c
            s = e.curves.get(8 + 2 * slot + axis)
            if s is not None and s.keys:
                if axis == 0:
                    su = s.keys[0][1][0]
                else:
                    sv = s.keys[0][1][0]
    if abs(su) < 1e-6 or abs(sv) < 1e-6:      # an unused slot's zeros
        su, sv = (su if abs(su) >= 1e-6 else 1.0), (sv if abs(sv) >= 1e-6 else 1.0)
    return su, sv, ou, ov, curves, (clip if curves else None)


def stage_file(sid):
    hi = os.path.join(ASBR, 'platform', 'data', 'stage', 'HIGH_SPEC', sid + '.xfbin')
    if SPEC == 'high' and os.path.exists(hi):
        return hi
    return os.path.join(ASBR, 'stage', 'data', 'stage', sid + '.xfbin')


def is_bg_clump(name, sid):
    return bool(BG_CLUMP.search(name)) or name in BG_EXTRA.get(sid, set())


def sky_speed(clump_name, sid, target):
    """SKY_SPEED for an animated target (coord / material name) of a sky clump, else 1."""
    return SKY_SPEED if is_bg_clump(clump_name, sid) and not NOT_SKY.search(target.lower()) else 1.0


def blend_part(mt, clump_name, sid):
    """World part for a NUD material: D3D blend factors (2 ONE, 5 SRC_ALPHA; dst 0 or (1, 1) = off).
    Skydome / backdrop clumps (`<id>bac`, `bac00`, `sky00`, `back00`) are the `bg` part."""
    if is_bg_clump(clump_name, sid):
        return 'bg'
    if mt.dst_factor == 0 or (mt.src_factor, mt.dst_factor) in ((2, 0), (1, 1)):
        return 'dec'                      # (1, 1) = blending off (water, windows, clouds, far trees)
    if mt.dst_factor == 2:
        return 'add'
    return 'ble'


WATER_LIGHT = np.array([-0.45, 0.35, 0.82])   # tangent-space light of the baked water sheets


def water_sheet(sid, matc, sheets):
    """The flat-tinted water surface made visible: its tangent-space normal map (`*seanormal*`,
    `*river*_normal*`) shaded under a fixed light into a 512² tint sheet -- tint x (0.86 + 0.6 x
    the slope towards the light) + a soft highlight -- so the material's UV scroll shows ripples
    moving (World cannot run the cube-mapped water shader). None without a normal map."""
    tint = np.array(WATER.get(sid, WATER_DEFAULT), dtype=np.float64) / 255.0
    normal = next((t.name for t in matc.textures() if re.search(r'normal', t.name.lower())), None) if matc else None
    t = sheets.index.get(normal) if normal else None
    if t is None or t.fmt not in X.NUT_DXT:
        return None
    stem = 'jj%swtr%s' % (sid[2:], re.sub(r'[^a-z0-9]', '', normal.lower())[-8:])
    if stem in bpy.data.images:
        return bpy.data.images[stem]
    path = os.path.join(sheets.dir, stem + '_nrm.dds')
    open(path, 'wb').write(t.to_dds(max_size=512))
    src = bpy.data.images.load(path)
    w, h = src.size
    px = np.array(src.pixels[:], dtype=np.float64).reshape(h, w, 4)
    n = px[..., :3] * 2.0 - 1.0
    n /= np.maximum(np.linalg.norm(n, axis=2, keepdims=True), 1e-6)
    L = WATER_LIGHT / np.linalg.norm(WATER_LIGHT)
    H = (L + np.array([0.0, 0.0, 1.0]))
    H /= np.linalg.norm(H)
    slope = n @ L - L[2]                      # 0 on a flat texel
    k = np.clip(0.86 + 0.6 * slope, 0.55, 1.25)
    spec = np.clip(n @ H, 0.0, 1.0) ** 120 * 0.35
    rgb = np.clip(tint[None, None, :] * k[..., None] + spec[..., None] * np.array([0.85, 0.92, 1.0]), 0.0, 1.0)
    out = np.concatenate([rgb, np.ones((h, w, 1))], axis=2)
    img = bpy.data.images.new(stem, w, h, alpha=True)
    img.pixels.foreach_set(out.astype(np.float32).ravel())
    img.update()
    img.pack()
    bpy.data.images.remove(src)
    return img


def candidate_sheets(matc):
    """[(texture name, slot index)] of the colour sheets of a material chunk, in slot order."""
    texs = [t.name for t in matc.textures()] if matc is not None else []
    out = [(t, i) for i, t in enumerate(texs) if not any(re.search(k, t.lower()) for k in AUX_TEX)]
    return out or ([(texs[0], 0)] if texs else [])


def pick_diffuse(matc, sheets, uv_sets, sky=False):
    """(texture name, uv set): the first colour sheet, unless a later one is a CUT-OUT (alpha
    coverage) and the mesh has a UV set for it -- ASBR vegetation puts the leaf shape on sheet 2 /
    UV set 1 and a flat colour on sheet 1 (World has one texture per material: keep the shape). The
    skydome (`sky`) keeps its gradient sheet, not the cloud cut-out layered over it."""
    cands = candidate_sheets(matc)
    texs = [t.name for t in matc.textures()] if matc is not None else []
    real = [c for c, _ in cands if not re.search(r'(white|black)\d*$|underwater|hight|cube', c.lower())]
    if not cands or (len(cands) == 1 and any(re.search(k, cands[0][0].lower()) for k in AUX_TEX)) or \
            (texs and re.search('cube', texs[0].lower()) and not real and
             any(re.search('seanormal|river00_normal|wtrhight', t.lower()) for t in texs)):
        return None, 0                    # an env-mapped water surface (cube map + normal + height sheets)
    if len(cands) > 1 and uv_sets > 1 and not sky:
        for name, slot in cands[1:]:
            if slot < uv_sets and sheets.transparency(name) > 0.03:
                return name, slot
    return cands[0][0], 0


class Sheets:
    """Texture chunks of the stage file (+ its `_sf00` / HIGH_SPEC siblings) by name -> Blender image."""

    def __init__(self, sid, files, out_dir):
        self.index = {}
        for f in files:
            if not os.path.exists(f):
                continue
            for tc in X.read_xfbin(f).by_kind('nuccChunkTexture'):
                if tc.textures and tc.name not in self.index:
                    self.index[tc.name] = tc.textures[0]
        self.sid = sid
        self.dir = os.path.join(out_dir, 'tex')
        os.makedirs(self.dir, exist_ok=True)
        self.images = {}
        self.used = set()

    def stem(self, name):
        short = re.sub(r'^%s_?' % re.escape(self.sid), '', name)
        short = re.sub(r'[^A-Za-z0-9]', '', short)[-13:] or 'tex'
        stem = 'jj%s%s' % (self.sid[2:], short)
        n = 2
        while stem in self.used:
            stem = stem[:19] + str(n)
            n += 1
        self.used.add(stem)
        return stem

    def transparency(self, name):
        """Fraction of texels with alpha < 0.5 (0 for an opaque / unknown sheet)."""
        img = self.image(name)
        if img is None:
            return 0.0
        if 'transp' not in img:
            px = img.pixels[:]
            a = px[3::4]
            img['transp'] = sum(1 for v in a if v < 0.5) / max(1, len(a))
        return float(img['transp'])

    def image(self, name):
        if name in self.images:
            return self.images[name]
        t = self.index.get(name)
        if t is None or t.fmt not in X.NUT_DXT:
            self.images[name] = None
            return None
        stem = self.stem(name)
        path = os.path.join(self.dir, stem + '.dds')
        # the block-space downsample is ~5 s per 2048² sheet: keep its result between runs
        cache = os.path.join(OUT_BASE, '_texcache', self.sid, re.sub(r'[^A-Za-z0-9_]', '_', name) + '.dds')
        if not os.path.exists(cache):
            os.makedirs(os.path.dirname(cache), exist_ok=True)
            open(cache, 'wb').write(t.to_dds(max_size=1024))
        shutil.copyfile(cache, path)
        img = bpy.data.images.load(path)
        img.name = stem
        w, h = img.size
        if (w & (w - 1)) or (h & (h - 1)):
            img.scale(1 << max(0, w.bit_length() - 1), 1 << max(0, h.bit_length() - 1))
            px = list(img.pixels)
            img.pixels = px
            img.update()
        self.images[name] = img
        return img


class Ctx:
    def __init__(self, **kw):
        self.__dict__.update(kw)


def build_stage(sid, out_dir):
    f = stage_file(sid)
    x = X.read_xfbin(f)
    sheets = Sheets(sid, [f, os.path.join(ASBR, 'stage', 'data', 'stage', sid + '.xfbin'),
                          os.path.join(ASBR, 'stage', 'data', 'stage', sid + '_sf00.xfbin')], out_dir)
    R = Matrix.Rotation(FACING_ROT[FACING] * 3.141592653589793 / 180.0, 4, 'Z')
    S = Matrix.Scale(SCALE, 4)
    anim = StageAnim(x, sid) if ANIMATE else None
    parts = {p: [] for p in PART_ORDER}
    mats = {}
    rig = {}        # bone name -> (clump, anchor coord): the flat bones the animated parts need
    plans = {}      # Blender material name -> its UV-offset animation (material_clip)
    bone_names = {}
    world_cache = {}
    stats = {'clumps': 0, 'skipped': [], 'verts': 0, 'tris': 0, 'notex': set(), 'decals': 0, 'water': 0, 'anchors': 0, 'anim_mats': 0}
    ctx = Ctx(sid=sid, sheets=sheets, mats=mats, plans=plans, stats=stats, anim=anim, R=R, S=S, rig=rig)
    for cl in x.by_kind('nuccChunkClump'):
        keep = cl.name in KEEP.get(sid, set())
        if not keep and (SKIP_CLUMP.search(cl.name) or cl.name in DROP.get(sid, set())):
            stats['skipped'].append(cl.name)
            continue
        W = cl.world_matrices()
        # the clump's footprint (source cm): a small prop parked at the arena centre is a gimmick
        pts = []
        nverts = 0
        for mdl in cl.models:
            if not isinstance(mdl, X.ModelChunk):
                continue
            T0 = W[mdl.coord] if mdl.coord is not None else None
            for g in mdl.nud.mesh_groups:
                for m in g.meshes:
                    nverts += len(m.positions)
                    for p in m.positions[::max(1, len(m.positions) // 60)]:
                        pts.append(X.mat_apply(T0, p) if T0 else p)
        if not pts or (nverts <= MARKER_VERTS and not keep):
            stats['skipped'].append(cl.name + ' (marker)')
            continue
        cx = (min(p[0] for p in pts) + max(p[0] for p in pts)) / 2
        cy = (min(p[1] for p in pts) + max(p[1] for p in pts)) / 2
        ext = max(max(p[0] for p in pts) - min(p[0] for p in pts), max(p[1] for p in pts) - min(p[1] for p in pts))
        if (cx * cx + cy * cy) ** 0.5 < NEAR_M * 100 and ext < NEAR_EXTENT_M * 100 and cl.name != sid + 'bac' and not keep:
            stats['skipped'].append(cl.name + ' (at centre)')
            continue
        animated = anim.animated_coords(cl) if anim else set()

        def bone_of(c):
            a = anchor_of(c, animated)
            if a is None:
                return 'root'
            key = (id(cl), id(a))
            if key not in bone_names:
                bone_names[key] = 'a%03d' % len(rig)
                rig[bone_names[key]] = clump_anchor(anim, cl, a, world_cache, sky_speed(cl.name, sid, a.name))
            return bone_names[key]

        for part, ob in emit_clump(ctx, cl, W, bone_of).items():
            parts[part].append(ob)
        stats['clumps'] += 1
    emit_laps(ctx, x, rig, parts, world_cache)
    if ANIMATE:
        emit_instances(ctx, x, parts)
    stats['anchors'] = len(rig)
    stats['anim_sources'] = sorted({pl['source'] for pl in plans.values()})
    return parts, stats, sheets, anim, rig, plans


def emit_clump(ctx, cl, W, bone_of, models=None, part_override=None, label=None, wanted=False):
    """Bake one clump's models (all, or those named in `models`) at the source worlds `W` into
    one object per World part ({part: object}); `bone_of(coord)` names the flat bone a vertex
    riding / skinned to that coord follows ('root' = static). `part_override` puts every mesh in
    one part bucket (the lap gimmick's own parts) with its blend part's flags."""
    sid, sheets, mats, plans, stats, anim, R, S = ctx.sid, ctx.sheets, ctx.mats, ctx.plans, ctx.stats, ctx.anim, ctx.R, ctx.S
    per_part = {}
    for mdl in cl.models:
        if not isinstance(mdl, X.ModelChunk) or (models is not None and mdl.name not in models):
            continue
        T = (R @ S @ Matrix([list(r) for r in W[mdl.coord]])) if mdl.coord is not None else (R @ S)
        N3 = T.to_3x3().inverted().transposed()
        rigid_bone = bone_of(mdl.coord) if mdl.coord is not None else 'root'
        for si, (g, m) in enumerate([(g, m) for g in mdl.nud.mesh_groups for m in g.meshes]):
            if not m.faces or not m.materials:
                continue
            mt = m.materials[0]
            if (MESH_SKIP.search(m_name := (mdl.name or '')) and not wanted) or m_name in MESH_DROP.get(sid, set()):
                stats['skipped'].append('%s/%s (mesh)' % (cl.name, m_name))
                continue
            if mt.z_offset != 0:
                # a depth-biased decal (sound-effect text, light pools, tyre marks): the source
                # draws it with a z bias World has no flag for -- it z-fights, so it goes
                stats['decals'] += 1
                continue
            matc = mdl.materials[si] if si < len(mdl.materials) else (mdl.materials[-1] if mdl.materials else None)
            part = blend_part(mt, cl.name, sid)
            bucket = part_override or part
            if part is None:
                stats['skipped'].append('%s/%s (zero blend)' % (cl.name, mdl.name))
                continue
            tex, uvset = pick_diffuse(matc, sheets, len(m.uvs[0]) if m.uvs else 0, sky=(part == 'bg'))
            two_sided = mt.cull_mode == 0
            # the texture's UV transform (the water sheet scrolls with transform 0, the normal
            # map's first layer), as World's m_vTexAnime = (1 / scale, offset) -- float32 in the
            # shader, where a baked uv * 100 would lose the half-float vertex uv's precision
            su, sv, ou, ov, curves, clip = uv_transform(matc, 0 if tex is None else uvset, anim)
            texs0 = [t.name.lower() for t in matc.textures()] if matc is not None else []
            if tex is not None and texs0 and 'cube' in texs0[0]:
                # an env-mapped material (cube + normal + ... + diffuse): its UV transforms and
                # their scrolls belong to the NORMAL-map layers (the wet sheen running down Wall
                # Eyes' rocks, Naples Station's wet road), not to the diffuse (2026-10-26)
                su, sv, ou, ov, curves, clip = 1.0, 1.0, 0.0, 0.0, {}, None
            speed = sky_speed(cl.name, sid, matc.name) if (curves and matc is not None) else 1.0
            wrap = mt.textures[uvset][2:4] if (tex is not None and uvset < len(mt.textures)) else (1, 1)
            bake = [False, False]       # a CLAMPED sky axis bakes its transform and clamps
            if part == 'bg':
                for ax, (s_, o_) in enumerate(((su, ou), (sv, ov))):
                    if ax not in curves and (wrap[ax] == 3 or (abs(s_ - 1.0) < 1e-6 and abs(o_) < 1e-6)):
                        bake[ax] = True
            p_s = [1.0 if bake[0] else su, 1.0 if bake[1] else sv]
            p_o = [0.0 if bake[0] else (ou if 0 in curves else ou - math.floor(ou)),
                   0.0 if bake[1] else (ov if 1 in curves else ov - math.floor(ov))]
            anim_name = matc.name if (curves and matc is not None) else None
            mkey = (tex, two_sided, part, round(p_s[0], 6), round(p_s[1], 6), round(p_o[0], 6), round(p_o[1], 6), anim_name, speed)
            if mkey not in mats:
                if tex is None:
                    img = water_sheet(sid, matc, sheets) if matc is not None else None
                    if img is None:
                        stem = 'jj%swater' % sid[2:]
                        if stem not in bpy.data.images:
                            P.palette_texture(stem, [tuple(c / 255.0 for c in WATER.get(sid, WATER_DEFAULT))], size=16)
                        img = bpy.data.images[stem]
                else:
                    img = sheets.image(tex)
                    if img is None:
                        stats['notex'].add(tex)
                        if 'jj%sflat' % sid[2:] not in bpy.data.images:
                            P.palette_texture('jj%sflat' % sid[2:], [(0.5, 0.5, 0.5)], size=16)
                        img = bpy.data.images['jj%sflat' % sid[2:]]
                name = '%s_%s_%s%s_%d' % (sid, part, img.name, '_2s' if two_sided else '', len(mats))
                mat = P.make_material(name, img, two_sided=two_sided and tex is not None)
                mat['ddr_flags'], mat['ddr_flags2'] = PART_FLAGS[part]
                if two_sided and tex is not None:
                    mat['ddr_flags'] = int(mat['ddr_flags']) | 0x0001
                mat['ddr_params'] = [1.0 / p_s[0], 1.0 / p_s[1], p_o[0], p_o[1], 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0]
                add_uv_mapping(mat, p_s, p_o)
                if anim_name:
                    plans[mat.name] = dict(source=anim_name, clip=clip, curves=curves, scale=(su, sv), speed=speed)
                    stats['anim_mats'] += 1
                mats[mkey] = mat
            if tex is None:
                stats['water'] += 1
            # a sky mesh played slowed (its scroll, or the cloud ring it rides) gets its own part: its
            # clip is SKY_SPEED^-1 times longer than the stage's other loops (lcm would explode)
            slow = speed < 1.0 or (rigid_bone != 'root' and ctx.rig[rigid_bone].get('speed', 1.0) < 1.0)
            if bucket == 'bg' and slow:
                bucket = 'sky_bg'
            acc = per_part.setdefault(bucket, dict(verts=[], faces=[], uv=[], col=[], nrm=[], slot=[], mats=[], wts=[]))
            mat = mats[mkey]
            if mat not in acc['mats']:
                acc['mats'].append(mat)
            slot = acc['mats'].index(mat)
            base = len(acc['verts'])
            skinned = bool(m.bone_ids) and mdl.rigging_flag & 2
            for i, p in enumerate(m.positions):
                acc['verts'].append(T @ Vector(p))
                if skinned:
                    wts = {}
                    for bi, w in zip(m.bone_ids[i], m.bone_weights[i]):
                        if w > 1e-4 and bi < len(cl.coords):
                            b = bone_of(cl.coords[bi])
                            wts[b] = wts.get(b, 0.0) + w
                    acc['wts'].append(sorted(wts.items(), key=lambda kv: -kv[1])[:4] or [('root', 1.0)])
                else:
                    acc['wts'].append([(rigid_bone, 1.0)])
            for (a, b, c) in m.faces:
                acc['faces'].append((base + a, base + b, base + c))
                acc['slot'].append(slot)
                for k in (a, b, c):
                    uv = m.uvs[k][min(uvset, len(m.uvs[k]) - 1)] if m.uvs and m.uvs[k] else (0.0, 0.0)
                    # the sky gradients and distant-view rings run v past 1 (sa04b's 64 px
                    # gradient to 1.21, sd06c's mountain ring to 1.76) on a CLAMPED sampler; the
                    # modpack samples WRAP, so the repeat painted a dark band / second ridge
                    uv = tuple(min(max(uv[ax] * s_ + o_, 0.0), 1.0) if bake[ax] else uv[ax]
                               for ax, (s_, o_) in enumerate(((su, ou), (sv, ov))))
                    acc['uv'].append((uv[0], 1.0 - uv[1]))
                    col = m.colors[k] if m.colors else (255, 255, 255, 255)
                    acc['col'].append(col)
                    nrm = m.normals[k]
                    acc['nrm'].append((N3 @ Vector(nrm)).normalized() if nrm else Vector((0, 0, 1)))
            stats['verts'] += len(m.positions)
            stats['tris'] += len(m.faces)
    out = {}
    for part, acc in per_part.items():
        out[part] = make_object('%s_%s_%s' % (sid, part, label or (cl.name[len(sid):].strip('_') or 'root')), acc)
    return out


class _Rest:
    position, rotation, scale = (0.0, 0.0, 0.0), (0.0, 0.0, 0.0), (1.0, 1.0, 1.0)


def emit_laps(ctx, x, rig, parts, cache):
    """LAPS: every model-carrying clump instance of the rig clip, baked at its template clump's
    rest pose, every coord a flat bone whose world(f) = route dummy(f) . the instance's chain(f)."""
    spec = LAPS.get(ctx.sid)
    if not spec or not ANIMATE:
        return
    anms = {a.name: a for a in x.by_kind('nuccChunkAnm')}
    clumps = {c.name: c for c in x.by_kind('nuccChunkClump')}
    a, route = anms[spec['rig']], anms[spec['route']]
    re_ = next(e for e in route.entries if e.kind == X.ANM_COORD)
    Pa, Pr = int(round(a.frames)), int(round(route.frames))
    for ci, (cr, bones, models) in enumerate(a.clumps):
        if cr is None or not models or cr.target not in clumps:
            continue
        tmpl = clumps[cr.target]
        by_coord = {bones[e.bone].target: e for e in a.entries
                    if e.clump == ci and e.kind == X.ANM_COORD and e.bone < len(bones)}

        def worlds(f, tmpl=tmpl, by_coord=by_coord, ci=ci):
            k = ('lap', ci, f)
            if k not in cache:
                base = re_.local_matrix(f % Pr, _Rest)
                W = {}
                for c in tmpl.coords:
                    e = by_coord.get(c.name)
                    L = e.local_matrix(f % Pa, c) if e is not None else X.trs_matrix(c.position, c.rotation, c.scale)
                    W[c] = X.mat_mul(W[c.parent] if c.parent is not None else base, L)
                cache[k] = W
            return cache[k]

        rest = tmpl.world_matrices()
        names = {}

        def bone_of(c, rest=rest, names=names, worlds=worlds, ci=ci):
            if c is None:
                return 'root'
            if id(c) not in names:
                names[id(c)] = 'k%03d' % len(rig)
                rig[names[id(c)]] = dict(rest=rest[c], at=(lambda f, c=c, worlds=worlds: worlds(f)[c]), periods=[Pa, Pr])
            return names[id(c)]

        wanted = {m.target for m in models if m is not None}
        for _part, ob in emit_clump(ctx, tmpl, rest, bone_of, models=wanted, part_override='lap', label=cr.name.replace(' ', '_')).items():
            parts['lap'].append(ob)
        print('  LAP %s (%s): %d bones' % (cr.name, tmpl.name, len(names)))


def emit_instances(ctx, x, parts):
    """INSTANCES: each listed template clump baked at its instance's frame-0 world in the clip."""
    for clip_name, templates in INSTANCES.get(ctx.sid, {}).items():
        a = next((c for c in x.by_kind('nuccChunkAnm') if c.name == clip_name), None)
        clumps = {c.name: c for c in x.by_kind('nuccChunkClump')}
        if a is None:
            continue
        for ci, (cr, bones, models) in enumerate(a.clumps):
            if cr is None or cr.target not in templates or cr.target not in clumps:
                continue
            tmpl = clumps[cr.target]
            by_coord = {bones[e.bone].target: e for e in a.entries
                        if e.clump == ci and e.kind == X.ANM_COORD and e.bone < len(bones)}
            W = {}
            for c in tmpl.coords:
                e = by_coord.get(c.name)
                L = e.local_matrix(0, c) if e is not None else X.trs_matrix(c.position, c.rotation, c.scale)
                W[c] = X.mat_mul(W[c.parent], L) if c.parent is not None else L
            for part, ob in emit_clump(ctx, tmpl, W, lambda c: 'root', label=cr.name[len(ctx.sid):].strip('_'), wanted=True).items():
                parts[part].append(ob)
            p0 = W[tmpl.coords[-1]]
            print('  INSTANCE %s (%s) at (%.0f, %.0f, %.0f) cm' % (cr.name, tmpl.name, p0[0][3], p0[1][3], p0[2][3]))


def add_uv_mapping(mat, p_s, p_o):
    """Preview only: the m_vTexAnime transform as a Mapping node in front of the texture (D3D
    top-down uv' = uv * s + o is Blender's v' = v * s + (1 - s - o)); the exporter reads the image
    and `ddr_params`, never this node."""
    if abs(p_s[0] - 1) < 1e-9 and abs(p_s[1] - 1) < 1e-9 and abs(p_o[0]) < 1e-9 and abs(p_o[1]) < 1e-9:
        return
    nt = mat.node_tree
    tex = next(n for n in nt.nodes if n.type == 'TEX_IMAGE')
    uvn = nt.nodes.new('ShaderNodeUVMap')
    mp = nt.nodes.new('ShaderNodeMapping')
    mp.vector_type = 'POINT'
    mp.inputs['Scale'].default_value = (p_s[0], p_s[1], 1.0)
    mp.inputs['Location'].default_value = (p_o[0], 1.0 - p_s[1] - p_o[1], 0.0)
    nt.links.new(uvn.outputs['UV'], mp.inputs['Vector'])
    nt.links.new(mp.outputs['Vector'], tex.inputs['Vector'])


def make_object(name, acc):
    me = bpy.data.meshes.new(name)
    me.from_pydata([tuple(v) for v in acc['verts']], [], acc['faces'])
    me.update()
    kept = [i for i, f in enumerate(acc['faces']) if len(set(f)) == 3]
    if len(me.polygons) != len(acc['faces']):
        print('WARN %s: %d of %d faces degenerate' % (name, len(acc['faces']) - len(me.polygons), len(acc['faces'])))
    uv = me.uv_layers.new(name='UVMap')
    col = me.color_attributes.new('Col', 'BYTE_COLOR', 'CORNER')
    flat_uv, flat_col, nrm = [], [], []
    if len(me.polygons) == len(acc['faces']):
        flat_uv = [c for u in acc['uv'] for c in u]
        flat_col = [c / 255.0 for cc in acc['col'] for c in cc]
        nrm = acc['nrm']
    else:
        lookup = {}
        for fi, f in enumerate(acc['faces']):
            for k, vi in enumerate(f):
                lookup[(fi, vi)] = (acc['uv'][3 * fi + k], acc['col'][3 * fi + k], acc['nrm'][3 * fi + k])
        for pi, poly in enumerate(me.polygons):
            fi = kept[pi]
            for li in poly.loop_indices:
                u, c, n = lookup[(fi, me.loops[li].vertex_index)]
                flat_uv += list(u)
                flat_col += [v / 255.0 for v in c]
                nrm.append(n)
    uv.data.foreach_set('uv', flat_uv)
    col.data.foreach_set('color_srgb', flat_col)
    me.normals_split_custom_set(nrm)
    for mat in acc['mats']:
        me.materials.append(mat)
    slots = acc['slot'] if len(me.polygons) == len(acc['faces']) else [acc['slot'][fi] for fi in kept]
    for p, s in zip(me.polygons, slots):
        p.material_index = s
    me.update()
    ob = bpy.data.objects.new(name, me)
    bpy.context.scene.collection.objects.link(ob)
    bones = sorted({b for w in acc['wts'] for b, _ in w})
    if bones != ['root']:
        groups = {}
        for vi, w in enumerate(acc['wts']):
            for b, wt in w:
                if b not in groups:
                    groups[b] = ob.vertex_groups.new(name=b)
                groups[b].add([vi], wt, 'ADD')
    ob['ddr_bones'] = bones
    return ob


def write_sidecar(path, key, parts_present, animated=False):
    row = ['%s%s' % (p, PART_PRIO.get(re.sub(r'\d+$', '', p), '')) for p in parts_present]
    with open(path, 'w') as f:
        f.write('# JoJo ASBR (PS4) stage, ported%s by tools/blender_ddr_addon/examples/port_stage_asbr.py\n'
                % (' with its looping clips' if animated else ' static'))
        f.write('%s, 000000, 000000, %s\n' % (key, ', '.join(row)))


# ---------------------------------------------------------------------------------------------
# animated parts: flat rig + `_play_loop.anm`, `_play_loop.sanm`
# ---------------------------------------------------------------------------------------------
def clip_length(periods):
    """Source frames of a part's clip: the lcm of its periods, capped at CLIP_CAP (a longer lcm
    is the largest period's whole multiple below the cap -- the shorter loops then restart early
    once per clip)."""
    ps = sorted({int(round(p)) for p in periods if p and p >= 2})
    if not ps:
        return 0
    L = 1
    for p in ps:
        L = L * p // math.gcd(L, p)
    if L > CLIP_CAP:
        big = ps[-1]
        L = big * max(1, CLIP_CAP // big) if big <= CLIP_CAP else CLIP_CAP
        print('  CLIP lcm of periods %s > %d: %d frames' % (ps, CLIP_CAP, L))
    return L


def rigid(M):
    """(rotation + translation Matrix, per-local-axis scale) of a Blender column affine; a
    mirrored frame keeps its reflection in the scale (z negated) so the rotation stays proper."""
    a = np.array(M, dtype=np.float64)
    sc = np.linalg.norm(a[:3, :3], axis=0)
    sc = np.where(sc > 1e-12, sc, 1.0)
    r = a[:3, :3] / sc
    if np.linalg.det(r) < 0:
        r[:, 2] = -r[:, 2]
        sc[2] = -sc[2]
    out = np.eye(4)
    out[:3, :3] = r
    out[:3, 3] = a[:3, 3]
    return Matrix(out.tolist()), sc


def rowmat_to_quat(r):
    """Row-vector 3x3 rotation -> (x, y, z, w) (tzm_dump.rowmat_to_quat)."""
    m = np.asarray(r, dtype=np.float64).T
    t = m[0, 0] + m[1, 1] + m[2, 2]
    if t > 0:
        s = 2.0 * math.sqrt(t + 1.0)
        q = ((m[2, 1] - m[1, 2]) / s, (m[0, 2] - m[2, 0]) / s, (m[1, 0] - m[0, 1]) / s, 0.25 * s)
    elif m[0, 0] > m[1, 1] and m[0, 0] > m[2, 2]:
        s = 2.0 * math.sqrt(1.0 + m[0, 0] - m[1, 1] - m[2, 2])
        q = (0.25 * s, (m[0, 1] + m[1, 0]) / s, (m[0, 2] + m[2, 0]) / s, (m[2, 1] - m[1, 2]) / s)
    elif m[1, 1] > m[2, 2]:
        s = 2.0 * math.sqrt(1.0 + m[1, 1] - m[0, 0] - m[2, 2])
        q = ((m[0, 1] + m[1, 0]) / s, 0.25 * s, (m[1, 2] + m[2, 1]) / s, (m[0, 2] - m[2, 0]) / s)
    else:
        s = 2.0 * math.sqrt(1.0 + m[2, 2] - m[0, 0] - m[1, 1])
        q = ((m[0, 2] + m[2, 0]) / s, (m[1, 2] + m[2, 1]) / s, 0.25 * s, (m[1, 0] - m[0, 1]) / s)
    q = np.array(q)
    return tuple(float(v) for v in q / np.linalg.norm(q))


def _slerp(a, b, t):
    d = sum(x * y for x, y in zip(a, b))
    if d < 0:
        b, d = tuple(-v for v in b), -d
    if d > 0.9995:
        v = tuple(x + (y - x) * t for x, y in zip(a, b))
    else:
        th = math.acos(min(1.0, d))
        s0, s1 = math.sin((1 - t) * th) / math.sin(th), math.sin(t * th) / math.sin(th)
        v = tuple(s0 * x + s1 * y for x, y in zip(a, b))
    n = math.sqrt(sum(x * x for x in v)) or 1.0
    return tuple(x / n for x in v)


def decimate(times, keys, tol, rot=False):
    """Drop keys the evaluator's interpolation (slerp / lerp) between the kept neighbours
    reproduces within `tol`; the first and last keys stay."""
    if len(keys) <= 2:
        return list(times), list(keys)
    out_t, out_k = [times[0]], [keys[0]]
    i = 0
    while i < len(keys) - 1:
        j = i + 1
        while j + 1 < len(keys):
            ok = True
            if rot and abs(sum(x * y for x, y in zip(keys[i], keys[j + 1]))) < 0.95:
                break                 # spans stay well short of 180 deg (a full-turn cloud ring)
            for k in range(i + 1, j + 2):
                u = (times[k] - times[i]) / float(times[j + 1] - times[i])
                v = _slerp(keys[i], keys[j + 1], u) if rot else tuple(x + (y - x) * u for x, y in zip(keys[i], keys[j + 1]))
                if rot:
                    err = 1.0 - abs(sum(x * y for x, y in zip(v, keys[k])))
                    if err > tol:
                        ok = False
                        break
                elif max(abs(x - y) for x, y in zip(v, keys[k])) > tol:
                    ok = False
                    break
            if not ok:
                break
            j += 1
        out_t.append(times[j])
        out_k.append(keys[j])
        i = j
    return out_t, out_k


def build_rig(name, bone_names, rig, anim, R, S):
    """The part's armature: `root` + one flat bone per animated anchor, bind = the anchor's rigid
    rest world (Blender metres). Returns (arm, {bone: rest scale})."""
    arm_data = bpy.data.armatures.new(name + '_rig')
    arm = bpy.data.objects.new(name + '_arm', arm_data)
    bpy.context.scene.collection.objects.link(arm)
    bpy.context.view_layer.objects.active = arm
    bpy.ops.object.mode_set(mode='EDIT')
    ebs = {}
    rest_scale = {}
    for n in bone_names:
        eb = arm_data.edit_bones.new(n)
        eb.head = (0.0, 0.0, 0.0)
        eb.tail = (0.0, 0.04, 0.0)
        if n == 'root':
            # the root's GAME bind is the identity (its keys are): in Blender that frame is the
            # axis conversion, not the Blender identity (which would tilt every root-weighted
            # vertex by 90 deg in game)
            eb.matrix = convert.rowmat_to_blender([1.0, 0, 0, 0, 0, 1.0, 0, 0, 0, 0, 1.0, 0, 0, 0, 0, 1.0])
        else:
            Mr = R @ S @ Matrix([list(r) for r in rig[n]['rest']])
            Rr, sc = rigid(Mr)
            eb.matrix = Rr
            rest_scale[n] = sc
        ebs[n] = eb
    for n in bone_names[1:]:
        ebs[n].parent = ebs['root']
    bpy.ops.object.mode_set(mode='OBJECT')
    arm['ddr_bone_order'] = bone_names
    return arm, rest_scale


def loop_spec(bone_names, rig, anim, R, S, rest_scale, L):
    """write_anm spec of the flat rig over L source frames (keys at every source frame = every
    STEP World frames, the evaluator slerps; the key at L is the wrap = key 0), decimated; plus
    the expected row-form worlds per (frame, bone) for check_loop."""
    # root: an exact float quaternion (a q48 identity is 2e-5 off -- 1 cm at a 250 m cloud ring)
    tracks = [dict(kind=1, target=0, keys=[(0.0, 0.0, 0.0, 1.0)]), dict(kind=0x1D, target=0, keys=[(0.0, 0.0, 0.0)])]
    expected = {}
    for b, n in enumerate(bone_names):
        if n == 'root':
            continue
        qs, ts, ss, prev = [], [], [], None
        stride = rig[n].get('stride', 1)
        frames = list(range(0, L, stride)) + [L]
        times = [STEP * f for f in frames]
        for f in frames:
            Mf = R @ S @ Matrix([list(r) for r in rig[n]['at'](f)])
            Rf, sf = rigid(Mf)
            g = np.array(convert.rowmat_from_blender(Rf), dtype=np.float64).reshape(4, 4)
            rel = sf / rest_scale[n]
            q = rowmat_to_quat(g[:3, :3])
            if prev is not None and sum(x * y for x, y in zip(prev, q)) < 0:
                q = tuple(-v for v in q)
            prev = q
            qs.append(q)
            ts.append(tuple(float(v) for v in g[3, :3]))
            ss.append(tuple(float(v) for v in rel))
            w = np.eye(4)
            w[:3, :3] = np.diag(rel) @ g[:3, :3]
            w[3, :3] = g[3, :3]
            expected[(f, b)] = w
        if len({tuple(round(v, 7) for v in q) for q in qs}) == 1:
            tracks.append(dict(kind=0x1C, target=b, keys=[qs[0]]))
        else:
            tt, kk = decimate(times, qs, 2e-7, rot=True)
            tracks.append(dict(kind=0x1C, target=b, times=tt, keys=kk))
        tt, kk = decimate(times, ts, 2e-4)
        tracks.append(dict(kind=0x1D, target=b, times=tt, keys=kk))
        if any(abs(v - 1.0) > 1e-4 for s_ in ss for v in s_):
            tt, kk = decimate(times, ss, 1e-4)
            tracks.append(dict(kind=10, target=b, times=tt, keys=kk))
    for t in tracks:
        if 'times' in t and len(t['times']) == 1:
            del t['times']
    parents = [-1] + [0] * (len(bone_names) - 1)
    return dict(frame_count=STEP * L, flag=1, hierarchy=parents, tracks=tracks), expected


def check_loop(data, expected, n_bones, L):
    """Worst (3x3 abs error, translation error relative to its magnitude) of the written loop against the source
    worlds over a spread of frames (anm_dump.evaluate_pose -- the DLL's evaluator)."""
    parsed = A.parse_anm(data)
    parents = [-1] + [0] * (n_bones - 1)
    worst_r = worst_t = 0.0
    keyed = sorted({f for f, _b in expected})       # bones keyed at different strides
    picks = {keyed[0], keyed[-1]} | {keyed[min(len(keyed) - 1, round(i * (len(keyed) - 1) / 8.0))] for i in range(9)}
    for f in sorted(picks | {1, 2, L - 1} & set(keyed)):
        pose = A.evaluate_pose(parsed, STEP * f, parents)
        for b in range(1, n_bones):
            if (f, b) not in expected:
                continue
            w = np.array(pose[b]['world'], dtype=float).reshape(4, 4)
            e = expected[(f, b)]
            worst_r = max(worst_r, float(np.abs(w[:3, :3] - e[:3, :3]).max()))
            worst_t = max(worst_t, float(np.abs(w[3, :3] - e[3, :3]).max()) / max(1.0, float(np.abs(e[3, :3]).max())))
    return worst_r, worst_t


def offset_series(plan, axis, L):
    """Per-source-frame offsets 0..L of a planned material axis: the clip's curve tiled over L,
    each repeat continuing the previous one's drift (a scroll's 0 -> 1 per period stays
    monotonic; a flip-book's drift is 0)."""
    c = plan['curves'][axis]
    speed = plan.get('speed', 1.0)
    P_ = int(round(plan['clip'].frames / speed))      # ported frames per source period
    base = [c.sample(f * speed)[0] for f in range(P_ + 1)]
    drift = base[P_] - base[0]
    return [base[f % P_] + (f // P_) * drift for f in range(L + 1)]


def is_stepped(vals, scale):
    """A flip-book axis: a sub-tile scale (the offset picks cells), or frame-to-frame jumps of a
    tenth of a tile or more (sa03e's fire walks a whole-tile sheet in quarters)."""
    if scale < 1.0 - 1e-6:
        return True
    d = sorted(abs(vals[f] - vals[f - 1]) for f in range(1, len(vals)) if abs(vals[f] - vals[f - 1]) > 1e-7)
    return bool(d) and d[len(d) // 2] >= 0.1


def unwrap(vals):
    """A scroll made continuous: each value moved by the whole tiles that bring it closest to the
    previous value + the previous slope (the source wraps 1.97 -> 0 at a loop seam; a tile
    repeat is invisible, interpolating back across it is not)."""
    out = [float(vals[0])]
    for f in range(1, len(vals)):
        pred = out[-1] + ((out[-1] - out[-2]) if f >= 2 else 0.0)
        v = float(vals[f])
        out.append(v + round(pred - v))
    return out


def offset_keys(vals, stepped, tol=1e-6):
    """(World times, values) of one offset axis: a flip-book axis holds each source frame's cell
    for its two World frames -- a PAIR of keys on the frame where it changes (the DLL's sampler
    takes the later of two equal-time keys); a scroll interpolates its unwrapped values.
    Collinear keys are dropped."""
    if not stepped:
        vals = unwrap(vals)
    times, out = [0], [float(vals[0])]
    for f in range(1, len(vals)):
        v, prev = float(vals[f]), float(vals[f - 1])
        if stepped:
            if abs(v - prev) > 1e-7:
                times += [STEP * f, STEP * f]
                out += [prev, v]
            continue
        times.append(STEP * f)
        out.append(v)
    if times[-1] != STEP * (len(vals) - 1):
        times.append(STEP * (len(vals) - 1))
        out.append(float(vals[-1]))
    t2, v2 = [times[0]], [out[0]]
    for t_, v_ in zip(times[1:], out[1:]):
        t2.append(t_)
        v2.append(v_)
        while len(t2) >= 3 and t2[-3] < t2[-2] < t2[-1]:
            a0, a1, a2 = t2[-3:]
            b0, b1, b2 = v2[-3:]
            if abs(b0 + (b2 - b0) * (a1 - a0) / (a2 - a0) - b1) > tol:
                break
            del t2[-2]
            del v2[-2]
    return t2, v2


def material_clip(objects, plans, model):
    """The part's .sanm spec (+ L, the per-track expected series) or None: one target per animated
    material (identity + shader hash read back from the exported .model), kind-8 tracks on
    params 2 / 3 (m_vTexAnime offU / offV)."""
    used, seen = [], set()
    for ob in objects:
        for mat in ob.data.materials:
            if mat.name in plans and mat.name not in seen:
                seen.add(mat.name)
                used.append((mat, plans[mat.name]))
    if not used:
        return None
    L = clip_length([pl['clip'].frames / pl.get('speed', 1.0) for _m, pl in used])
    by_ident = {mm['identity']: mm for mm in model['materials']}
    targets, tracks, n_params = [], [], 0
    for mat, pl in used:
        ident = K.pack_identity(mat.name)
        mm = by_ident.get(ident)
        assert mm is not None, 'material %s not in the exported model' % mat.name
        n_params += len(pl['curves'])
        assert n_params <= MAX_MAT_PARAMS, 'more animated material params than the frame board holds'
        slot = len(targets)
        targets.append(dict(identity=ident, identity2=0, hash=mm['shader_hash'], flags=0x2000))
        for ax in sorted(pl['curves']):
            vals = offset_series(pl, ax, L)
            stepped = is_stepped(vals, pl['scale'][ax])
            times, keys = offset_keys(vals, stepped)
            if not stepped:
                vals = unwrap(vals)
            assert times[-1] <= 0xFFFF and len(times) < 0xFFFF
            tracks.append(dict(kind=8, target=slot, sub=2 + ax, times=times, keys=[(v,) for v in keys], expected=vals, stepped=stepped))
    return dict(frame_count=STEP * L, flag=1, fps=60, material_tracks=tracks, material_targets=targets), L


def check_material_clip(data, spec, L):
    """Worst deviation of the written .sanm (anm_dump.evaluate_materials) from the source offsets
    at the source frames (a flip-book: the cell held over the frame)."""
    parsed = A.parse_anm(data)
    worst = 0.0
    for f in range(0, L + 1, max(1, L // 97)):
        got = A.evaluate_materials(parsed, float(STEP * f) + (0.5 if f < L else 0.0))
        for t in spec['material_tracks']:
            want = t['expected'][f]
            if not t['stepped'] and f < L:
                want = want + (t['expected'][f + 1] - want) * 0.25
            d = got[t['target']][t['sub']] - want
            worst = max(worst, abs(d - round(d)))     # whole tiles are invisible
    return worst


def export_part(mapset, key, part, objs, rig, anim, plans, R, S):
    """Write one part (static, or rigged with its loops). Returns a one-line summary."""
    d = os.path.join(mapset, 'gm_%s_%s' % (key, part))
    os.makedirs(d, exist_ok=True)
    model_path = os.path.join(d, 'gm_%s_%s.model' % (key, part))
    bones = sorted({b for o in objs for b in o.get('ddr_bones', ['root']) if b != 'root'})
    anim_mats = [m for o in objs for m in o.data.materials if m.name in plans]
    if not bones:
        # no object loop: a static part (the static `mdl_bg_*` shaders; a `.sanm` binds by material
        # identity either way -- stock gm_boom00_bg scrolls its sky so). Rigging a part that has
        # only a material loop moved it to `mdl_ch_*` and Rocky Mountain Village's cut-out trees
        # drew black boxes (2026-10-26)
        for o in objs:
            o.select_set(True)
        written, spec = export_model.export_model(model_path, None, objs, write_textures=True)
        info = 'gm_%s_%s: %d meshes, %d textures' % (key, part, len(spec['meshes']), len([w for w in written if w.endswith('.dds')]))
        if anim_mats:
            info += write_sanm(d, key, part, objs, plans, K.parse_model(open(model_path, 'rb').read()))
        return info
    bone_names = ['root'] + bones
    arm, rest_scale = build_rig('gm_%s_%s' % (key, part), bone_names, rig, anim, R, S)
    for o in objs:
        o.parent = arm
        if 'root' not in o.vertex_groups:
            g = o.vertex_groups.new(name='root')
            if not o.get('ddr_bones') or list(o['ddr_bones']) == ['root']:
                g.add(list(range(len(o.data.vertices))), 1.0, 'REPLACE')
        mod = o.modifiers.new('Armature', 'ARMATURE')
        mod.object = arm
    bpy.context.view_layer.update()
    written, spec = export_model.export_model(model_path, arm, objs, write_textures=True)
    raw = open(model_path, 'rb').read()
    model = K.parse_model(raw)
    assert len(model['bones']) == len(bone_names), (len(model['bones']), len(bone_names))
    info = 'gm_%s_%s: %d meshes, %d textures, %d bones' % (key, part, len(spec['meshes']), len([w for w in written if w.endswith('.dds')]), len(bone_names))
    root_bind = np.array(model['bones'][0]['bind'], dtype=np.float64).reshape(4, 4)
    assert np.abs(root_bind - np.eye(4)).max() < 1e-5, 'root bind is not the game identity'
    if bones:
        # the exporter must keep the binds as built (a re-framed bind would not commute with the
        # relative-scale keys)
        for b, n in enumerate(bone_names[1:], start=1):
            fb = np.array(model['bones'][b]['bind'], dtype=np.float64).reshape(4, 4)
            mb = np.array(convert.rowmat_from_blender(arm.data.bones[n].matrix_local), dtype=np.float64).reshape(4, 4)
            assert np.abs(fb - mb).max() < 1e-4, 'bone %s: the exporter re-framed the bind' % n
        L = clip_length([p for n in bones for p in rig[n]['periods']])
        lspec, expected = loop_spec(bone_names, rig, anim, R, S, rest_scale, L)
        data = A.write_anm(lspec)
        err_r, err_t = check_loop(data, expected, len(bone_names), L)
        assert err_r < 3e-3 and err_t < 2e-4, '%s: loop error rot %.5f trans %.5f rel' % (part, err_r, err_t)
        open(os.path.join(d, 'gm_%s_%s_play_loop.anm' % (key, part)), 'wb').write(data)
        info += ', loop %d frames (%.1f s, %d anchors, %d bytes, err %.1e / %.1e rel)' % (
            lspec['frame_count'], lspec['frame_count'] / 60.0, len(bones), len(data), err_r, err_t)
    return info + write_sanm(d, key, part, objs, plans, model)


def write_sanm(d, key, part, objs, plans, model):
    res = material_clip(objs, plans, model)
    if not res:
        return ''
    sspec, L = res
    sdata = A.write_anm(sspec)
    err = check_material_clip(sdata, sspec, L)
    assert err < 1e-3, '%s: sanm error %.2e' % (part, err)
    open(os.path.join(d, 'gm_%s_%s_play_loop.sanm' % (key, part)), 'wb').write(sdata)
    return ', sanm %d frames (%.0f s, %d materials, %d tracks, err %.1e)' % (
        sspec['frame_count'], sspec['frame_count'] / 60.0, len(sspec['material_targets']), len(sspec['material_tracks']), err)


def split_part(objs, cap=MAX_ANCHORS):
    """Greedy object groups of at most `cap` distinct anchors (frame board: 64 bones)."""
    groups, cur, cur_b = [], [], set()
    for o in objs:
        b = {x for x in o.get('ddr_bones', ['root']) if x != 'root'}
        if cur and len(cur_b | b) > cap:
            groups.append(cur)
            cur, cur_b = [], set()
        cur.append(o)
        cur_b |= b
    if cur:
        groups.append(cur)
    return groups


def preview_materials():
    """EEVEE stand-ins for the game's unlit shaders (Workbench ignores the m_vTexAnime Mapping
    nodes): emission = texture x COLOR0, alpha-tested ('dec' / 'bg') or blended by the part."""
    for mat in bpy.data.materials:
        if not mat.use_nodes:
            continue
        nt = mat.node_tree
        tex = next((n for n in nt.nodes if n.type == 'TEX_IMAGE'), None)
        out = next((n for n in nt.nodes if n.type == 'OUTPUT_MATERIAL'), None)
        if tex is None or out is None:
            continue
        vc = nt.nodes.new('ShaderNodeVertexColor')
        vc.layer_name = 'Col'
        mul = nt.nodes.new('ShaderNodeMix')
        mul.data_type = 'RGBA'
        mul.blend_type = 'MULTIPLY'
        mul.inputs['Factor'].default_value = 1.0
        nt.links.new(tex.outputs['Color'], mul.inputs['A'])
        nt.links.new(vc.outputs['Color'], mul.inputs['B'])
        em = nt.nodes.new('ShaderNodeEmission')
        nt.links.new(mul.outputs['Result'], em.inputs['Color'])
        tr = nt.nodes.new('ShaderNodeBsdfTransparent')
        amul = nt.nodes.new('ShaderNodeMath')
        amul.operation = 'MULTIPLY'
        nt.links.new(tex.outputs['Alpha'], amul.inputs[0])
        nt.links.new(vc.outputs['Alpha'], amul.inputs[1])
        flags = int(mat.get('ddr_flags', 0))
        additive = int(mat.get('ddr_flags2', 0)) == 4
        mix = nt.nodes.new('ShaderNodeMixShader')
        if additive:
            # emission x alpha over a fully transparent surface = an additive layer
            sc_ = nt.nodes.new('ShaderNodeMath')
            sc_.operation = 'MULTIPLY'
            nt.links.new(amul.outputs[0], sc_.inputs[0])
            sc_.inputs[1].default_value = 1.0
            nt.links.new(sc_.outputs[0], em.inputs['Strength'])
            add = nt.nodes.new('ShaderNodeAddShader')
            nt.links.new(em.outputs[0], add.inputs[0])
            nt.links.new(tr.outputs[0], add.inputs[1])
            nt.links.new(add.outputs[0], out.inputs['Surface'])
            mat.surface_render_method = 'BLENDED'
            mat.use_backface_culling = False
            continue
        if flags & 0x0040:
            nt.links.new(amul.outputs[0], mix.inputs['Fac'])
            mat.surface_render_method = 'BLENDED'
        else:
            gt = nt.nodes.new('ShaderNodeMath')
            gt.operation = 'GREATER_THAN'
            gt.inputs[1].default_value = 0.5
            nt.links.new(amul.outputs[0], gt.inputs[0])
            nt.links.new(gt.outputs[0], mix.inputs['Fac'])
            mat.surface_render_method = 'DITHERED'
        nt.links.new(tr.outputs[0], mix.inputs[1])
        nt.links.new(em.outputs[0], mix.inputs[2])
        nt.links.new(mix.outputs[0], out.inputs['Surface'])
        mat.use_backface_culling = not (flags & 0x0001)


def preview(out_dir, key, extent, frame_tag=''):
    P.studio()
    sc = bpy.context.scene
    sc.render.engine = 'BLENDER_EEVEE'
    try:
        sc.world.use_nodes = False
    except (AttributeError, TypeError):
        pass
    sc.display.shading.show_backface_culling = True
    t = Vector((0.0, 0.0, 1.0))
    cam = bpy.data.objects.get('Preview camera') or bpy.data.objects.new('Preview camera', bpy.data.cameras.new('Preview camera'))
    if cam.name not in sc.collection.objects:
        sc.collection.objects.link(cam)
    cam.data.clip_end = 5000.0
    for tag, pos, scale in (('front', Vector((0.0, -6.0, 1.6)), 6.0), ('front_far', Vector((0.0, -25.0, 6.0)), 30.0),
                            ('back', Vector((0.0, 6.0, 1.6)), 6.0), ('side', Vector((8.0, 0.0, 1.6)), 8.0),
                            ('top', Vector((0.0, -0.001, 60.0)), max(40.0, extent))):
        P.render_camera(os.path.join(out_dir, '%s_%s%s.png' % (key, tag, frame_tag)), pos, t, scale, res=(1200, 700))
    # what the DDR camera sees (perspective, from the dancer's front)
    cam.data.type = 'PERSP'
    cam.data.lens = 22.0
    cam.location = Vector((0.0, -4.5, 1.4))
    cam.rotation_euler = (Vector((0.0, 4.0, 1.6)) - cam.location).to_track_quat('-Z', 'Y').to_euler()
    sc.camera = cam
    sc.render.filepath = os.path.join(out_dir, '%s_persp%s.png' % (key, frame_tag))
    bpy.ops.render.render(write_still=True)


def port(sid):
    label = STAGES.get(sid, sid)
    key = 'jj' + sid
    out_dir = os.path.join(OUT_BASE, label)
    os.makedirs(out_dir, exist_ok=True)
    P.fresh_scene()
    parts, stats, sheets, anim, rig, plans = build_stage(sid, out_dir)
    print('%s (%s): %d clumps, %d verts, %d tris, %d decal meshes dropped, %d water meshes, %d anchors, %d animated materials, skipped %s, untextured %s' % (
        sid, label, stats['clumps'], stats['verts'], stats['tris'], stats['decals'], stats['water'], stats['anchors'], stats['anim_mats'],
        stats['skipped'], sorted(x for x in stats['notex'] if x)))
    print('  animated materials: %s' % ', '.join(stats['anim_sources']))
    for part in PART_ORDER:
        print('  part %-3s objects=%d verts=%d' % (part, len(parts[part]), sum(len(o.data.vertices) for o in parts[part])))
    R = Matrix.Rotation(FACING_ROT[FACING] * 3.141592653589793 / 180.0, 4, 'Z')
    S = Matrix.Scale(SCALE, 4)
    export = os.path.join(out_dir, 'export')
    shutil.rmtree(export, ignore_errors=True)
    mapset = os.path.join(export, 'mapset_' + key)
    present = []
    for part in PART_ORDER:
        objs = parts[part]
        if not objs:
            continue
        for gi, group in enumerate(split_part(objs)):
            name = part if gi == 0 else '%s%d' % (part, gi + 1)
            print('  wrote ' + export_part(mapset, key, name, group, rig, anim, plans, R, S))
            present.append(name)
    write_sidecar(os.path.join(export, 'map_resources.rlist.txt'), key, present, animated=bool(rig or plans))
    if sid in CAMERA_FROM and os.path.isdir(CAMERA_FROM[sid]):
        cam_dir = os.path.join(mapset, 'camera')
        os.makedirs(cam_dir, exist_ok=True)
        n = 0
        for f in sorted(os.listdir(CAMERA_FROM[sid])):
            if f.endswith('.camanm'):
                shot = f.rsplit('_', 1)[-1]          # griffin_st01.camanm -> st01
                shutil.copy(os.path.join(CAMERA_FROM[sid], f), os.path.join(cam_dir, '%s_%s' % (key, shot)))
                n += 1
        print('  cameras: %d shots borrowed from %s' % (n, os.path.relpath(CAMERA_FROM[sid], REPO)))
        # the borrowed shots were checked against the stage at CAMERA_FROM_SCALE: scale their eyes
        # with the stage, then derive the `_non` cut-aways the director cuts to at every dance-clip
        # change (without them the set kept filming through the clip switch, 2026-10-26)
        import gen_cutaway_shots as G
        G.main([cam_dir, '--prefix', key, '--scale', repr(SCALE / CAMERA_FROM_SCALE)]
               + (['--far', repr(CAMERA_FAR[sid])] if sid in CAMERA_FAR else []))
    all_objs = [o for p in PART_ORDER for o in parts[p]]
    xs = [v.co.x for o in all_objs for v in o.data.vertices if 'bg' not in o.name]
    extent = max(abs(min(xs)), abs(max(xs))) if xs else 50.0
    bpy.ops.wm.save_as_mainfile(filepath=os.path.join(out_dir, key + '.blend'))
    if PREVIEW:
        preview_materials()
        preview(out_dir, key, extent)
    print('STAGE COMPLETE', sid, label, key)


ids = os.environ.get('STAGES', 'sd04a')
ids = sorted(STAGES) if ids == 'all' else [s.strip() for s in ids.split(',') if s.strip()]
failed = []
for sid in ids:
    try:
        port(sid)
    except Exception as e:  # noqa: BLE001
        import traceback
        traceback.print_exc()
        failed.append((sid, repr(e)))
print('BATCH DONE %d ported, %d failed %s' % (len(ids) - len(failed), len(failed), failed))
