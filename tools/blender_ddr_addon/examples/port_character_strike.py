"""EXAMPLE / PORT: the System 573-engine polygon dancers of the PS2 DDR games -- DDR STRIKE (JP 2006),
DDR FESTIVAL (JP 2004) and DDR PARTY COLLECTION (JP 2003) -- WITH THEIR OWN RIG AND THEIR OWN
CHOREOGRAPHY, as Background Dancers custom dancers.

The three discs carry the Konami System 573 dancer engine's data almost unchanged
(docs/ps2_ddr_filedata_research.md §4, §6). The meshes and the rig tables have 573's layouts;
the textures are TCB images and the motion files have a PS2 key-block layout, which
scripts/sys573_dancer_dump.py reads. So every step is port_character_sys573.py's
(`port_loaded`: rig with hand/face helper bones, one rigid mesh on a 512x256 atlas, role
aliases, 16 routines to `motion/<routine>.anm` checked < 1 mm per joint, sidecar). Only the
loading differs, and only in where things sit on each disc (GAMES below):

  * mesh      unpacked/<id>/001.cmd  (the dancer ids, minus multiples of 8, in table order)
  * texture   unpacked/<id>/000.tcb  (192x256 8bpp, the left of the 256x256 page the UVs
              address; palette alpha 0 = transparent, anything else opaque: the game's
              own alpha is not consistent, e.g. STRIKE's NAOKI1 is 0x40 everywhere)
  * rig       elf/chara.lst, elf/chara20.lst, elf/chara.pos (copied from the ELF by the
              extractor; byte-identical on the three discs). 28-object meshes take chara.lst,
              20-object ones (one hand shape per hand; 6 of Party Collection's) chara20.lst --
              the table's type field (flags bits 0-1: 1 / 0) says which, and the mesh agrees
  * motion    unpacked/<set>/<slot>.cmm, 8 sets of 17 routine slots; each routine is taken
              once (the copies differ only in padding); slot 0 (`normal`, 1-measure idles) is
              skipped as in the 573 port. The three discs share the 16 routines

The character table is in each ELF: 0x1C-byte records {f32 scale, f32 shadow brightness, u32
flags, ptr, ptr, ptr per-joint, char *name}, record k = the k-th dancer id. `scale` goes to the
sidecar's model_scale: the game multiplies every joint's world translation by it and pre-divides
the dancer's floor position by it, i.e. it scales the whole character (BABY-LON 0.4 is its small
mascot). The record's +0x10 pointer is the character's motion-set list: 4 set indices, one drawn
at random per song (`rnd_get_dancer_mot` in the discs' DWARF info). Values 1..4 are the male
routine sets and 5..7 the female ones (Party Collection 0x2A2634..3C and Festival 0x2A4580..84
hold the female lists), which gives the sidecar's sex. Like STRIKE's, every dancer still gets all
16 routines. When the ELF sits beside the extraction (`<disc>/<elf>` with
`<disc>/extracted_full`), the hard-coded tables below are checked against it.

Output: OUT_BASE/<source>/<label>/ -- the source folder of the Background Dancers menu
(`DDR Strike`, `DDR FESTIVAL`, `DDR PARTY COLLN`), one friendly folder per dancer.

Inputs (environment):
  GAME        strike (default) | festival | pc (Party Collection)
  PS2_DIR     the extraction (scripts/extract_ps2_ddr_data.py extract strike_jp | festival_jp |
              party_collection_jp ... --unpack), default ~/Desktop/DDR PS2 ISOs/<disc>/extracted_full
              (STRIKE_DIR is still read for GAME=strike)
  DANCERS     comma list of game names (default the first), or 'all'
  ROOT_MODE   / OUT_BASE / PREVIEW / PREVIEW_DIR   as port_character_sys573.py (OUT_BASE is the
              dancers folder the source folder goes in, default data_mods/custom_models/dancers)
Run: GAME=festival DANCERS=all /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
       --python tools/blender_ddr_addon/examples/port_character_strike.py
"""
import glob
import os
import struct
import sys

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.normpath(os.path.join(HERE, '..', '..', '..'))
sys.path.insert(0, HERE)
sys.path.insert(0, os.path.join(REPO, 'scripts'))
import port_character_sys573 as PS  # noqa: E402  (registers the add-on)
import sys573_dancer_dump as S  # noqa: E402
from extract_ps2_ddr_data import decode_tcb, elf_segments, va_to_offset  # noqa: E402

DISCS = '~/Desktop/DDR PS2 ISOs'

# The ELF character tables, in record order: (game name, label, key stem, sex, model scale, objects).
# Labels are the shipped friendly-folder names (<= 15 bytes, the options row's SSO budget).

# DDR STRIKE (SLPM_662.42 VA 0x2B0410, 45 records). Sex is judged from the models (it picks the
# sidecar's shadow scale, 0.75 F / 0.8 M, as the 573 port does); the 573 characters keep that
# port's call (BABY-LON is 5thMIX's `qp`: M). Three names are shortened: BABY-LON -> Baby-Lon,
# PRINCESS-ZUKIN -> P-Zukin, ROBO2001 -> Robo.
STRIKE = [
    ('BLUES1', 'Blues 1', 'blues1', 'M', 1.0), ('BLUES2', 'Blues 2', 'blues2', 'M', 1.0),
    ('RHYTHM1', 'Rhythm 1', 'rhythm1', 'F', 1.0), ('RHYTHM2', 'Rhythm 2', 'rhythm2', 'F', 1.0),
    ('DRUM1', 'Drum 1', 'drum1', 'M', 1.0), ('DRUM2', 'Drum 2', 'drum2', 'M', 1.0),
    ('BASS1', 'Bass 1', 'bass1', 'F', 1.0), ('BASS2', 'Bass 2', 'bass2', 'F', 1.0),
    ('RAGE1', 'Rage 1', 'rage1', 'M', 0.97), ('RAGE2', 'Rage 2', 'rage2', 'M', 0.98),
    ('EMI1', 'Emi 1', 'emi1', 'F', 1.0), ('EMI2', 'Emi 2', 'emi2', 'F', 0.97),
    ('ASTRO1', 'Astro 1', 'astro1', 'M', 1.0), ('ASTRO2', 'Astro 2', 'astro2', 'M', 1.0),
    ('CHARMY1', 'Charmy 1', 'charmy1', 'F', 1.0), ('CHARMY2', 'Charmy 2', 'charmy2', 'F', 1.0),
    ('ALICE1', 'Alice 1', 'alice1', 'F', 1.0), ('ALICE2', 'Alice 2', 'alice2', 'F', 1.0),
    ('BABY-LON1', 'Baby-Lon 1', 'babylon1', 'M', 0.4), ('BABY-LON2', 'Baby-Lon 2', 'babylon2', 'M', 0.4),
    ('BOLDO1', 'Boldo 1', 'boldo1', 'M', 1.0), ('BOLDO2', 'Boldo 2', 'boldo2', 'M', 1.0),
    ('TRACY1', 'Tracy 1', 'tracy1', 'F', 1.0), ('TRACY2', 'Tracy 2', 'tracy2', 'F', 1.0),
    ('JENNY1', 'Jenny 1', 'jenny1', 'F', 1.0), ('JENNY2', 'Jenny 2', 'jenny2', 'F', 1.0),
    ('JOHNNY1', 'Johnny 1', 'johnny1', 'M', 1.0), ('JOHNNY2', 'Johnny 2', 'johnny2', 'M', 1.0),
    ('PRINCESS-ZUKIN1', 'P-Zukin 1', 'pzukin1', 'F', 0.93), ('PRINCESS-ZUKIN2', 'P-Zukin 2', 'pzukin2', 'F', 0.93),
    ('ROBO20011', 'Robo 1', 'robo1', 'M', 0.98), ('ROBO20012', 'Robo 2', 'robo2', 'M', 0.98),
    ('NAOKI1', 'Naoki 1', 'naoki1', 'M', 1.01), ('NAOKI2', 'Naoki 2', 'naoki2', 'M', 1.01),
    ('LADY1', 'Lady 1', 'lady1', 'F', 1.0), ('LADY2', 'Lady 2', 'lady2', 'F', 1.0),
    ('AKIRA1', 'Akira 1', 'akira1', 'M', 1.06), ('AKIRA2', 'Akira 2', 'akira2', 'M', 1.06),
    ('YUNI1', 'Yuni 1', 'yuni1', 'F', 0.96), ('YUNI2', 'Yuni 2', 'yuni2', 'F', 0.96),
    ('J.C.1', 'J.C. 1', 'jc1', 'M', 1.0), ('J.C.2', 'J.C. 2', 'jc2', 'M', 1.0),
    ('SA-JA1', 'Sa-Ja 1', 'saja1', 'F', 1.0), ('SA-JA2', 'Sa-Ja 2', 'saja2', 'F', 1.0),
    ('RHYTHM3', 'Rhythm 3', 'rhythm3', 'F', 1.0),
]

# DDR FESTIVAL (SLPM_657.75 VA 0x29ED80, 26 records): eight characters in three or four costumes.
FESTIVAL = [
    ('BLUES1', 'Blues 1', 'blues1', 'M', 1.0), ('BLUES2', 'Blues 2', 'blues2', 'M', 1.0),
    ('BLUES3', 'Blues 3', 'blues3', 'M', 1.0),
    ('DRUM1', 'Drum 1', 'drum1', 'M', 1.0), ('DRUM2', 'Drum 2', 'drum2', 'M', 1.0),
    ('DRUM3', 'Drum 3', 'drum3', 'M', 1.0),
    ('DISCO1', 'Disco 1', 'disco1', 'M', 1.03), ('DISCO2', 'Disco 2', 'disco2', 'M', 1.03),
    ('DISCO3', 'Disco 3', 'disco3', 'M', 1.03),
    ('RAGE1', 'Rage 1', 'rage1', 'M', 0.97), ('RAGE2', 'Rage 2', 'rage2', 'M', 0.98),
    ('RAGE3', 'Rage 3', 'rage3', 'M', 0.98),
    ('RHYTHM1', 'Rhythm 1', 'rhythm1', 'F', 1.0), ('RHYTHM2', 'Rhythm 2', 'rhythm2', 'F', 1.0),
    ('RHYTHM3', 'Rhythm 3', 'rhythm3', 'F', 1.0), ('RHYTHM4', 'Rhythm 4', 'rhythm4', 'F', 1.0),
    ('BASS1', 'Bass 1', 'bass1', 'F', 1.0), ('BASS2', 'Bass 2', 'bass2', 'F', 1.0),
    ('BASS3', 'Bass 3', 'bass3', 'F', 1.0), ('BASS4', 'Bass 4', 'bass4', 'F', 1.0),
    ('LADY1', 'Lady 1', 'lady1', 'F', 1.0), ('LADY2', 'Lady 2', 'lady2', 'F', 1.0),
    ('LADY3', 'Lady 3', 'lady3', 'F', 1.0),
    ('EMI1', 'Emi 1', 'emi1', 'F', 1.0), ('EMI2', 'Emi 2', 'emi2', 'F', 0.97),
    ('EMI3', 'Emi 3', 'emi3', 'F', 0.97),
]

# DDR PARTY COLLECTION (SLPM_624.27 VA 0x29E270, 60 records): the dancers of 1st..7thMIX and the
# CS mixes, named `<NAME>(<mix>)`; the label is `<Name> <mix>`. Shortened: OSHARE-ZUKIN -> O-Zukin,
# PRINCESS-ZUKIN -> P-Zukin, KONSENTO:03/2 (4th) -> Konsento 4th, EMI(unpublished) -> Emi
# Unpublished. Six are 20-object (chara20.lst) models.
PARTY_COLLECTION = [
    ('AFRO(1st)', 'Afro 1st', 'afro1st', 'M', 1.03),
    ('KONSENTO:01(1st)', 'Konsento 1st', 'konsento1st', 'M', 0.98),
    ('SPACE MAN(CS1st)', 'Space Man CS1st', 'spacemancs1st', 'M', 0.97),
    ('DISK:A(1st)', 'Disk A 1st', 'diska1st', 'M', 1.0),
    ('AFRO(2nd)', 'Afro 2nd', 'afro2nd', 'M', 1.03), ('DREAD(2nd)', 'Dread 2nd', 'dread2nd', 'M', 1.06),
    ('KONSENTO:02(2nd)', 'Konsento 2nd', 'konsento2nd', 'M', 0.98),
    ('MAMEO(CS2nd)', 'Mameo CS2nd', 'mameocs2nd', 'M', 1.0),
    ('ASTRO(3rd)', 'Astro 3rd', 'astro3rd', 'M', 1.0), ('BOLDO(3rd)', 'Boldo 3rd', 'boldo3rd', 'M', 1.0),
    ('RAGE(3rd)', 'Rage 3rd', 'rage3rd', 'M', 0.97), ('KONSENTO:03(3rd)', 'Konsento 3rd', 'konsento3rd', 'M', 0.98),
    ('BOLDO(4th)', 'Boldo 4th', 'boldo4th', 'M', 1.0), ('AKIRA(4th)', 'Akira 4th', 'akira4th', 'M', 1.06),
    ('ASTRO(4th)', 'Astro 4th', 'astro4th', 'M', 0.98), ('IZAM(4th)', 'Izam 4th', 'izam4th', 'M', 1.0),
    ('JOHNNY(4th)', 'Johnny 4th', 'johnny4th', 'M', 1.0), ('RAGE(4th)', 'Rage 4th', 'rage4th', 'M', 0.98),
    ('ROBO2000(4th)', 'Robo2000 4th', 'robo2000x4th', 'M', 0.98),
    ('KONSENTO:03/2(4th)', 'Konsento 4th', 'konsento4th', 'M', 0.98),
    ('AFRO(5th)', 'Afro 5th', 'afro5th', 'M', 1.03), ('BABY-LON(5th)', 'Baby-Lon 5th', 'babylon5th', 'M', 0.4),
    ('JOHNNY(5th)', 'Johnny 5th', 'johnny5th', 'M', 1.03), ('NAOKI(5th)', 'Naoki 5th', 'naoki5th', 'M', 1.01),
    ('NMR(5th)', 'NMR 5th', 'nmr5th', 'M', 1.01), ('RAGE(5th)', 'Rage 5th', 'rage5th', 'M', 0.98),
    ('ROBO2001(5th)', 'Robo2001 5th', 'robo2001x5th', 'M', 0.98), ('SPIKE(5th)', 'Spike 5th', 'spike5th', 'M', 1.01),
    ('U1(CS5th)', 'U1 CS5th', 'u1cs5th', 'M', 1.0), ('2MB(CS5th)', '2MB CS5th', '2mbcs5th', 'M', 1.0),
    ('TRAIN(7th)', 'Train 7th', 'train7th', 'M', 0.98),
    ('LADY(1st)', 'Lady 1st', 'lady1st', 'F', 1.0), ('DISK:B(1st)', 'Disk B 1st', 'diskb1st', 'F', 1.0),
    ('TAMAKO(CS1st)', 'Tamako CS1st', 'tamakocs1st', 'F', 1.0),
    ('OSHARE-ZUKIN(1st)', 'O-Zukin 1st', 'osharezukin1st', 'F', 0.97),
    ('JANET(2nd)', 'Janet 2nd', 'janet2nd', 'F', 1.05),
    ('KAERU-ZUKIN(2nd)', 'Kaeru-Zukin 2nd', 'kaeruzukin2nd', 'F', 0.97),
    ('LADY(2nd)', 'Lady 2nd', 'lady2nd', 'F', 1.02), ('COWKO(CS2nd)', 'Cowko CS2nd', 'cowkocs2nd', 'F', 0.97),
    ('CHARMY(3rd)', 'Charmy 3rd', 'charmy3rd', 'F', 1.0), ('EMI(3rd)', 'Emi 3rd', 'emi3rd', 'F', 1.0),
    ('DEVIL-ZUKIN(3rd)', 'Devil-Zukin 3rd', 'devilzukin3rd', 'F', 0.97),
    ('TRACY(3rd)', 'Tracy 3rd', 'tracy3rd', 'F', 1.0),
    ('CHARMY(4th)', 'Charmy 4th', 'charmy4th', 'F', 1.0),
    ('DEVIL-ZUKIN(4th)', 'Devil-Zukin 4th', 'devilzukin4th', 'F', 1.0),
    ('EMI(1) (4th)', 'Emi 1 4th', 'emi1x4th', 'F', 0.97), ('EMI(2) (4th)', 'Emi 2 4th', 'emi2x4th', 'F', 0.97),
    ('EMI(unpublished)', 'Emi Unpublished', 'emiunpub', 'F', 0.97), ('JENNY(4th)', 'Jenny 4th', 'jenny4th', 'F', 1.0),
    ('MAID-ZUKIN(4th)', 'Maid-Zukin 4th', 'maidzukin4th', 'F', 0.98), ('NI-NA(4th)', 'Ni-Na 4th', 'nina4th', 'F', 1.01),
    ('TRACY(4th)', 'Tracy 4th', 'tracy4th', 'F', 1.0), ('YUNI(4th)', 'Yuni 4th', 'yuni4th', 'F', 0.96),
    ('ALICE(5th)', 'Alice 5th', 'alice5th', 'F', 1.0), ('CHARMY(5th)', 'Charmy 5th', 'charmy5th', 'F', 1.0),
    ('EMI(5th)', 'Emi 5th', 'emi5th', 'F', 0.97), ('JANET(5th)', 'Janet 5th', 'janet5th', 'F', 1.05),
    ('MAHO(5th)', 'Maho 5th', 'maho5th', 'F', 1.03), ('PRINCESS-ZUKIN(5th)', 'P-Zukin 5th', 'pzukin5th', 'F', 0.93),
    ('BUS(7th)', 'Bus 7th', 'bus7th', 'F', 0.97),
]


def _ids(first, last):
    return [i for i in range(first, last + 1) if i % 8]  # the TOC skips multiples of 8


# Per disc: the extraction folder, the character table, the FILEDATA ids of the dancers (record k =
# k-th id) and of the 8 motion sets, the menu source folder, the key prefix (key = prefix + stem + '00').
GAMES = {
    'strike': dict(
        title='DDR STRIKE', disc='Strike', elf='SLPM_662.42',
        table_va=0x2B0410, female_groups=None, characters=STRIKE, dancer_ids=_ids(0xCB2, 0xCE4),
        motion_sets=[0xCA9, 0xCAA, 0xCAB, 0xCAC, 0xCAD, 0xCAE, 0xCAF, 0xCB1],
        source='DDR Strike', key_prefix='strk'),
    'festival': dict(
        title='DDR FESTIVAL', disc='Festival', elf='SLPM_657.75',
        table_va=0x29ED80, female_groups=(0x2A4580, 0x2A4584), characters=FESTIVAL,
        dancer_ids=_ids(0x3DF, 0x3FC), motion_sets=[0x3D6, 0x3D7, 0x3D9, 0x3DA, 0x3DB, 0x3DC, 0x3DD, 0x3DE],
        source='DDR FESTIVAL', key_prefix='fest'),
    'pc': dict(
        title='DDR PARTY COLLECTION', disc='Party Collection', elf='SLPM_624.27',
        table_va=0x29E270, female_groups=(0x2A2634, 0x2A2638, 0x2A263C), characters=PARTY_COLLECTION,
        dancer_ids=_ids(0x134, 0x177), motion_sets=[0x12B, 0x12C, 0x12D, 0x12E, 0x12F, 0x131, 0x132, 0x133],
        source='DDR PARTY COLLN', key_prefix='pc'),
}
for _g in GAMES.values():
    assert len(_g['dancer_ids']) == len(_g['characters']), _g['title']

GAME = os.environ.get('GAME', 'strike')
if GAME not in GAMES:
    raise SystemExit('GAME must be one of %s, not %r' % (', '.join(GAMES), GAME))
G = GAMES[GAME]
_default_dir = os.path.join(DISCS, G['disc'], 'extracted_full')
PS2_DIR = os.path.expanduser(os.environ.get('PS2_DIR') or (os.environ.get('STRIKE_DIR') if GAME == 'strike' else None)
                             or _default_dir)
CHARACTERS = G['characters']


def names_for(stem, label):
    key = G['key_prefix'] + stem + '00'
    assert len(label.encode()) <= 15, label  # the options row's SSO budget (catalog::MAX_LABEL_BYTES)
    assert key.isalnum() and key.islower(), key
    return label, key


def check_table():
    """Compare the hard-coded table with the ELF's when the ELF is beside the extraction: names,
    scales, sex groups (Festival / Party Collection) and the object table type per record."""
    elf_path = os.path.join(os.path.dirname(PS2_DIR.rstrip(os.sep)), G['elf'])
    if not os.path.exists(elf_path):
        print('TABLE not checked (no %s beside the extraction)' % G['elf'])
        return
    elf = open(elf_path, 'rb').read()
    segs = elf_segments(elf)

    def cstr(va):
        o = va_to_offset(segs, va)
        return elf[o:elf.index(b'\0', o)].decode('latin1')

    for k, (name, _label, _stem, sex, scale) in enumerate(CHARACTERS):
        o = va_to_offset(segs, G['table_va'] + 0x1C * k, 0x1C)
        f_scale, _outline, flags, _p0, group, _p2, p_name = struct.unpack_from('<ffIIIII', elf, o)
        assert cstr(p_name) == name, (k, cstr(p_name), name)
        assert abs(f_scale - scale) < 5e-3, (name, f_scale, scale)
        if G['female_groups']:
            assert ('F' if group in G['female_groups'] else 'M') == sex, (name, sex)
        nobj = open(os.path.join(PS2_DIR, 'unpacked', '%04x' % G['dancer_ids'][k], '001.cmd'), 'rb').read(12)[8]
        assert nobj == (28 if flags & 3 == 1 else 20), (name, flags, nobj)
    print('TABLE %s: %d records match %s' % (G['title'], len(CHARACTERS), G['elf']))


def texture_page(tcb_path):
    """The 256x256 page the .cmd UVs address, with the TCB at its left edge."""
    w, h, rgba = decode_tcb(open(tcb_path, 'rb').read())
    img = np.frombuffer(rgba, np.uint8).reshape(h, w, 4).copy()
    img[..., 3] = np.where(img[..., 3] > 0, 255, 0)
    page = np.zeros((256, 256, 4), np.uint8)
    page[:min(h, 256), :min(w, 256)] = img[:256, :256]
    return page


def load_character(index):
    fid = '%04x' % G['dancer_ids'][index]
    objects = S.parse_cmd(open(os.path.join(PS2_DIR, 'unpacked', fid, '001.cmd'), 'rb').read())
    lst_name = 'chara.lst' if len(objects) == 28 else 'chara20.lst'
    lst = S.parse_lst(open(os.path.join(PS2_DIR, 'elf', lst_name), 'rb').read())
    assert len(lst) == len(objects), (fid, len(lst), len(objects))
    pos = S.parse_pos(open(os.path.join(PS2_DIR, 'elf', 'chara.pos'), 'rb').read())
    groups = {}
    for i, (joint, _parent) in enumerate(lst):
        groups.setdefault(joint, []).append(i)
    tex = os.path.join(PS2_DIR, 'unpacked', fid, '000.tcb')
    return dict(name=CHARACTERS[index][0], objects=objects, lst=lst, rest=pos[1:17], groups=groups,
                texture=texture_page(tex), texture_path=tex)


def ps2_routines():
    """{routine: (clips, clip names)} over the 8 motion sets, each routine once."""
    out = {}
    for mset in G['motion_sets']:
        for path in sorted(glob.glob(os.path.join(PS2_DIR, 'unpacked', '%04x' % mset, '*.cmm'))):
            clips = S.load_motion(path)
            for rname, names in S.routines(clips).items():
                if len(names) > 1 and not rname.startswith('normal') and rname not in out:
                    out[rname] = (clips, names)
    return out


def port(index, routines):
    game_name, label, stem, sex, scale = CHARACTERS[index]
    label, key = names_for(stem, label)
    ch = load_character(index)
    source = '%s (PS2) "%s" (FILEDATA id 0x%04X)' % (G['title'], game_name, G['dancer_ids'][index])
    return PS.port_loaded(ch, label, key, sex, sorted(routines.items()), source, model_scale=scale,
                          script='port_character_strike.py GAME=%s' % GAME,
                          out_base=os.path.join(PS.OUT_BASE, G['source']))


if __name__ == '__main__':
    want = os.environ.get('DANCERS') or str(CHARACTERS[0][0])
    by_name = {c[0]: k for k, c in enumerate(CHARACTERS)}
    todo = range(len(CHARACTERS)) if want == 'all' else [by_name[n.strip()] for n in want.split(',') if n.strip()]
    assert len({c[1] for c in CHARACTERS}) == len({c[2] for c in CHARACTERS}) == len(CHARACTERS)
    check_table()
    routines = ps2_routines()
    assert len(routines) == 16, sorted(routines)
    for index in todo:
        port(index, routines)
    print('DONE')
