"""EXAMPLE: port a JoJo's Bizarre Adventure: Eyes of Heaven (PS4) character onto the DDR 33-bone dancer
rig (the pose-conform + weight-retarget flow of port_lib; README "Porting an existing model").

Written for the first EoH batch (Jotaro, Dio Brando, DIO, Star Platinum, The World, Gyro; 2026-10-24),
which ships under data_mods/custom_models/dancers/JOJO EOH/. Reads the CPK extraction directly through
examples/eoh_source.py + scripts/xfbin_dump.py; the research is docs/jojo_eoh_research.md.

Inputs (environment):
  EOH_DIR      the CPK extraction root (<dir>/data0/data/spc/..., <dir>/dataRegion0/data/spc/PS4/...)
  CHARA        character code(s), comma-separated, or `all` for every portable body (roster(); default 3jtr01)
  COL          colour set 0..3 (default 0; the file's `col<N>` diffuse)
  OUT_BASE     where <Label>/ work dirs go (default ~/Desktop/EoH Dancers); each has export/ to ship
  DDR_3D_DATA  unpacked game data root with chara/pl_rage00 + pl_emi00 (+ mc_* clips for previews)
  DDR_3D_RLIST chara_resources.rlist from startup.arc
  PREVIEW=1    Workbench renders (rest + mc_male clips from DDR_3D_DATA/chara/mc_male*)
  SKIP         comma list of model names to leave out (default per character in SKIP_MODELS)
  ALTS=1       with CHARA=all, also port the `11/21/..` alternate outfits (default: base bodies only)
Run: EOH_DIR=~/Desktop/EoH/cpk CHARA=3jtr01 PREVIEW=1 /Applications/Blender.app/Contents/MacOS/Blender -b \
       --factory-startup --python tools/blender_ddr_addon/examples/port_character_eoh.py

Source specifics this handles:
  * a 3ds Max Biped-style rig (pelvis / spine..spine2 / neck / head, l|r clavicle / upperarm / forearm /
    hand, thigh / calf / foot / toe0) in a T-pose, Z-up centimetres facing -Y: the same axes as Blender
    and the DDR donor, so the conform is a straight joint-to-joint placement. The trunk takes the
    linear z-map of the CJ / Fortnite ports (thigh sockets -> DDR UpLeg, neck -> DDR Neck), the thighs
    keep the source socket width, the hands / toes follow their parents (terminal bones).
  * ~130-190 coords: twist helpers (`arm bone0N`, `clavicle bone`) ride their limb; `cloth0N` chains
    (coat tails, scarves) are `skirt`; collar / chain / pocket / holster props go with their anchor;
    everything on the face (eye / mayu / lip / cheek / nose / jaw / hair / cap) is rigid on Head.
  * textures: `_d` diffuse (2048x1024 DXT5 -> top mip dropped, 1024x512, copied as DDS) is the only
    sheet the unlit game needs; `_n` / `_m` / `_s` are left out. The eyes sample the 256^2 iris sheet
    through UV set 1 centred on (0, 0) with wrap -- the sheet is rolled by half (eoh_source.roll_half_dds)
    and the UVs shifted by 0.5 so no wrap is needed.
  * vertex colours are white everywhere; one opaque material per source material; NUD cull mode 0 (DIO's
    cape lining, Gyro's eyes) exports two-sided, 0x405 (back) with culling.
"""
import os
import re
import shutil
import sys

import bpy
from mathutils import Matrix, Vector

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import eoh_source  # noqa: E402
import port_lib as P  # noqa: E402
import xfbin_dump as X  # noqa: E402

# 4-letter character code -> (label <= 13 bytes so a costume suffix " 2".." 9" still fits 15, sex).
# Names are the game's own (data/ui_hi/name/full_name/eng textures, read 2026-10-25); Stands and
# NPCs have no name sheet and are labelled from the series. `<code>01` is the default costume,
# `02`.. a different base body (its own entry below), `11/21/..` the alternate outfits of `01`
# (label "<Name> 2", "<Name> 3", ... in suffix order).
NAMES = {
    '1dio': ('Dio Brando', 'M'), '1jnt': ('Jonathan', 'M'), '1sdw': ('Speedwagon', 'M'), '1zpl': ('Will Zeppeli', 'M'),
    '1nkm': ('Part1 NPC', 'M'),
    '2csr': ('Caesar', 'M'), '2esd': ('Esidisi', 'M'), '2jsp': ('Joseph', 'M'), '2krs': ('Kars', 'M'),
    '2lsa': ('Lisa Lisa', 'F'), '2shm': ('Stroheim', 'M'), '2wmu': ('Wamuu', 'M'), '2his': ('Part2 NPC', 'M'),
    '3abd': ('Avdol', 'M'), '3dio': ('DIO', 'M'), '3hhs': ('Hol Horse', 'M'), '3jsp': ('Old Joseph', 'M'),
    '3jtr': ('Jotaro Kujo', 'M'), '3kki': ('Kakyoin', 'M'), '3mra': ('Mariah', 'F'), '3pln': ('Polnareff', 'M'),
    '3udr': ("N'Doul", 'M'), '3vni': ('Vanilla Ice', 'M'), '3dbi': ("D'Arby", 'M'),
    '3stp': ('Star Platinum', 'M'), '3wld': ('The World', 'M'), '3heg': ('Hierophant', 'M'), '3mgr': ('Magicians Red', 'M'),
    '3slc': ('Silver Chariot', 'M'), '3crm': ('Cream', 'M'), '3hdm': ('Hanged Man', 'M'), '3hru': ('Horus', 'M'),
    '4jsk': ('Josuke', 'M'), '4jtr': ('Jotaro P4', 'M'), '4kir': ('Kira', 'M'), '4koi': ('Koichi', 'M'),
    '4kwk': ('Kosaku', 'M'), '4oky': ('Okuyasu', 'M'), '4oti': ('Otoishi', 'M'), '4rhn': ('Rohan', 'M'),
    '4sgc': ('Shigechi', 'M'), '4ykk': ('Yukako', 'F'), '4kwh': ('Hayato', 'M'),
    '4crd': ('Crazy Diamond', 'M'), '4hdr': ('Heavens Door', 'M'), '4hnd': ('The Hand', 'M'), '4klq': ('Killer Queen', 'M'),
    '4rhc': ('RHCP', 'M'), '4ech': ('Echoes', 'M'),
    '5bct': ('Bucciarati', 'M'), '5dvl': ('Diavolo', 'M'), '5fgo': ('Fugo', 'M'), '5grn': ('Giorno', 'M'),
    '5mst': ('Mista', 'M'), '5nrc': ('Narancia', 'M'), '5trs': ('Trish', 'F'), '5abk': ('Abbacchio', 'M'),
    '5dpo': ('Doppio', 'M'), '5prs': ('Prosciutto', 'M'),
    '5gex': ('Gold Exp', 'M'), '5gxr': ('GE Requiem', 'M'), '5kcr': ('King Crimson', 'M'), '5spg': ('Spice Girl', 'F'),
    '6ans': ('Anasui', 'M'), '6elm': ('Ermes', 'F'), '6jln': ('Jolyne', 'F'), '6pci': ('Pucci', 'M'), '6wet': ('Weather Rep', 'M'),
    '6cmo': ('C-Moon', 'M'), '6kis': ('Kiss', 'F'), '6mih': ('Made in Heavn', 'M'), '6stf': ('Stone Free', 'F'),
    '6whs': ('Whitesnake', 'M'), '6wrp': ('WR Stand', 'M'), '6ddw': ('Diver Down', 'M'),
    '7dio': ('Diego', 'M'), '7jir': ('Gyro Zeppeli', 'M'), '7jny': ('Johnny', 'M'), '7vtn': ('Valentine', 'M'),
    '7dfc': ('D4C', 'M'), '7tsk': ('Tusk', 'M'), '7wld': ('The World D', 'M'), '7bbr': ('Ball Breaker', 'M'),
    '8jsh': ('Joshu', 'M'), '8jsk': ('Josuke8', 'M'), '8nkc': ('Nut King Call', 'M'), '8saw': ('Soft and Wet', 'M'),
    'bdio': ('DIO Heaven', 'M'), 'bwld': ('World Heaven', 'M'),
}
# `<code>02`.. base bodies with their own name
BASE_LABELS = {'2wmu02': 'Wamuu Armor', '2wmu12': 'Wamuu Armor 2', '2wmu22': 'Wamuu Armor 3', '3slc02': 'Chariot 2',
               '3slc03': 'Chariot Req', '3crm02': 'Cream 2', '6wet02': 'Weather Rep B', '7tsk04': 'Tusk ACT4',
               '1nkm02': 'Part1 NPC B', '4ech03': 'Echoes ACT3', '3jtr81': 'Jotaro Kujo 9', '3jtr91': 'Jotaro Kujo 10'}
# never ported: not a Biped humanoid, a user-rig attachment (1 coord), an effect or a prop
SKIP_CODES = {'3igy', '3psp', '3fll', '3geb', '3bst', '4hvt', '5ars', '5sxp', '7blt', '7sld', '7val', '7scm', '7cps', '5pph', '5sfg'}
SKIP_BODIES = {'4ech01', '4ech02', '5trs21', '7tsk01', '7tsk02', '7tsk03',    # Echoes ACT1/2, armless Trish alt, Tusk ACT1-3
               '2wmu02', '2wmu12', '2wmu22'}                                  # Wamuu's wind-armour FX body (env map only)
# models left out per code (beyond eoh_source.SKIP_MODEL_KEYS)
SKIP_MODELS = {'1dio01': ('arm cool',)}
REF_HEAD_CM = 180.0          # a 1.80 m source dances at scale 1.0 (the stock male)
MIN_SCALE, MAX_SCALE = 0.4, 1.15


def roster(root):
    """[(code, label, sex)] for every portable body under <root>/dataRegion0/data/spc/PS4."""
    import glob
    codes = sorted(os.path.basename(p)[:-len('bod1.xfbin')] for p in glob.glob(os.path.join(root, 'dataRegion0', 'data', 'spc', 'PS4', '*bod1.xfbin')))
    codes = [c for c in codes if len(c) == 6 and c[:4] in NAMES and c[:4] not in SKIP_CODES and c not in SKIP_BODIES]
    if os.environ.get('ALTS', '0') != '1':      # the `11/21/..` alternate outfits of a base body
        codes = [c for c in codes if not (c[5] == '1' and c[4] != '0')]
    out = []
    for c in codes:
        base, suf = c[:4], c[4:]
        name, sex = NAMES[base]
        if c in BASE_LABELS:
            label = BASE_LABELS[c]
        elif suf == '01':
            label = name
        elif suf[1] == '1' and suf[0] in '123456789':
            label = '%s %d' % (name, int(suf[0]) + 1)
        elif suf == '0' + suf[1] or suf[1] != '1':
            label = '%s %s' % (name, suf)      # another base body without a name of its own
        else:
            label = '%s %s' % (name, suf)
        if len(label.encode()) > 15:
            raise ValueError('label %r too long for %s' % (label, c))
        missing = eoh_source.humanoid_missing(root, c)
        if missing:
            print('SKIP %s (%s): no Biped joints %s' % (c, label, missing))
            continue
        out.append((c, label, sex))
    return out


# Dynamic chains (the DLL's dynbones solver, `background_dancers/dynbones_logic.rs`): source bone
# chains that become `DB_*` bones appended to the DDR rig, driven by lag + gravity in game. `dirs` is
# the drape (Blender axes, unit) of each segment after the first bone -- the chain is posed to hang
# that way before the bake (EoH authors chains / scarves streaming straight out in the T-pose), and
# that pose is the solver's rest. `parent` is the DDR bone the root rides. `boxes`: joint-local AABBs
# (metres, the DDR bone's own frame: Spine +x is up, +y back, +z the dancer's right) the chain is
# pushed out of.
DYN_CHAINS = {
    '3jtr01': [dict(parent='Neck', chain=('chain01', 'chain02', 'chain03', 'chain04'),
                    dirs=[(0.25, -0.45, -0.86), (0.15, -0.25, -0.96), (0.05, -0.1, -0.99)],
                    stiffness=0.1, damping=0.3, limit=1.0, gravity=1,
                    boxes=[dict(id=1, joint='Spine2', min=(-0.05, -0.16, -0.22), max=(0.22, 0.16, 0.22))])],
    '1dio01': [dict(parent='Spine2', chain=('muffler01', 'muffler02', 'muffler03', 'muffler04'),
                    dirs=[(0.0, 0.5, -0.87), (0.0, 0.15, -0.99), (0.0, 0.05, -1.0)],
                    stiffness=0.08, damping=0.25, limit=0.9, gravity=1,
                    boxes=[dict(id=1, joint='Spine', min=(-0.08, -0.15, -0.22), max=(0.42, 0.15, 0.22))])],
}
# long hair strands (parent Head): `dirs='hang'` re-poses the strand to fall from its root (outward
# from the head centre, then straight down); `'keep'` leaves the source pose (already draped)
HAIR = dict(stiffness=0.12, damping=0.35, limit=1.2, gravity=1)
for _c, _s in (('l hair02 ', ('01', '02', '03', '04', '05')), ('l hair02 ', ('06', '07', '08', '09', '10')),
               ('l hair02 ', ('11', '12', '13', '14', '15')), ('r hair02 ', ('01', '02', '03'))):
    DYN_CHAINS.setdefault('5dvl01', []).append(dict(parent='Head', chain=tuple(_c + n for n in _s), dirs='hang', **HAIR))
for _c in ('hair c ', 'hair e '):
    DYN_CHAINS['5dvl01'].append(dict(parent='Head', chain=tuple(_c + n for n in ('01', '02', '03', '04')), dirs='keep', **HAIR))
for _c in ('l hair ', 'r hair '):
    DYN_CHAINS.setdefault('bdio01', []).append(dict(parent='Head', chain=tuple(_c + n for n in ('01', '02', '03', '04')), dirs='hang', **HAIR))
HAIR_BOX = dict(id=1, joint='Spine2', min=(-0.10, -0.16, -0.22), max=(0.30, 0.16, 0.22))
for _code in ('5dvl01', 'bdio01'):
    DYN_CHAINS[_code][0]['boxes'] = [HAIR_BOX]
# a strand's end helper ('<last> d') carries the tip vertices: weight them to the last chain bone
TIP_SUFFIX = ' d'
SOLE_TARGET_Z = float(os.environ.get('SOLE_Z', '0.012'))   # stock rage00 soles sit at ~0.013 m

EOH = os.path.expanduser(os.environ['EOH_DIR'])
COL = int(os.environ.get('COL', '0'))
OUT_BASE = os.path.expanduser(os.environ.get('OUT_BASE', '~/Desktop/EoH Dancers'))
PREVIEW = os.environ.get('PREVIEW', '0') == '1'
codes = os.environ.get('CHARA', '3jtr01')


_TEX_INDEX = None


def texture_index(root):
    """texture chunk name -> xfbin path, over every spc xfbin (cached as texture_index.json under
    OUT_BASE; ~3 s to build). Sheets live where the game loads them from: `<code>col<N>` (diffuse
    sets), `<code>bod1`, `<code>acc1col<N>` / `acc1tex` (accessories), another character's files."""
    global _TEX_INDEX
    if _TEX_INDEX is not None:
        return _TEX_INDEX
    import glob
    import json
    cache = os.path.join(OUT_BASE, 'texture_index.json')
    if os.path.exists(cache):
        _TEX_INDEX = json.load(open(cache))
        return _TEX_INDEX
    idx = {}
    for f in sorted(glob.glob(os.path.join(root, '*', 'data', 'spc', '**', '*.xfbin'), recursive=True)):
        try:
            for kind, name in X.list_chunks(f):
                if kind == 'nuccChunkTexture':
                    idx.setdefault(name, os.path.relpath(f, root))
        except Exception as e:  # noqa: BLE001
            print('texture index: %s: %r' % (f, e))
    os.makedirs(OUT_BASE, exist_ok=True)
    json.dump(idx, open(cache, 'w'))
    _TEX_INDEX = idx
    return idx


def find_sheet(root, name, col):
    """The NutTexture for a chunk name: `name`, then its `_#<col>` / any `_#N` variant / the plain
    name; None when no file stores it."""
    idx = texture_index(root)
    base = re.sub(r'_#\d+$', '', name)
    cands = [name, '%s_#%d' % (base, col), base] + ['%s_#%d' % (base, n) for n in range(4)]
    for c in cands:
        f = idx.get(c)
        if f:
            for tc in X.read_xfbin(os.path.join(root, f)).by_kind('nuccChunkTexture'):
                if tc.name == c and tc.textures:
                    return tc.textures[0]
    return None


def sheet_stem(code, tex_name, used):
    """`eoh<code><sheet>`: <= 20 alphanumerics, unique within the character. The sheet part drops
    `t_`/`tw`, every `<code>`-like token, `bod1`, `_#N` and non-alphanumerics, then keeps its last
    11 characters ('t_3jtr01_body01_d_#0' -> 'eoh3jtr01body01d')."""
    short = re.sub(r'_#\d+$', '', tex_name)
    short = re.sub(r'[0-9][a-z]{3}[0-9]{2}', '', short)
    short = re.sub(r'bod1', '', short)
    short = re.sub(r'^t_?', '', short)
    short = re.sub(r'[^A-Za-z0-9]', '', short)[-11:] or 'tex'
    stem = 'eoh%s%s' % (code, short)
    n = 2
    while stem in used:
        stem = ('eoh%s%s' % (code, short))[:19] + str(n)
        n += 1
    used.add(stem)
    return stem


def pot_floor(n):
    return 1 << max(0, n.bit_length() - 1)


def db_name(src):
    return 'DB_' + src.replace(' ', '_')


def add_dynamic_bones(arm, order, dyn, posed, chain_w, meshes):
    """Append one DB_ bone per chain joint to the DDR armature (game +x = the Blender bone's X axis
    = the segment direction; origin at the posed joint), parent the roots to their DDR bone, extend
    `ddr_bone_order`, and move each mesh's chain weights onto them (the DDR share of a mixed vertex
    is kept and scaled by the remainder)."""
    if not dyn:
        return []
    bpy.context.view_layer.objects.active = arm
    for o in bpy.context.view_layer.objects:
        o.select_set(o == arm)
    bpy.ops.object.mode_set(mode='EDIT')
    ebs = arm.data.edit_bones
    names = []
    for d in dyn:
        chain = d['chain']
        pts = [posed[n] for n in chain]
        dirs = [(pts[i + 1] - pts[i]) for i in range(len(pts) - 1)]
        dirs.append(dirs[-1].copy())
        prev = None
        for n, p, dv in zip(chain, pts, dirs):
            x = dv.normalized()
            up = Vector((0.0, 0.0, 1.0)) if abs(x.z) < 0.9 else Vector((0.0, -1.0, 0.0))
            z = x.cross(up).normalized()
            y = z.cross(x).normalized()
            M = Matrix(((x.x, y.x, z.x, p.x), (x.y, y.y, z.y, p.y), (x.z, y.z, z.z, p.z), (0.0, 0.0, 0.0, 1.0)))
            e = ebs.new(db_name(n))
            e.head = p
            e.tail = p + y * max(dv.length, 0.02)
            e.matrix = M
            e.use_connect = False
            e.parent = ebs[prev] if prev else ebs[d['parent']]
            e.inherit_scale = 'NONE'
            prev = db_name(n)
            names.append(prev)
    bpy.ops.object.mode_set(mode='OBJECT')
    arm['ddr_bone_order'] = list(order) + names
    moved = 0
    for o in meshes:
        groups = {vg.name: vg for vg in o.vertex_groups}
        for n in names:
            if n not in groups:
                groups[n] = o.vertex_groups.new(name=n)
        for v in o.data.vertices:
            ws = chain_w.get((o.name, v.index))
            if not ws:
                continue
            share = min(1.0, sum(w for _, w in ws))
            keep = [(o.vertex_groups[g.group].name, g.weight) for g in v.groups if g.weight > 0.0 and not o.vertex_groups[g.group].name.startswith('DB_')]
            tot_keep = sum(w for _, w in keep)
            for g in list(v.groups):
                o.vertex_groups[g.group].remove([v.index])
            out = {db_name(n): w for n, w in ws}
            if share < 0.999 and tot_keep > 0.0:
                for n, w in keep:
                    out[n] = out.get(n, 0.0) + (1.0 - share) * w / tot_keep
            top = sorted(out.items(), key=lambda kv: -kv[1])[:4]
            tot = sum(w for _, w in top)
            for n, w in top:
                groups[n].add([v.index], w / tot, 'REPLACE')
            moved += 1
    print('dynamic bones %s: %d vertices re-weighted' % (names, moved))
    return names


def dynbones_text(code, dyn, posed, db_names):
    """`pl_<key>/dynbones.txt` for the DLL (grammar: dynbones_logic::parse). Lengths are the posed
    segments; the last bone repeats the previous length. units_per_metre is the Winx port's (the
    solver's gravity / lag constants are in those units)."""
    out = ['# JoJo EoH %s: dynamic chains for the Background Dancers solver (dynbones_logic.rs)' % code,
           '# generated by tools/blender_ddr_addon/examples/port_character_eoh.py -- do not edit',
           'dynbones 1', 'units_per_metre 8.858763']
    for d in dyn:
        chain = d['chain']
        pts = [posed[n] for n in chain]
        lens = [(pts[i + 1] - pts[i]).length for i in range(len(pts) - 1)]
        lens.append(lens[-1])
        ax = '1,1,1,%.6f,%.6f,%.6f,%.6f' % (-d['limit'], d['limit'], d['stiffness'], d['damping'])
        excl = ','.join(str(b['id']) for b in d.get('boxes', [])) or '-'
        parent = d['parent']
        for n, ln in zip(chain, lens):
            out.append('bone %s parent=%s len=%.6f gravity=%d z=%s y=%s exclude=%s' % (
                db_name(n), parent, ln, d.get('gravity', 1), ax, ax, excl if n == chain[0] else '-'))
            parent = db_name(n)
        for b in d.get('boxes', []):
            out.append('box %d joint=%s min=%s max=%s' % (b['id'], b['joint'], ','.join('%.6f' % v for v in b['min']),
                                                         ','.join('%.6f' % v for v in b['max'])))
    return '\n'.join(out) + '\n'


def port(code, label=None, sex=None):
    if label is None:
        label, sex = next(((l, sx) for c, l, sx in roster(EOH) if c == code), (code, 'M'))
    skip = SKIP_MODELS.get(code, ())
    if os.environ.get('SKIP'):
        skip = tuple(s.strip() for s in os.environ['SKIP'].split(','))
    if COL:
        label = '%s %d' % (label, COL + 1)
    key = 'eoh%s' % code if COL == 0 else 'eoh%sc%d' % (code, COL)
    out_dir = os.path.join(OUT_BASE, label)
    os.makedirs(out_dir, exist_ok=True)
    os.environ['OUT_DIR'] = out_dir
    os.environ['SRC'] = eoh_source.body_path(EOH, code)
    os.environ['CHARA_KEY'] = key
    env = P.read_env(default_key=key, default_donor='pl_rage00' if sex == 'M' else 'pl_emi00')

    P.fresh_scene()
    arm, J, order = P.load_ddr_rig(env)
    fa, meshes, info = eoh_source.build(EOH, code, name=key, skip_models=skip, col=COL)
    SRC_CO = {o.name: [v.co.copy() for v in o.data.vertices] for o in meshes}   # cm, source rest

    # --- conform ----------------------------------------------------------------------------------------
    rh = lambda n: P.rest_head(fa, n)  # noqa: E731
    THIGH = rh('l thigh')
    NECK = rh('neck') if 'neck' in fa.data.bones else rh('head')
    kz = ((J['Neck'] if 'neck' in fa.data.bones else J['Head']).z - J['LeftUpLeg'].z) / (NECK.z - THIGH.z)     # metres per cm along the trunk
    S = kz                                                          # uniform pre-scale: keep proportions
    JAW_Z = rh('jaw').z if 'jaw' in fa.data.bones else rh('head').z - 8.0
    # the conform puts every head at the donor's 1.49 m; the sidecar's model scale restores the
    # character's own height (head joint / REF_HEAD_CM; Shigechi 0.43, Jotaro 1.01, Cream 1.15 cap)
    model_scale = round(max(MIN_SCALE, min(MAX_SCALE, rh('head').z / REF_HEAD_CM)), 2)
    print('%s: head %.0f cm -> model scale %.2f' % (code, rh('head').z, model_scale))
    print('%s: trunk kz %.5f m/cm (source %.0f cm neck-to-hip vs DDR %.3f m)' % (code, kz, NECK.z - THIGH.z, J['Neck'].z - J['LeftUpLeg'].z))

    def torso_map(p):
        return Vector((p.x * S, J['LeftUpLeg'].y + (p.y - THIGH.y) * S, J['LeftUpLeg'].z + (p.z - THIGH.z) * kz))

    targets, next_of = {}, {}
    has = lambda n: n in fa.data.bones  # noqa: E731
    for n in ('root', 'pelvis'):
        if has(n):
            targets[n] = torso_map(rh(n))
    # trunk chain from what the rig has: spine [spine1 [spine2 [neck]]] head; the DDR joints the
    # missing links would have taken are spread over the present ones by arc length
    trunk = [n for n in ('spine', 'spine1', 'spine2', 'neck') if has(n)]
    ddr_trunk = ['Spine', 'Spine1', 'Spine2', 'Neck']
    if len(trunk) == 4:
        for a_, b_ in zip(trunk, trunk[1:] + ['head']):
            targets[a_] = torso_map(rh(a_)) if a_ != 'neck' else J['Neck']
            next_of[a_] = b_
    else:
        poly = [J[n] for n in ddr_trunk] + [J['Head']]
        tm = P.map_chain_arclength(fa, trunk + ['head'], poly)
        for a_, b_ in zip(trunk, trunk[1:] + ['head']):
            targets[a_] = tm[a_]
            next_of[a_] = b_
        print('trunk: %s mapped by arc length' % trunk)
    targets['head'] = J['Head']
    for sd, Sd in (('l', 'Left'), ('r', 'Right')):
        if has(sd + ' clavicle'):
            targets[sd + ' clavicle'] = J[Sd + 'Collar']
            next_of[sd + ' clavicle'] = sd + ' upperarm'
        targets[sd + ' upperarm'] = J[Sd + 'Arm']
        next_of[sd + ' upperarm'] = sd + ' forearm'
        targets[sd + ' forearm'] = J[Sd + 'ForeArm']
        next_of[sd + ' forearm'] = sd + ' hand'
        targets[sd + ' hand'] = J[Sd + 'Hand']
        targets[sd + ' thigh'] = torso_map(rh(sd + ' thigh'))
        next_of[sd + ' thigh'] = sd + ' calf'
        targets[sd + ' calf'] = J[Sd + 'Leg']
        next_of[sd + ' calf'] = sd + ' foot'
        targets[sd + ' foot'] = J[Sd + 'Foot']
        if has(sd + ' toe0'):
            next_of[sd + ' foot'] = sd + ' toe0'
            targets[sd + ' toe0'] = J[Sd + 'ToeBase']
    dyn = [d for d in DYN_CHAINS.get(code, []) if all(n in fa.data.bones for n in d['chain'])]
    head_src = rh('head')
    for d in dyn:
        chain = d['chain']
        if d['parent'] == 'Head':
            root = J['Head'] + (rh(chain[0]) - head_src) * S      # rides the rigid head
        else:
            root = torso_map(rh(chain[0]))
        targets[chain[0]] = root
        if d['dirs'] == 'keep':
            for a_, b_ in zip(chain, chain[1:]):
                targets[b_] = targets[a_] + (rh(b_) - rh(a_)) * S
                next_of[a_] = b_
        else:
            if d['dirs'] == 'hang':
                # fall beside the head (never over the face): outward on the root's side, a little
                # back, then straight down
                side = 1.0 if root.x - J['Head'].x >= 0.0 else -1.0
                out = Vector((side * 0.8, 0.5, 0.0)).normalized()
                dirs = [(out * 0.7 + Vector((0, 0, -0.7))).normalized(), (out * 0.25 + Vector((0, 0, -0.97))).normalized()]
                dirs += [Vector((0.0, 0.0, -1.0))] * (len(chain) - 3)
            else:
                dirs = [Vector(x) for x in d['dirs']]
            for a_, b_, dr in zip(chain, chain[1:], dirs):
                seg = (rh(b_) - rh(a_)).length * S
                targets[b_] = targets[a_] + Vector(dr).normalized() * seg
                next_of[a_] = b_
        print('dynamic chain %s: root %s -> tip %s' % (chain[0], tuple(round(c, 3) for c in targets[chain[0]]), tuple(round(c, 3) for c in targets[chain[-1]])))
    follow = ('l hand', 'r hand', 'l toe0', 'r toe0', 'head')
    P.align_chain_bones(fa, next_of)
    P.conform(fa, S, targets, next_of, follow_parent_rot=follow)
    # the posed chain joints (Blender metres) and each mesh's chain weights, before the rig goes
    posed = {n: fa.matrix_world @ fa.pose.bones[n].head for d in dyn for n in d['chain']}
    alias = {}                                   # source group -> chain bone (tips -> the last link)
    for d in dyn:
        for n in d['chain']:
            alias[n] = n
        alias[d['chain'][-1] + TIP_SUFFIX] = d['chain'][-1]
    chain_groups = set(alias)
    chain_w = {}
    for o in meshes:
        gn = {vg.index: vg.name for vg in o.vertex_groups}
        for v in o.data.vertices:
            ws = [(alias[gn[g.group]], g.weight) for g in v.groups if gn[g.group] in chain_groups and g.weight > 0.0]
            if ws:
                chain_w[(o.name, v.index)] = ws
    P.bake_meshes(meshes, fa)

    # soles: a source whose feet reach below the DDR ankle more than the stock shoes do sinks into the
    # floor; stretch the part below the ankle so the lowest vertex lands on the stock sole height
    ankle_z = J['LeftFoot'].z
    sole = min(v.co.z for o in meshes for v in o.data.vertices)
    if abs(sole - SOLE_TARGET_Z) > 0.002:
        f = (ankle_z - SOLE_TARGET_Z) / (ankle_z - sole)
        for o in meshes:
            me = o.data
            co = [0.0] * (3 * len(me.vertices))
            me.vertices.foreach_get('co', co)
            for i in range(len(me.vertices)):
                z = co[3 * i + 2]
                if z < ankle_z:
                    co[3 * i + 2] = ankle_z - (ankle_z - z) * f
            me.vertices.foreach_set('co', co)
            me.update()
        print('soles: below-ankle z stretch x%.3f (%.4f -> %.4f)' % (f, sole, SOLE_TARGET_Z))

    # --- weights ----------------------------------------------------------------------------------------
    def classify(g):
        side = 'l' if g.startswith(('l ', 'fl ', 'bl ')) else ('r' if g.startswith(('r ', 'fr ', 'br ')) else None)
        n = g.split(' ', 1)[1] if g.split(' ', 1)[0] in ('l', 'r', 'fl', 'fr', 'bl', 'br', 'b', 'c') and ' ' in g else g
        if g in ('root', 'trall', 'pelvis', 'body') or n.startswith(('pocket', 'holster', 'belt')):
            return 'hips', None
        if n.startswith('cloth') or g.startswith('skirt'):
            return 'skirt', None
        if g in chain_groups:
            return None, None            # re-weighted onto its DB_ bone by add_dynamic_bones
        if g in ('spine', 'spine1', 'spine2') or n.startswith(('collar', 'chain', 'muffler', 'mant', 'cape')):
            return 'spine', None
        if g == 'neck':
            return 'neck', None
        if g in ('head', 'face', 'face01', 'face02', 'jaw', 'tongue', 'tongue01', 'upper teeth', 'lower teeth', 'kiba') or \
           n.startswith(('eye', 'mayu', 'lip', 'cheek', 'nose', 'hair', 'cap', 'hat', 'ear', 'brow', 'mouth')) or g.startswith(('hair', 'eyebone', 'cap')):
            return 'head', None
        if n in ('clavicle', 'clavicle bone', 'shoulder'):
            return 'collar', side
        if n == 'upperarm' or n in ('arm bone01', 'arm bone02', 'arm bone03', 'elbow'):
            return 'upperarm', side
        if n == 'forearm' or n in ('arm bone04', 'arm bone05', 'arm bone06'):
            return 'lowerarm', side
        if n == 'hand' or n.startswith(('finger', 'wep')):
            return 'hand', side
        if n == 'thigh' or n == 'knee':
            return 'thigh', side
        if n == 'calf':
            return 'calf', side
        if n in ('foot', 'heel'):
            return 'foot', side
        if n.startswith('toe'):
            return 'toe', side
        return None, side

    P.retarget_weights(meshes, arm, J, order, classify)
    db_names = add_dynamic_bones(arm, order, dyn, posed, chain_w, meshes)

    n_rigid = 0
    for o in meshes:
        src = SRC_CO[o.name]
        head = o.vertex_groups['Head']
        for v in o.data.vertices:
            if src[v.index].z < JAW_Z - 3.0:
                continue
            ws = {o.vertex_groups[g.group].name: g.weight for g in v.groups if g.weight > 0}
            if 'Head' in ws and ws['Head'] < 0.999:
                for g in list(v.groups):
                    o.vertex_groups[g.group].remove([v.index])
                head.add([v.index], 1.0, 'REPLACE')
                n_rigid += 1
    print('rigid head: %d vertices above jaw z %.1f cm forced to Head 1.0' % (n_rigid, JAW_Z - 3.0))

    # --- textures / materials -----------------------------------------------------------------------------
    tex_dir = os.path.join(out_dir, 'tex')
    os.makedirs(tex_dir, exist_ok=True)
    stems = {}
    used_stems = set()

    def sheet_image(tex_name, roll=False):
        if tex_name in stems:
            return stems[tex_name]
        t = find_sheet(EOH, tex_name, COL)
        if t is None:
            return None
        stem = sheet_stem(code, tex_name, used_stems)
        dds = t.to_dds(max_size=1024)
        if roll:
            dds = eoh_source.roll_half_dds(dds)
        path = os.path.join(tex_dir, stem + '.dds')
        open(path, 'wb').write(dds)
        img = bpy.data.images.load(path)
        img.name = stem
        w, h = img.size
        if roll or (w & (w - 1)) or (h & (h - 1)):
            # the rolled iris (truncated DXT mip chain) and non-power-of-two sheets (Lisa Lisa's
            # 320x256 glasses) ship as the stock A8R8G8B8 3-mip DDS: hand the exporter pixels
            if (w & (w - 1)) or (h & (h - 1)):
                img.scale(pot_floor(w), pot_floor(h))
                print('texture %s: %dx%d -> %dx%d (power of two)' % (stem, w, h, *img.size))
            px = list(img.pixels)
            img.pixels = px
            img.update()
        stems[tex_name] = img
        return img

    mats = {}
    for o in meshes:
        me = o.data
        P.white_color_attribute(o)
        is_eye = o.name.endswith(('eye_l', 'eye_r')) and any('iris' in t for m in me.materials for t in m.get('eoh_textures', []))
        if is_eye:
            # the iris sits at UV (0, 0) in set 1 and wraps: shift onto the rolled sheet's centre.
            # Blender v is already 1 - v_file, so the file's +0.5 is -0.5 here (the game CLAMPS: a
            # wrong-way shift put every eye texel on the white edge row -- in-game 2026-10-25)
            uv1 = me.uv_layers['UVMap1']
            for d in uv1.data:
                d.uv = (d.uv[0] + 0.5, d.uv[1] - 0.5)
            P.keep_uv_layer(o, 'UVMap1')
        else:
            P.keep_uv_layer(o, 'UVMap')
        new = []
        for mat in me.materials:
            texs = list(mat.get('eoh_textures', []))
            if is_eye:
                tex = next((t for t in texs if 'iris' in t), None)
                img = sheet_image(tex, roll=True) if tex else None
            else:
                aux = ('haching', 'ecmn_', '_n', '_s_', '_m', 'normal', 'cubemap')
                plain = [t for t in texs if not any(k in t for k in aux) or t.endswith('_d') or '_d_' in t]
                tex = next((t for t in texs if '_d' in t), plain[0] if plain else None)
                img = sheet_image(tex) if tex else None
            if img is None:
                print('WARN %s: no texture among %s -- flat grey' % (mat.name, texs))
                if 'eoh%sflat' % code not in bpy.data.images:
                    P.palette_texture('eoh%sflat' % code, [(0.5, 0.5, 0.5)], size=16)
                img = bpy.data.images['eoh%sflat' % code]
            two_sided = int(mat.get('eoh_cull', 0x405)) == 0     # NUD cull 0 = none, 0x405 = back
            mkey = (img.name, mat.name, two_sided)
            if mkey not in mats:
                mats[mkey] = P.make_material(key + '_' + mat.name.split('bod1_', 1)[-1], img, two_sided=two_sided)
            new.append(mats[mkey])
        idx = [p.material_index for p in me.polygons]
        me.materials.clear()
        for m in new:
            me.materials.append(m)
        for p, i in zip(me.polygons, idx):
            p.material_index = i
        me.update()
    print('materials', sorted(k for k in bpy.data.materials.keys() if k.startswith(key)), 'sheets', {k: tuple(v.size) for k, v in stems.items()})

    bpy.context.view_layer.update()
    lo = min(v.co.z for o in meshes for v in o.data.vertices)
    print('lowest vertex z %.4f (stock soles ~0.013)' % lo)
    bpy.ops.wm.save_as_mainfile(filepath=os.path.join(out_dir, key + '.blend'))

    # --- export -----------------------------------------------------------------------------------------
    if os.path.isdir(env['EXPORT']):
        shutil.rmtree(env['EXPORT'])
    os.makedirs(env['EXPORT'])
    P.export_and_check(env, arm)
    P.write_sidecar_rlist_txt(env, sex=sex, cls='A', model_scale=model_scale, shadow_scale=round(0.8 * model_scale, 2))
    if dyn:
        path = os.path.join(env['EXPORT'], 'pl_' + key, 'dynbones.txt')
        open(path, 'w').write(dynbones_text(code, dyn, posed, db_names))
        print('DYNBONES', path, len(db_names), 'bones')
    if PREVIEW:
        cd = os.path.join(env['DATA'], 'chara')
        pool = 'mc_male' if sex == 'M' else 'mc_female'
        clips = [os.path.join(cd, pool, f) for f in sorted(os.listdir(os.path.join(cd, pool))) if f.endswith('_exec.anm')][:2]
        P.preview_renders(env, arm, clips=clips, frames=(200, 700, 1200), label=key)
        for tag, c, off, sc in (('face', (0.0, 0.0, 1.52), (0.0, -3.0, 0.0), 0.5), ('face34', (0.0, 0.0, 1.52), (2.1, -2.1, 0.2), 0.5),
                                ('hand', (-0.58, 0.0, 1.32), (0.0, -3.0, 0.0), 0.45), ('feet', (0.0, 0.0, 0.1), (0.0, -3.0, 0.0), 0.6)):
            c = Vector(c)
            P.render_camera(os.path.join(out_dir, '%s_close_%s.png' % (key, tag)), c + Vector(off), c, sc, res=(800, 800))
    print('PORT COMPLETE', code, label, key)


if codes == 'all':
    done, failed = [], []
    for c, label, sex in roster(EOH):
        try:
            port(c, label, sex)
            done.append((c, label))
        except Exception as e:  # noqa: BLE001 -- one bad body must not stop the batch
            import traceback
            traceback.print_exc()
            failed.append((c, label, repr(e)))
    print('BATCH DONE %d ported, %d failed' % (len(done), len(failed)))
    for c, label, e in failed:
        print('  FAILED %s (%s): %s' % (c, label, e))
else:
    for c in [c.strip() for c in codes.split(',') if c.strip()]:
        port(c)
