"""EXAMPLE / SHIPPED PORT: the DanceDanceRevolution Universe 2 (Xbox 360, PAL) dancers WITH THEIR OWN
RIGS AND THEIR OWN CLIPS -- port_character_universe.py (Universe 1) run against Universe 2's data,
into the shared `UNIVERSE 1-3` source folder (RE notes:
docs/dancing_stage_unleashed_dancers_port_feasibility.md section 15).

Universe 2 keeps Universe 1's formats (`.ddm` 0x112 with palette groups, 87 / 113-joint HD rigs,
15 Hz `.ani` clips). What differs, and what this script does about it:
  * every file is renamed `z2_*`: `z2_default_models.csv`, `z2_animations.csv` (the same 26-row
    table), `z2_<name>_costume.csv` with FIVE costume columns (SHADERS1..5);
  * Emi, Rage, Afro and Honey costumes A-D are Universe 1's meshes in new colourways (same heads and
    faces, new outfit textures): they ship as `<Name> <n> Alt` beside U1's `<Name> <n>`. Costume E
    is new: `<Name> 5`;
  * B (`b'` in the table) and Queen have four new costumes each: `B 5..8` and `Queen 2..5`
    (U1's Queen is `Queen 1`). Column 5 repeats column 4 for both and is dropped (checked: same
    model and the same resolved texture slots);
  * Angel is new (hidden in the table; one model, five identical columns). Lady is not in the game.
    `z2_Britney_*` is an unused near-copy of Angel (same mesh, a slightly different texture set)
    named only in the `___z2_default_models___*` leftovers, so it is not ported;
  * RENDER `specular` is new (B, Queen, Honey 5's enamel; the cell names the base texture, the
    `*_spe` maps are not used): drawn like `toon`;
  * keys are `uni2<type><costume:02>`, texture stems `u2<type><costume>_<texture>` (U1's are
    `uv...`, so the two games' textures never share a registry key).
Not ported, as in Universe 1: the FACE swaps, the toon ramp / outline, the sphere maps' view
dependence, and the create-your-own avatar parts (`z2_avt_bd_[fm]00..21` bodies +
`z2_avt_hd_[fm]00..04_a..d` heads; maintainer decision 2026-11-10).

Inputs (environment): UNIVERSE_DIR (default ~/Desktop/universe/extracted; scripts/xdvdfs_extract.py
then `scripts/extract_ultramix_data.py universe2_eu`), DANCERS (default all: emi,rage,afro,honey,b,
queen,angel; `emi:5` = one costume column), OUT_BASE, SOURCE (default `UNIVERSE 1-3`), GLASS_ALPHA,
PREVIEW, PREVIEW_DIR as in port_character_universe.py; PLAN_ONLY=1 prints each costume's material
plan and exits without building anything.
Run: /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
       --python tools/blender_ddr_addon/examples/port_character_universe2.py
"""
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
os.environ.setdefault('UNIVERSE_DIR', '~/Desktop/universe/extracted')
import port_character_universe as V  # noqa: E402
from port_character_ultramix import U  # noqa: E402

V.TITLE = 'DanceDanceRevolution Universe 2 (Xbox 360)'
V.SCRIPT = 'tools/blender_ddr_addon/examples/port_character_universe2.py'
V.MODELS_CSV = 'z2_default_models.csv'
V.ANIMATIONS_CSV = 'z2_animations.csv'
V.COSTUME_COLUMNS = 5
V.TEXTURE_PREFIXES = ('z2_',)
V.EXTRA_TEXTURE_PREFIXES = {'honey': ('honney_', 'honne_')}
PLAN_ONLY = os.environ.get('PLAN_ONLY', '0') == '1'

# our type -> (z2_default_models.csv TYPE, label stem, {costume column: label}, columns that repeat
# the previous column)
ALT = {n: '%s ' + str(n) + ' Alt' for n in range(1, 5)}
DANCERS = {
    'emi': ('emi', 'Emi', {**ALT, 5: '%s 5'}, ()),
    'rage': ('rage', 'Rage', {**ALT, 5: '%s 5'}, ()),
    'afro': ('afro', 'Afro', {**ALT, 5: '%s 5'}, ()),
    'honey': ('honey', 'Honey', {**ALT, 5: '%s 5'}, ()),
    'b': ("b'", 'B', {1: '%s 5', 2: '%s 6', 3: '%s 7', 4: '%s 8'}, (5,)),
    'queen': ('queen', 'Queen', {1: '%s 2', 2: '%s 3', 3: '%s 4', 4: '%s 5'}, (5,)),
    'angel': ('angel', 'Angel', {1: '%s'}, ()),
}


def resolved(model_file, table, key, kind, prefix):
    model = U.parse_ddm(open(V.rip_path(model_file), 'rb').read())
    slots, plan, _ = V.plan_slots(model, table, key, kind, prefix)
    return model_file.lower(), [(os.path.basename(src).lower(), t) for _, src, t in slots], plan


def port(kind, only=None):
    csv_type, stem, labels, repeats = DANCERS[kind]
    row = V.dancer_row(csv_type)
    sex, shadow, _ = V.GENDERS[row['GENDER']]
    clips, groups = V.clip_pool(row)
    sets = V.costumes(row)  # drops columns that repeat an earlier one's model and table exactly
    by_number = {n: (m, t) for n, m, t in sets}
    for n in repeats:
        if n in by_number:  # not an exact CSV repeat: check it resolves to the previous column
            a = resolved(*by_number[n], 'uni2%s%02d' % (kind, n), kind, 'u2%s%d' % (kind, n))
            b = resolved(*by_number[n - 1], 'uni2%s%02d' % (kind, n - 1), kind, 'u2%s%d' % (kind, n - 1))
            assert a == b, '%s costume %d is not a repeat of %d: %r vs %r' % (kind, n, n - 1, a, b)
            print('SKIP %s costume %d: same model and texture slots as costume %d' % (kind, n, n - 1))
    for number, model_file, table in sets:
        if number in repeats or (only is not None and number != only):
            continue
        assert number in labels, (kind, number)
        label = labels[number] % stem
        key = 'uni2%s%02d' % (kind, number)
        prefix = 'u2%s%d' % (kind, number)
        if PLAN_ONLY:
            model = U.parse_ddm(open(V.rip_path(model_file), 'rb').read())
            slots, plan, dropped = V.plan_slots(model, table, key, kind, prefix)
            renders = sorted({r for _, r in plan if r})
            print('PLAN %-14s %-11s %-22s %d bones, %d slots %s; dropped %s; renders %s; %d clips' % (
                label, key, model_file, len(model['bones']), len(slots), [s for s, _, _ in slots],
                dropped, renders, len(clips)))
            continue
        V.port_costume(kind, row, label, key, number, model_file, table, sex, shadow, clips, groups,
                       prefix=prefix)


if __name__ == '__main__':
    for item in [s.strip() for s in os.environ.get('DANCERS', ','.join(DANCERS)).split(',') if s.strip()]:
        kind, _, number = item.partition(':')
        port(kind, int(number) if number else None)
    print('DONE')
