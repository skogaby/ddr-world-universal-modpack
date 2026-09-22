"""Minimal PMX 2.x reader (plain Python, no Blender): prints the texture table, every material
(name, texture, sphere map, flags, diffuse, edge colour, face range) and the bone names in order.

An mmd_tools FBX export carries NO texture bindings, so this is how you fill the MAT_TEX table in
port_character_mmd_fbx.py. PMX material flag bit 0 = double-sided (0x0e = single-sided, no edge).

    python3 examples/pmx_dump.py "PS - Hatsune Miku.pmx"
"""
import struct
import sys


class R:
    def __init__(self, d):
        self.d = d
        self.o = 0

    def u(self, fmt):
        v = struct.unpack_from('<' + fmt, self.d, self.o)
        self.o += struct.calcsize('<' + fmt)
        return v if len(v) != 1 else v[0]

    def text(self):
        n = self.u('i')
        s = self.d[self.o:self.o + n]
        self.o += n
        return s.decode(self.enc, errors='replace')

    def idx(self, size, signed=True):
        return self.u({1: 'b', 2: 'h', 4: 'i'}[size] if signed else {1: 'B', 2: 'H', 4: 'i'}[size])


def main(path):
    d = open(path, 'rb').read()
    r = R(d)
    assert d[:4] == b'PMX ', d[:4]
    r.o = 4
    ver = r.u("f")
    n = r.u('B')
    g = list(r.u('%dB' % n))
    enc, addvec, vidx, tidx, midx, bidx, mo_idx, ridx = g[:8]
    r.enc = 'utf-16-le' if enc == 0 else 'utf-8'
    name, name_e, comment, comment_e = r.text(), r.text(), r.text(), r.text()
    print('PMX', ver, 'name', name, '|', name_e)
    nv = r.u('i')
    for i in range(nv):
        r.o += 4 * 3 + 4 * 3 + 4 * 2 + 16 * addvec
        wt = r.u('B')
        if wt == 0:
            r.idx(bidx)
        elif wt == 1:
            r.idx(bidx); r.idx(bidx); r.o += 4
        elif wt == 2:
            for _ in range(4):
                r.idx(bidx)
            r.o += 16
        elif wt == 3:
            r.idx(bidx); r.idx(bidx); r.o += 4 + 36
        elif wt == 4:
            for _ in range(4):
                r.idx(bidx)
            r.o += 16
        r.o += 4  # edge scale
    nf = r.u('i')
    r.o += nf * vidx
    nt = r.u('i')
    texs = [r.text() for _ in range(nt)]
    print('TEXTURES', texs)
    nm = r.u('i')
    face_cursor = 0
    for i in range(nm):
        mname = r.text()
        mname_e = r.text()
        diffuse = r.u('4f')
        spec = r.u('3f')
        specpow = r.u('f')
        amb = r.u('3f')
        flags = r.u('B')
        edge = r.u('4f')
        edge_size = r.u('f')
        tex = r.idx(tidx)
        sph = r.idx(tidx)
        sph_mode = r.u('B')
        toon_shared = r.u('B')
        toon = r.u('B') if toon_shared else r.idx(tidx)
        memo = r.text()
        cnt = r.u('i')
        print('MAT %2d %-16s | %-16s tex=%s sph=%s flags=0x%02x diffuse=%s edge=%s faces=%d..%d memo=%r' % (
            i, mname, mname_e, texs[tex] if 0 <= tex < len(texs) else tex, sph, flags,
            tuple(round(x, 2) for x in diffuse), tuple(round(x, 2) for x in edge), face_cursor // 3, (face_cursor + cnt) // 3, memo[:40]))
        face_cursor += cnt
    nb = r.u('i')
    names = []
    for i in range(nb):
        bn = r.text()
        bn_e = r.text()
        names.append((bn, bn_e))
        r.o += 12
        r.idx(bidx)
        r.o += 4
        fl = r.u('H')
        if fl & 0x0001:
            r.idx(bidx)
        else:
            r.o += 12
        if fl & 0x0300:
            r.idx(bidx); r.o += 4
        if fl & 0x0400:
            r.o += 12
        if fl & 0x0800:
            r.o += 24
        if fl & 0x2000:
            r.o += 4
        if fl & 0x0020:
            r.idx(bidx); r.o += 4 + 4
            nl = r.u('i')
            for _ in range(nl):
                r.idx(bidx)
                lim = r.u('B')
                if lim:
                    r.o += 24
    print('BONES', len(names))
    for i, (a, b) in enumerate(names):
        print('  %3d %s | %s' % (i, a, b))


main(sys.argv[1])
