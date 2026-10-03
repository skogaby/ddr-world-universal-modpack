"""EXAMPLE / PORT: the DanceDanceRevolution SuperNova (PS2, JP 2006) and SuperNova 2 (PS2, JP 2008)
polygon dancers WITH THEIR OWN RIG AND THEIR OWN CHOREOGRAPHY, as Background Dancers custom
dancers -- the Omnimix path of the Ultramix / System 573 / STRIKE ports (port_character_ultramix.py),
fed by the TZM decoders in scripts/tzm_dump.py (formats + RE: docs/ps2_ddr_filedata_research.md §7.4).

SuperNova's engine is new (XSI exports: skinned strip meshes with up to three weights per
vertex on a 22-joint HumanIK-named skeleton, 30 Hz quaternion clips), not the 573's. Per dancer:
  1. rebuild the rig from the MODEL chunk's bone list in World game space (Y-up metres, facing
     +Z, left at +X -- the TZM frame's own handedness, so only a scale applies:
     tzm_dump.GAME_SCALE puts the Hip at World's 0.97 m; BABYLON's `SCALE` node (0.6; JULIO's
     0.9 in SuperNova 2) is folded into that scale and dropped from the rig);
  2. build ONE skinned mesh from every mesh of every object (object transforms applied -- AFRO's
     muffler is authored in its own frame), the file's own normals via `ddr_normal`, white
     COLOR0 (a mesh with vertex colours keeps them: GUS's glasses are 60 % alpha and go to a
     second, alpha-blended material slot), strip winding made consistent with the normals;
     texture = the pack's 512² CLUT sheet (alpha 0x80 -> opaque), exported with the add-on;
  3. add World ROLE-BONE ALIASES to the body `.b2it` (Hips / Left|RightToeBase -> Hip /
     Left|RightToes; Spine2 and Head are named alike);
  4. convert the character's OWN routine list (the ELF character table: SLPM_666.09 0x3A5260,
     SLPM_669.30 0x3D2D90 / 0x3D3480) to `motion/<clip>.anm` against the EXPORTED bind frames
     (tzm_dump.clip_to_anm_spec: 30 Hz keys every 2nd frame of the 60 fps timeline; the clips
     are authored at 120 BPM like World's), each checked against the TZM pose (< 1 mm per
     joint). The 4 s `*_NE_01` idles are left out of the dance pool;
  5. write the `chara_resources.rlist.txt` sidecar (sex from the routine family: FF / MM).

SuperNova 2 (GAME=sn2): 12 characters x 2 costumes (`<skin>01` / `<skin>02`, the ELF table has
one 0x94-byte record per costume; labels `<Name> 1` / `<Name> 2`), the same 29 routine packs
byte for byte, root node named after the pack (tzm_dump matches the clip root by position).
The body meshes have NO face there: the eyes / mouth are `<skin>_face.TZM` expression masks (three
128² sheets on an unskinned mask the game hangs off the Head) -- the port joins the neutral one
(FACE, default `face01`) to the body as its own material slot weighted to `Head`
(tzm_dump.face_overlay). CONCENT's spinning chest fan (`body01 > body_trans_null > fan01` in the
same pack, hung off Spine1, spun -720 deg per 4 s by the pack's `ddr_concent_fan` record) becomes an
extra joint `fan01` under `Spine1` (tzm_dump.attach_part_bone) whose spin is laid onto every dance
clip at the clip's own key frames (tzm_dump.part_spin_track; `SPIN=0` keeps it static).

DDR X (GAME=x, JP 2008) and DDR X2 (GAME=x2, US 2009): the same engine and the same 29 routine
packs again (byte-identical), one 0xDC-byte ELF record per costume (SLPM_550.90 0x34686C, 41
records; SLUS_219.17 0x2E433C, 35 -- see X_CHARACTERS). Both games bundle SuperNova 2's skins
as their costumes 02 / 03 (X) and 03 (X2) and add new ones: X's costume 01 for all twelve,
a second BABYLON, BONNIE, ZERO; X2's costume 02 (X's 01 bodies on new sheets), a second
BONNIE / ZERO and the four PIX pigs. `DANCERS=all` ports only what SuperNova 2 does not have
(the two games ship into ONE source folder, `DDR X + X2`, beside `DDR SUPRNVA 1+2`); the
DISK-A / -B / -? ring mannequins (`wakka_*`, zero-area strips the GS drew as lines) are left
out. Differences from SuperNova 2 handled here: the table names each costume's face root
(`face01` except PIX's `jx_pixNN_face1`) and face pack (X2's bonnie02 / zero02 reuse the 01
pack), X2 marks the mixed-routine PIX female (so the sex is the table's flag, not the routine
family), the per-character shadow scale is the table's own (+0x8C: BABYLON 0.35 .. CONCENT
0.85), and CONCENT's fan (`parts/convent01_body01.tzm`, hung off `Spine1`) is joined as a
static part (tzm_dump.part_overlay).

Inputs (environment):
  GAME        sn (default: SuperNova) | sn2 (SuperNova 2) | x (DDR X) | x2 (DDR X2) -- picks the
              extraction, the character table, the key prefix (`sn` / `sn2` / `x` / `x2`) and
              the staging folder
  SN_DIR      the extraction (scripts/extract_ps2_ddr_data.py extract supernova_jp | supernova2_jp |
              x_jp | x2_us ...), default ~/Desktop/PS2 DDR ISOs/<disc>/extracted_full
  DANCERS     comma list of skin names (sn: afro babylon emi gus jenny rage robozukin ruby;
              sn2 / x / x2: afro01 .. zukin03, pix01 ..), default the first, or 'all'
  OUT_BASE    default ~/Desktop/<Game> Dancers (one folder per character, named after it; NOT
              data_mods/custom_models -- move the folders into
              data_mods/custom_models/dancers/<Source>/ by hand)
  PREVIEW     1 = also render Workbench previews of the RE-IMPORTED export into PREVIEW_DIR
              (default the system temp dir -- never into the repo)
Run: GAME=sn2 DANCERS=all /Applications/Blender.app/Contents/MacOS/Blender -b --factory-startup \
       --python tools/blender_ddr_addon/examples/port_character_supernova.py
"""
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
import tzm_dump as Z  # noqa: E402
from blender_ddr_addon import convert, export_character, import_anm, import_character  # noqa: E402
from blender_ddr_addon.codec import anm as A  # noqa: E402
from blender_ddr_addon.codec import ktmdl as K  # noqa: E402

SHADER = 'mdl_ch_constant_vc'

# SuperNova: the ELF character table (SLPM_666.09 0x3A5260, 0x60 bytes per record: name, 1,
# IMAGE file index, "globalSRT", the motion index list terminated by 30, ..., sex at +0x54).
# Motion names index the list at 0x3A5B8D (FF_NE_01 .. FF_SF_03 = 0..13, MM_NE_01 .. MM_SF_03 =
# 14..28). skin -> (folder / label, key, the character's routines minus its NE idle)
SN_CHARACTERS = {
    'afro': ('Afro', 'snafro00', ['MM_JA_01', 'MM_JA_02', 'MM_SF_01', 'MM_SF_02', 'MM_SF_03']),
    'emi': ('Emi', 'snemi00', ['FF_HH_02', 'FF_HT_01', 'FF_HT_02', 'FF_SF_01', 'FF_SF_02', 'FF_SF_03']),
    'babylon': ('Baby-Lon', 'snbabylon00',
                ['MM_HT_01', 'MM_HT_02', 'MM_JA_01', 'MM_JA_02', 'MM_SF_01', 'MM_SF_02', 'MM_SF_03']),
    'robozukin': ('Robo-Zukin', 'snrobozukin00',
                  ['FF_HT_01', 'FF_HT_02', 'FF_JA_02', 'FF_SF_01', 'FF_SF_02', 'FF_SF_03']),
    'rage': ('Rage', 'snrage00', ['MM_BR_01', 'MM_BR_02', 'MM_BR_03', 'MM_HH_01', 'MM_HH_02', 'MM_HT_01',
                                  'MM_HT_02', 'MM_HT_03', 'MM_HT_04']),
    'jenny': ('Jenny', 'snjenny00', ['FF_BR_01', 'FF_BR_02', 'FF_HH_01', 'FF_HH_02', 'FF_HH_03']),
    'gus': ('Gus', 'sngus00', ['MM_HT_01', 'MM_HT_02', 'MM_HT_03', 'MM_HT_04']),
    'ruby': ('Ruby', 'snruby00', ['FF_BR_01', 'FF_BR_02', 'FF_HH_03', 'FF_JA_01']),
}
# SuperNova 2: the ELF character table (SLPM_669.30: costume 01 records at 0x3D2D90, costume 02 at
# 0x3D3480, 12 x 0x94 bytes each: name, body IMAGE index, body skin name, face IMAGE index, the
# three face mesh names, [CONCENT: fan part], f32, the motion index list terminated by 30 (the
# same 29-name list, at 0x3D4152), ..., an RGBA theme colour at +0x8C). Both costumes of a
# character dance the same routines. (stem, label, routines minus the NE idle)
SN2_CHARACTERS = [
    ('afro', 'Afro', ['MM_JA_01', 'MM_JA_02', 'MM_SF_01', 'MM_SF_02', 'MM_SF_03']),
    ('emi', 'Emi', ['FF_HH_02', 'FF_HT_01', 'FF_HT_02', 'FF_SF_01', 'FF_SF_02', 'FF_SF_03']),
    ('babylon', 'Baby-Lon', ['MM_HT_01', 'MM_HT_02', 'MM_JA_01', 'MM_JA_02', 'MM_SF_01', 'MM_SF_02', 'MM_SF_03']),
    ('zukin', 'Robo-Zukin', ['FF_HT_01', 'FF_HT_02', 'FF_JA_02', 'FF_SF_01', 'FF_SF_02', 'FF_SF_03']),
    ('rage', 'Rage', ['MM_BR_01', 'MM_BR_02', 'MM_BR_03', 'MM_HH_01', 'MM_HH_02', 'MM_HT_01', 'MM_HT_02',
                      'MM_HT_03', 'MM_HT_04']),
    ('jenny', 'Jenny', ['FF_BR_01', 'FF_BR_02', 'FF_HH_01', 'FF_HH_02', 'FF_HH_03']),
    ('gus', 'Gus', ['MM_HT_01', 'MM_HT_02', 'MM_HT_03', 'MM_HT_04']),
    ('ruby', 'Ruby', ['FF_BR_01', 'FF_BR_02', 'FF_HH_03', 'FF_JA_01']),
    ('yuni', 'Yuni', ['FF_HH_01', 'FF_HH_02', 'FF_HT_03', 'FF_SF_01', 'FF_SF_02', 'FF_SF_03']),
    ('alice', 'Alice', ['FF_HT_01', 'FF_HT_02', 'FF_JA_01', 'FF_JA_02', 'FF_SF_02', 'FF_SF_03']),
    ('concent', 'Concent', ['MM_HT_01', 'MM_JA_01', 'MM_JA_02', 'MM_SF_01', 'MM_SF_02', 'MM_SF_03']),
    ('julio', 'Julio', ['MM_BR_01', 'MM_BR_02', 'MM_BR_03', 'MM_HH_01', 'MM_HH_02', 'MM_HT_01', 'MM_HT_02', 'MM_SF_02']),
]
# SuperNova 2's costume-01 bodies of the eight returning characters ARE the SuperNova skins
# (same textures -- EMI's and ROBOZUKIN's recoloured -- same rigs, same geometry minus the face
# triangles the expression mask replaces), so `all` ports the sixteen that are new: every
# costume 02 and both costumes of YUNI, ALICE, CONCENT, JULIO. `DANCERS=afro01` still works.
SN2_RETURNING = {'afro', 'emi', 'babylon', 'zukin', 'rage', 'jenny', 'gus', 'ruby'}

# DDR X / X2: the ELF character table (SLPM_550.90 0x34686C, 41 records; SLUS_219.17 0x2E433C, 35),
# 0xDC bytes per costume: u32 name hash, char *name, (u32 IMAGE index, char *root node) for the
# body skin and the three faces (neutral first -- `face01`, PIX `jx_pixNN_face1`), [CONCENT: its
# fan `body01` on `Spine1`], the 2D cut-in assets, f32 shadow scale at +0x8C, the motion index
# list at +0x90 terminated by 31 (the 29-name list + `MM_TU_bsd`, at 0x3490C6 / 0x2E66EB), u32
# female at +0xD4. Every costume of a character dances the same routines; X's male set is the
# whole MM_* family minus MM_BR_01 / 02 (RAGE and ZERO dance those too), the female set the whole
# FF_* family, PIX a mixed six (female by the flag). stem -> (label, sex, shadow scale, routines
# minus the NE idle, face root)
X_MALE = ['MM_BR_03', 'MM_HH_01', 'MM_HH_02', 'MM_HT_01', 'MM_HT_02', 'MM_HT_03', 'MM_HT_04', 'MM_JA_01', 'MM_JA_02',
          'MM_SF_01', 'MM_SF_02', 'MM_SF_03']
X_FEMALE = ['FF_BR_01', 'FF_BR_02', 'FF_HH_01', 'FF_HH_02', 'FF_HH_03', 'FF_HT_01', 'FF_HT_02', 'FF_HT_03', 'FF_JA_01',
            'FF_JA_02', 'FF_SF_01', 'FF_SF_02', 'FF_SF_03']
X_PIX = ['MM_HT_03', 'MM_HT_04', 'FF_HH_02', 'FF_HT_03', 'FF_SF_01', 'FF_SF_02']
X_CHARACTERS = {
    'afro': ('Afro', 'M', 0.75, X_MALE, 'face01'),
    'emi': ('Emi', 'F', 0.6, X_FEMALE, 'face01'),
    'babylon': ('Baby-Lon', 'M', 0.35, X_MALE, 'face01'),
    'zukin': ('Robo-Zukin', 'F', 0.8, X_FEMALE, 'face01'),
    'rage': ('Rage', 'M', 0.7, ['MM_BR_01', 'MM_BR_02'] + X_MALE, 'face01'),
    'jenny': ('Jenny', 'F', 0.6, X_FEMALE, 'face01'),
    'gus': ('Gus', 'M', 0.75, X_MALE, 'face01'),
    'ruby': ('Ruby', 'F', 0.6, X_FEMALE, 'face01'),
    'yuni': ('Yuni', 'F', 0.6, X_FEMALE, 'face01'),
    'alice': ('Alice', 'F', 0.6, X_FEMALE, 'face01'),
    'concent': ('Concent', 'M', 0.85, X_MALE, 'face01'),
    'julio': ('Julio', 'M', 0.45, X_MALE, 'face01'),
    'bonnie': ('Bonnie', 'F', 0.65, X_FEMALE, 'face01'),
    'zero': ('Zero', 'M', 0.7, ['MM_BR_01', 'MM_BR_02'] + X_MALE, 'face01'),
    'pix01': ('Pix 1', 'F', 0.35, X_PIX, 'jx_pix01_face1'),
    'pix02': ('Pix 2', 'F', 0.35, X_PIX, 'jx_pix02_face01'),
    'pix03': ('Pix 3', 'F', 0.35, X_PIX, 'jx_pix03_face1'),
    'pix04': ('Pix 4', 'F', 0.35, X_PIX, 'jx_pix04_face1'),
}
# CONCENT's chest fan: X moved it out of the face pack into its own part pack hung off Spine1
# (the table's `body01` / `Spine1` slot); joined as a static part. (pack, joint, root or None)
X_EXTRAS = {'concent': [('parts/convent01_body01.tzm', 'Spine1', None)]}
# Which costumes each game adds (skin -> label; the rest of its table is SuperNova 2's content:
# X's 02 / 03 = SuperNova 2's 01 / 02, X2's 01 = X's 01 and its 03 = SuperNova 2's 02) and which
# face pack a costume uses when it is not its own (X2's bonnie02 / zero02 reuse the 01 masks).
X_NEW = {'%s01' % s: '%s 1' % X_CHARACTERS[s][0] for s in X_CHARACTERS if not s.startswith('pix')}
X_NEW['babylon02'] = 'Baby-Lon 2'
X2_NEW = {'%s02' % s: '%s 2' % X_CHARACTERS[s][0] for s in X_CHARACTERS if not s.startswith('pix')}
X2_NEW['babylon02'] = 'Baby-Lon 3'   # X already has two
X2_NEW.update({p: X_CHARACTERS[p][0] for p in ('pix01', 'pix02', 'pix03', 'pix04')})
X2_FACE_PACKS = {'bonnie02': 'bonnie01_face', 'zero02': 'zero01_face'}


def x_table(prefix, new, face_packs, costumes):
    """The CHARACTERS dict of one X-era game: every costume of every character (`DANCERS=afro03`
    works), labelled from `new` when the game adds it, `<Name> <n>` otherwise."""
    out = {}
    for stem, (label, sex, shadow, routines, face_root) in X_CHARACTERS.items():
        skins = [stem] if stem.startswith('pix') else ['%s%02d' % (stem, c) for c in costumes]
        for skin in skins:
            out[skin] = dict(label=new.get(skin, '%s %s' % (label, skin[-1])), key=prefix + skin, sex=sex,
                             shadow=shadow, routines=routines, face_root=face_root,
                             face_pack=face_packs.get(skin, skin + '_face'), extras=X_EXTRAS.get(stem, []))
    return out


GAMES = {
    'sn': dict(title='DDR SuperNova (PS2)', faces=False,
               dir='~/Desktop/PS2 DDR ISOs/Dance Dance Revolution SuperNova (Japan)/extracted_full',
               out='~/Desktop/SuperNova Dancers', characters=SN_CHARACTERS, default=list(SN_CHARACTERS)),
    'sn2': dict(title='DDR SuperNova 2 (PS2)', faces=True,
                dir='~/Desktop/PS2 DDR ISOs/Dance Dance Revolution SuperNova 2 (Japan)/extracted_full',
                out='~/Desktop/SuperNova 2 Dancers',
                characters={'%s%02d' % (stem, c): ('%s %d' % (label, c), 'sn2%s%02d' % (stem, c), routines)
                            for stem, label, routines in SN2_CHARACTERS for c in (1, 2)},
                default=['%s%02d' % (stem, c) for stem, _l, _r in SN2_CHARACTERS for c in (1, 2)
                         if not (c == 1 and stem in SN2_RETURNING)]),
    'x': dict(title='DDR X (PS2)', faces=True,
              dir='~/Desktop/PS2 DDR ISOs/Dance Dance Revolution X (Japan)/extracted_full',
              out='~/Desktop/DDR X Dancers',
              characters={k: v for k, v in x_table('x', X_NEW, {}, (1, 2, 3)).items() if not k.startswith('pix')},
              default=sorted(X_NEW)),
    'x2': dict(title='DDR X2 (PS2)', faces=True,
               dir='~/Desktop/PS2 DDR ISOs/Dance Dance Revolution X2 (USA)/extracted_full',
               out='~/Desktop/DDR X2 Dancers',
               characters=x_table('x2', X2_NEW, X2_FACE_PACKS, (1, 2, 3)),
               default=sorted(X2_NEW)),
}
GAME = os.environ.get('GAME', 'sn')
if GAME not in GAMES:
    sys.exit('GAME must be one of %s' % ', '.join(GAMES))
CFG = GAMES[GAME]
CHARACTERS = CFG['characters']
SN_DIR = os.path.expanduser(os.environ.get('SN_DIR', CFG['dir']))
MODEL_DIR = os.path.join(SN_DIR, 'files', 'IMAGE', 'model')
OUT_BASE = os.path.expanduser(os.environ.get('OUT_BASE', CFG['out']))
PREVIEW = os.environ.get('PREVIEW', '0') == '1'
PREVIEW_DIR = os.environ.get('PREVIEW_DIR') or os.path.join(tempfile.gettempdir(), 'supernova_port_previews')
# SuperNova 2: which expression sheet of `<skin>_face.TZM` becomes the (static) face -- face01 is
# the neutral open-eyed one, face02 the smile, face03 eyes shut; '' = no mask (a blank face)
FACE_EXPRESSION = os.environ.get('FACE', 'face01')
# SuperNova 2: CONCENT's chest fan as its own spinning joint (`SPIN=0` joins it as a static part)
SPIN = os.environ.get('SPIN', '1') == '1'
# World role bone -> the SuperNova joint playing it (`Spine2` and `Head` are named alike).
ROLE_ALIASES = {'Hips': 'Hip', 'LeftToeBase': 'LeftToes', 'RightToeBase': 'RightToes'}


def sex_of(routines):
    """F / M from the routine family (`FF_*` female, `MM_*` male -- the ELF flag says the same)."""
    fams = {r[:2] for r in routines}
    assert len(fams) == 1, routines
    return 'F' if fams == {'FF'} else 'M'


def entry_of(skin):
    """One CHARACTERS value as a dict: the SuperNova / SuperNova 2 tables are (label, key, routines)
    tuples -- sex from the routine family, shadow 0.75 F / 0.8 M, face root `face01` in
    `<skin>_face.TZM`, CONCENT's fan from its face pack (SuperNova 2); the X tables carry every
    field."""
    e = CHARACTERS[skin]
    if isinstance(e, dict):
        return e
    label, key, routines = e
    sex = sex_of(routines)
    # CONCENT's chest fan lives in its face pack (`body01 > body_trans_null > fan01`, hung off Spine1)
    # together with the `ddr_concent_fan` loop that spins it: ported as its own joint (see SPIN)
    extras = [('skin/%s_face.TZM' % skin, 'Spine1', 'body01', 'ddr_concent_fan')] if skin.startswith('concent') else []
    return dict(label=label, key=key, sex=sex, shadow=0.75 if sex == 'F' else 0.8, routines=routines,
                face_root=FACE_EXPRESSION, face_pack=skin + '_face', extras=extras)


def load_skin(skin, entry):
    """(model, colour sheet, [(overlay, joint, slot)], [spin]) -- an overlay is tzm_dump.part_overlay's
    tuple in the body's game space: the `<skin>_face.TZM` expression mask (SuperNova 2 / X; FACE
    picks the expression on the sn2 packs, the table names the X roots) and the game's extra part
    packs (CONCENT's fan). An extra with a 4th field names the pack's MOTION record that animates
    the part: the part then gets ITS OWN JOINT under `joint` (tzm_dump.attach_part_bone -- the
    returned model carries it), the overlay weights to that joint and `spins` holds
    `(part record, object, bone, chain rotation)` for tzm_dump.part_spin_track to lay the loop
    onto every dance clip. The colour sheet is the one the body meshes' material names (gus02's
    pack carries a stray `gus_face02_png` first), else the largest texture of the pack."""
    chunks = Z.load_tzm(os.path.join(MODEL_DIR, 'chara', 'skin', skin + '.TZM'))
    d = dict(chunks)
    model = Z.parse_model(d['MODEL'])
    textures = Z.textures_of(chunks)
    materials = Z.parse_materiallist(d['MATERIALLIST']) if 'MATERIALLIST' in d else {}
    tex = None
    for m in model['meshes']:
        mat = Z.material_for(materials, m['material']) or {}
        tex = next((textures[t] for t in mat.get('textures', []) if t in textures), None)
        if tex is not None:
            break
    if tex is None:
        tex = max(textures.values(), key=lambda t: t['width'] * t['height'])
    overlays = []
    face_path = os.path.join(MODEL_DIR, 'chara', 'skin', entry['face_pack'] + '.TZM')
    if CFG['faces'] and FACE_EXPRESSION and os.path.exists(face_path):
        face = Z.face_overlay(model, Z.load_tzm(face_path), entry['face_root'])
        if face is not None:
            overlays.append((face, Z.FACE_BONE, 'face'))
    spins = []
    for extra in entry['extras']:
        pack, joint, root = extra[:3]
        spin_record = extra[3] if len(extra) > 3 and SPIN else None
        part_chunks = Z.load_tzm(os.path.join(MODEL_DIR, 'chara', pack))
        weight_to = joint
        if spin_record:
            part_rec = next((r for r in Z.parse_motion(dict(part_chunks)['MOTION']) if r['name'] == spin_record), None) \
                if 'MOTION' in dict(part_chunks) else None
            if part_rec is not None:
                model, bone, obj, above = Z.attach_part_bone(model, part_chunks, root, joint)
                if bone is not None:
                    weight_to = bone
                    spins.append((part_rec, obj, bone, above))
        part = Z.part_overlay(model, part_chunks, joint, root)
        if part is not None:
            overlays.append((part, weight_to, root or os.path.splitext(os.path.basename(pack))[0].split('_')[-1]))
    return model, tex, overlays, spins


def load_clip(name):
    return Z.parse_motion(dict(Z.load_tzm(os.path.join(MODEL_DIR, 'chara', 'motion', name + '.TZM')))['MOTION'])[0]


def build_armature(key, rig, binds):
    arm_data = bpy.data.armatures.new(key + '_rig')
    arm = bpy.data.objects.new(key + '_Armature', arm_data)
    bpy.context.scene.collection.objects.link(arm)
    bpy.context.view_layer.objects.active = arm
    bpy.ops.object.mode_set(mode='EDIT')
    ebs = {}
    for n, _p in rig:
        eb = arm_data.edit_bones.new(n)
        eb.head = (0.0, 0.0, 0.0)
        eb.tail = (0.0, 0.04, 0.0)
        eb.matrix = convert.rowmat_to_blender([float(x) for x in np.asarray(binds[n]).reshape(16)])
        ebs[n] = eb
    for n, p in rig:
        if p is not None:
            ebs[n].parent = ebs[p]
    bpy.ops.object.mode_set(mode='OBJECT')
    arm['ddr_bone_order'] = [n for n, _ in rig]
    arm['ddr_chara_key'] = key
    return arm


def texture_png(key, tex):
    out = os.path.join(tempfile.gettempdir(), 'supernova_port_textures', key + '.png')
    os.makedirs(os.path.dirname(out), exist_ok=True)
    Z.P.write_png(out, tex['width'], tex['height'], tex['rgba'].tobytes())
    return out


def build_mesh(key, model, tex, arm, overlays=()):
    """ONE skinned mesh: every body mesh (slot 0, translucent meshes in an alpha-blended slot) plus
    one material slot per overlay `((pos, nrm, uv, tris, tex), joint, slot)` (tzm_dump.part_overlay:
    the expression mask on its own 128² sheet, CONCENT's fan), each rigidly weighted to its joint.
    Returns (object, translucent source meshes, overlay vertex count)."""
    pos, nrm, uv, col, weights, tris, src_mesh = Z.game_mesh(model)
    n_over = 0
    weights = list(weights)
    for i, ((opos, onrm, ouv, otris, _otex), joint, _slot) in enumerate(overlays):
        n_over += len(opos)
        otris = otris + len(pos)
        pos, nrm, uv = np.concatenate([pos, opos]), np.concatenate([nrm, onrm]), np.concatenate([uv, ouv])
        if col is not None:
            col = np.concatenate([col, np.ones((len(opos), 4))])
        weights += [[(joint, 1.0)]] * len(opos)
        tris = np.concatenate([tris, otris])
        src_mesh = np.concatenate([src_mesh, np.full(len(otris), -1 - i)])   # overlay i -> -1 - i
    me = bpy.data.meshes.new(key + '_body')
    me.from_pydata([tuple(convert.vec_to_blender(p)) for p in pos], [], tris.tolist())
    me.update()
    lay = me.uv_layers.new(name='UVMap')
    loops_v = np.zeros(len(me.loops), dtype=np.int64)
    me.loops.foreach_get('vertex_index', loops_v)
    luv = uv[loops_v].copy()
    luv[:, 1] = 1.0 - luv[:, 1]  # v-down -> Blender v-up (the exporter flips back)
    lay.data.foreach_set('uv', luv.astype(np.float32).ravel())
    exact = me.attributes.new('ddr_normal', 'FLOAT_VECTOR', 'POINT')
    exact.data.foreach_set('vector', np.array([tuple(convert.vec_to_blender(n)) for n in nrm]).ravel())
    ob = bpy.data.objects.new(key + '_body', me)
    bpy.context.scene.collection.objects.link(ob)
    ob.parent = arm
    groups = {n: ob.vertex_groups.new(name=n) for n in arm['ddr_bone_order']}
    for vi, ws in enumerate(weights):
        for bone, w in ws:
            if w > 0:
                groups[bone].add([vi], w, 'ADD')
    mod = ob.modifiers.new('Armature', 'ARMATURE')
    mod.object = arm
    colour = P.white_color_attribute(ob)
    image = P.load_texture(key + '_tex', texture_png(key, tex))
    me.materials.append(P.make_material(key + '_body', image, two_sided=False, shader=SHADER))
    mat_idx = np.zeros(len(tris), dtype=np.int32)
    # vertex colours: a mesh with its own (GUS's 60 % alpha glasses) keeps them and, when
    # translucent, moves to an alpha-blended second slot
    translucent = set()
    if col is not None:
        rgba = col[loops_v].astype(np.float32)
        colour.data.foreach_set('color', rgba.ravel())
        translucent = {int(k) for k in np.unique(src_mesh) if k >= 0 and model['meshes'][k]['colours'] is not None
                       and float(model['meshes'][k]['colours'][:, 3].min()) < 0.999}
    if translucent:
        blend = P.make_material(key + '_blend', image, two_sided=False, shader=SHADER)
        blend.surface_render_method = 'BLENDED'
        me.materials.append(blend)
        mat_idx[np.isin(src_mesh, list(translucent))] = len(me.materials) - 1
    for i, ((_opos, _onrm, _ouv, _otris, otex), _joint, slot) in enumerate(overlays):
        o_image = P.load_texture('%s_%s' % (key, slot), texture_png('%s_%s' % (key, slot), otex))
        me.materials.append(P.make_material('%s_%s' % (key, slot), o_image, two_sided=False, shader=SHADER))
        mat_idx[src_mesh == -1 - i] = len(me.materials) - 1
    if len(me.materials) > 1:
        me.polygons.foreach_set('material_index', mat_idx)
        me.update()
    return ob, sorted(translucent), n_over


def add_role_aliases(b2it_path, names):
    entries = K.parse_b2it(open(b2it_path, 'rb').read())
    have = {n for n, _ in entries}
    for role, joint in ROLE_ALIASES.items():
        if role not in have and joint in names:
            entries.append((role, names.index(joint)))
    open(b2it_path, 'wb').write(K.write_b2it(entries))
    return sorted(n for n, _ in entries if n in ROLE_ALIASES)


def exported_rig(body_dir, body):
    m = K.parse_model(open(os.path.join(body_dir, body + '.model'), 'rb').read())
    table = K.parse_b2it(open(os.path.join(body_dir, body + '.b2it'), 'rb').read())
    by_index = {i: n for n, i in table if n not in ROLE_ALIASES}
    names = [by_index[i] for i in range(len(m['bones']))]
    parents = [b['parent'] for b in m['bones']]
    binds = [np.array(b['bind'], dtype=float).reshape(4, 4) for b in m['bones']]
    return m, names, parents, binds


def check_clip(anm_bytes, worlds, parents, step):
    """Max joint-position error (m) of the written .anm against the TZM pose at a few keys."""
    parsed = A.parse_anm(anm_bytes)
    n = worlds.shape[0]
    worst = 0.0
    for k in sorted({0, 1, n // 3, n // 2, n - 2, n - 1}):
        pose = A.evaluate_pose(parsed, step * k, parents)
        for i in range(worlds.shape[1]):
            w = np.array(pose[i]['world'], dtype=float).reshape(4, 4)
            worst = max(worst, float(np.abs(w[3, :3] - worlds[k][i][3, :3]).max()))
    return worst


def port(skin):
    entry = entry_of(skin)
    label, key, sex, routines = entry['label'], entry['key'], entry['sex'], entry['routines']
    assert len(label.encode()) <= 15, label  # the options row's SSO budget (catalog::MAX_LABEL_BYTES)
    P.fresh_scene()
    model, tex, overlays, spins = load_skin(skin, entry)
    rig, _index = Z.rig_bones(model)
    binds = Z.game_bind_matrices(model)
    arm = build_armature(key, rig, binds)
    _ob, translucent, n_over = build_mesh(key, model, tex, arm, overlays)
    bpy.context.view_layer.update()
    print('MODEL %s: %d bones, %d meshes (%d translucent), %d vertices (+ %d in overlays %s), scale %.4f m/unit' % (
        skin, len(rig), len(model['meshes']), len(translucent), sum(m['count'] for m in model['meshes']),
        n_over, ' '.join('%s@%s' % (slot, joint) for _o, joint, slot in overlays) or '-', Z.unit_scale(model)))

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
    assert sorted(file_names) == sorted(n for n, _ in rig), (file_names, rig)
    idents = [b['identity'] for b in m['bones']]
    assert len(set(idents)) == len(idents), 'bone identity collision'
    assert all(len(me['palette']) <= 52 for me in m['meshes'])
    assert len(file_names) <= 64, 'more posed bones than the frame board holds'
    assert K.write_model(K.model_to_spec(m)) == open(os.path.join(body_dir, body + '.model'), 'rb').read()
    aliases = add_role_aliases(os.path.join(body_dir, body + '.b2it'), file_names)
    assert set(aliases) == set(ROLE_ALIASES), aliases
    print('B2IT role aliases', aliases)

    for name in routines:
        rec = load_clip(name)
        for part_rec, obj, bone, above in spins:   # CONCENT's fan: its loop on this clip's timeline
            rec = Z.part_spin_track(rec, part_rec, obj, bone, above)
        spec, worlds = Z.clip_to_anm_spec(model, rec, file_names, parents, file_binds)
        data = A.write_anm(spec)
        step = spec['tracks'][0].get('times', [0, 2])[1] if len(spec['tracks'][0].get('times', [])) > 1 else 2
        err = check_clip(data, worlds, parents, step)
        assert err < 1e-3, '%s: joint error %.5f m' % (name, err)
        open(os.path.join(motion_dir, name + '.anm'), 'wb').write(data)
        print('CLIP %-9s %4d keys @%g Hz -> %4d frames @60, %6d bytes, max joint err %.2e m' % (
            name, worlds.shape[0], rec['fps'], spec['frame_count'], len(data), err))

    sidecar = os.path.join(out_dir, 'chara_resources.rlist.txt')
    with open(sidecar, 'w') as f:
        f.write('# %s "%s" (%s), ported with its own rig and choreography\n' % (CFG['title'], label, skin))
        f.write('# (tools/blender_ddr_addon/examples/port_character_supernova.py GAME=%s; routines %s)\n' % (
            GAME, ' '.join(routines)))
        f.write('%s, pl, %s, A, 1.0, %s, 0.0\n' % (key, sex, export_character.fmt_num(entry['shadow'])))
    print('SIDECAR', sidecar)
    if PREVIEW:
        preview(out_dir, key, routines[0])
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
    import_anm.load_anm(os.path.join(out_dir, body, 'motion', clip + '.anm'), arm)
    for f in (0, 300, 600, 900):
        bpy.context.scene.frame_set(f)
        P.render_camera(os.path.join(PREVIEW_DIR, '%s_%s_f%04d.png' % (key, clip, f)),
                        Vector((0.0, -6.0, 1.0)), Vector((0.0, 0.0, 1.0)), scale=4.0, res=(600, 600))


if __name__ == '__main__':
    want = os.environ.get('DANCERS', CFG['default'][0])
    todo = CFG['default'] if want == 'all' else [s.strip().lower() for s in want.split(',') if s.strip()]
    unknown = [s for s in todo if s not in CHARACTERS]
    if unknown:
        sys.exit('unknown DANCERS %s for GAME=%s (have %s)' % (unknown, GAME, ' '.join(CHARACTERS)))
    for skin in todo:
        port(skin)
    print('DONE')
