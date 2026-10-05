"""EXAMPLE / PORT: the HOTTEST PARTY flight EFFECTS -- the flyer's light orb, rainbow trail, orbiting
hand stars with light trails and the take-off burst -- for every ported FLIGHT stage
(`data_mods/custom_models/stages/**/mapset_<key>/flight.txt`), from the game's own effect bank
(`boss_ddr3.TEB` + `boss_ddr3.tpl`: MUSIC FIT `game/GAME_CHR_EFF.bin`; HP4 `character/CHR_EFF.bin`
for HP4's stage). RE: docs/wii_ddr_zan_effects_research.md; reference simulator
scripts/teb_dump.py; the DLL runtime src/mods/background_dancers/flight_fx.rs.

The effects are particle EMITTERS (zan `CzanEff`), not models: the DLL runs them from the TEB itself
every frame. What World needs from us is something to draw them WITH -- its renderer has no
particle path -- so per stage this writes, into the stage's mapset folder:

  flight_fx/flight_fx.teb       the TEB bytes, verbatim (the DLL's flight_fx::parse_teb reads it)
  flight_fx/flight_fx.txt       the pool layout manifest (grammar below)
  fx_<key>_s<p><k>/             per player p (0..3): SPRITE POOL model(s) (k = a, b, ...: a model
                                holds <= 255 bones, KTMDL's byte blend indices) -- one unit quad
                                (teb_dump.QUAD, the game's own strip and default / flipped UVs, a
                                flip-book's first cell) per particle the player's effects can show
                                at once, each quad its own mesh, material and bone (identity bind)
                                so the DLL places it with a bone matrix (camera-facing billboards
                                from its own camera), colours it through the draw record's colour
                                and steps a flip-book through the material's m_vTexAnime offset;
  fx_<key>_r<p><k>/             per player p: RIBBON POOL model(s) -- one strip per ribbon the
                                player's effects can show at once, a vertex pair (edge -1 / +1 in
                                bone space, u 0 / 1) per point on its own bone, v = j / (cap - 1)
                                (the DLL rescales it per frame through m_vTexAnime.y); strips longer
                                than 52 points are split into overlapping meshes (the loader's
                                52-bone palette limit);
  textures (<key>fx<NN>.dds)    the TPL images the pools sample, each in the first model folder
                                that uses it; a fully transparent image (the carrier particles'
                                32x32, which only exist to drag ribbons) gets no pool at all.

Which effects a player plays is main.dol code (`FUN_8004b5a8`, research §1.1): player p, mode m
-> effects 8m + 2p (at joint A) and 8m + 2p + 1 (at joints B and C); the DLL plays mode 2 (the
leap) and mode 0 (the flight). A pool holds, per (texture, flipped-UV, flip-book cell, blend,
depth) group, the sum of `max` particles of every part drawing that group in those effects x
their instance counts -- the exact peak. Blend 2 = additive (mesh flags 0x06C1 / flags2 4); else
alpha-blended (0x02C1); a part flagged 0x200 adds 0x0020 (no depth test). Every mesh's bounding
sphere and bone box are huge (the quads move anywhere; the engine's cull must never drop them).

Manifest (`flight_fx.txt`, whitespace-separated, `#` comments):
    flight_fx 1
    metres_per_unit <f>                      zan_dump.GAME_SCALE
    sprites <player> <model> <quads>
    group <first> <count> <tex> <flip 0|1> <cell> <additive 0|1> <depth 0|1>
    ribbons <player> <model> <bones> <records>
    strip <first_bone> <points> <first_record> <records> <tex> <additive 0|1>
  (quad i = mesh / draw record / material / bone i; a strip's material = its index in the model)

Run (plain python3 + numpy; no Blender):
    python3 tools/blender_ddr_addon/examples/port_flight_fx.py
Inputs (environment): MUSIC_FIT_DIR, HP4_GAME (the dumped disc trees, default ~/Desktop/DDR Wii
ISOs/<title>); STAGES (comma list of stage keys, default every flight stage found).
"""
import glob
import math
import os
import struct
import sys

REPO = os.path.abspath(os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', '..'))
sys.path.insert(0, os.path.join(REPO, 'scripts'))
import ktmdl_dump as K  # noqa: E402
import teb_dump as T  # noqa: E402
import zan_dump as Z  # noqa: E402

ISOS = os.path.expanduser('~/Desktop/DDR Wii ISOs')
MUSIC_FIT = os.path.expanduser(os.environ.get('MUSIC_FIT_DIR', os.path.join(ISOS, 'Dance Dance Revolution - Music Fit (Japan)')))
HP4 = os.path.expanduser(os.environ.get('HP4_GAME', os.path.join(ISOS, 'Hottest Party 4 (Europe)')))
STAGES_DIR = os.path.join(REPO, 'data_mods', 'custom_models', 'stages')

PLAYERS = 4
MODES = (2, 0)                    # the leap, the flight (mode 1 = play mode 4, not played)
SHADER = 'mdl_ch_constant_vc'     # skinned, unlit, COLOR0 x tint; applies m_vTexAnime
PARAMS = [[1.0, 1.0, 0.0, 0.0], [1.0, 1.0, 1.0, 0.0], [0.0, 0.0, 0.0, 0.0]]
FLAGS_ADD, FLAGS2_ADD = 0x06C1, 4  # additive, two-sided, no z-write (the stage ports' `add`)
FLAGS_BLE, FLAGS2_BLE = 0x02C1, 0  # alpha-blended, two-sided
FAR = 1000.0                      # bounding sphere / bone box half-size, metres
RIBBON_MAX_POINTS = 64            # flight_fx::RIBBON_MAX_POINTS (1 s of points at 60 Hz = 61)
MAX_BONES = 255                   # a KTMDL model's limit (byte blend indices); frame_board::MAX_BONES = 256
MAX_RECORDS = 128                 # frame_board::MAX_RECORD_COLOURS
BLEND_ADDITIVE = 2                # a draw block's blend 2: SRCALPHA / ONE
IDENTITY = [1.0, 0, 0, 0, 0, 1.0, 0, 0, 0, 0, 1.0, 0, 0, 0, 0, 1.0]


def bank_for(stage_dir):
    """(source label, WII archive path) of the effect bank a stage uses."""
    if 'HOTTEST PARTY 4' in stage_dir and os.path.exists(os.path.join(HP4, 'character', 'CHR_EFF.bin')):
        return 'HP4 character/CHR_EFF.bin', os.path.join(HP4, 'character', 'CHR_EFF.bin')
    return 'MUSIC FIT game/GAME_CHR_EFF.bin', os.path.join(MUSIC_FIT, 'game', 'GAME_CHR_EFF.bin')


def player_effects(p, mode):
    """`FUN_8004b5a8`'s table: (effect, instance count) of player p in `mode`."""
    return [(8 * mode + 2 * p, 1), (8 * mode + 2 * p + 1, 2)]


def visible_textures(images):
    return {i for i, im in enumerate(images) if int(im[..., 3].max()) > 0}


def pools_for(fx, images, p):
    """(sprite groups, ribbon strips) of player p: {key: count}, [(tex, points, additive)]."""
    vis = visible_textures(images)
    groups, strips = {}, []
    for mode in MODES:
        for ei, inst in player_effects(p, mode):
            if ei >= len(fx['effects']):
                continue
            for nd in fx['effects'][ei]['nodes']:
                part = nd.get('part')
                if not part:
                    continue
                n = part.get('emitter', {}).get('max', 0) * inst
                d = part.get('draw')
                if not n or not d:
                    continue
                additive = d['blend'] == BLEND_ADDITIVE
                depth = not part['flags'] & T.P_NO_DEPTH
                flip = bool(part['flags'] & T.P_FLIP_UV)
                if d['tex'] in vis:
                    cell = 0.0
                    if d['flags'] & T.D_FLIPBOOK:
                        fb = d['flipbook']
                        cell = 1.0 / max(1, fb['width'] // max(1, fb['cell']))
                    key = (d['tex'], flip, cell, additive, depth)
                    groups[key] = groups.get(key, 0) + n
                rb = part.get('ribbon')
                if rb and rb['tex'] in vis:
                    strips += [(rb['tex'], ribbon_peak(part), additive)] * n
    return groups, strips


def ribbon_peak(part):
    """Points a particle's ribbon can hold at once: one push per 60 Hz step, a point lives
    flight_fx::RIBBON_POINT_LIFE (1 s), the particle at most life + life_rand (an immortal one
    forever), capped by `segments` and flight_fx::RIBBON_MAX_POINTS."""
    em = part.get('emitter', {})
    life = 1.0 if part['flags'] & T.P_IMMORTAL else min(1.0, em.get('life', 0.0) + abs(em.get('life_rand', 0.0)))
    return max(2, min(part['ribbon']['segments'], RIBBON_MAX_POINTS, int(math.ceil(life * 60.0)) + 1))


def quad_uvs(flip, cell):
    """The quad's corner UVs (teb_dump._pose): the default / flipped strip, a flip-book's cell 0."""
    if not cell:
        return T.UV_FLIPPED.tolist() if flip else T.UV_DEFAULT.tolist()
    c = cell
    return [[c, c], [0.0, c], [c, 0.0], [0.0, 0.0]] if flip else [[0.0, 0.0], [0.0, c], [c, 0.0], [c, c]]


def f16(v):
    return struct.unpack('<H', struct.pack('<e', max(-65504.0, min(65504.0, v))))[0]


ELEMENTS = [(0, 0, 2, 0x10), (0, 12, 0xB, 0x21), (0, 16, 2, 0x20), (0, 28, 2, 0x12), (0, 40, 0x10, 0x16), (0, 44, 0x12, 0x13)]
STRIDE = 48   # layout A: position, blend indices, 3 weights, normal, uv (f16), COLOR0


def mesh(flags, flags2, material, tex_slot, verts, tris, palette):
    """A KTMDL mesh over `verts` [(pos, uv, palette-local bone)]."""
    data = b''.join(struct.pack('<3f', *pos) + bytes((local, 0, 0, 0)) + struct.pack('<3f', 0.0, 0.0, 0.0)
                    + struct.pack('<3f', 0.0, 0.0, 1.0) + struct.pack('<HH', f16(uv[0]), f16(uv[1]))
                    + bytes((255, 255, 255, 255)) for pos, uv, local in verts)
    idx = [i for t in tris for i in t]
    return dict(flags=flags, flags2=flags2, primitive_raw=1, material=material, node=0,
                texture_slots=[tex_slot], texture_slot_count=1,
                bounding_sphere=[0.0, 0.0, 0.0, FAR * math.sqrt(3.0)], elements=ELEMENTS, stride=STRIDE,
                vertex_count=len(verts), vertex_data=data,
                index_count=len(idx), index_data=struct.pack('<%dH' % len(idx), *idx),
                palette_count=len(palette), palette=list(palette))


def bones(n):
    out = []
    for i in range(n):
        out.append(dict(identity=K.pack_identity('fx%d' % i), bind=list(IDENTITY), inverse_bind=list(IDENTITY),
                        aabb_min=[-FAR] * 3, aabb_max=[FAR] * 3, parent=-1 if i == 0 else 0, flags=None))
    return out


class Model:
    """Collects meshes / materials / textures of one pool model."""

    def __init__(self, name, tex_stem):
        self.name, self.tex_stem = name, tex_stem
        self.meshes, self.materials, self.texnames, self.textures, self.tex_of = [], [], [], [], {}

    def tex_slot(self, tpl_index):
        if tpl_index not in self.tex_of:
            stem = self.tex_stem(tpl_index)
            self.tex_of[tpl_index] = len(self.textures)
            self.texnames.append(K.pack_texname(stem))
            self.textures.append(dict(identity=K.pack_identity('file%d' % (len(self.textures) + 1)), kind=0,
                                      texname_index=len(self.texnames) - 1, f10=1.0, f14=1.0))
        return self.tex_of[tpl_index]

    def material(self, label):
        self.materials.append(dict(identity=K.pack_identity(label), shader=SHADER, params=[list(r) for r in PARAMS]))
        return len(self.materials) - 1

    def write(self, out_dir, nbones):
        spec = dict(bones=bones(nbones), palette=[0], meshes=self.meshes, nodes=None,
                    info=dict(bbox_max=[FAR] * 3 + [0.0], bbox_min=[-FAR] * 3 + [0.0]),
                    materials=self.materials, texnames=self.texnames, textures=self.textures,
                    debug=dict(texture_names=[self.tex_stem(t) + '.dds' for t in sorted(self.tex_of, key=lambda t: self.tex_of[t])],
                               shader_names=[SHADER]))
        d = os.path.join(out_dir, self.name)
        os.makedirs(d, exist_ok=True)
        with open(os.path.join(d, self.name + '.model'), 'wb') as f:
            f.write(K.write_model(spec))
        with open(os.path.join(d, self.name + '.b2it'), 'wb') as f:
            f.write(K.write_b2it([('fx%d' % i, i) for i in range(nbones)]))
        with open(os.path.join(d, self.name + '.grp2it'), 'wb') as f:
            f.write(K.write_b2it([('model', 0)]))
        return d


def flags_for(additive, depth):
    f, f2 = (FLAGS_ADD, FLAGS2_ADD) if additive else (FLAGS_BLE, FLAGS2_BLE)
    return (f | (0 if depth else K.MESH_FLAG_NO_ZTEST)), f2


def sprite_model(name, groups, tex_stem):
    """One sprite pool model over `groups` [(key, count)] (<= MAX_BONES quads in total)."""
    m = Model(name, tex_stem)
    lines, q = [], 0
    for (tex, flip, cell, additive, depth), count in groups:
        f, f2 = flags_for(additive, depth)
        uvs = quad_uvs(flip, cell)
        slot = m.tex_slot(tex)
        lines.append('group %d %d %d %d %.6f %d %d' % (q, count, tex, flip, cell, additive, depth))
        for _ in range(count):
            mat = m.material('fxq%d' % q)
            assert mat == q
            block, local = divmod(q, K.PALETTE_BLOCK)
            pal = list(range(block * K.PALETTE_BLOCK, min((block + 1) * K.PALETTE_BLOCK, MAX_BONES)))
            verts = [(T.QUAD[k].tolist(), uvs[k], local) for k in range(4)]
            m.meshes.append(mesh(f, f2, mat, slot, verts, [(0, 1, 2), (2, 1, 3)], pal))
            q += 1
    return m, q, lines


def chunk_groups(groups):
    """Split {key: count} into model loads of <= min(MAX_BONES, MAX_RECORDS) quads (a group may
    straddle two models)."""
    cap = min(MAX_BONES, MAX_RECORDS)
    loads, cur, used = [], [], 0
    for key, count in sorted(groups.items()):
        while count:
            take = min(count, cap - used)
            cur.append((key, take))
            used += take
            count -= take
            if used == cap:
                loads.append(cur)
                cur, used = [], 0
    if cur:
        loads.append(cur)
    return loads


def chunk_strips(strips):
    """First-fit the strips into model loads of <= MAX_BONES bones / MAX_RECORDS meshes."""
    loads = []
    for st in sorted(strips, key=lambda x: -x[1]):
        nrec = strip_meshes(st[1])
        for ld in loads:
            if sum(x[1] for x in ld) + st[1] <= MAX_BONES and sum(strip_meshes(x[1]) for x in ld) + nrec <= MAX_RECORDS:
                ld.append(st)
                break
        else:
            loads.append([st])
    return loads


def strip_meshes(pts):
    """Meshes a strip of `pts` points splits into (<= 52 bones each, one shared point at a seam)."""
    return max(1, -(-(pts - 1) // (K.PALETTE_BLOCK - 1)))


def ribbon_model(name, strips, tex_stem):
    m = Model(name, tex_stem)
    lines, bone, rec = [], 0, 0
    for s, (tex, pts, additive) in enumerate(strips):
        f, f2 = flags_for(additive, True)
        mat = m.material('fxr%d' % s)
        assert mat == s
        slot = m.tex_slot(tex)
        first_rec = rec
        start = 0
        while start < pts - 1:
            end = min(start + K.PALETTE_BLOCK, pts)
            pal = [bone + j for j in range(start, end)]
            verts, tris = [], []
            for j in range(start, end):
                v = j / float(pts - 1)
                local = j - start
                verts += [([-1.0, 0.0, 0.0], [0.0, v], local), ([1.0, 0.0, 0.0], [1.0, v], local)]
            for j in range(end - start - 1):
                a = 2 * j
                tris += [(a, a + 1, a + 2), (a + 1, a + 3, a + 2)]
            m.meshes.append(mesh(f, f2, mat, slot, verts, tris, pal))
            rec += 1
            start = end - 1
        lines.append('strip %d %d %d %d %d %d' % (bone, pts, first_rec, rec - first_rec, tex, additive))
        bone += pts
    return m, bone, rec, lines


def port_stage(mapset, bank_label, bank_path):
    key = os.path.basename(mapset)[len('mapset_'):]
    blob = open(bank_path, 'rb').read()
    (_p, teb, tpl), = T.teb_members(blob)
    fx = T.parse_teb(teb)
    images = Z.tpl_images(tpl)

    def tex_stem(i):
        return '%sfx%02d' % (key, i)

    for i in range(len(images)):
        K.texture_registry_key(tex_stem(i))   # (the stems stay <= 20 alphanumerics: key + fx + 2)
    fx_dir = os.path.join(mapset, 'flight_fx')
    os.makedirs(fx_dir, exist_ok=True)
    for old in glob.glob(os.path.join(mapset, 'fx_%s_*' % key)):
        for f in glob.glob(os.path.join(old, '*')):
            os.remove(f)
        os.rmdir(old)
    with open(os.path.join(fx_dir, 'flight_fx.teb'), 'wb') as f:
        f.write(teb)
    out = ['# HOTTEST PARTY flight effects for %s, from %s (boss_ddr3.TEB)' % (key, bank_label),
           '# generated by tools/blender_ddr_addon/examples/port_flight_fx.py -- do not edit; re-run the script',
           'flight_fx 1', 'metres_per_unit %.9f' % Z.GAME_SCALE]
    placed = set()

    def put_textures(d, model):
        for t in sorted(model.tex_of):
            if t in placed:
                continue
            placed.add(t)
            im = images[t]
            rows = [bytes(im[r].astype('uint8').tobytes()) for r in range(im.shape[0])]
            with open(os.path.join(d, tex_stem(t) + '.dds'), 'wb') as f:
                f.write(K.write_dds_a8r8g8b8(im.shape[1], im.shape[0], rows))

    for p in range(PLAYERS):
        groups, strips = pools_for(fx, images, p)
        for k, load in enumerate(chunk_groups(groups)):
            sm, quads, glines = sprite_model('fx_%s_s%d%s' % (key, p, chr(ord('a') + k)), load, tex_stem)
            put_textures(sm.write(mapset, quads), sm)
            out.append('sprites %d %s %d' % (p, sm.name, quads))
            out += glines
            print('  %s player %d: %s %d quads in %d groups' % (key, p, sm.name, quads, len(glines)))
        for k, load in enumerate(chunk_strips(strips)):
            rm, rbones, rrecs, slines = ribbon_model('fx_%s_r%d%s' % (key, p, chr(ord('a') + k)), load, tex_stem)
            assert rbones <= MAX_BONES and rrecs <= MAX_RECORDS, (key, p, rbones, rrecs)
            put_textures(rm.write(mapset, rbones), rm)
            out.append('ribbons %d %s %d %d' % (p, rm.name, rbones, rrecs))
            out += slines
            print('  %s player %d: %s %d strips (%d bones, %d meshes)' % (key, p, rm.name, len(slines), rbones, rrecs))
    s = '\n'.join(out) + '\n'
    assert isinstance(s, str)
    with open(os.path.join(fx_dir, 'flight_fx.txt'), 'w') as f:
        f.write(s)


def main():
    want = [s for s in os.environ.get('STAGES', '').split(',') if s]
    found = sorted(glob.glob(os.path.join(STAGES_DIR, '**', 'mapset_*', 'flight.txt'), recursive=True))
    if not found:
        sys.exit('no flight stage under data_mods/custom_models/stages')
    for marker in found:
        mapset = os.path.dirname(marker)
        key = os.path.basename(mapset)[len('mapset_'):]
        if want and key not in want:
            continue
        label, path = bank_for(mapset)
        if not os.path.exists(path):
            sys.exit('%s: effect bank missing (%s)' % (key, label))
        print('%s <- %s' % (key, label))
        port_stage(mapset, label, path)


if __name__ == '__main__':
    main()
