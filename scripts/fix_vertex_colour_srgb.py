#!/usr/bin/env python3
"""Undo the sRGB re-encoding of COLOR0 in already-ported .model files, in place.

Until 2026-10-04 the stage ports (HOTTEST PARTY 1 `port_stage_hottest.py`, SUPERNOVA / X
`port_stage_supernova.py`) and the SuperNova / X dancer port (`port_character_supernova.py`) wrote
vertex colours through Blender's LINEAR `color` accessor. Blender stores a BYTE_COLOR attribute
sRGB-encoded and the add-on exporter writes those stored bytes verbatim (`color_srgb`), so every
shipped COLOR0 byte is round(255 * srgb_encode(v)) instead of round(255 * v): too bright and
washed out (0.5 -> 188, 0.2 -> 124). The game multiplies COLOR0 bytes directly (D3D9
`mdl_*_vc`), as the source consoles did. Alpha is stored linearly and is left alone.

STATUS (2026-10-04): NOTHING SHIPPED NEEDS IT. Every affected source was re-ported from its disc with
the fixed scripts, so every shipped COLOR0 byte is exact and this tool would darken it. It stays for
models someone ported with an older checkout of those scripts, and refuses to write unless
--legacy-port says so.

This tool maps every COLOR0 RGB byte b -> round(255 * srgb_decode(b / 255)). 0 and 255 are fixed
points; the error against the true source is <= 1 (measured on HOTTEST PARTY 2's stages, which
were re-ported from the disc). Use it ONLY for content whose source is not at hand: a fixed port
writes exact bytes, and patching those darkens them a second time.

Idempotency: each ROOT gets a `colour0_srgb_fixed.json` manifest {model path relative to ROOT:
sha256 of the patched file}. A model whose bytes match its manifest entry is skipped; one listed
with different bytes (re-ported since?) is refused unless --force. The manifest sits in ROOT (a
source folder such as `stages/SUPERNOVA 1 & 2/`), outside every model folder, so it never ends up
in a packed arc. No format field is touched to mark a file.

Every model must round-trip byte-exactly through scripts/ktmdl_dump.py (parse -> spec -> write)
before it is touched; anything else is reported and skipped.

Usage:
    fix_vertex_colour_srgb.py --dry-run ROOT...
    fix_vertex_colour_srgb.py --legacy-port [--force] ROOT...   # writes
Tests: scripts/test_fix_vertex_colour_srgb.py (run by scripts/validate_background_dancers.sh).
"""
import argparse
import hashlib
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import ktmdl_dump as K  # noqa: E402

MANIFEST = 'colour0_srgb_fixed.json'


def srgb_decode(x):
    return x / 12.92 if x <= 0.04045 else ((x + 0.055) / 1.055) ** 2.4


def srgb_encode(v):
    return 12.92 * v if v <= 0.0031308 else 1.055 * v ** (1 / 2.4) - 0.055


# byte -> byte: the inverse of the ports' accidental round(255 * srgb_encode(v))
DECODE_LUT = bytes(int(round(255 * srgb_decode(b / 255.0))) for b in range(256))


def colour_elements(model):
    """[(mesh index, vertex buffer, element)] for every D3DCOLOR COLOR0 element."""
    out = []
    for me in model['meshes']:
        for vb in me['vertex_buffers']:
            for e in vb['elements']:
                if e['usage'] == 'COLOR0' and e['type'] == 'D3DCOLOR':
                    out.append((me['index'], vb, e))
    return out


def fix_model_bytes(data):
    """(patched bytes, stats) with stats = dict(vertices, changed, delta_sum, channels). Raises
    ValueError when the file does not round-trip through the ktmdl writer."""
    model = K.parse_model(data)
    if K.write_model(K.model_to_spec(model)) != data:
        raise ValueError('does not round-trip byte-exactly through ktmdl_dump')
    out = bytearray(data)
    stats = dict(vertices=0, changed=0, delta_sum=0, channels=0)
    for _mi, vb, e in colour_elements(model):
        for v in range(vb['count']):
            o = vb['data_offset'] + v * vb['stride'] + e['offset']
            stats['vertices'] += 1
            changed = False
            for k in range(3):                      # D3DCOLOR is B, G, R, A in memory
                b = out[o + k]
                nb = DECODE_LUT[b]
                if nb != b:
                    out[o + k] = nb
                    stats['delta_sum'] += b - nb
                    stats['channels'] += 1
                    changed = True
            stats['changed'] += changed
    out = bytes(out)
    K.parse_model(out)                              # still a well-formed model
    return out, stats


def sha256(b):
    return hashlib.sha256(b).hexdigest()


def group_of(rel):
    """The friendly folder (`Stage 01`, `Gus 1`) a model path belongs to, for the report."""
    return rel.split('/', 1)[0] if '/' in rel else '.'


def fix_root(root, dry_run=False, force=False, log=print):
    """Patch every .model under root. Returns dict(models, patched, skipped, refused, failed,
    vertices, changed, delta_sum, channels)."""
    mpath = os.path.join(root, MANIFEST)
    manifest = {}
    if os.path.exists(mpath):
        with open(mpath) as f:
            manifest = json.load(f)
    totals = dict(models=0, patched=0, skipped=0, refused=0, failed=0,
                  vertices=0, changed=0, delta_sum=0, channels=0)
    groups = {}
    files = []
    for d, _dirs, names in os.walk(root):
        for n in names:
            if n.endswith('.model'):
                files.append(os.path.relpath(os.path.join(d, n), root).replace(os.sep, '/'))
    for rel in sorted(files):
        path = os.path.join(root, rel)
        with open(path, 'rb') as f:
            data = f.read()
        totals['models'] += 1
        if rel in manifest:
            if manifest[rel] == sha256(data):
                totals['skipped'] += 1
                continue
            if not force:
                log('REFUSED %s: patched before but changed since (re-ported? --force to patch anyway)' % rel)
                totals['refused'] += 1
                continue
        try:
            new, st = fix_model_bytes(data)
        except ValueError as e:
            log('FAILED %s: %s' % (rel, e))
            totals['failed'] += 1
            continue
        for k in ('vertices', 'changed', 'delta_sum', 'channels'):
            totals[k] += st[k]
        g = groups.setdefault(group_of(rel), dict(models=0, vertices=0, changed=0, delta_sum=0, channels=0))
        g['models'] += 1
        for k in ('vertices', 'changed', 'delta_sum', 'channels'):
            g[k] += st[k]
        totals['patched'] += 1
        if not dry_run:
            if new != data:
                with open(path, 'wb') as f:
                    f.write(new)
            manifest[rel] = sha256(new)
    for name, g in sorted(groups.items()):
        log('  %-24s %2d models  %7d / %7d COLOR0 vertices change  mean byte delta %.1f' % (
            name, g['models'], g['changed'], g['vertices'], g['delta_sum'] / max(1, g['channels'])))
    if not dry_run and totals['patched']:
        with open(mpath, 'w') as f:
            json.dump(manifest, f, indent=1, sort_keys=True)
            f.write('\n')
    return totals


def main(argv=None):
    ap = argparse.ArgumentParser(description='Undo the sRGB re-encoding of COLOR0 in ported .model files.')
    ap.add_argument('roots', nargs='+', help='source folders, e.g. data_mods/custom_models/stages/X\\ \\&\\ X2')
    ap.add_argument('--dry-run', action='store_true', help='report only, write nothing')
    ap.add_argument('--force', action='store_true', help='patch models changed since a recorded patch')
    ap.add_argument('--legacy-port', action='store_true',
                    help='required to write: ROOT was ported by a pre-2026-10-04 script (`color` accessor)')
    a = ap.parse_args(argv)
    if not a.dry_run and not a.legacy_port:
        print('refusing to write: every shipped model is already exact (see the docstring); pass --legacy-port '
              'only for a model ported with a pre-2026-10-04 script, or --dry-run to measure')
        return 2
    bad = 0
    for root in a.roots:
        print('%s%s' % ('[dry run] ' if a.dry_run else '', root))
        t = fix_root(root, a.dry_run, a.force)
        print('  total: %d models, %d patched, %d already fixed, %d refused, %d failed; %d / %d COLOR0 vertices '
              'change, mean byte delta %.1f' % (t['models'], t['patched'], t['skipped'], t['refused'], t['failed'],
                                                t['changed'], t['vertices'], t['delta_sum'] / max(1, t['channels'])))
        bad += t['refused'] + t['failed']
    return 1 if bad else 0


if __name__ == '__main__':
    sys.exit(main())
