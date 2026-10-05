"""EXAMPLE / PORT: the 3D stages of the zan-engine HOTTEST PARTY games -- DanceDanceRevolution
FuruFuru Party (Wii, JP 2008 = HOTTEST PARTY 2), DanceDanceRevolution MUSIC FIT (Wii, JP 2009 =
HOTTEST PARTY 3) and DanceDanceRevolution HOTTEST PARTY 4 / 5 (Wii, EU 2010 / 2011) -- as
Background Dancers custom stages, fed by the zan decoder in scripts/zan_dump.py (formats + RE:
docs/wii_ddr_hottest_party_2_3_research.md, docs/wii_ddr_hottest_party_4_5_research.md). The part /
loop / camera machinery is port_stage_hottest.py's (HOTTEST PARTY 1); only the sources differ.

A stage (stage/STG<nnn>.bin) is a set of ZMB models, each with its own one-loop ZAB motion of node
SRT tracks: DRAW_* (the stage and set), BG_* (the backdrop), OBJ[AB]_[NZS]_<name>_* props placed by
the COL_* layout model's OBJSET_<name>_<nn> nodes (COL's own meshes are cull hulls; its EFF / LIG
nodes are effect and light spots), plus its camera shots. Per stage:
  1. every (model, instance) becomes an entry; a mesh node's vertices (node-local) are baked
     through its frame-0 world (node world under its motion x the instance world under COL's
     motion) into game metres (zan_dump.GAME_SCALE, the dancers' scale); the material picks the
     World blend group (zan_dump.material_mode: additive -> `add`, ZERO+INVSRCALPHA -> `sub`, a
     soft alpha blend with real partial alpha -> `ble`, else `dec`, alpha-tested); CULLING as on the
     Wii: a material's no-cull flag (material_mode) -> two-sided, else single-sided in the Wii's
     own winding (hsf_dump.cull_winding; a mesh under a mirroring world stays two-sided) -- until
     2026-10-05 every mesh shipped two-sided and back-to-back faces z-fought; COLOR0 = the vertex
     colours;
  2. MOVIE SCREENS: the `root` quad of a `*_MOV*` prop (where the game plays its stage movie / the
     song's PV) is textured `offscreen1` (README "Stage screens": World's STAGE SCREENS mode plays
     the song's movie there), its v range remapped onto the 16:9 band, opaque white, no animation;
     HP4 / HP5: every mesh on a colour-group-92 material (zan_dump.is_screen_material) likewise,
     keeping its blend and vertex alpha, its v mapped onto the band as authored;
  3. FLIP-BOOKS (a material cycling TPL images, zan_dump.flip_book) become ATLASES: World's .sanm
     only animates shader parameters, so the frames go side by side along the axis that does not
     scroll, the triangles are clipped at that axis' tile lines into one cell, and the UV offset
     steps from cell to cell (apply_atlases; each atlas is sampling-checked against the source);
  4. parts: the backdrop's opaque meshes -> `bg` (priority -2); everything else one part per
     (blend group, loop group). Entries whose loop length divides a longer one share its part; a
     part holds at most 63 animated anchors and 48 animated material floats (overflow opens
     `dec2`, ...);
  5. rig per part: `root` + one FLAT bone per animated anchor (a mesh's deepest animated
     ancestor, or the instance itself when only COL moves it), every vertex rigidly on it; the
     bind = the nearest rotation of the anchor's rest world (a shearing anchor's: its motion's
     principal axes), the keys = bind . rest^-1 . world(t) against the bind the exporter wrote;
  6. `gm_<key>_<part>_play_loop.anm` (loop bit): per bone q / t / scale, keys every 2nd frame +
     a wrap key, checked against the zan worlds (a node rotating under a non-uniformly scaled
     parent shears, which TRS bones cannot carry: logged as SHEAR with its worst vertex offset);
  7. `gm_<key>_<part>_play_loop.sanm` (every part with animated materials, static ones too): the
     UV keys / constant scrolls (the texture-matrix translation (-u, +v), zan_dump.texmtx_offset)
     and the atlas steps on params 2 / 3, its own clip (the lcm of the periods, <= 6 min), steps
     as two keys on one frame; checked frame by frame against the analytic offsets;
  8. cameras: the stage's shots -> `camera/<key>_st01..`, the generic dance cameras of
     game/GAME_DEF_CAM.bin /#0 -> `_non01..` (position + aim, no roll; the FOV is MTXPerspective's
     vertical angle, kept on World's 16:9 frame); HP4 / HP5 stages with fewer than three moving
     shots of their own get GAME_DEF_CAM's eight 6 s front shots as mains too (main_cameras);
  9. sidecar `map_resources.rlist.txt`: `<key>, 000000, 000000, bg:-2, dec, ..., ble:-1`.
STG<nnn>_S.bin (the split-screen copies) are not ported, nor MUSIC FIT's STG000 / STG041..055
(FuruFuru Party's again, shipped once from there with their screens: is_hp2_reexport). HP4 / HP5
port what zan_dump.plan_stage_ports leaves (planned_stages) less NEAR_DUPLICATES: HP4 26 stages, HP5 14 -- not their
re-shipped HP2 / HP3 / HP4 stages, their in-disc duplicates, the COL-only STG4xx, STG<nnn>_P (HP4's
copies of HP2's STG042 / 046) or HP4 / HP5's props no OBJSET node places (unused, at the origin).

Inputs (environment):
  GAME        hp2 | hp3 | hp4 | hp5 (default hp2); HP2_GAME .. HP5_GAME the dumped disc trees
              (scripts/extract_wii_ddr_data.py disc ...; default ~/Desktop/DDR Wii ISOs/<title>;
              hp4 / hp5 read the earlier discs too, to skip what those already ship)
  STAGES      comma list (STG027, 27, ...), default the first stage, or 'all'
  OUT_BASE    default data_mods/custom_models/stages/HOTTEST PARTY 2|3|4|5 (one folder per stage,
              `Stage 27` ..; keys hp2stage027 .. hp5stage030)
  PREVIEW     1 = render Workbench / EEVEE previews of the RE-IMPORTED parts into PREVIEW_DIR (a
              test card on the screens), plus a ported dancer through two of the written .camanm
Run: GAME=hp2 STAGES=all /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
       --python tools/blender_ddr_addon/examples/port_stage_hottest2.py
"""
import hashlib
import math
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
import zan_dump as Z  # noqa: E402
import hsf_dump as H  # noqa: E402  (consistent_winding)
import tzm_dump as T  # noqa: E402  (look_at_rows, rowmat_to_quat, world_camanm_fov)
import extract_wii_ddr_data as W  # noqa: E402
from blender_ddr_addon import convert, export_model, import_anm, import_model  # noqa: E402
from blender_ddr_addon.codec import anm as A  # noqa: E402
from blender_ddr_addon.codec import ktmdl as K  # noqa: E402

GAME = os.environ.get('GAME', 'hp2').lower()
assert GAME in ('hp2', 'hp3', 'hp4', 'hp5'), GAME
DISCS = {'hp2': '~/Desktop/DDR Wii ISOs/Dance Dance Revolution - Furu Furu Party (Japan)', 'hp3': '~/Desktop/DDR Wii ISOs/Dance Dance Revolution - Music Fit (Japan)',
         'hp4': '~/Desktop/DDR Wii ISOs/Hottest Party 4 (Europe)', 'hp5': '~/Desktop/DDR Wii ISOs/Hottest Party 5 (Europe)'}


def disc_dir(game):
    return os.path.expanduser(os.environ.get('%s_GAME' % game.upper(), DISCS[game]))


DISC = disc_dir(GAME)
OTHER_DISC = disc_dir('hp2')
SOURCES = {g: 'HOTTEST PARTY %s' % g[2] for g in DISCS}
SOURCE = SOURCES[GAME]
TITLE = {'hp2': 'DanceDanceRevolution FuruFuru Party (Wii, JP) = HOTTEST PARTY 2',
         'hp3': 'DanceDanceRevolution MUSIC FIT (Wii, JP) = HOTTEST PARTY 3',
         'hp4': 'DanceDanceRevolution HOTTEST PARTY 4 (Wii, EU)',
         'hp5': 'DanceDanceRevolution HOTTEST PARTY 5 (Wii, EU)'}[GAME]
OUT_BASE = os.path.expanduser(os.environ.get('OUT_BASE', os.path.join(REPO, 'data_mods', 'custom_models', 'stages', SOURCE)))
PREVIEW = os.environ.get('PREVIEW', '0') == '1'
PREVIEW_DIR = os.environ.get('PREVIEW_DIR') or os.path.join(tempfile.gettempdir(), 'hottest_party_%s_stage_previews' % GAME)
PREVIEW_DANCER = os.path.expanduser(os.environ.get('PREVIEW_DANCER', os.path.join(
    REPO, 'data_mods', 'custom_models', 'dancers', 'HOTTSTPARTY 1-3', 'Rena 5', 'pl_hprena05', 'pl_hprena05.model')))
STAGE_DIR = os.path.join(DISC, 'stage')

S = Z.GAME_SCALE
KEY_STEP = 2                 # keys every 2nd frame of the 60 fps timeline
MAX_ANCHORS = 63             # + root = scene3d::frame_board::MAX_BONES
MAX_MAT_PARAMS = 48          # scene3d::frame_board::MAX_MAT_PARAMS
MAX_LOOP = 7200              # a combined (prop x layout) loop longer than this keeps the longer one
FLAGS = {'dec': (0x0001, 0), 'bg': (0x0001, 0), 'ble': (0x02C1, 0), 'add': (0x06C1, 4), 'sub': (0x06C1, 8)}
KIND_ORDER = ['bg', 'dec', 'add', 'sub', 'ble']
PRIORITY = {'bg': -2, 'ble': -1}
NEAR, FAR = 0.1, 32768.0


def stage_list():
    if GAME in ('hp4', 'hp5'):
        return planned_stages(GAME)
    out = []
    for f in sorted(os.listdir(STAGE_DIR)):
        if not (f.startswith('STG') and f.endswith('.bin')) or '_S' in f or '_EFF' in f:
            continue
        if GAME == 'hp3' and is_hp2_reexport(f):
            continue       # FuruFuru Party's stage again: shipped once, in HOTTEST PARTY 2
        out.append(f[:-4])
    return out


# MUSIC FIT's shipped stages (stage_list(hp3) -- its STG202 is FuruFuru Party's STG101 again,
# shipped twice since the HP2 / HP3 port)
HP3_SHIPPED = ['STG%03d' % n for n in list(range(101, 112)) + list(range(201, 207))]


# planned_stages' result with all four discs present (2026-10-04); used when the older dumps are
# not at hand (the plan needs them to know what HOTTEST PARTY 2 / 3 / 4 already ship)
RECORDED_PLAN = {
    'hp4': ['STG001', 'STG002', 'STG003', 'STG004', 'STG005', 'STG006', 'STG007', 'STG008', 'STG042', 'STG043',
            'STG044', 'STG101', 'STG102', 'STG103', 'STG104', 'STG105', 'STG106', 'STG200', 'STG201', 'STG301',
            'STG402', 'STG403', 'STG404', 'STG405', 'STG406', 'STG431', 'STG432'],
    'hp5': ['STG001', 'STG002', 'STG003', 'STG012', 'STG013', 'STG014', 'STG015', 'STG016', 'STG018', 'STG019',
            'STG020', 'STG028', 'STG029', 'STG030', 'STG426'],
}


# stages the content signature cannot fold: HP4 STG201 is STG301 (shipped) with half-size textures
# and its camera rig nudged; HP5 STG012 is STG003's DRAW_STG102_01 set at about half the polygons,
# its two stacked monitor-wall layers swapped (the song's PV in front of the stage video) -- the
# same set on screen
NEAR_DUPLICATES = {'hp4': {'STG201': 'STG301 at half the texture resolution (the same set)'},
                   'hp5': {'STG012': 'STG003 at half the polygons, PV / stage-video wall layers swapped (the same set)'}}


# FLIGHT stages (research 2/3 §7.5): the dancers take off from a launch platform and fly through a
# tunnel. Background Dancers plays the take-off + flight clips there (`mapset_<key>/flight.txt` marks
# the stage) and switches the stage at the end of the take-off. MUSIC FIT's main.dol runs the switch
# as an intro script over the stage objects (`FUN_8003729c` arms it, `FUN_80037354` runs it; the
# objects are the stage file's models in file order): until the switch the platform (STG109's stage,
# object 5, dims to 0.15), its sky + sea (object 3, fades out at 6-7 s), a plain space (object 4) and
# the tunnel mouth (object 6, fades in at 5-6 s and plays its one-shot opening -- Dummy_scale 0.3 -> 1
# over frames 0..134, then held) show; at the switch the tunnel (DRAW_*, object 0) and its space with
# the planets (BG_*, object 1) replace them all. The parts are named by role -- `pre_plat_*`,
# `pre_sky_*`, `pre_space_*`, `pre_hole_*`, `fly_*` -- and the DLL replays the script on them
# (`director_math::intro_look`). A model whose motion does not return to its first pose is a
# one-shot clip there (no loop bit, its last key the end pose) instead of re-opening every 33 s.
# Only a stage with the launch platform has phases: FuruFuru Party's ring tunnels (STG102 / 103) are
# the marker alone.
FLIGHT_PHASES = [('pre_plat', re.compile(r'^OBJA_Z_DRAW109', re.I)),
                 ('pre_sky', re.compile(r'^OBJA_Z_BG109', re.I)),
                 ('pre_space', re.compile(r'^OBJA_Z_BG201', re.I)),
                 ('pre_hole', re.compile(r'^OBJA_Z_hole\d+', re.I)),
                 ('fly', re.compile(r'^(DRAW|BG)_STG\d+(_0\d)?$', re.I))]
FLIGHT_PLATFORM = re.compile(r'^OBJA_Z_DRAW109', re.I)
FLIGHT_STAGES = {'hp2': {'STG102', 'STG103'}, 'hp3': {'STG201', 'STG205', 'STG206'}, 'hp4': {'STG301'}}
FLIGHT_MARKER = 'flight.txt'


def is_flight_stage(stage):
    return stage in FLIGHT_STAGES.get(GAME, set())


def entry_phase(stage, stem, stems=()):
    """The flight role ('pre_plat' / 'pre_sky' / 'pre_space' / 'pre_hole' / 'fly') of model `stem`
    on a flight stage with a launch platform among `stems`, else None (always shown)."""
    if not is_flight_stage(stage) or not any(FLIGHT_PLATFORM.match(s) for s in stems):
        return None
    for phase, rx in FLIGHT_PHASES:
        if rx.match(stem):
            return phase
    return None


def planned_stages(game):
    """HOTTEST PARTY 4 / 5: zan_dump.plan_stage_ports against what the earlier sources ship --
    HP4 re-ships MUSIC FIT's STG041..055 (FuruFuru Party's) / 107..111 / 202..206, HP5 re-ships
    HP4's STG201 / 301 / 4xx and holds several stages twice; the bare COL-only STG4xx draw
    nothing. Prints the skip reasons."""
    hp2, hp3, hp4 = (os.path.join(disc_dir(g), 'stage') for g in ('hp2', 'hp3', 'hp4'))
    missing = [d for d in (hp2, hp3, hp4) if not os.path.isdir(d)]
    if missing:
        print('PLAN %s: no %s -- using the plan recorded with all four discs (RECORDED_PLAN)' % (
            GAME, ', '.join(Z._tilde(d) for d in missing)))
        return [p for p in RECORDED_PLAN[game] if p not in NEAR_DUPLICATES.get(game, {})]
    _p3, _s3, c3 = Z.plan_stage_ports(hp3, [(SOURCES['hp2'], hp2, None)], reexport_of=(SOURCES['hp2'], hp2))
    shipped = [(SOURCES['hp2'], hp2, None), (SOURCES['hp3'], hp3, HP3_SHIPPED)]
    p4, s4, c4 = Z.plan_stage_ports(hp4, shipped, c3)
    port, skipped = p4, s4
    if game == 'hp5':
        port, skipped, _c5 = Z.plan_stage_ports(STAGE_DIR, shipped + [(SOURCES['hp4'], hp4, p4)], {**c3, **c4})
    for stage, why in NEAR_DUPLICATES.get(game, {}).items():
        if stage in port:
            port = [p for p in port if p != stage]
            skipped[stage] = why
    for stage, why in sorted(skipped.items()):
        if why != 'draws nothing':
            print('SKIP %s %s: %s' % (GAME, stage, why))
    print('SKIP %s (draw nothing): %s' % (GAME, ' '.join(k for k, v in sorted(skipped.items()) if v == 'draws nothing')))
    return port


def is_hp2_reexport(f):
    """MUSIC FIT's STG000 is FuruFuru Party's byte for byte and its STG041..055 are re-exports of
    FuruFuru Party's (same members, the textures largely byte-identical, the movie-screen props
    replaced by plain geometry); its STG101..103 reuse the numbers for new stages (no member
    shared). A stage counts as FuruFuru Party's when the same-named file there shares at least one
    byte-identical named member."""
    other = os.path.join(OTHER_DISC, 'stage', f)
    if not os.path.exists(other):
        return False
    mine, theirs = open(os.path.join(STAGE_DIR, f), 'rb').read(), open(other, 'rb').read()
    if mine == theirs:
        return True
    a = {n: hashlib.md5(b).hexdigest() for _p, n, b in Z.members(mine) if n}
    b = {n: hashlib.md5(b).hexdigest() for _p, n, b in Z.members(theirs) if n}
    return any(b.get(n) == h for n, h in a.items())


STAGES = stage_list()


def stage_number(stage):
    return int(stage[3:6])


def stage_label(stage):
    return 'Stage %02d' % stage_number(stage)


def stage_key(stage):
    return '%sstage%03d' % (GAME, stage_number(stage))


# ---------------------------------------------------------------------------------------------
# loading
# ---------------------------------------------------------------------------------------------
def animated_nodes(model, motion, tol=1e-5):
    """Indices of the nodes whose own channels change over the loop."""
    out = set()
    if not motion:
        return out
    for nm, ch in motion['bones'].items():
        i = model['by_name'].get(nm)
        if i is None:
            continue
        for _k, (_fr, v) in ch.items():
            if len(v) and np.abs(v - v[0]).max() > tol:
                out.add(i)
                break
    return out


def load_stage(stage):
    """([entry dict(index, stem, kind, model, textures, motion, inst, length, worlds)], cams)."""
    blob = open(os.path.join(STAGE_DIR, stage + '.bin'), 'rb').read()
    src = Z.stage_sources(blob)
    inst = Z.stage_instances(src)
    col_model, col_motion = src['col'] if src['col'] else (None, None)
    col_anim = animated_nodes(col_model, col_motion) if col_model else set()
    entries = []
    for e in src['models']:
        frames = inst.get(e['stem'], []) if e['kind'] == 'obj' else [None]
        if e['kind'] == 'obj' and not frames:
            if GAME in ('hp4', 'hp5'):
                # HP4 / HP5 leave unused props in the archive: every one without an OBJSET node sits
                # at the origin (HP4 STG042 keeps three of HP2 STG011's sixteen, STG102 / 106 a spare
                # twin of a placed prop, STG103 STG003's monitor without its layout slots)
                print('  PROP %s: no OBJSET node places it -- not drawn' % e['stem'])
                continue
            print('  PROP %s: no OBJSET node places it -- drawn where it was modelled' % e['stem'])
            frames = [None]
        own = animated_nodes(e['model'], e['motion'])
        L_own = int(e['motion']['length']) if own else 0
        for fr in frames:
            fi = col_model['by_name'][fr] if fr else None
            inst_anim = fi is not None and fi in col_anim
            L_inst = int(col_motion['length']) if inst_anim else 0
            if L_own and L_inst:
                L = L_own * L_inst // math.gcd(L_own, L_inst)
                if L > MAX_LOOP:
                    print('  LOOP %s x %s: lcm %d > %d, keeping %d' % (e['stem'], fr, L, MAX_LOOP, max(L_own, L_inst)))
                    L = max(L_own, L_inst)
            else:
                L = L_own or L_inst
            entry = dict(index=len(entries), stem=e['stem'], kind=e['kind'], model=e['model'], textures=e['textures'],
                         motion=e['motion'], inst=fr, inst_index=fi, L_own=L_own, L_inst=L_inst, length=L,
                         animated=set(own) | ({0} if inst_anim else set()))
            entries.append(entry)
    stems = [e['stem'] for e in entries]
    for e in entries:
        e['worlds'] = make_worlds(e, col_model, col_motion)
        e['rest'] = e['worlds']([0.0])[0]
        e['unit_rest'] = e['worlds']([0.0], True)[0]
        e['phase'] = entry_phase(stage, e['stem'], stems)
        e['once'] = False
        if e['phase'] and e['L_own'] and not e['L_inst']:
            end = e['worlds']([e['L_own'] - 1e-3])[0]
            e['once'] = bool(np.abs(end[:, 3, :3] - e['rest'][:, 3, :3]).max() > 1.0 or
                             np.abs(end[:, :3, :3] - e['rest'][:, :3, :3]).max() > 1e-2)
            if e['once']:
                print('  FLIGHT %s: one-shot motion (%d frames, not a loop)' % (e['stem'], e['L_own']))
        if e['phase']:
            print('  FLIGHT %s: phase %s' % (e['stem'], e['phase']))
    return entries, src['cams']


def make_worlds(e, col_model, col_motion):
    def worlds(times, unit=False):
        t = np.asarray(times, dtype=np.float64)
        w = Z.posed_worlds(e['model'], e['motion'], t % e['L_own'] if e['L_own'] else t * 0, unit_scale=unit)
        if e['inst'] is not None:
            ti = t % e['L_inst'] if e['L_inst'] else t * 0
            wi = Z.posed_worlds(col_model, col_motion, ti, unit_scale=unit)[:, e['inst_index']]
            w = np.einsum('fnij,fjk->fnik', w, wi)
        return w
    return worlds


def generic_cameras():
    blob = open(os.path.join(DISC, 'game', 'GAME_DEF_CAM.bin'), 'rb').read()
    return [(p, Z.parse_cam(b)) for p, _n, b in Z.members(blob, 'cam') if p.startswith('/#0/') and p.count('/') == 2]


MIN_MAIN = 3                  # HP4 / HP5: fewer own shots than this -> add the fallback mains
FALLBACK_MAIN = range(78, 86)  # HP4 / HP5 GAME_DEF_CAM /#0/#78..#85: their new 6 s front shots


def main_cameras(cams, generic):
    """The `_st` (main) shots of a stage. HP2 / HP3: the stage's own. HP4 / HP5 choreograph most
    cameras per song (dance/DANCE_*_FRE.bin) and many stages carry a single default shot or none
    (HP5's are one static 3 s view; HP4's STG4xx have no camera file): there the own shots that
    move at all are kept and, below MIN_MAIN, the eight 6-second front shots HP4 added to
    GAME_DEF_CAM (dolly / pan / crane moves on the dancers, 2.4..4.1 m out) join them -- World's
    director keeps a fixed camera when a stage has no main clip."""
    if GAME not in ('hp4', 'hp5'):
        return cams
    own = [(p, c) for p, c in cams if Z.cam_length(c) > 0.05]
    if len(own) >= MIN_MAIN:
        return own
    extra = [generic[i] for i in FALLBACK_MAIN if i < len(generic)]
    print('  CAMERAS %d own shot(s) + %d GAME_DEF_CAM front shots as main' % (len(own), len(extra)))
    return own + extra


# ---------------------------------------------------------------------------------------------
# meshes -> part groups
# ---------------------------------------------------------------------------------------------
def alpha_class(rgba):
    a = rgba[..., 3]
    if a.min() == 255:
        return 'opaque'
    mid = (a > 8) & (a < 247)
    return 'partial' if mid.mean() > 0.002 else 'binary'


_ALPHA = {}


def world_kind(e, mi, has_vertex_alpha):
    mt = e['model']['materials'][mi]
    blend, soft, _two, _lit = Z.material_mode(mt)
    if blend == Z.BLEND_ADD:
        return 'add'
    if blend == Z.BLEND_DARKEN:
        return 'sub'
    if blend == Z.BLEND_OPAQUE or not soft:
        return 'dec'
    if has_vertex_alpha:
        return 'ble'
    b = mt['textures'][0] if mt['textures'] else None
    if b is not None and b < len(e['textures']):
        key = (id(e['textures']), b)
        if key not in _ALPHA:
            _ALPHA[key] = alpha_class(e['textures'][b])
        if _ALPHA[key] == 'partial':
            return 'ble'
    return 'dec'


SCREEN_TEXTURE = export_model.SCREEN_TEXTURE_KEY
SCREEN_BAND = (0.21875, 0.78125)


def is_screen(e, nd):
    """A movie screen: the `root` node's mesh of a `*_MOV*` prop -- the game (main.dol FUN_800386bc
    flags `_MOV` props 0x20000000) shows its stage movie / the song's PV there; the rest of the prop
    is bezel, letterbox bars and a glass overlay. The quad comes white with vertex alpha 0."""
    return '_MOV' in e['stem'].upper() and nd['name'] == 'root'


# HOTTEST PARTY 4 / 5 model their screens as ordinary meshes on a colour-group-92 material
# (zan_dump.is_screen_material: HP5's pv01_43 / pv02_CE / pv03_WI monitors, its movieBox wall,
# HP4's STG044 dome, the STG001 / 405 / 431 monitors). They keep their own blend and vertex alpha
# (STG044's dome shows the movie at a third of its strength, STG431's glows additively). MUSIC FIT
# uses the same group (its TV sets): on since the 2026-10-04 re-port.
GROUP_SCREENS = GAME in ('hp3', 'hp4', 'hp5')


def is_group_screen(e, mt):
    return GROUP_SCREENS and Z.is_screen_material(mt)


# STAGE VIDEOS: a colour-group-91 material is where the game plays the STAGE's own video
# (movie/stage/*.thp -- abstract VJ loops, not the song's PV): HP4 / HP5's `_Prm.bin` names it
# (zan_dump.stage_params: `quarter` = a 2x2 mosaic whose quadrants the surfaces' UVs pick,
# `single02`..), MUSIC FIT and HP4's plain stages leave the pick to the song (ani / fvo / mvo / pop /
# upt, 01..04), here fixed to DEFAULT_STAGE_VIDEO. The port decodes STAGE_VIDEO_FRAMES frames
# (ffmpeg) into a flip-book on that material -- the atlas machinery below does the rest.
VIDEO_SURFACES = GAME in ('hp3', 'hp4', 'hp5')
VIDEO_GROUP = 91
DEFAULT_STAGE_VIDEO = 'upt01'
STAGE_VIDEO_FRAMES = 16
STAGE_VIDEO_PX = 240          # a 16-frame strip of 240 px cells + gutters fits ATLAS_MAX unfolded
STAGE_VIDEO_STEP = 45         # 60 Hz frames per video frame (0.75 s; 12 s per loop)
_VIDEO = {}


def stage_video_name(stage):
    """The movie/stage/*.thp a stage's group-91 surfaces play. `_Prm` type 3 names it (`single02`:
    main.dol's `movie/stage/%s.thp`); type 2 `quarter` is a per-genre 2x2 mosaic, main.dol's
    `movie/stage/<genre>_%s%02d.thp` = upt / pop / mvo / fvo `_quarter01..03` (512 px squares;
    the disc's bare `quarter01.thp` -- HP4's a 640 x 480 4:3 cut -- is never opened, and the port
    used it until 2026-10-05); no name (MUSIC FIT, plain HP4 stages): the song's genre loop.
    The genre is the song's: fixed here to DEFAULT_STAGE_VIDEO's."""
    prm = Z.stage_params(open(os.path.join(STAGE_DIR, stage + '.bin'), 'rb').read())
    name = prm['movie'] if prm and prm['type'] in (2, 3) and prm['movie'] else DEFAULT_STAGE_VIDEO
    if prm and prm['type'] == 2:
        name = '%s_%s01' % (DEFAULT_STAGE_VIDEO[:3], name)
    return name


def video_frames(name):
    """[RGBA (STAGE_VIDEO_PX square) x STAGE_VIDEO_FRAMES] of movie/stage/<name>.thp, evenly over
    its first STAGE_VIDEO_FRAMES * STAGE_VIDEO_STEP / 60 s (wrapping a shorter video), or None."""
    if name in _VIDEO:
        return _VIDEO[name]
    import subprocess
    path = os.path.join(DISC, 'movie', 'stage', name + '.thp')
    out = None
    if os.path.exists(path):
        n, px = STAGE_VIDEO_FRAMES, STAGE_VIDEO_PX
        fps = 60.0 / STAGE_VIDEO_STEP
        # (ffmpeg's -stream_loop does not seek THP: a shorter video wraps here instead)
        raw = subprocess.run(['ffmpeg', '-v', 'error', '-i', path, '-vf',
                              'fps=%.6f,scale=%d:%d:flags=area' % (fps, px, px), '-frames:v', str(n),
                              '-f', 'rawvideo', '-pix_fmt', 'rgba', '-'], capture_output=True, check=True).stdout
        got = len(raw) // (px * px * 4)
        if got:
            frames = np.frombuffer(raw[:got * px * px * 4], np.uint8).reshape(got, px, px, 4)
            out = [frames[k % got].copy() for k in range(n)]
    print('  VIDEO %s: %s' % (name, '%d frames' % len(out) if out else 'not on the disc -- placeholder kept'))
    _VIDEO[name] = out
    return out


def is_video_material(mt):
    return VIDEO_SURFACES and Z.material_group(mt) == VIDEO_GROUP


def attach_video(e, stage):
    """Append the stage video's frames to the entry's pictures once; their indices, or None."""
    if 'video' not in e:
        frames = video_frames(stage_video_name(stage))
        e['video'] = None
        if frames:
            e['video'] = list(range(len(e['textures']), len(e['textures']) + len(frames)))
            e['textures'] = list(e['textures']) + frames
    return e['video']


def screen_kind(mt, col):
    """World blend group of a group-92 screen: additive stays additive, an alpha-blended one with
    real vertex alpha blends, anything else is an opaque screen (its placeholder texture's alpha
    no longer applies once the movie replaces it)."""
    blend, _soft, _two, _lit = Z.material_mode(mt)
    if blend == Z.BLEND_ADD:
        return 'add'
    if blend == Z.BLEND_ALPHA and (col[:, 3] < 0.999).any():
        return 'ble'
    return 'dec'


def mesh_records(e):
    """Every (mesh node, material) piece of one entry, baked into game space at its frame-0
    world: dict(kind, anchor, material, bitmap, pos, nrm, uv, col, tris, obj)."""
    model = e['model']
    nodes = model['nodes']
    out = []
    for nd in nodes:
        if not nd['submeshes']:
            continue
        anchor, i = None, nd['index']
        while i >= 0:
            if i in e['animated']:
                anchor = i
                break
            i = nodes[i]['parent']
        Wm = e['rest'][nd['index']]
        # a mirroring world reverses the screen winding; whether the engine re-flips its cull mode
        # there (FUN_8010dec8's cull-front argument) is not settled, so those few meshes (HP2 / HP3
        # STG043 / 045 backdrops, ~0.6 % of the single-sided ones) stay two-sided
        mirrored = bool(np.linalg.det(Wm[:3, :3]) < 0)
        for sm in nd['submeshes']:
            if not len(sm['pos']) or not sm['packets']:
                continue
            mi = sm['material']
            if mi >= len(model['materials']):
                continue
            mt = model['materials'][mi]
            index, P_, N_, UV_, C_, tri_out = {}, [], [], [], [], []
            for pk in sm['packets']:
                n = pk['corners']
                cs = []
                for c in range(n):
                    key = (int(pk['pos'][c]), int(pk['nrm'][c]) if pk['nrm'] is not None else 0,
                           int(pk['col'][c]) if pk['col'] is not None else 0, int(pk['uv0'][c]) if pk['uv0'] is not None else 0)
                    if key not in index:
                        index[key] = len(P_)
                        P_.append((sm['pos'][key[0]] @ Wm[:3, :3] + Wm[3, :3]) * S)
                        if len(sm['nrm']) and key[1] < len(sm['nrm']):
                            nv = sm['nrm'][key[1]] @ np.linalg.pinv(Wm[:3, :3]).T
                        else:
                            nv = np.array([0.0, 1.0, 0.0])
                        N_.append(nv / (np.linalg.norm(nv) or 1.0))
                        UV_.append(sm['uv'][key[3]] if len(sm['uv']) and key[3] < len(sm['uv']) else (0.0, 0.0))
                        C_.append(sm['col'][key[2]] if sm['col'] is not None and key[2] < len(sm['col']) else np.ones(4))
                    cs.append(index[key])
                for a, b, c in Z.strip_triangles(n):
                    t = (cs[a], cs[b], cs[c])
                    if len(set(t)) == 3:
                        tri_out.append(t)
            if not tri_out:
                continue
            col = np.array(C_)
            group_screen = is_group_screen(e, mt)
            screen = is_screen(e, nd) or group_screen
            if is_video_material(mt) and e.get('video'):
                kind = screen_kind(mt, col)               # the video replaces the placeholder card
            elif group_screen:
                kind = screen_kind(mt, col)
                col = col.copy()
                col[:, :3] = 1.0                          # the movie's own colours, the authored alpha
            else:
                kind = 'dec' if screen else world_kind(e, mi, bool((col[:, 3] < 0.999).any()))
            if screen and not group_screen:
                col = np.ones_like(col)                  # the movie at full strength
            out.append(dict(kind=kind, anchor=anchor, material=mi, screen=screen, group_screen=group_screen,
                            bitmap=mt['textures'][0] if mt['textures'] and mt['textures'][0] < len(e['textures']) else None,
                            pos=np.array(P_), nrm=np.array(N_), uv=np.array(UV_), col=col, tris=np.array(tri_out),
                            obj=nd['index'], two_sided=Z.material_mode(mt)[2] or mirrored, mirrored=mirrored))
    return out


def plan_parts(entries):
    """[(part name, kind, loop length, once, [(entry, records)])]: entries grouped by flight role
    (`pre_plat_` / `pre_sky_` / `pre_space_` / `pre_hole_` / `fly_` name prefix; one-shot motions apart), then by loop length (an entry joins a
    group whose length its own divides), then by blend group, then split at MAX_ANCHORS animated
    anchors / MAX_MAT_PARAMS animated material floats."""
    groups = []
    for e in sorted(entries, key=lambda e: -e['length']):
        tag = (e.get('phase'), e.get('once', False))
        for g in groups:
            if g[2] == tag and (e['length'] == 0 or (g[0] and g[0] % e['length'] == 0)) and not (tag[1] and g[0] != e['length']):
                g[1].append(e)
                break
        else:
            groups.append([e['length'], [e], tag])
    parts = []
    for gi, (length, members, tag) in enumerate(groups):
        for kind in KIND_ORDER:
            chunks = []
            for e in members:
                for r in e['records']:
                    if r['kind'] != kind:
                        continue
                    a = None if r['anchor'] is None else (e['index'], r['anchor'])
                    pl = None if r['screen'] else e.get('plans', {}).get(r['material'])
                    mk, need = (e['index'], r['material']), (pl['params'] if pl else 0)
                    for ch in chunks:
                        if (a is None or a in ch[1] or len(ch[1]) < MAX_ANCHORS) and \
                                (not need or mk in ch[2] or sum(ch[2].values()) + need <= MAX_MAT_PARAMS):
                            break
                    else:
                        ch = [{}, set(), {}]
                        chunks.append(ch)
                    if a is not None:
                        ch[1].add(a)
                    if need:
                        ch[2][mk] = need
                    ch[0].setdefault(e['index'], (e, []))[1].append(r)
            for ci, ch in enumerate(chunks):
                parts.append([kind, gi, ci, length, tag, list(ch[0].values())])
    out, counts = [], {}
    for kind, gi, ci, length, (phase, once), ch in parts:
        base = '%s_%s' % (phase, kind) if phase else kind
        counts[base] = counts.get(base, 0) + 1
        name = base if counts[base] == 1 else '%s%d' % (base, counts[base])
        out.append((name, kind, length, once, ch))
    return out


def split_backdrop(entries):
    # a flight stage's intro sky + sea and its plain space are skydomes too (OBJA_Z_BG109 /
    # OBJA_Z_BG201: `_bg` parts, so the scene style leaves them unlit like every backdrop)
    for e in entries:
        if e['kind'] == 'bg' or e.get('phase') in ('pre_sky', 'pre_space'):
            for r in e['records']:
                if r['kind'] == 'dec':
                    r['kind'] = 'bg'


# ---------------------------------------------------------------------------------------------
# build + export one part
# ---------------------------------------------------------------------------------------------
def texture_stem(key, e, b):
    rgba = e['textures'][b]
    return '%s_%s' % (key.replace('stage', 's'), hashlib.md5(np.ascontiguousarray(rgba).tobytes()).hexdigest()[:8])


def pow2(rgba):
    h, w = rgba.shape[:2]
    nh, nw = 1 << max(0, int(round(math.log2(h)))), 1 << max(0, int(round(math.log2(w))))
    if (nh, nw) == (h, w):
        return rgba
    ys = (np.arange(nh) * h // nh).clip(0, h - 1)
    xs = (np.arange(nw) * w // nw).clip(0, w - 1)
    return rgba[ys][:, xs]


def texture_image(key, e, b):
    stem = texture_stem(key, e, b)
    img = bpy.data.images.get(stem)
    if img is not None:
        return img
    path = os.path.join(tempfile.gettempdir(), 'hottest_party_%s_stage_textures' % GAME, stem + '.png')
    os.makedirs(os.path.dirname(path), exist_ok=True)
    rgba = np.ascontiguousarray(pow2(e['textures'][b]))
    W.write_png(path, rgba.shape[1], rgba.shape[0], rgba.tobytes())
    return P.load_texture(stem, path)


def atlas_image(key, at):
    rgba = np.ascontiguousarray(at['rgba'])
    stem = '%s_a%s' % (key.replace('stage', 's'), hashlib.md5(rgba.tobytes()).hexdigest()[:7])
    img = bpy.data.images.get(stem)
    if img is not None:
        return img
    path = os.path.join(tempfile.gettempdir(), 'hottest_party_%s_stage_textures' % GAME, stem + '.png')
    os.makedirs(os.path.dirname(path), exist_ok=True)
    W.write_png(path, rgba.shape[1], rgba.shape[0], rgba.tobytes())
    return P.load_texture(stem, path, max_size=ATLAS_MAX)


def rigid_row(m, m_unit):
    """(rotation + translation, per-axis scale) of a ROW-vector world, M = diag(scale) . R with
    det R = +1 (a mirrored node keeps its reflection as a negative x scale); a flattened prop
    (a ~0 scale) takes its rotation from the unit-scale chain. Translation from the real chain."""
    scale = np.linalg.norm(m[:3, :3], axis=1)
    if scale.min() > 1e-6:
        r = m[:3, :3] / scale[:, None]
    else:
        u = m_unit[:3, :3]
        r = u / np.maximum(np.linalg.norm(u, axis=1, keepdims=True), 1e-12)
    if np.linalg.det(r) < 0:
        r = r.copy()
        r[0] *= -1.0
        scale = scale.copy()
        scale[0] *= -1.0
    out = np.eye(4)
    out[:3, :3] = r
    out[3, :3] = m[3, :3]
    return out, scale


def polar_rows(m3):
    """(Q, s): the nearest proper rotation Q (row-vector) of a 3x3 and the per-row scales with
    m3 ~ diag(s) . Q (a mirrored m3 keeps its reflection as a negative s[0]); shear is what
    Q / s cannot carry."""
    u, _sv, vt = np.linalg.svd(m3)
    q = u @ vt
    if np.linalg.det(q) < 0:
        q[0] *= -1.0
    return q, np.einsum('ij,ij->i', m3, q)


def degenerate(m):
    return float(np.linalg.norm(m[:3, :3], axis=1).min()) <= 1e-6


def anchor_bind(e, oi, length):
    """A flat bone's bind (game units) at the anchor's rest origin. Its rotation: the nearest
    proper rotation of the rest world -- unless the anchor SHEARS over the loop (a rotating node
    under a non-uniformly scaled parent: world = R(t) . diag(s) . R_parent, which World's TRS
    bones cannot carry). Then the bind is turned onto the motion's mean principal (stretch) axes,
    which keeps the TRS fit of bind . rest^-1 . world(t) closest. A flattened prop takes the
    unit-scale chain's rotation."""
    rest, unit_rest = e['rest'][oi], e['unit_rest'][oi]
    out = np.eye(4)
    out[3, :3] = rest[3, :3] * S
    if degenerate(rest):
        out[:3, :3] = polar_rows(unit_rest[:3, :3])[0]
        return out
    q0 = polar_rows(rest[:3, :3])[0]
    out[:3, :3] = q0
    if not length:
        return out
    t = np.linspace(0.0, float(length), 61)[:-1]
    inv0 = np.linalg.inv(rest[:3, :3])
    ds = [inv0 @ w[oi][:3, :3] for w in e['worlds'](t)]
    us, worst, ref = [], 0.0, None
    for d in ds:
        u, sv, _vt = np.linalg.svd(d)
        aniso = float(sv.max() - sv.min())
        us.append((u, aniso))
        if aniso > worst:
            worst, ref = aniso, u
    if worst < 1e-3:
        return out
    acc = np.zeros((3, 3))
    for u, aniso in us:
        if aniso < 1e-3:
            continue
        # align u's columns to the reference by a signed permutation
        m = ref.T @ u
        perm = np.zeros((3, 3))
        for c in range(3):
            r = int(np.argmax(np.abs(m[:, c])))
            perm[c, r] = np.sign(m[r, c]) or 1.0
        acc += u @ perm
    um = polar_rows(acc.T)[0].T                       # mean principal axes (columns)
    b3 = um.T                                          # bind rows = the principal axes
    if np.linalg.det(b3) < 0:
        b3[0] *= -1.0
    out[:3, :3] = b3
    return out


def game_rowm(m):
    out = np.array(m, copy=True)
    out[3, :3] *= S
    return out


def build_part(key, part, chunk):
    """Armature (root + flat anchor bones) + one mesh object per (entry, material). Returns
    (arm, objects, anchors [(entry index, node index)], binds (row, game))."""
    anchors = sorted({(e['index'], r['anchor']) for e, recs in chunk for r in recs if r['anchor'] is not None})
    entry_of = {e['index']: e for e, _r in chunk}
    bone_names = ['root'] + ['m%d.%d' % a for a in anchors]
    binds = [np.eye(4)] + [anchor_bind(entry_of[ei], oi, entry_of[ei]['length']) for ei, oi in anchors]
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
        eb.matrix = convert.rowmat_to_blender([float(x) for x in b.reshape(16)])
        ebs[n] = eb
    for n in bone_names[1:]:
        ebs[n].parent = ebs['root']
    bpy.ops.object.mode_set(mode='OBJECT')
    arm['ddr_bone_order'] = bone_names

    objects = []
    for e, recs in chunk:
        by_mat = {}
        for r in recs:
            by_mat.setdefault((r['material'], bool(r.get('screen')), r['mirrored']), []).append(r)
        for (mi, screen, mirrored), rs in sorted(by_mat.items()):
            pos = np.concatenate([r['pos'] for r in rs])
            nrm = np.concatenate([r['nrm'] for r in rs])
            uv = np.concatenate([r['uv'] for r in rs])
            col = np.concatenate([r['col'] for r in rs])
            offs = np.cumsum([0] + [len(r['pos']) for r in rs])
            tris = np.concatenate([r['tris'] + o for r, o in zip(rs, offs)])
            bones = np.concatenate([np.full(len(r['pos']), 0 if r['anchor'] is None else 1 + anchors.index((e['index'], r['anchor'])))
                                    for r in rs])
            two = bool(rs[0]['two_sided'])
            tris = H.cull_winding(pos, nrm, tris, two)
            name = 'gm_%s_%s_m%02d_%03d%s%s' % (key, part, e['index'], mi, 's' if screen else '', 'r' if mirrored else '')
            if screen:
                # the screen's authored v band (the whole picture) -> World's 16:9 movie band; a
                # group-92 surface samples the movie through its own UVs (0..1 = the whole picture;
                # HP5's movieBox wall gives each box its window of it), so its v maps as is
                uv = uv.copy()
                v_lo, v_hi = (0.0, 1.0) if rs[0].get('group_screen') else (float(uv[:, 1].min()), float(uv[:, 1].max()))
                uv[:, 1] = SCREEN_BAND[0] + (uv[:, 1] - v_lo) / max(v_hi - v_lo, 1e-6) * (SCREEN_BAND[1] - SCREEN_BAND[0])
            me = bpy.data.meshes.new(name)
            me.from_pydata([tuple(convert.vec_to_blender(p)) for p in pos], [], tris.tolist())
            me.update()
            lay = me.uv_layers.new(name='UVMap')
            loops_v = np.zeros(len(me.loops), dtype=np.int64)
            me.loops.foreach_get('vertex_index', loops_v)
            luv = uv[loops_v].copy()
            luv[:, 1] = 1.0 - luv[:, 1]
            lay.data.foreach_set('uv', luv.astype(np.float32).ravel())
            exact = me.attributes.new('ddr_normal', 'FLOAT_VECTOR', 'POINT')
            exact.data.foreach_set('vector', np.array([tuple(convert.vec_to_blender(n)) for n in nrm]).ravel())
            ob = bpy.data.objects.new(name, me)
            bpy.context.scene.collection.objects.link(ob)
            ob.parent = arm
            groups = {n: ob.vertex_groups.new(name=n) for n in bone_names}
            for bi in np.unique(bones):
                groups[bone_names[int(bi)]].add(np.nonzero(bones == bi)[0].tolist(), 1.0, 'REPLACE')
            mod = ob.modifiers.new('Armature', 'ARMATURE')
            mod.object = arm
            c_attr = P.white_color_attribute(ob)
            # additive keeps its vertex alpha: the Wii draws it SRCALPHA + ONE (FUN_8010def4), as World's
            # flags2 = 4 does, so a 0.25-alpha floor glow stays a glow (until 2026-10-04 it shipped at 1.0)
            rgba = col[loops_v].astype(np.float32)
            # color_srgb = the raw bytes the exporter writes (the linear `color` accessor would re-encode
            # them: a file 0.5 would ship as 0.74)
            c_attr.data.foreach_set('color_srgb', rgba.ravel())
            b = rs[0]['bitmap']
            if screen:
                # the image NAME is what matters (the exporter writes the 8x8 `offscreen1.dds` marker,
                # the game binds its movie render target); a black square dresses the preview
                image = bpy.data.images.get(SCREEN_TEXTURE) or P.palette_texture(SCREEN_TEXTURE, [(0.0, 0.0, 0.0)], size=8)
            elif rs[0].get('atlas'):
                image = atlas_image(key, rs[0]['atlas'])
            elif b is not None:
                image = texture_image(key, e, b)
            else:
                image = P.palette_texture('%s_white' % key.replace('stage', 's'), [(1.0, 1.0, 1.0)], size=8)
            mat = P.make_material(name, image, two_sided=two)
            if rs[0]['kind'] in ('ble', 'add', 'sub'):
                mat.surface_render_method = 'BLENDED'
            if not screen:
                mat['ddr_zan_material'] = '%d.%d' % (e['index'], mi)
            me.materials.append(mat)
            f1, f2 = FLAGS[rs[0]['kind']]
            ob['ddr_flags'] = f1 if two else f1 & ~K.MESH_FLAG_TWO_SIDED
            ob['ddr_flags2'] = f2
            objects.append(ob)
    return arm, objects, anchors, binds


MAX_KEYS = 3000              # a very slow, very long loop (STG109's 32000 frames) keys sparser


def key_step(length):
    return max(KEY_STEP, -(-length // MAX_KEYS))


def part_times(length):
    step = key_step(length)
    n = max(1, length // step)
    return np.arange(n) * step, n * step


def loop_spec(anchors, entry_of, my_binds, file_binds, length, once=False):
    """write_anm spec for the flat rig: bone b's world per key = the anchor's game rotation, its
    translation, scale relative to rest, + a wrap key (`once`: the end pose instead, no loop bit --
    a flight stage's tube opening, held by the DLL after its last frame)."""
    times, total = part_times(length)
    key_times = list(times) + [total]
    eval_times = np.append(times, total - 1e-3) if once else times
    tracks = [dict(kind=0x1C, target=0, keys=[(0.0, 0.0, 0.0, 1.0)]), dict(kind=0x1D, target=0, keys=[(0.0, 0.0, 0.0)])]
    expected = np.zeros((len(times), 1 + len(anchors), 4, 4))
    expected[:, 0] = np.eye(4)
    shear = {}
    cache = {}
    for b, (ei, oi) in enumerate(anchors, start=1):
        e = entry_of[ei]
        if ei not in cache:
            cache[ei] = (e['worlds'](eval_times.astype(np.float64)), e['worlds'](eval_times.astype(np.float64), True))
        wf, uf = cache[ei]
        s_rest = rigid_row(e['rest'][oi], e['unit_rest'][oi])[1]
        fb, mb = np.asarray(file_binds[b]), np.asarray(my_binds[b])
        # the exporter's bind comes back through Blender's bone roll, a few 1e-4 off ours at
        # some 180-degree turns: key the bone so the WRITTEN bind skins the vertices where ours
        # would (skin = v . bind^-1 . world, so world_file = bind_file . bind_ours^-1 . world_ours)
        corr = fb @ np.linalg.inv(mb)
        dev = float(np.abs(corr[:3, :3] - np.eye(3)).max())
        assert dev < 2e-2, 'bone %d: the exporter re-framed the bind by %.4f\n%s\n%s' % (b, dev, fb, mb)
        if dev > 1e-3:
            print('    WARN bone %d: exporter bind %.1e off ours -- keyed against the written one' % (b, dev))
        M0 = game_rowm(e['rest'][oi])
        flat = degenerate(e['rest'][oi])
        inv0 = None if flat else np.linalg.inv(M0)
        quats, trans, scales, prev = [], [], [], None
        for f in range(len(eval_times)):
            if flat:
                # a flattened prop (a ~0 rest scale, vertices baked flat): rotation from the
                # unit-scale chain, the scale relative to the rest's
                rig, sc = rigid_row(wf[f, oi], uf[f, oi])
                rel = np.where(np.abs(s_rest) > 1e-9, sc / np.where(np.abs(s_rest) > 1e-9, s_rest, 1.0), 1.0)
                mine = np.eye(4)
                mine[:3, :3] = np.diag(rel) @ rig[:3, :3]
                mine[3, :3] = rig[3, :3] * S
            else:
                # skin = v . bind^-1 . world must equal v . M0^-1 . M(t) (the vertices are baked
                # through the rest world M0), so world = bind . M0^-1 . M(t)
                mine = mb @ inv0 @ game_rowm(wf[f, oi])
            world = corr @ mine
            r_row, sc_f = polar_rows(world[:3, :3])
            fit = np.eye(4)
            fit[:3, :3] = np.diag(sc_f) @ r_row
            fit[3, :3] = world[3, :3]
            shear[b] = max(shear.get(b, 0.0), float(np.abs(fit[:3, :3] - world[:3, :3]).max()))
            if f < len(times):
                expected[f, b] = fit
            qv = T.rowmat_to_quat(r_row)
            if prev is not None and sum(a * c for a, c in zip(prev, qv)) < 0:
                qv = tuple(-c for c in qv)
            prev = qv
            quats.append(qv)
            trans.append(tuple(float(x) for x in world[3, :3]))
            scales.append(tuple(float(x) for x in sc_f))
        if not once:
            for lst in (quats, trans, scales):
                lst.append(lst[0])
        tracks.append(dict(kind=0x1C, target=b, times=key_times, keys=quats))
        tracks.append(dict(kind=0x1D, target=b, times=key_times, keys=trans))
        if any(abs(c - 1.0) > 1e-4 for s_ in scales for c in s_):
            tracks.append(dict(kind=10, target=b, times=key_times, keys=scales))
    return dict(frame_count=total, flag=0 if once else 1, hierarchy=[-1] + [0] * len(anchors), tracks=tracks), expected, shear


def anchor_extents(chunk, anchors, binds):
    """{bone: max distance (m) of its vertices from the bone's rest origin}."""
    out = {}
    for e, recs in chunk:
        for r in recs:
            if r['anchor'] is None:
                continue
            b = 1 + anchors.index((e['index'], r['anchor']))
            d = float(np.linalg.norm(r['pos'] - binds[b][3, :3], axis=1).max()) if len(r['pos']) else 0.0
            out[b] = max(out.get(b, 0.0), d)
    return out


def check_loop(anm_bytes, expected, times_of):
    parsed = A.parse_anm(anm_bytes)
    n_f, n_b = expected.shape[:2]
    parents = [-1] + [0] * (n_b - 1)
    worst_r = worst_t = 0.0
    for f in sorted({0, 1, n_f // 3, n_f // 2, n_f - 1}):
        pose = A.evaluate_pose(parsed, float(times_of[f]), parents)
        for b in range(n_b):
            w = np.array(pose[b]['world'], dtype=float).reshape(4, 4)
            worst_r = max(worst_r, float(np.abs(w[:3, :3] - expected[f, b][:3, :3]).max())
                          / max(1.0, float(np.abs(expected[f, b][:3, :3]).max())))
            t_err = float(np.abs(w[3, :3] - expected[f, b][3, :3]).max())
            worst_t = max(worst_t, t_err / max(1.0, float(np.abs(expected[f, b][3, :3]).max())))
    return worst_r, worst_t


# ---------------------------------------------------------------------------------------------
# material animation: UV scrolls (keys / constant speed) and texture flip-books -> one .sanm per
# part on params 2 / 3 (m_vTexAnime offU / offV). World's .sanm animates shader parameters only
# (its .tanm texture tracks have no evaluator: docs/3d_model_format_research.md §1 / §8), so a
# flip-book becomes an ATLAS (its frames side by side along one axis) driven by STEPPED offsets.
# ---------------------------------------------------------------------------------------------
SANM_CAP = 21600             # a part's material clip: lcm of its periods, at most 6 min of 60 Hz frames
ATLAS_GUTTER = 4             # px of wrap padding around each atlas cell (the DDS keeps 3 mip levels)
ATLAS_MAX = 4096             # atlas length cap (cells are resampled down to fit beyond it)


def _lcm(a, b):
    return a * b // math.gcd(a, b)


def _pow2ceil(n):
    return 1 << max(0, int(math.ceil(math.log2(max(1, n)))))


def uv_period_frames(keys, flags):
    """Frames after which a UV-key motion repeats (the set's clock and each animated axis')."""
    k0 = keys[0, 0]
    out = [int(round((keys[-1, 0] - k0) * 60.0))] if len(keys) > 1 else []
    for ax, na in enumerate(Z.uv_axis_counts(flags)):
        if na >= 2:
            out.append(int(round((keys[na - 1, 0] - k0) * 60.0)))
    out = [p for p in out if p > 0]
    r = 1
    for p in out:
        r = _lcm(r, p)
    return r if out else 0


def material_plan(e, mi, blob_of):
    """How a material animates, or None: dict(uv = frames -> (F, 2) texture-matrix offset or None,
    periods [frames], axes {animated uv axes}, flip (tex list, ends, period) or None)."""
    model = e['model']
    mt = model['materials'][mi]
    uv, periods = None, []
    if model['material_version'] == 3.0:
        w = mt['words']
        if w[14] and w[15] and w[14] < 4096:
            keys, flags = Z.uv_keys(blob_of(model), w[14], w[15])
            if len(keys) >= 2:
                uv = (lambda f, k=keys, fl=flags: Z.texmtx_offset(k, fl, np.asarray(f, dtype=np.float64) / 60.0))
                periods.append(uv_period_frames(keys, flags))
        elif w[7] and (w[8] or w[9]):
            su, sv = Z._f32(blob_of(model), mt['offset'] + 0x20), Z._f32(blob_of(model), mt['offset'] + 0x24)
            if su or sv:
                uv = (lambda f, a=su, b=sv: Z.scroll_offset(a, b, f))
    flip = Z.flip_book(mt)
    if is_video_material(mt) and e.get('video'):
        idx = e['video']
        flip = (idx, [STAGE_VIDEO_STEP * (k + 1) for k in range(len(idx))], STAGE_VIDEO_STEP * len(idx))
    if flip and not all(t < len(e['textures']) for t in flip[0]):
        print('  WARN %s material %d: flip-book textures %s beyond the TPL (%d)' % (e['stem'], mi, flip[0], len(e['textures'])))
        flip = None
    if flip and len(set(flip[0])) < 2:
        flip = None
    axes = set()
    if uv is not None:
        probe = uv(np.arange(0, max([p for p in periods if p] + [600]) + 1, 1.0))
        axes = {ax for ax in (0, 1) if np.abs(probe[:, ax] - probe[0, ax]).max() > 1e-6}
        if not axes:
            uv = None
    if flip:
        periods.append(flip[2])
    if uv is None and not flip:
        return None
    return dict(uv=uv, periods=[p for p in periods if p > 0], axes=axes, flip=flip)


def _resample(rgba, h, w):
    H_, W_ = rgba.shape[:2]
    ys = (np.arange(h) * H_ // h).clip(0, H_ - 1)
    xs = (np.arange(w) * W_ // w).clip(0, W_ - 1)
    return rgba[ys][:, xs]


def build_atlas(e, plan, axis, grid_ok=False):
    """Lay the flip-book's distinct frames out along `axis` (0 = u / columns, 1 = v / rows), each in
    a slot of [gutter | frame (twice when that axis also scrolls) | gutter] with wrap padding. The
    strip spans the whole texture across the axis (that axis keeps the texture's repeat), unless
    it would pass ATLAS_MAX: then, when the material never wraps across the axis (`grid_ok`), the
    strip folds into rows (gutters across too), else the cells shrink along the axis.
    dict(rgba, axis, double, grid, cell (uv length of one frame copy along), cell_c (across),
    origin {tex index: (uv start along, uv start across)})."""
    tex = []
    for t in plan['flip'][0]:
        if t not in tex:
            tex.append(t)
    pics = [e['textures'][t] for t in tex]
    n = len(pics)
    double = axis in plan['axes']
    S = max(p.shape[1 - axis] for p in pics)        # along the axis: width for u, height for v
    O = _pow2ceil(max(p.shape[axis] for p in pics))  # across it
    g = ATLAS_GUTTER
    grid = grid_ok and n * (S * (2 if double else 1) + 2 * g) > ATLAS_MAX
    if not grid:
        while n * (S * (2 if double else 1) + 2 * g) > ATLAS_MAX and S > 8:
            S //= 2
            print('    ATLAS %s: %d frames, cells halved to %d px to stay within %d' % (e['stem'], n, S, ATLAS_MAX))
    slot = S * (2 if double else 1) + 2 * g
    slot_c = O + 2 * g if grid else O
    per_row = n
    if grid:                                         # the fold with the smallest power-of-two area
        best = None
        for k in range(1, max(1, ATLAS_MAX // slot) + 1):
            a, c = _pow2ceil(k * slot), _pow2ceil(-(-n // k) * slot_c)
            if c <= ATLAS_MAX and (best is None or (a * c, abs(a - c)) < best[0]):
                best = ((a * c, abs(a - c)), k)
        per_row = best[1]
    rows = -(-n // per_row)
    total = _pow2ceil(min(n, per_row) * slot)
    total_c = _pow2ceil(rows * slot_c) if grid else O
    work = np.zeros((total_c, total, 4), dtype=np.uint8)   # (across, along)
    origin = {}
    for j, (t, pic) in enumerate(zip(tex, pics)):
        cell = _resample(pic, O, S) if axis == 0 else _resample(pic, S, O).transpose(1, 0, 2)
        body = np.concatenate([cell, cell], 1) if double else cell
        strip = np.concatenate([body[:, -g:], body, body[:, :g]], 1)
        if grid:
            strip = np.concatenate([strip[-g:], strip, strip[:g]], 0)
        r, c = divmod(j, per_row)
        x0, y0 = c * slot, r * slot_c
        work[y0:y0 + strip.shape[0], x0:x0 + slot] = strip
        origin[t] = ((x0 + g) / float(total), ((y0 + g) / float(total_c)) if grid else 0.0)
    atlas = work if axis == 0 else work.transpose(1, 0, 2)
    return dict(rgba=np.ascontiguousarray(atlas), axis=axis, double=double, grid=grid, cell=S / float(total),
                cell_c=(O / float(total_c)) if grid else 1.0, origin=origin, total=total, total_c=total_c,
                S=S, O=O)


def clip_records_on_axis(r, axis):
    """Split a record's triangles at the integer lines of uv[axis] so every piece lies in one tile;
    returns the record with uv[axis] made tile-local (0..1) and the tile shift recorded."""
    pos, nrm, uv, col = r['pos'], r['nrm'], r['uv'], r['col']
    P_, N_, U_, C_, T_ = [], [], [], [], []
    index = {}

    def vert(p, n, u, c, key=None):
        if key is not None and key in index:
            return index[key]
        P_.append(p)
        N_.append(n / (np.linalg.norm(n) or 1.0))
        U_.append(u)
        C_.append(c)
        if key is not None:
            index[key] = len(P_) - 1
        return len(P_) - 1

    eps = 1e-5
    for tri in r['tris']:
        ua = uv[list(tri), axis]
        k0, k1 = int(math.floor(ua.min() + eps)), int(math.ceil(ua.max() - eps))
        if k1 <= k0 + 1:
            k = k0
            ids = [vert(pos[i], nrm[i], uv[i] - (np.eye(2)[axis] * k), col[i], (int(i), k)) for i in tri]
            T_.append(ids)
            continue
        poly = [(pos[i], nrm[i], uv[i], col[i]) for i in tri]
        for k in range(k0, k1):
            piece = poly
            for lo, keep_ge in ((k, True), (k + 1, False)):
                out = []
                for idx in range(len(piece)):
                    a, b = piece[idx], piece[(idx + 1) % len(piece)]
                    da, db = a[2][axis] - lo, b[2][axis] - lo
                    ina, inb = (da >= -eps) if keep_ge else (da <= eps), (db >= -eps) if keep_ge else (db <= eps)
                    if ina:
                        out.append(a)
                    if ina != inb:
                        f = da / (da - db)
                        out.append(tuple(x + f * (y - x) for x, y in zip(a, b)))
                piece = out
                if len(piece) < 3:
                    break
            if len(piece) < 3:
                continue
            ids = [vert(p, n, u - np.eye(2)[axis] * k, c) for p, n, u, c in piece]
            for q in range(1, len(ids) - 1):
                T_.append([ids[0], ids[q], ids[q + 1]])
    out = dict(r)
    out.update(pos=np.array(P_), nrm=np.array(N_), uv=np.array(U_), col=np.array(C_), tris=np.array(T_, dtype=np.int64))
    return out


def tile_crossings(records, axis):
    n = 0
    for r in records:
        ua = r['uv'][r['tris'], axis]
        n += int(np.sum(np.ceil(ua.max(1) - 1e-5) - np.floor(ua.min(1) + 1e-5) > 1))
    return n


def apply_atlases(e, plans):
    """Give every flip-book material of an entry its atlas: pick the strip axis (the one that does
    not scroll; else u, with doubled cells), clip the records' triangles at that axis' tile lines,
    map them into one cell; check the mapping by sampling (the UV-offset sign check)."""
    for mi, plan in plans.items():
        if not plan or not plan['flip']:
            continue
        recs = [r for r in e['records'] if r['material'] == mi and not r.get('screen')]
        if not recs:
            continue
        if plan['axes'] == {0}:
            axis = 1
        elif plan['axes'] == {1}:
            axis = 0
        elif plan['axes']:
            axis = 0
        else:
            axis = 0 if tile_crossings(recs, 0) <= tile_crossings(recs, 1) else 1
        cross = 1 - axis
        at = build_atlas(e, plan, axis, grid_ok=cross not in plan['axes'] and tile_crossings(recs, cross) == 0)
        plan['atlas'] = at
        for r in recs:
            c = clip_records_on_axis(r, axis)
            if at['grid']:
                c = clip_records_on_axis(c, cross)      # one tile across already: only the shift
            uvn = c['uv'].copy()
            uvn[:, axis] *= at['cell']
            if at['grid']:
                uvn[:, cross] *= at['cell_c']
            r.update(pos=c['pos'], nrm=c['nrm'], uv=uvn, col=c['col'], tris=c['tris'], atlas=at)
            r['orig_uv_tile'] = c['uv']
        check_atlas(e, plan, recs)


def atlas_axes(at):
    return {0, 1} if at['grid'] else {at['axis']}


def material_offsets(plan, frames):
    """(F, 2) World offsets (offU, offV) of a planned material at 60 Hz `frames`."""
    f = np.asarray(frames, dtype=np.float64)
    out = np.zeros((len(f), 2))
    if plan['uv'] is not None:
        out[:] = plan['uv'](f)
    at = plan.get('atlas')
    if at:
        ax = at['axis']
        tex, ends, _period = plan['flip']
        org = np.array([at['origin'][tex[i]] for i in Z.flip_index(ends, f)]).reshape(-1, 2)
        if ax in plan['axes']:
            out[:, ax] = org[:, 0] + np.mod(out[:, ax], 1.0) * at['cell']
        else:
            out[:, ax] = org[:, 0]
        if at['grid']:
            out[:, 1 - ax] = org[:, 1]
    return out


def check_atlas(e, plan, recs, n_points=400, seed=7):
    """The UV-offset sign check: sample the atlas where World will (mesh uv + offset, nearest
    texel) and the source flip-book frame where the Wii would (tile uv + texture-matrix offset)
    at interior points of the triangles over a cycle; they must agree."""
    at = plan['atlas']
    rng = np.random.default_rng(seed)
    tex, ends, period = plan['flip']
    frames = np.linspace(0, max(plan['periods'] + [period]) - 1, 13).round()
    A_ = at['rgba']
    hits = total = 0
    for r in recs:
        if not len(r['tris']):
            continue
        sel = rng.integers(0, len(r['tris']), min(n_points, len(r['tris']) * 4))
        bc = rng.uniform(0.2, 0.6, (len(sel), 3))
        bc /= bc.sum(1, keepdims=True)
        tri = r['tris'][sel]
        uv_w = np.einsum('nk,nkc->nc', bc, r['uv'][tri])
        uv_t = np.einsum('nk,nkc->nc', bc, r['orig_uv_tile'][tri])
        for fr in frames:
            off = material_offsets(plan, [fr])[0]
            raw = plan['uv']([fr])[0] if plan['uv'] is not None else np.zeros(2)
            src = e['textures'][tex[int(Z.flip_index(ends, [fr])[0])]]
            sw = uv_w + off
            # the World sample may only leave the cell's slot by the gutter
            ys = np.mod(np.floor(np.mod(sw[:, 1], 1.0) * A_.shape[0]), A_.shape[0]).astype(int)
            xs = np.mod(np.floor(np.mod(sw[:, 0], 1.0) * A_.shape[1]), A_.shape[1]).astype(int)
            got = A_[ys, xs].astype(int)
            ss = uv_t + raw
            across = at['O'] if at['grid'] else at['total_c']
            pic = _resample(src, across, at['S']) if at['axis'] == 0 else _resample(src, at['S'], across)
            py = np.floor(np.mod(ss[:, 1], 1.0) * pic.shape[0]).astype(int) % pic.shape[0]
            px = np.floor(np.mod(ss[:, 0], 1.0) * pic.shape[1]).astype(int) % pic.shape[1]
            want = pic[py, px].astype(int)
            hits += int(np.sum(np.abs(got - want).max(1) <= 8))
            total += len(sel)
    rate = hits / max(total, 1)
    assert rate > 0.97, '%s: atlas sampling agrees on %.1f%% only' % (e['stem'], 100 * rate)
    plan['check'] = rate


def sanm_length(plans):
    periods = sorted({p for pl in plans for p in pl['periods'] if p > 0})
    L = 1
    for p in periods:
        L = _lcm(L, p)
    if not periods:
        return 600                                   # a constant scroll alone: 10 s ramps
    if L > SANM_CAP:
        big = periods[-1]
        L = big * max(1, SANM_CAP // big) if big <= SANM_CAP else SANM_CAP
        print('    SANM lcm of periods %s > %d: clip %d frames (the shorter loops restart early)' % (periods, SANM_CAP, L))
    return L


def offset_keys(plan, ax, L, tol=1e-6):
    """(times, values) of one offset axis over 0..L: a key where the slope changes and a PAIR of
    keys on one frame where the value jumps (flip-book steps, held keys, wraps) -- the DLL's
    sampler (core/anm/sample.rs) takes the later of two equal-time keys, so the step is exact."""
    f = np.arange(L + 1, dtype=np.float64)
    v = material_offsets(plan, f)[:, ax]                  # the value AT frame i (right limit)
    vl = material_offsets(plan, f - 1e-4)[:, ax]          # just before it (left limit)
    vl[0] = v[0]
    jump = np.abs(vl - v) > 1e-5
    slope = np.r_[0.0, vl[1:] - v[:-1]]                   # slope of the segment ending at frame i
    times, vals = [0], [float(v[0])]
    for i in range(1, L + 1):
        if jump[i]:
            times += [i, i]
            vals += [float(vl[i]), float(v[i])]
        elif i == L or abs(slope[i + 1] - slope[i]) > tol:
            times.append(i)
            vals.append(float(v[i]))
    # drop a key that lies on the line between its neighbours
    out_t, out_v = [times[0]], [vals[0]]
    for t_, v_ in zip(times[1:], vals[1:]):
        out_t.append(t_)
        out_v.append(v_)
        while len(out_t) >= 3 and out_t[-3] < out_t[-2] < out_t[-1]:
            t0, t1, t2 = out_t[-3:]
            v0, v1, v2 = out_v[-3:]
            if abs(v0 + (v2 - v0) * (t1 - t0) / (t2 - t0) - v1) > tol:
                break
            del out_t[-2]
            del out_v[-2]
    return out_t, out_v


def material_clip(objects, plans_of):
    """The part's .sanm spec + (L, checks), or None."""
    used = []
    for ob in objects:
        for mat in ob.data.materials:
            ek = mat.get('ddr_zan_material')
            pl = plans_of.get(ek) if ek else None
            if pl:
                used.append((mat, pl))
    if not used:
        return None
    L = sanm_length([pl for _m, pl in used])
    targets, tracks, n_params = [], [], 0
    for mat, pl in used:
        axes = set(pl['axes']) | (atlas_axes(pl['atlas']) if pl.get('atlas') else set())
        if not axes:
            continue
        n_params += len(axes)
        assert n_params <= MAX_MAT_PARAMS, 'more animated material params than the frame board holds'
        slot = len(targets)
        shader = mat.get('ddr_shader') or 'mdl_ch_constant_vc'
        targets.append(dict(identity=K.pack_identity(mat.name), identity2=0, hash=K.fnv1(shader), flags=0x2000))
        for ax in sorted(axes):
            times, vals = offset_keys(pl, ax, L)
            assert times[-1] <= 0xFFFF and len(times) < 0xFFFF
            tracks.append(dict(kind=8, target=slot, sub=2 + ax, times=times, keys=[(float(x),) for x in vals],
                               plan=pl, axis=ax))
    if not tracks:
        return None
    return dict(frame_count=L, flag=1, fps=60, material_tracks=tracks, material_targets=targets)


def check_sanm(data, spec):
    """Sample the written clip like the DLL (anm_dump.evaluate_materials) on every frame and at
    half frames away from steps; compare to the analytic offsets."""
    parsed = A.parse_anm(data)
    L = spec['frame_count']
    f = np.arange(0, L + 1, max(1, L // 2000))
    worst = 0.0
    for t in spec['material_tracks']:
        want = material_offsets(t['plan'], f)[:, t['axis']]
        for fr, w in zip(f, want):
            got = A.evaluate_materials(parsed, float(fr))[t['target']][t['sub']]
            worst = max(worst, abs(got - w))
    return worst


# ---------------------------------------------------------------------------------------------
# cameras
# ---------------------------------------------------------------------------------------------
def camanm_fov(fov_v_deg):
    """zan vertical FOV (MTXPerspective's fovY) -> camanm slot-2 degrees, keeping the vertical
    extent on World's 16:9."""
    return T.world_camanm_fov(math.tan(math.radians(fov_v_deg) / 2.0) * 16.0 / 9.0)


def camera_spec(cam):
    L = max(1, int(round(Z.cam_length(cam) * 60.0)))
    times = list(range(0, L + 1))
    s = Z.cam_samples(cam, np.asarray(times) / 60.0)
    pos_m, aim_m = s['pos'] * S, s['aim'] * S
    fov = s['fov'][:, 0] if 'fov' in s else np.full(len(times), 45.0)
    quats, prev = [], None
    for i in range(len(times)):
        qv = T.rowmat_to_quat(T.look_at_rows(pos_m[i], aim_m[i], 0.0))
        if prev is not None and sum(a * c for a, c in zip(prev, qv)) < 0:
            qv = tuple(-c for c in qv)
        quats.append(qv)
        prev = qv
    pos_cm = [tuple(float(x) for x in p * 100.0) for p in pos_m]
    degs = [(camanm_fov(float(f)),) for f in fov]

    def collapse(kind, target, keys, tol):
        if all(abs(c - c0) <= tol for k in keys for c, c0 in zip(k, keys[0])):
            return dict(kind=kind, target=target, times=[0], keys=[keys[0]])
        return dict(kind=kind, target=target, times=times, keys=keys)

    def const(target, value):
        return dict(kind=8, target=target, times=[0], keys=[(float(value),)])

    camt = [collapse(1, 0, quats, 1e-7), collapse(4, 1, pos_cm, 1e-4), collapse(8, 2, degs, 1e-4),
            const(3, NEAR), const(4, FAR), const(5, 4.0 / 3.0)]
    return dict(frame_count=max(times[-1], 1), flag=0, fps=60, camera=camt), times, (pos_m, aim_m)


def check_camera(data, times, pos_m, aim_m):
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


_GENERIC = None


def export_cameras(cams, set_dir, key):
    global _GENERIC
    if _GENERIC is None:
        _GENERIC = generic_cameras()
    cam_dir = os.path.join(set_dir, 'camera')
    os.makedirs(cam_dir, exist_ok=True)
    for stale in os.listdir(cam_dir):
        if stale.lower().endswith('.camanm'):
            os.remove(os.path.join(cam_dir, stale))
    cams = main_cameras(cams, _GENERIC)
    plan = [('%s_st%02d' % (key, i + 1), c) for i, (_p, c) in enumerate(cams)]
    plan += [('%s_non%02d' % (key, i + 1), c) for i, (_p, c) in enumerate(_GENERIC)]
    stems, worst = [], (0.0, 0.0)
    for stem, c in plan:
        spec, times, (pos_m, aim_m) = camera_spec(c)
        data = A.write_anm(spec)
        ep, ed = check_camera(data, times, pos_m, aim_m)
        assert ep < 1e-3 and ed < 1e-3, '%s: camera error pos %.4f m dir %.4f' % (stem, ep, ed)
        worst = (max(worst[0], ep), max(worst[1], ed))
        open(os.path.join(cam_dir, stem + '.camanm'), 'wb').write(data)
        stems.append(stem)
    print('  CAMERAS %d main + %d close-ups, worst err %.1e m / %.1e' % (len(cams), len(_GENERIC), *worst))
    return stems


# ---------------------------------------------------------------------------------------------
# port
# ---------------------------------------------------------------------------------------------
def port(stage):
    label, key = stage_label(stage), stage_key(stage)
    assert len(label.encode()) <= 15, label
    entries, cams = load_stage(stage)
    blob_cache = {}
    raw = open(os.path.join(STAGE_DIR, stage + '.bin'), 'rb').read()
    for p, _n, b in Z.members(raw, 'zmb'):
        blob_cache[p] = b
    zmb_blob = {}
    for e in entries:
        for p, b in blob_cache.items():
            if p.endswith('/' + e['stem'] + '.zmb'):
                zmb_blob[id(e['model'])] = b

    def blob_of(model):
        return zmb_blob[id(model)]

    plans_of = {}
    for e in entries:
        if any(is_video_material(e['model']['materials'][sm['material']]) for nd in e['model']['nodes']
               for sm in nd['submeshes'] if sm['material'] < len(e['model']['materials'])):
            attach_video(e, stage)
        e['records'] = mesh_records(e)
        used = sorted({r['material'] for r in e['records'] if not r['screen']})
        plans = {mi: material_plan(e, mi, blob_of) for mi in used}
        apply_atlases(e, plans)
        for mi, pl in plans.items():
            if pl:
                pl['params'] = len(set(pl['axes']) | (atlas_axes(pl['atlas']) if pl.get('atlas') else set()))
                plans_of['%d.%d' % (e['index'], mi)] = pl
        e['plans'] = plans
    split_backdrop(entries)
    parts = plan_parts(entries)
    flips = [pl for pl in plans_of.values() if pl.get('atlas')]
    print('STAGE %s: %d entries (%d props placed), loops %s, %d cameras, %d screens, %d UV-animated materials, '
          '%d flip-books (atlases, sampling check worst %.3f), parts %s' % (
              stage, len(entries), sum(1 for e in entries if e['inst']), sorted({e['length'] for e in entries}), len(cams),
              sum(1 for e in entries for r in e['records'] if r['screen']), sum(1 for pl in plans_of.values() if pl['uv']),
              len(flips), min([pl['check'] for pl in flips] + [1.0]), ['%s@%d%s' % (n, L, ' once' if o else '') for n, _k, L, o, _c in parts]))

    out_dir = os.path.join(OUT_BASE, label)
    set_dir = os.path.join(out_dir, 'mapset_' + key)
    os.makedirs(set_dir, exist_ok=True)
    for stale in os.listdir(set_dir):
        if stale.startswith('gm_%s_' % key):
            pdir = os.path.join(set_dir, stale)
            for f in os.listdir(pdir):
                os.remove(os.path.join(pdir, f))
            os.rmdir(pdir)
    written = []
    for part, kind, length, once, chunk in parts:
        P.fresh_scene()
        arm, objects, anchors, binds = build_part(key, part, chunk)
        if not objects:
            continue
        entry_of = {e['index']: e for e, _r in chunk}
        sspec = material_clip(objects, plans_of)
        bpy.context.view_layer.update()
        model_name = 'gm_%s_%s' % (key, part)
        pdir = os.path.join(set_dir, model_name)
        os.makedirs(pdir, exist_ok=True)
        _w, spec = export_model.export_model(os.path.join(pdir, model_name + '.model'), arm, objects, True)
        m = K.parse_model(open(os.path.join(pdir, model_name + '.model'), 'rb').read())
        assert K.write_model(K.model_to_spec(m)) == open(os.path.join(pdir, model_name + '.model'), 'rb').read()
        assert len(m['bones']) == len(anchors) + 1 <= 64, (len(m['bones']), len(anchors))
        info = '  PART %-5s %3d objects -> %3d KTMDL meshes, %2d bones, flags %s' % (
            part, len(objects), len(spec['meshes']), len(spec['bones']),
            sorted({(hex(me['flags']), me.get('flags2', 0)) for me in m['meshes']}))
        file_binds = [np.array(b['bind'], dtype=float).reshape(4, 4) for b in m['bones']]
        if anchors and length:
            lspec, expected, shear = loop_spec(anchors, entry_of, binds, file_binds, length, once)
            ext = anchor_extents(chunk, anchors, binds)
            sheared = {b: v * ext.get(b, 0.0) for b, v in shear.items() if v * ext.get(b, 0.0) > 0.005}
            if sheared:
                print('    SHEAR %s: %d anchor(s) shear under a non-uniformly scaled parent; TRS fit off by up to '
                      '%.3f m at their vertices' % (part, len(sheared), max(sheared.values())))
            data = A.write_anm(lspec)
            er, et = check_loop(data, expected, part_times(length)[0])
            assert er < 2e-3 and et < 2e-4, '%s %s: loop error rot %.5f trans %.5f' % (stage, part, er, et)
            open(os.path.join(pdir, model_name + '_play_loop.anm'), 'wb').write(data)
            info += ', %s %d frames (%d anchors, err %.1e / %.1e rel)' % (
                'ONE-SHOT' if once else 'loop', lspec['frame_count'], len(anchors), er, et)
        if sspec:
            idents = {mm['identity'] for mm in m['materials']}
            missing = [t for t in sspec['material_targets'] if t['identity'] not in idents]
            assert not missing, missing
            sdata = A.write_anm(sspec)
            worst = check_sanm(sdata, sspec)
            assert worst < 1e-4, '%s %s: sanm error %.2e' % (stage, part, worst)
            open(os.path.join(pdir, model_name + '_play_loop.sanm'), 'wb').write(sdata)
            info += ', sanm %d frames, %d materials / %d tracks / %d keys (err %.1e)' % (
                sspec['frame_count'], len(sspec['material_targets']), len(sspec['material_tracks']),
                sum(len(t['times']) for t in sspec['material_tracks']), worst)
        print(info)
        written.append((part, kind))

    marker = os.path.join(set_dir, FLIGHT_MARKER)
    if is_flight_stage(stage):
        with open(marker, 'w') as f:
            f.write('# Background Dancers FLIGHT stage (%s %s): the dancers play motion/flight/takeoff, then fly;\n'
                    '# parts pre_* show until the take-off ends (pre_plat / pre_sky / pre_hole follow the\n'
                    '# game\'s intro script), fly_* after it (port_stage_hottest2.FLIGHT_PHASES)\n'
                    % (TITLE, stage))
    elif os.path.exists(marker):
        os.remove(marker)
    fields = ['%s:%d' % (p, PRIORITY[k]) if k in PRIORITY else p for p, k in written]
    with open(os.path.join(out_dir, 'map_resources.rlist.txt'), 'w') as f:
        f.write('# %s %s.bin, ported with its models as parts and their loops\n' % (TITLE, stage))
        f.write('# (tools/blender_ddr_addon/examples/port_stage_hottest2.py GAME=%s)\n' % GAME)
        f.write('%s, 000000, 000000, %s\n' % (key, ', '.join(fields)))
    print('SIDECAR', fields)
    stems = export_cameras(cams, set_dir, key)
    if PREVIEW:
        preview(set_dir, key, [p for p, _k in written], stems)
    return out_dir


# ---------------------------------------------------------------------------------------------
# previews (port_stage_hottest.py's)
# ---------------------------------------------------------------------------------------------
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


def screen_test_card():
    """A 1280-square stand-in for World's movie target in the previews: a 16:9 card (red left,
    blue right, a yellow top stripe) in the 16:9 band, black around it -- shows which surfaces
    are screens and that the picture is upright and unmirrored."""
    img = bpy.data.images.get('preview_screen_card')
    if img:
        return img
    n = 128
    px = np.zeros((n, n, 4), dtype=np.float32)
    px[..., 3] = 1.0
    v0, v1 = int(SCREEN_BAND[0] * n), int(SCREEN_BAND[1] * n)
    px[v0:v1, : n // 2, 0] = 1.0                       # red left
    px[v0:v1, n // 2:, 2] = 1.0                        # blue right
    px[v0:v0 + 6, :, :3] = (1.0, 1.0, 0.0)            # yellow top (D3D rows top-down)
    img = bpy.data.images.new('preview_screen_card', n, n, alpha=True)
    img.pixels.foreach_set(px[::-1].ravel())           # Blender stores rows bottom-up
    return img


def unlit_preview_materials():
    for ob in bpy.data.objects:
        if ob.type != 'MESH':
            continue
        for mat in ob.data.materials:
            if mat is not None and mat.use_nodes and not mat.get('ddr_preview_unlit'):
                tex = next((n for n in mat.node_tree.nodes if n.type == 'TEX_IMAGE'), None)
                if tex is not None and tex.image is not None and \
                        tex.image.name.lower().replace('_', '').startswith(SCREEN_TEXTURE):
                    tex.image = screen_test_card()
                _unlit(mat, int(ob.get('ddr_flags', 1)), int(ob.get('ddr_flags2', 0) or 0))


def _unlit(mat, flags, flags2):
    nt = mat.node_tree
    tex = next((n for n in nt.nodes if n.type == 'TEX_IMAGE'), None)
    out = next((n for n in nt.nodes if n.type == 'OUTPUT_MATERIAL'), None)
    if tex is None or out is None:
        return
    vc = nt.nodes.new('ShaderNodeVertexColor')
    mul = nt.nodes.new('ShaderNodeMixRGB')
    mul.blend_type = 'MULTIPLY'
    mul.inputs['Fac'].default_value = 1.0
    nt.links.new(tex.outputs['Color'], mul.inputs['Color1'])
    nt.links.new(vc.outputs['Color'], mul.inputs['Color2'])
    am = nt.nodes.new('ShaderNodeMath')
    am.operation = 'MULTIPLY'
    nt.links.new(tex.outputs['Alpha'], am.inputs[0])
    nt.links.new(vc.outputs['Alpha'], am.inputs[1])
    em = nt.nodes.new('ShaderNodeEmission')
    nt.links.new(mul.outputs['Color'], em.inputs['Color'])
    tr = nt.nodes.new('ShaderNodeBsdfTransparent')
    if flags2 == 4:
        mix = nt.nodes.new('ShaderNodeAddShader')
        em2 = nt.nodes.new('ShaderNodeMixRGB')
        em2.blend_type = 'MULTIPLY'
        em2.inputs['Fac'].default_value = 1.0
        nt.links.new(mul.outputs['Color'], em2.inputs['Color1'])
        nt.links.new(am.outputs['Value'], em2.inputs['Color2'])
        nt.links.new(em2.outputs['Color'], em.inputs['Color'])
        nt.links.new(tr.outputs['BSDF'], mix.inputs[0])
        nt.links.new(em.outputs['Emission'], mix.inputs[1])
    else:
        mix = nt.nodes.new('ShaderNodeMixShader')
        nt.links.new(am.outputs['Value'], mix.inputs['Fac'])
        nt.links.new(tr.outputs['BSDF'], mix.inputs[1])
        nt.links.new(em.outputs['Emission'], mix.inputs[2])
    nt.links.new(mix.outputs['Shader'], out.inputs['Surface'])
    # opaque parts (FLAGS 'dec' / 'bg': no blend bits) must write depth: EEVEE sorts BLENDED
    # surfaces by object origin, so a sky sphere drawn blended after a nearer additive or
    # alpha part painted over it (the flight stages' fly_dec space hid the whole tunnel)
    mat.surface_render_method = 'BLENDED' if flags & 0x02C0 else 'DITHERED'
    mat.use_backface_culling = not (flags & K.MESH_FLAG_TWO_SIDED)   # the exported cull mode
    mat['ddr_preview_unlit'] = 1


def eevee_scene():
    sc = bpy.context.scene
    sc.render.engine = 'BLENDER_EEVEE'
    sc.world = bpy.data.worlds.get('Preview black') or bpy.data.worlds.new('Preview black')
    sc.world.color = (0.02, 0.02, 0.03)
    sc.view_settings.view_transform = 'Standard'
    sc.render.image_settings.file_format = 'PNG'


def preview(set_dir, key, parts, stems):
    P.fresh_scene()
    for part in parts:
        name = 'gm_%s_%s' % (key, part)
        import_model.load_model(os.path.join(set_dir, name, name + '.model'), import_textures=True, with_armature=True)
    unlit_preview_materials()
    eevee_scene()
    os.makedirs(PREVIEW_DIR, exist_ok=True)
    render_persp(os.path.join(PREVIEW_DIR, '%s_front.png' % key), Vector((0.0, -8.0, 4.5)), Vector((0.0, 0.0, 1.2)))
    render_persp(os.path.join(PREVIEW_DIR, '%s_wide.png' % key), Vector((0.0, -25.0, 10.0)), Vector((0.0, 0.0, 2.0)))
    if os.path.exists(PREVIEW_DANCER):
        import_model.load_model(PREVIEW_DANCER, import_textures=True, with_armature=True)
        unlit_preview_materials()
    sc = bpy.context.scene
    for stem in [s for s in stems if '_st' in s][:2] + [s for s in stems if '_non' in s][:1]:
        cam = import_anm.load_camanm(os.path.join(set_dir, 'camera', stem + '.camanm'))
        cam.data.clip_end = 1000.0
        sc.camera = cam
        sc.render.resolution_x, sc.render.resolution_y = 960, 540
        frame = sc.frame_end // 2
        sc.frame_set(frame)
        sc.render.filepath = os.path.join(PREVIEW_DIR, '%s_cam_%s_f%d.png' % (key, stem.split('_')[-1], frame))
        bpy.ops.render.render(write_still=True)


if __name__ == '__main__':
    want = os.environ.get('STAGES', STAGES[0])
    if want == 'all':
        todo = STAGES
    else:
        todo = []
        for w in (s.strip() for s in want.split(',') if s.strip()):
            st = w.upper() if w.upper().startswith('STG') else 'STG%03d' % int(w)
            if st not in STAGES:
                sys.exit('unknown stage %s (have %s)' % (w, ' '.join(STAGES)))
            todo.append(st)
    for stage in todo:
        port(stage)
    print('DONE')
