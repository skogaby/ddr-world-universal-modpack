"""EXAMPLE / SHIPPED PORT: the nine DanceDanceRevolution Universe 3 (Xbox 360, NTSC-U) dancers WITH
THEIR OWN RIGS AND THEIR OWN CLIPS -- port_character_universe.py (Universe 1) run against Universe
3's data, into the shared `UNIVERSE 1-3` source folder (RE notes:
docs/dancing_stage_unleashed_dancers_port_feasibility.md section 16).

Universe 3 keeps the Universe 1 / 2 formats (`.ddm` 0x112 with palette groups, 87 / 113-joint HD
rigs, 15 Hz `.ani` clips; the skeleton is Universe 2's). What differs, and what this script does
about it:
  * a new cast of nine, one costume each: `z3_character_models.csv` (`ID, NAME, HIDDEN, COSTUME,
    GENDER` -- no TYPE column), `z3_chr_<id>.csv` with ONE costume column, `z3_chr_<id>_a.ddm`;
    `z3_animations.csv` is the same 26-row table again (`z3_` names);
  * texture names are `z3_chr_<id>_<part>`: the World stem is `u3<name>_<part>`, the key
    `uni3<name>01` and the label the NAME column in title case (`Valencia`, `Guy`, ...). No name
    clashes with a Universe 1 / 2 label.
Not ported, as in Universe 1 / 2: the FACE swaps, the toon ramp / outline, the sphere maps' view
dependence, the `specular` maps, and the create-your-own avatar parts (`z3_avt_*`: 41 female and
40 male bodies in six colourways a..f, 5 + 5 heads; maintainer decision 2026-11-10).

Inputs (environment): UNIVERSE_DIR (default ~/Desktop/universe/u3/extracted;
scripts/xdvdfs_extract.py then `scripts/extract_ultramix_data.py universe_us` -- Universe 3 keeps
Universe 1's US file names), DANCERS (default all; NAME column values, case-insensitive, or IDs),
OUT_BASE, SOURCE, GLASS_ALPHA, PREVIEW, PREVIEW_DIR as in port_character_universe.py; PLAN_ONLY=1
prints each dancer's material plan and exits without building anything.
Run: /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
       --python tools/blender_ddr_addon/examples/port_character_universe3.py
"""
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
os.environ.setdefault('UNIVERSE_DIR', '~/Desktop/universe/u3/extracted')
import port_character_universe as V  # noqa: E402
from port_character_ultramix import U  # noqa: E402

V.TITLE = 'DanceDanceRevolution Universe 3 (Xbox 360)'
V.SCRIPT = 'tools/blender_ddr_addon/examples/port_character_universe3.py'
V.MODELS_CSV = 'z3_character_models.csv'
V.ANIMATIONS_CSV = 'z3_animations.csv'
V.COSTUME_COLUMNS = 1
V.TEXTURE_PREFIXES = ('z3_',)
PLAN_ONLY = os.environ.get('PLAN_ONLY', '0') == '1'


def characters():
    return V.csv_rows(V.MODELS_CSV)


def port(row):
    name = row['NAME'].lower()
    kind = 'chr_%02d' % int(row['ID'])  # stripped from `chr_<id>_<part>` texture names
    label = row['NAME'].title()
    key = 'uni3%s01' % name
    prefix = 'u3' + name
    assert key.isalnum() and key.islower(), key
    sex, shadow, _ = V.GENDERS[row['GENDER']]
    clips, groups = V.clip_pool(row)
    sets = V.costumes(row)
    assert len(sets) == 1, (label, [n for n, _, _ in sets])
    number, model_file, table = sets[0]
    if PLAN_ONLY:
        model = U.parse_ddm(open(V.rip_path(model_file), 'rb').read())
        slots, plan, dropped = V.plan_slots(model, table, key, kind, prefix)
        renders = sorted({r for _, r in plan if r})
        print('PLAN %-9s %-14s %-16s %d bones, %d slots %s; dropped %s; renders %s; %d clips' % (
            label, key, model_file, len(model['bones']), len(slots), [s for s, _, _ in slots],
            dropped, renders, len(clips)))
        return
    V.port_costume(kind, row, label, key, number, model_file, table, sex, shadow, clips, groups,
                   prefix=prefix)


if __name__ == '__main__':
    rows = characters()
    wanted = [s.strip().lower() for s in os.environ.get('DANCERS', '').split(',') if s.strip()]
    for row in rows:
        if not wanted or row['NAME'].lower() in wanted or row['ID'] in wanted:
            port(row)
    print('DONE')
