"""EXAMPLE / PORT: the DDR STRIKE (PS2, JP 2006) polygon dancers WITH THEIR OWN RIG AND THEIR
OWN CHOREOGRAPHY, as Background Dancers custom dancers.

STRIKE's dancers are the Konami System 573 dancer engine's data, carried to the PS2 almost
unchanged (docs/ps2_ddr_filedata_research.md §4). The meshes and the rig tables have 573's
layouts; the textures are TCB images and the motion files have a PS2 key-block layout, which
scripts/sys573_dancer_dump.py reads. So every step is port_character_sys573.py's
(`port_loaded`: rig with hand/face helper bones, one rigid mesh on a 512x256 atlas, role
aliases, 16 routines to `motion/<routine>.anm` checked < 1 mm per joint, sidecar). Only the
loading differs:

  * mesh      unpacked/<id>/001.cmd  (ids 0xCB2..0xCE4 minus multiples of 8, in table order)
  * texture   unpacked/<id>/000.tcb  (192x256 8bpp, the left of the 256x256 page the UVs
              address; palette alpha 0 = transparent, anything else opaque: the game's
              own alpha is not consistent, e.g. NAOKI1 is 0x40 everywhere)
  * rig       elf/chara.lst, elf/chara.pos (copied from the ELF by the extractor)
  * motion    unpacked/0ca9..0cb1/<slot>.cmm, 8 sets of 17 routine slots; each routine is
              taken once (the copies differ only in padding); slot 0 (`normal`, 1-measure
              idles) is skipped as in the 573 port

The character table is at SLPM_662.42 VA 0x2B0410: 45 records of 0x1C bytes
{f32 scale, f32 outline, u32 flags, ptr, ptr, ptr per-joint, char *name}. Record k is the
k-th dancer id. `scale` goes to the sidecar's model_scale: the game multiplies every joint's
world translation by it (FUN_001b1430) and pre-divides the dancer's floor position by it
(FUN_001adda0), i.e. it scales the whole character (BABY-LON 0.4 is its small mascot).
Names are the game's (`BLUES1` = costume 1, `BLUES2` = costume 2), shortened only where the
15-byte label budget needs it: BABY-LON -> BabyLon, PRINCESS-ZUKIN -> P-Zukin, ROBO2001 -> Robo.

Inputs (environment):
  STRIKE_DIR  the extraction (scripts/extract_ps2_ddr_data.py extract strike_jp ... --unpack),
              default ~/Desktop/PS2 DDR ISOs/Dance Dance Revolution Strike (Japan)/extracted_full
  DANCERS     comma list of game names (default RAGE1), or 'all'
  ROOT_MODE   / OUT_BASE / PREVIEW / PREVIEW_DIR   as port_character_sys573.py
Run: /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
       --python tools/blender_ddr_addon/examples/port_character_strike.py
"""
import glob
import os
import sys

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.normpath(os.path.join(HERE, '..', '..', '..'))
sys.path.insert(0, HERE)
sys.path.insert(0, os.path.join(REPO, 'scripts'))
import port_character_sys573 as PS  # noqa: E402  (registers the add-on)
import sys573_dancer_dump as S  # noqa: E402
from extract_ps2_ddr_data import decode_tcb  # noqa: E402

STRIKE_DIR = os.path.expanduser(os.environ.get(
    'STRIKE_DIR', '~/Desktop/PS2 DDR ISOs/Dance Dance Revolution Strike (Japan)/extracted_full'))
MOTION_SETS = ['0ca9', '0caa', '0cab', '0cac', '0cad', '0cae', '0caf', '0cb1']

# The ELF character table, in order: (game name, label name, key stem, sex, model scale).
# Sex is judged from the models (it picks the sidecar's shadow scale, 0.75 F / 0.8 M, as the
# 573 port does); the 573 characters keep that port's call (BABY-LON is 5thMIX's `qp`: M).
CHARACTERS = [
    ('BLUES1', 'Blues1', 'blues1', 'M', 1.0), ('BLUES2', 'Blues2', 'blues2', 'M', 1.0),
    ('RHYTHM1', 'Rhythm1', 'rhythm1', 'F', 1.0), ('RHYTHM2', 'Rhythm2', 'rhythm2', 'F', 1.0),
    ('DRUM1', 'Drum1', 'drum1', 'M', 1.0), ('DRUM2', 'Drum2', 'drum2', 'M', 1.0),
    ('BASS1', 'Bass1', 'bass1', 'F', 1.0), ('BASS2', 'Bass2', 'bass2', 'F', 1.0),
    ('RAGE1', 'Rage1', 'rage1', 'M', 0.97), ('RAGE2', 'Rage2', 'rage2', 'M', 0.98),
    ('EMI1', 'Emi1', 'emi1', 'F', 1.0), ('EMI2', 'Emi2', 'emi2', 'F', 0.97),
    ('ASTRO1', 'Astro1', 'astro1', 'M', 1.0), ('ASTRO2', 'Astro2', 'astro2', 'M', 1.0),
    ('CHARMY1', 'Charmy1', 'charmy1', 'F', 1.0), ('CHARMY2', 'Charmy2', 'charmy2', 'F', 1.0),
    ('ALICE1', 'Alice1', 'alice1', 'F', 1.0), ('ALICE2', 'Alice2', 'alice2', 'F', 1.0),
    ('BABY-LON1', 'BabyLon1', 'babylon1', 'M', 0.4), ('BABY-LON2', 'BabyLon2', 'babylon2', 'M', 0.4),
    ('BOLDO1', 'Boldo1', 'boldo1', 'M', 1.0), ('BOLDO2', 'Boldo2', 'boldo2', 'M', 1.0),
    ('TRACY1', 'Tracy1', 'tracy1', 'F', 1.0), ('TRACY2', 'Tracy2', 'tracy2', 'F', 1.0),
    ('JENNY1', 'Jenny1', 'jenny1', 'F', 1.0), ('JENNY2', 'Jenny2', 'jenny2', 'F', 1.0),
    ('JOHNNY1', 'Johnny1', 'johnny1', 'M', 1.0), ('JOHNNY2', 'Johnny2', 'johnny2', 'M', 1.0),
    ('PRINCESS-ZUKIN1', 'P-Zukin1', 'pzukin1', 'F', 0.93), ('PRINCESS-ZUKIN2', 'P-Zukin2', 'pzukin2', 'F', 0.93),
    ('ROBO20011', 'Robo1', 'robo1', 'M', 0.98), ('ROBO20012', 'Robo2', 'robo2', 'M', 0.98),
    ('NAOKI1', 'Naoki1', 'naoki1', 'M', 1.01), ('NAOKI2', 'Naoki2', 'naoki2', 'M', 1.01),
    ('LADY1', 'Lady1', 'lady1', 'F', 1.0), ('LADY2', 'Lady2', 'lady2', 'F', 1.0),
    ('AKIRA1', 'Akira1', 'akira1', 'M', 1.06), ('AKIRA2', 'Akira2', 'akira2', 'M', 1.06),
    ('YUNI1', 'Yuni1', 'yuni1', 'F', 0.96), ('YUNI2', 'Yuni2', 'yuni2', 'F', 0.96),
    ('J.C.1', 'J.C.1', 'jc1', 'M', 1.0), ('J.C.2', 'J.C.2', 'jc2', 'M', 1.0),
    ('SA-JA1', 'Sa-Ja1', 'saja1', 'F', 1.0), ('SA-JA2', 'Sa-Ja2', 'saja2', 'F', 1.0),
    ('RHYTHM3', 'Rhythm3', 'rhythm3', 'F', 1.0),
]
DANCER_IDS = [i for i in range(0xCB2, 0xCE5) if i % 8]  # the TOC skips multiples of 8
assert len(DANCER_IDS) == len(CHARACTERS) == 45


def names_for(stem, label_name):
    label = 'Strike ' + label_name
    key = 'strk' + stem + '00'
    assert len(label.encode()) <= 15, label  # the options row's SSO budget (catalog::MAX_LABEL_BYTES)
    assert key.isalnum() and key.islower(), key
    return label, key


def texture_page(tcb_path):
    """The 256x256 page the .cmd UVs address, with the TCB at its left edge."""
    w, h, rgba = decode_tcb(open(tcb_path, 'rb').read())
    img = np.frombuffer(rgba, np.uint8).reshape(h, w, 4).copy()
    img[..., 3] = np.where(img[..., 3] > 0, 255, 0)
    page = np.zeros((256, 256, 4), np.uint8)
    page[:min(h, 256), :min(w, 256)] = img[:256, :256]
    return page


def load_character(index):
    fid = '%04x' % DANCER_IDS[index]
    objects = S.parse_cmd(open(os.path.join(STRIKE_DIR, 'unpacked', fid, '001.cmd'), 'rb').read())
    lst = S.parse_lst(open(os.path.join(STRIKE_DIR, 'elf', 'chara.lst'), 'rb').read())
    pos = S.parse_pos(open(os.path.join(STRIKE_DIR, 'elf', 'chara.pos'), 'rb').read())
    groups = {}
    for i, (joint, _parent) in enumerate(lst[:len(objects)]):
        groups.setdefault(joint, []).append(i)
    tex = os.path.join(STRIKE_DIR, 'unpacked', fid, '000.tcb')
    return dict(name=CHARACTERS[index][0], objects=objects, lst=lst, rest=pos[1:17], groups=groups,
                texture=texture_page(tex), texture_path=tex)


def strike_routines():
    """{routine: (clips, clip names)} over the 8 motion sets, each routine once."""
    out = {}
    for mset in MOTION_SETS:
        for path in sorted(glob.glob(os.path.join(STRIKE_DIR, 'unpacked', mset, '*.cmm'))):
            clips = S.load_motion(path)
            for rname, names in S.routines(clips).items():
                if len(names) > 1 and not rname.startswith('normal') and rname not in out:
                    out[rname] = (clips, names)
    return out


def port(index, routines):
    game_name, label_name, stem, sex, scale = CHARACTERS[index]
    label, key = names_for(stem, label_name)
    ch = load_character(index)
    source = 'DDR STRIKE (PS2) "%s" (FILEDATA id 0x%04X)' % (game_name, DANCER_IDS[index])
    return PS.port_loaded(ch, label, key, sex, sorted(routines.items()), source, model_scale=scale,
                          script='port_character_strike.py')


if __name__ == '__main__':
    want = os.environ.get('DANCERS', 'RAGE1')
    by_name = {c[0]: k for k, c in enumerate(CHARACTERS)}
    todo = range(len(CHARACTERS)) if want == 'all' else [by_name[n.strip()] for n in want.split(',') if n.strip()]
    routines = strike_routines()
    assert len(routines) == 16, sorted(routines)
    for index in todo:
        port(index, routines)
    print('DONE')
