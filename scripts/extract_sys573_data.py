#!/usr/bin/env python3
"""Extract the flash filesystem of a Konami System 573 DDR / Dancing Stage mix.

A System 573 mix installs two images to the cabinet: GAME.DAT (the 16 MiB on-board flash)
and CARD.DAT (the 32 MiB PCMCIA card). SuperDisc builds ship them per mix as
`IN/<id>G` + `IN/<id>C` (`superdisc` below lists them from the installer's own table).
Formats and RE: docs/sys573_dancers_research.md section 1.

File table (DDR mixes): at GAME.DAT + 0xFE4000, 16-byte entries
    { u32 name_hash, u16 offset/0x800, u16 location (0 GAME.DAT, 1 CARD.DAT),
      u8 compressed, u8 encrypted, u16 ?, u32 size },
terminated by name_hash 0xFFFFFFFF + offset 0xFFFF. Size-0 entries are skipped.
Stored sizes are the on-disk (compressed) sizes.

Names are not stored, only a 32-bit LFSR hash of the path (`name_hash`). This tool recovers
them by hashing candidates mined from the images themselves, in rounds:
  * literal paths in any decoded file (the game executable, config.dat `conversion` lines);
  * directory prefixes (strings ending in '/') x identifiers found in the executable
    x the usual layouts (`<dir><id>/<id>.<ext>`, `<dir><id>/<id>_<part>.<ext>`, ...);
  * song ids from mdb.bin / *_mdb.bin (every lowercase 3-6 char token), and the
    `texbind.bin` / `rembind.bin` texture lists (0x30-byte rows, name at +0x10).
Anything still unnamed is written as `_unnamed/<hash>.bin`. `--names` adds candidates (a text
list, or another mix's `_manifest.json` — later mixes keep files of songs they no longer list).

Encryption is a single-byte-keyed stream (`key * 0x41C64E6D + i * 0x3039 >> 5`); the key byte
is found per file by brute force (256 tries, scored on a clean LZ decode) — no key table.
Compression is Konami's LZ variant (decode_lz).

Usage:
    extract_sys573_data.py extract --game GAME.DAT [--card CARD.DAT] --out DIR [--names F]...
    extract_sys573_data.py superdisc <disc dir>          # list mixes -> IN/ image pairs
    extract_sys573_data.py hash <path>...                # print name hashes

Writes DIR/_manifest.json (one row per table entry). Import-safe:
`from extract_sys573_data import name_hash, decode_lz, read_file_table`.
"""
import argparse
import json
import os
import re
import struct
import sys

TABLE_OFFSET = 0xFE4000
POLY = 0x04C11DB7


def _hash_table():
    table = []
    for top in range(64):
        h = top << 26
        for _ in range(6):
            h = ((POLY if h & 0x80000000 else 0) ^ (h << 1)) & 0xFFFFFFFF
        table.append(h)
    return table


_T = _hash_table()
_INS = [sum(((c >> i) & 1) << (5 - i) for i in range(6)) for c in range(64)]


def hash_continue(h, s):
    """Advance the name-hash state `h` over string `s` (6 bits per character, LSB first)."""
    for ch in s:
        h = _T[h >> 26] ^ ((h << 6) & 0xFFFFFFFF) ^ _INS[ord(ch) & 63]
    return h


def name_hash(path):
    return hash_continue(0, path)


def decode_lz(src):
    """Konami 573 LZ: 8-flag control bytes; literal / short (2-byte) / near (1-byte) copies,
    0xC0..0xFE literal runs, 0xFF end. Raises ValueError on a malformed stream."""
    out = bytearray()
    i = 0
    control = 0
    n = len(src)
    try:
        while True:
            control >>= 1
            if not control & 0x100:
                control = src[i] | 0xFF00
                i += 1
            b = src[i]
            i += 1
            if not control & 1:
                out.append(b)
                continue
            if not b & 0x80:
                dist = ((b & 3) << 8) | src[i]
                i += 1
                length = (b >> 2) + 3
            elif not b & 0x40:
                dist = (b & 0x0F) + 1
                length = (b >> 4) - 6
            elif b == 0xFF:
                return bytes(out), i
            else:
                length = b - 0xB8
                out += src[i:i + length]
                if i + length > n:
                    raise ValueError('literal run past end')
                i += length
                continue
            start = len(out) - dist
            if start < 0:
                raise ValueError('copy before start')
            for k in range(length):
                out.append(out[start + k])
    except IndexError:
        raise ValueError('truncated stream') from None


def decrypt(data, key_byte):
    key1 = (0x41C64E6D * key_byte) & 0xFFFFFFFF
    return bytes((b ^ ((key1 + i * 0x3039) >> 5)) & 0xFF for i, b in enumerate(data))


def config_decrypt(data):
    """boot/config.dat: XOR with (crc32_msb("/s573/config.dat") >> 8) & 0xFF."""
    crc = 0xFFFFFFFF
    for c in b'/s573/config.dat':
        crc ^= c << 24
        for _ in range(8):
            crc = ((crc << 1) ^ POLY if crc & 0x80000000 else crc << 1) & 0xFFFFFFFF
    k = (crc >> 8) & 0xFF
    return bytes(b ^ k for b in data)


def read_file_table(game):
    files = []
    o = TABLE_OFFSET
    while o + 16 <= len(game):
        h, off, loc, comp, enc, unk, size = struct.unpack_from('<IHHBBHI', game, o)
        o += 16
        if h == 0xFFFFFFFF and off == 0xFFFF:
            break
        if size:
            files.append(dict(index=len(files), hash=h, offset=off * 0x800, location=loc,
                              compressed=comp, encrypted=enc, unk=unk, size=size))
    return files


def load_entry(entry, game, card):
    img = card if entry['location'] == 1 else game
    if img is None:
        return None, 'no card image'
    raw = img[entry['offset']:entry['offset'] + entry['size']]
    if entry['encrypted']:
        best = None
        for k in range(256):
            dec = decrypt(raw, k)
            if entry['compressed']:
                try:
                    out, used = decode_lz(dec)
                except ValueError:
                    continue
                score = (used >= len(raw) - 3, len(out))
            else:
                score = (True, sum(32 <= c < 127 for c in dec))
                out = dec
            if best is None or score > best[0]:
                best = (score, out, k)
        if best is None:
            return raw, 'undecryptable'
        return best[1], 'key 0x%02x' % best[2]
    if entry['compressed']:
        try:
            return decode_lz(raw)[0], ''
        except ValueError as e:
            return raw, 'lz: %s' % e
    return raw, ''


# ---------------------------------------------------------------------------
# name recovery
# ---------------------------------------------------------------------------
EXTS = ('bin exe dat cmt tim cms lmp per csq ssq cmm cmd pos ctx lst tmd vab sbs can anm mbk '
        'txt tex str vas olb lz bs').split()
PARTS = 'bk nm th ta in cd fr 25 16 all'.split()
PATH_RE = re.compile(rb'[A-Za-z0-9_][A-Za-z0-9_./-]{2,62}\.[A-Za-z0-9]{1,4}(?=\0|\s|$)')
STEM_RE = re.compile(rb'(?<![A-Za-z0-9_./])(?:[a-z0-9_]+/){1,4}[a-z]{1,8}(?=\0)')
DIR_RE = re.compile(rb'(?<![A-Za-z0-9_./])(?:[a-z0-9_]+/){1,4}(?=\0)')
TOKEN_RE = re.compile(rb'(?<![A-Za-z0-9_])[a-z][a-z0-9_]{1,11}(?=\0)')
SONG_RE = re.compile(rb'(?<![a-z0-9])[a-z][a-z0-9]{2,5}(?![a-z0-9])')


def _mine(data):
    paths = {m.group().decode('latin1').lstrip('/') for m in PATH_RE.finditer(data)}
    dirs = {m.group().decode('latin1') for m in DIR_RE.finditer(data)}
    toks = {m.group().decode('latin1') for m in TOKEN_RE.finditer(data)}
    stems = {m.group().decode('latin1') for m in STEM_RE.finditer(data)}
    exts = {m.group()[1:].decode('latin1') for m in re.finditer(rb'(?<=\0)\.[a-z0-9]{2,4}(?=\0)', data)}
    for line in re.findall(rb'conversion ([^\r\n]+)', data):
        paths |= {p.decode('latin1').lstrip('/') for p in line.split(b':') if b'/' in p}
    return paths, dirs, toks, exts, stems


def _binds(data):
    names = set()
    for i in range(len(data) // 0x30):
        s = data[i * 0x30 + 0x10:i * 0x30 + 0x30].split(b'\0')[0]
        if s and all(32 < c < 127 for c in s):
            names.add(s.decode('latin1'))
    return names


class Namer:
    def __init__(self, hashes):
        self.want = set(hashes)
        self.found = {}
        self.solved = set()  # names spelled by solve_layouts (hash-exact, spelling guessed)

    def offer(self, path):
        h = name_hash(path)
        if h in self.want and h not in self.found:
            self.found[h] = path
            return True
        return False

    def offer_under(self, prefix, suffixes):
        """Hash prefix once, then each suffix incrementally."""
        h0 = hash_continue(0, prefix)
        new = 0
        for s in suffixes:
            h = hash_continue(h0, s)
            if h in self.want and h not in self.found:
                self.found[h] = prefix + s
                new += 1
        return new


REGIONS = 'engl japa kore span ital germ fren'.split()
NAME_RE = re.compile(r'^[a-z][a-z_]*[0-9]*$')


def _siblings(path):
    """Numbered and region siblings of a known path (course/cos01 -> cos02.., lang/japa -> engl..)."""
    out = set()
    for m in re.finditer(r'[0-9]+', path):
        w = len(m.group())
        for k in range(min(10 ** w, 100)):
            out.add(path[:m.start()] + '%0*d' % (w, k) + path[m.end():])
    m = re.match(r'^(.*_)(%s)(\.[a-z0-9]+)$' % '|'.join(PARTS), path)
    if m:
        out |= {m.group(1) + part + m.group(3) for part in PARTS}
    for r in REGIONS:
        if '/%s/' % r in path:
            out |= {path.replace('/%s/' % r, '/%s/' % r2) for r2 in REGIONS}
    return out


def _spellings(bits, n):
    """6-bit values -> every spelling. Only ord(c) & 63 is hashed, so '0'..'9' alias 'p'..'y'
    and case is lost; each aliasing position yields both spellings."""
    opts = []
    for i in range(n):
        v = (bits >> (6 * i)) & 63
        if 33 <= v <= 58:
            o = [chr(v + 0x40)]
            if 48 <= v <= 57:
                o.append(chr(v))
        elif v == 31:
            o = ['_']
        else:
            return []
        opts.append(o)
    out = ['']
    for o in opts:
        out = [a + c for a in out for c in o]
    return [x for x in out if NAME_RE.match(x)]


def _pick_spelling(cands, known, digit_tail):
    """A known token wins. Otherwise letters everywhere but the last character, which is a
    digit when the directory's named entries end in digits (`data/anime/mnor1` ->
    `mnor2`, not `mnorr`)."""
    for c in cands:
        if c in known:
            return c
    cands = [c for c in cands if not any(ch.isdigit() for ch in c[:-1])]
    if not cands:
        return None
    return sorted(cands, key=lambda c: (c[-1].isdigit() != digit_tail, c))[0]


def _solver(template, n):
    """The name hash is linear over GF(2) (zero init), so hash(template with X) =
    base ^ M.x for the 6n bits x of X. Returns (base, pivot rows) for Gaussian back-solve."""
    zero = template.replace('{X}', '@' * n)
    base = name_hash(zero)
    rows = []  # (column mask over 32 hash bits, x-bit mask)
    for b in range(6 * n):
        i, bit = divmod(b, 6)
        x = ['@'] * n
        x[i] = chr(0x40 | (1 << bit)) if (1 << bit) != 0x40 else '@'
        col = name_hash(template.replace('{X}', ''.join(x))) ^ base
        rows.append([col, 1 << b])
    piv = []
    for bitpos in range(31, -1, -1):  # after this each pivot's top bit is its own
        sel = next((r for r in rows if r[0] >> bitpos & 1), None)
        if sel is None:
            continue
        rows.remove(sel)
        for r in rows + piv:
            if r[0] >> bitpos & 1:
                r[0] ^= sel[0]
                r[1] ^= sel[1]
        piv.append(sel)
    if rows:  # free variables would make every hash solvable: refuse (n too long)
        return None
    return base, piv


def _solve(solver, target):
    base, piv = solver
    t = target ^ base
    x = 0
    for col, xm in piv:
        if (t >> (col.bit_length() - 1)) & 1:
            t ^= col
            x ^= xm
    return x if t == 0 else None


def solve_layouts(namer, known, max_len=5):
    """Name directories by structure. Layouts are taken from the named files, in two shapes:
    `<dir>{X}/{X}<tail>` (`data/mdb/{X}/{X}_bk.cmt`) and `<dir>{X}/<rest>` (`data/mdb/{X}/all.csq`).
    Each unnamed hash is solved for X of every length 1..max_len (unique per length).

    A layout is used only when >= 3 named directories share it. Evidence rules. The hash is linear with a zero start state, so for two layouts of the same
    shape whose tails have EQUAL length, hash(a) ^ hash(b) is one constant for every X: a false
    X that solves one solves the other, and the pair proves nothing. A solved name is kept when
      * it is a known token (mined from a decoded file, or --names), or
      * it hits two layouts of different signature (shape, tail length), or
      * every named sibling in <dir> (>= 3) has the solved length and the same digit/letter
        tail style (`data/anime/mnor1..` -> `mfja2`): a false fit then needs a 1-in-4 existence
        and a ~(26/64)^n spelling coincidence at that exact length.
    The spelling is a guess where characters alias (see _spellings); rows are flagged."""
    support = {}
    for p in namer.found.values():
        m = re.match(r'^(.*/)([a-z0-9_]+)/\2([^/]*)$', p)
        if m:
            key = (m.group(1) + '{X}/{X}' + m.group(3), ('xx', len(m.group(3))))
        else:
            m = re.match(r'^(.*/)([a-z0-9_]+)/([^/]+)$', p)
            if not m:
                continue
            key = (m.group(1) + '{X}/' + m.group(3), ('x', len(m.group(3))))
        support[key] = support.get(key, 0) + 1
    # A layout counts only when >= 3 named directories share it: a one-off file such as
    # data/tim/wfont/wfont_w.bin would otherwise "fit" ~1/4 of all hashes at length 5.
    layouts = {lay: sig for (lay, sig), k in support.items() if k >= 3}
    sib = {}
    for p in namer.found.values():
        m = re.match(r'^(.*/)([a-z0-9_]+)/', p)
        if m:
            sib.setdefault(m.group(1), set()).add(m.group(2))
    todo = namer.want - set(namer.found)
    hits = {}  # (dir, spelling) -> {(hash, layout)}
    for lay, _sig in layouts.items():
        prefix = lay.split('{X}')[0]
        names = sib.get(prefix, set())
        digit_tail = sum(n[-1].isdigit() for n in names) * 2 > len(names)
        for n in range(1, max_len + 1):
            sv = _solver(lay, n)
            if sv is None:
                continue
            for h in todo:
                x = _solve(sv, h)
                if x is None:
                    continue
                name = _pick_spelling(_spellings(x, n), known, digit_tail)
                if name:
                    hits.setdefault((prefix, name), set()).add((h, lay))
    added = 0
    for (prefix, name), hs in sorted(hits.items()):
        sigs = {layouts[lay] for _, lay in hs}
        names = sib.get(prefix, set())
        uniform = (len(names) >= 3 and {len(x) for x in names} == {len(name)}
                   and {x[-1].isdigit() for x in names} == {name[-1].isdigit()})
        if not (name in known or len(sigs) > 1 or uniform):
            continue
        for h, lay in hs:
            path = lay.replace('{X}', name)
            if name_hash(path) == h and h not in namer.found:
                namer.found[h] = path
                namer.solved.add(h)
                added += 1
    return added


def recover_names(files, blobs, extra=()):
    """blobs: callable(entry) -> decoded bytes. Returns the Namer (.found {hash: path}, .solved)."""
    namer = Namer(f['hash'] for f in files)
    for p in extra:
        namer.offer(p)
    mined = set()
    songs = set()
    dirs = {'data/', 'data/mdb/', 'data/chara/', 'data/motion/', 'data/course/', 'data/movie/common/',
            'data/tim/', 'data/gpct/', 'data/bpct/', 'data/lang/', 'data/anime/', 'boot/', 'soft/s573/', 's573/'}
    toks, exts, paths, stems = set(), set(EXTS), set(), set()
    # the boot/executable files are always under these names
    for p in ('boot/config.dat', 'boot/psx.bin', 'boot/checksum.dat', 's573/config.dat', 's573/psx.bin',
              's573/aout.exe', 's573/checksum.dat', 'soft/s573/aout.exe', 'data/mdb/mdb.bin',
              'data/mdb/ja_mdb.bin', 'data/all/texbind.bin', 'data/tex/rembind.bin', 'data/tex/subbind.bin',
              'data/chara/chara.lst', 'data/chara/chara.pos'):
        namer.offer(p)
    sibled = set()
    for _round in range(8):
        before = len(namer.found)
        for f in files:
            if f['hash'] in namer.found and f['hash'] not in mined:
                mined.add(f['hash'])
                data = blobs(f)
                if not data:
                    continue
                name = namer.found[f['hash']]
                if name.endswith('config.dat') and data[:1] not in (b'#', b'/', b'c'):
                    data = config_decrypt(data)
                p, d, t, e, st = _mine(data)
                stems |= st
                paths |= p
                dirs |= d
                exts |= e
                if name.endswith(('.exe', '.olb', 'psx.bin')):
                    # identifiers only from code: data blobs yield garbage tokens, and a garbage
                    # token can win an aliased hash ('q0' vs the game's 'qp': same 6-bit codes)
                    toks |= t
                    songs |= {x for x in t if 3 <= len(x) <= 6 and x.isalnum()}
                if 'mdb' in name and name.endswith('.bin'):
                    songs |= {m.group().decode() for m in SONG_RE.finditer(data)}
                if name.endswith(('texbind.bin', 'rembind.bin', 'subbind.bin')):
                    for b in _binds(data):
                        paths |= {'data/%s.%s' % (b, x) for x in ('cmt', 'tim', 'bin')}
        ext_list = sorted(exts)
        for p in paths:
            namer.offer(p)
        for p in list(namer.found.values()):
            if p not in sibled:
                sibled.add(p)
                for q in _siblings(p):
                    namer.offer(q)
        for s in songs:
            base = 'data/mdb/%s/' % s
            sfx = ['%s.%s' % (s, x) for x in ext_list] + ['all.%s' % x for x in ext_list]
            sfx += ['%s_%s.%s' % (s, part, x) for part in PARTS for x in ('cmt', 'tim')]
            namer.offer_under(base, sfx)
            namer.offer_under('data/course/', ['%s_%s.%s' % (s, part, x) for part in PARTS for x in ('cmt', 'tim')])
        for st in stems:  # `data/course/cos` + NN + _part.ext
            namer.offer_under(st, ['%02d_%s.%s' % (k, part, x) for k in range(100) for part in PARTS
                                   for x in ('cmt', 'tim')] + ['%02d.%s' % (k, x) for k in range(100) for x in ext_list])
        for d in sorted(x for x in dirs if x.startswith('data/')):
            names = []
            for t in toks:
                names.append(t)
                names += ['%s.%s' % (t, x) for x in EXTS]
                names += ['%s/%s.%s' % (t, t, x) for x in EXTS]
                names += ['%s/%s.%s' % (t, x, x) for x in ('cmt', 'tim')]
            namer.offer_under(d, names)
        solve_layouts(namer, toks | songs | {q for p in extra for q in re.split(r'[/._]', p)})
        if len(namer.found) == before:
            break
    return namer


# ---------------------------------------------------------------------------
# SuperDisc installer table
# ---------------------------------------------------------------------------
def superdisc_mixes(installer):
    """(label, game image, card image) rows from a SuperDisc PSX.EXE's mix table."""
    t_addr = struct.unpack_from('<I', installer, 0x18)[0]
    body = installer[0x800:]

    def cstr(va):
        o = va - t_addr
        if not 0 <= o < len(body):
            return None
        e = body.find(b'\0', o)
        s = body[o:e]
        return s.decode('latin1') if s and all(32 <= c < 127 for c in s) else None

    rows = []
    for o in range(0, len(body) - 12, 4):
        a, g, c = struct.unpack_from('<III', body, o)
        gs = cstr(g)
        if gs and gs.startswith('/cdrom/IN/') and (c == 0 or (cstr(c) or '').startswith('/cdrom/IN/')):
            label = cstr(a)
            row = (label, gs[len('/cdrom/'):], (cstr(c) or '')[len('/cdrom/'):] or None)
            if label and row not in rows:
                rows.append(row)
    return rows


def _tilde(p):
    home = os.path.expanduser('~')
    return '~' + p[len(home):] if p.startswith(home) else p


def cmd_extract(args):
    game = open(args.game, 'rb').read()
    card = open(args.card, 'rb').read() if args.card and os.path.getsize(args.card) else None
    files = read_file_table(game)
    cache = {}

    def blob(f):
        if f['index'] not in cache:
            cache[f['index']] = load_entry(f, game, card)
        return cache[f['index']][0]

    extra = []
    for src in args.names or ():
        if src.endswith('.json'):  # another mix's _manifest.json: reuse its names
            extra += [r['name'] for r in json.load(open(src)) if not r['name'].startswith('_unnamed/')]
        else:
            extra += [ln.strip() for ln in open(src) if ln.strip() and not ln.startswith('#')]
    namer = recover_names(files, blob, extra)
    names = namer.found
    manifest = []
    for f in files:
        data = blob(f)
        note = cache[f['index']][1]
        rel = names.get(f['hash'], '_unnamed/%08x.bin' % f['hash'])
        out = os.path.join(args.out, rel)
        if f['hash'] in namer.solved:
            note = (note + '; ' if note else '') + 'name solved from hash (spelling may alias)'
        row = dict(f, hash='%08x' % f['hash'], name=rel, note=note, written=False)
        if data is not None and not os.path.exists(out):
            os.makedirs(os.path.dirname(out), exist_ok=True)
            with open(out, 'wb') as fh:
                fh.write(data)
            row['written'] = True
        manifest.append(row)
    with open(os.path.join(args.out, '_manifest.json'), 'w') as fh:
        json.dump(manifest, fh, indent=1)
    unnamed = sum(1 for f in files if f['hash'] not in names)
    print('%d entries, %d named, %d unnamed -> %s' % (len(files), len(files) - unnamed, unnamed, _tilde(args.out)))


def main(argv):
    ap = argparse.ArgumentParser(description='Extract a Konami System 573 DDR flash filesystem.')
    sub = ap.add_subparsers(dest='cmd', required=True)
    e = sub.add_parser('extract')
    e.add_argument('--game', required=True)
    e.add_argument('--card')
    e.add_argument('--out', required=True)
    e.add_argument('--names', action='append',
                   help='extra candidate paths: a text file (one per line) or another _manifest.json; repeatable')
    s = sub.add_parser('superdisc')
    s.add_argument('disc')
    h = sub.add_parser('hash')
    h.add_argument('paths', nargs='+')
    args = ap.parse_args(argv)
    if args.cmd == 'extract':
        cmd_extract(args)
    elif args.cmd == 'superdisc':
        exe = os.path.join(args.disc, 'PSX.EXE')
        if not os.path.exists(exe):
            exe = next(os.path.join(args.disc, f) for f in os.listdir(args.disc) if f.upper().endswith('PSX.EXE'))
        for label, g, c in superdisc_mixes(open(exe, 'rb').read()):
            present = all(os.path.exists(os.path.join(args.disc, p)) for p in (g, c) if p)
            print('%-26s %-8s %-8s %s' % (label, g, c or '-', 'present' if present else 'missing'))
    else:
        for p in args.paths:
            print('%08x  %s' % (name_hash(p), p))


if __name__ == '__main__':
    main(sys.argv[1:])
