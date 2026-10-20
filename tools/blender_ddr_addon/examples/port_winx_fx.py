"""EXAMPLE / PORT: DanceDanceRevolution Winx Club's dancer EFFECTS for Background Dancers -- the
Enchantix transformation effect (`game/EVENT_EFFECT.bin` = chenge_effect.TEB: ribbon + sparkles +
glow, main.dol FUN_800a6d3c spawns effect 0 at the wipe line) and the wing-tip sparkle trail
(`game/CHARA_EFFECT.bin` = chara_effects.TEB) -- written into every ported Fairy (Enchantix) dancer's
body folder (`data_mods/custom_models/dancers/WINX CLUB/<X Fairy>/pl_<key>/`). RE:
docs/wii_ddr_winx_club_research.md §3 / §7.4; the runtime is the flight effects' CzanEff port
(src/mods/background_dancers/flight_fx.rs, `chara_fx.rs`), the pool models are port_flight_fx.py's.

Per Fairy dancer `pl_<key>/`:
  winx_fx/chenge.teb + chenge.txt   the transformation bank verbatim + its pool layout (player 0)
  winx_fx/wing.teb + wing.txt       the wing sparkle bank + its pool layout (player 0)
  fx_<key>_c<k>a.., fx_<key>_c<k>r.. the transformation's sprite / ribbon POOL models
  fx_<key>_w<k>a..                  the wing sparkles' sprite pool
  <key>cfxNN.dds / <key>wfxNN.dds   the TPL images the pools sample
Manifest grammar: port_flight_fx.py's (`flight_fx 1`, `sprites` / `group` / `ribbons` / `strip`).

The pools are sized at the effects' simulated PEAK (teb_dump.simulate over a moving attach, x the
instances that share them, + a margin, capped at the TEB's `max`) rather than at `max` alone:
chara_effects declares 300 particles but one emitter never holds more than ~33 alive, and the
frame board's slots are shared with the stage.

The wing sparkles' binding is an ASSUMPTION: main.dol loads CHARA_EFFECT.bin (resource manager
+0x3c) but never registers or plays it (research §7.4); the port attaches its one effect at both
`DB_*wing_02` tips of a Fairy dancer, continuously.

Run (plain python3 + numpy; no Blender), after port_character_hottest2.py GAME=winx:
    python3 tools/blender_ddr_addon/examples/port_winx_fx.py
Inputs (environment): WINX_GAME (the dumped disc, default ~/Desktop/ddr_wii/Winx Club (Europe)),
OUT_BASE (default data_mods/custom_models/dancers/WINX CLUB).
"""
import glob
import math
import os
import sys

import numpy as np

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.normpath(os.path.join(HERE, '..', '..', '..'))
sys.path.insert(0, HERE)
sys.path.insert(0, os.path.join(REPO, 'scripts'))
import ktmdl_dump as K  # noqa: E402
import teb_dump as T  # noqa: E402
import zan_dump as Z  # noqa: E402
import port_flight_fx as F  # noqa: E402

DISC = os.path.expanduser(os.environ.get('WINX_GAME', '~/Desktop/ddr_wii/Winx Club (Europe)'))
OUT_BASE = os.path.expanduser(os.environ.get('OUT_BASE', os.path.join(REPO, 'data_mods', 'custom_models', 'dancers', 'WINX CLUB')))
FX_DIR = 'winx_fx'
# (layout stem, bank file, effect, instances, pool tag, texture tag, simulated frames)
BANKS = [('chenge', 'EVENT_EFFECT.bin', 0, 1, 'c', 'cfx', 180),
         ('wing', 'CHARA_EFFECT.bin', 0, 2, 'w', 'wfx', 360)]
MARGIN = 2


def peak_counts(fx, effect, frames):
    """{tex: peak sprites alive}, peak ribbons (>= 2 points) of one effect instance over a moving
    attach (the dancer's own motion is of this order)."""
    peak, rib = {}, 0

    def attach(i):
        m = np.eye(4)
        m[0, 3] = 4.0 * math.sin(i * 0.07)
        m[1, 3] = 2.0 * math.sin(i * 0.05)
        return m
    for sprites in T.simulate(fx['effects'][effect], attach, frames):
        c, r = {}, 0
        for s in sprites:
            c[s['tex']] = c.get(s['tex'], 0) + 1
            if s.get('ribbon') and len(s['ribbon']['points']) >= 2:
                r += 1
        for k, v in c.items():
            peak[k] = max(peak.get(k, 0), v)
        rib = max(rib, r)
    return peak, rib


def sized_pools(fx, images, effect, instances, frames):
    """port_flight_fx.pools_of, cut down to the simulated peak (+ MARGIN per instance)."""
    groups, strips = F.pools_of(fx, images, [(effect, instances)])
    peak, rib = peak_counts(fx, effect, frames)
    out = {}
    for key, cap in groups.items():
        want = (peak.get(key[0], 0) + MARGIN) * instances
        out[key] = max(1, min(cap, want))
    n = min(len(strips), (rib + MARGIN) * instances) if strips else 0
    return out, strips[:n]


def port_fairy(body_dir, key):
    fx_dir = os.path.join(body_dir, FX_DIR)
    os.makedirs(fx_dir, exist_ok=True)
    for old in glob.glob(os.path.join(body_dir, 'fx_%s_*' % key)):
        for f in glob.glob(os.path.join(old, '*')):
            os.remove(f)
        os.rmdir(old)
    for f in glob.glob(os.path.join(fx_dir, '*')):
        os.remove(f)
    for stem, bank, effect, inst, ptag, ttag, frames in BANKS:
        (_p, teb, tpl), = T.teb_members(open(os.path.join(DISC, 'game', bank), 'rb').read())
        fx = T.parse_teb(teb)
        images = Z.tpl_images(tpl)

        def tex_stem(i, ttag=ttag):
            return '%s%s%02d' % (key, ttag, i)
        for i in range(len(images)):
            K.texture_registry_key(tex_stem(i))
        with open(os.path.join(fx_dir, stem + '.teb'), 'wb') as f:
            f.write(teb)
        groups, strips = sized_pools(fx, images, effect, inst, frames)
        out = ['# DanceDanceRevolution Winx Club %s effect %d for %s (%s x%d instance(s))' % (stem, effect, key, bank, inst),
               '# generated by tools/blender_ddr_addon/examples/port_winx_fx.py -- do not edit; re-run the script',
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
        for k, load in enumerate(F.chunk_groups(groups)):
            sm, quads, glines = F.sprite_model('fx_%s_%s%sa' % (key, ptag, chr(ord('a') + k)), load, tex_stem)
            put_textures(sm.write(body_dir, quads), sm)
            out.append('sprites 0 %s %d' % (sm.name, quads))
            out += glines
            print('  %s %s: %s %d quads in %d groups' % (key, stem, sm.name, quads, len(glines)))
        for k, load in enumerate(F.chunk_strips(strips)):
            rm, rbones, rrecs, slines = F.ribbon_model('fx_%s_%s%sr' % (key, ptag, chr(ord('a') + k)), load, tex_stem)
            assert rbones <= F.MAX_BONES and rrecs <= F.MAX_RECORDS, (key, rbones, rrecs)
            put_textures(rm.write(body_dir, rbones), rm)
            out.append('ribbons 0 %s %d %d' % (rm.name, rbones, rrecs))
            out += slines
            print('  %s %s: %s %d strips (%d bones, %d meshes)' % (key, stem, rm.name, len(slines), rbones, rrecs))
        with open(os.path.join(fx_dir, stem + '.txt'), 'w') as f:
            f.write('\n'.join(out) + '\n')


def main():
    found = sorted(glob.glob(os.path.join(OUT_BASE, '*', 'pl_*', 'pl_*.model')))
    done = 0
    for model in found:
        body_dir = os.path.dirname(model)
        key = os.path.basename(body_dir)[3:]
        if not os.path.basename(model) == 'pl_%s.model' % key:
            continue
        b2it = os.path.join(body_dir, 'pl_%s.b2it' % key)
        names = {n for n, _i in K.parse_b2it(open(b2it, 'rb').read())}
        if 'DB_Lwing_02' not in names:      # a Fairy (Enchantix) body: it has the wings
            continue
        print('%s (%s)' % (key, os.path.basename(os.path.dirname(body_dir))))
        port_fairy(body_dir, key)
        done += 1
    if not done:
        sys.exit('no Winx Club Fairy dancer under %s (run port_character_hottest2.py GAME=winx first)' % Z._tilde(OUT_BASE))
    print('DONE (%d Fairy dancers)' % done)


if __name__ == '__main__':
    main()
