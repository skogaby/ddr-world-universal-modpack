#!/usr/bin/env python3
"""Host tests for the Wii Hottest Party tools: scripts/extract_wii_ddr_data.py (Hudson data packs
and codecs, GX textures, sprites, TPL, U8, Nintendo LZ, DSP-ADPCM / BRSTM, RWSD waves, messages,
main.dol / REL tables) and scripts/hsf_dump.py (HSFV037 parsing, envelopes, the motion curve
rules, the World-space conversion). Everything is built synthetically -- no disc needed.

Run: scripts/validate_wii_ddr_tools.sh (or `python3 -m unittest test_wii_ddr_formats` in scripts/).
"""
import os
import struct
import sys
import tempfile
import unittest
import zlib

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import extract_wii_ddr_data as W  # noqa: E402
import hsf_dump as H  # noqa: E402


# ---------------------------------------------------------------------------------------------
# helpers
# ---------------------------------------------------------------------------------------------
def tile(pixels, bw, bh):
    """(h, w, ...) array -> GX tile order (block rows, blocks, rows, pixels)."""
    h, w = pixels.shape[:2]
    by, bx = h // bh, w // bw
    rest = pixels.shape[2:]
    return pixels.reshape((by, bh, bx, bw) + rest).transpose((0, 2, 1, 3) + tuple(range(4, 4 + len(rest)))).reshape(
        (-1,) + rest)


def lz_literals(data):
    """A HuDecodeLz stream of literals only (control byte 0xFF per 8 bytes)."""
    out = bytearray()
    for i in range(0, len(data), 8):
        out.append(0xFF)
        out += data[i:i + 8]
    return bytes(out)


def pack(entries):
    """A HuData pack from [(codec, raw)]."""
    bodies = []
    for codec, raw in entries:
        if codec == W.CODEC_ZLIB:
            z = zlib.compress(raw)
            bodies.append(struct.pack('>IIII', len(raw), 7, len(raw), len(z)) + z)
        elif codec == W.CODEC_LZ:
            bodies.append(struct.pack('>II', len(raw), 1) + lz_literals(raw))
        else:
            bodies.append(struct.pack('>II', len(raw), 0) + raw)
    head = 4 + 4 * len(bodies)
    offs, pos = [], head
    for b in bodies:
        offs.append(pos)
        pos += (len(b) + 3) & ~3
    out = struct.pack('>I', len(bodies)) + b''.join(struct.pack('>I', o) for o in offs)
    for b in bodies:
        out += b.ljust((len(b) + 3) & ~3, b'\0')
    return out


class HsfWriter:
    """Builds a minimal HSFV037 file: a `Reference` root, joints Hips > Head, one enveloped
    triangle mesh with a 4x4 RGB565 bitmap, and one motion (Head rotY 0 -> 90 over 10 frames)."""

    def build(self, extra_track=None):
        names = b''
        offs = {}

        def name(s):
            nonlocal names
            if s not in offs:
                offs[s] = len(names)
                names += s.encode() + b'\0'
            return offs[s]

        for s in ('Reference', 'Hips', 'Head', 'body', 'mat', 'tex', 'buf', 'MayaConverter'):
            name(s)
        symbols = [1, 3, 2, 0]   # Reference children [Hips, body] @0, Hips children [Head] @2, mat attrs [0] @3
        sec = {}
        blob = bytearray(0xB0)

        def put(section, data, count):
            sec[section] = (len(blob), count)
            blob.extend(data)
            while len(blob) % 4:
                blob.append(0)

        # material (1) -> attribute 0 -> bitmap 0
        mat = bytearray(H.MATERIAL_SIZE)
        struct.pack_into('>I', mat, 0, name('mat'))
        struct.pack_into('>HB', mat, 8, 0, 1)
        struct.pack_into('>III', mat, 0x30, H.MATERIAL_FLAG_NOCULL, 1, 3)
        put('material', mat, 1)
        att = bytearray(H.ATTRIBUTE_SIZE)
        struct.pack_into('>I', att, 0, 0xFFFFFFFF)
        struct.pack_into('>i', att, 0x80, 0)
        put('attribute', att, 1)
        verts = np.array([[0, 10, 0], [1, 10, 0], [1, 15, 0]], '>f4')
        put('vertex', struct.pack('>IiI', name('buf'), 3, 0) + verts.tobytes(), 1)
        put('normal', struct.pack('>IiI', name('buf'), 3, 0) + np.tile(np.array([[0, 0, 1]], '>f4'), (3, 1)).tobytes(), 1)
        put('st', struct.pack('>IiI', name('buf'), 3, 0) + np.array([[0, 0], [1, 0], [1, 1]], '>f4').tobytes(), 1)
        face = bytearray(H.FACE_SIZE)
        struct.pack_into('>hh', face, 0, H.FACE_TRI, 0)
        struct.pack_into('>12h', face, 4, 0, 0, -1, 0, 1, 1, -1, 1, 2, 2, -1, 2)
        put('face', struct.pack('>IiI', name('buf'), 1, 0) + face, 1)
        # objects: 0 Reference (null), 1 Hips (joint), 2 Head (joint), 3 body (mesh)
        objs = bytearray()
        layout = [('Reference', H.OBJ_NULL1, -1, (2, 0), (0, 0, 0)), ('Hips', H.OBJ_JOINT, 0, (1, 2), (0, 10, 0)),
                  ('Head', H.OBJ_JOINT, 1, (0, 0), (0, 5, 0)), ('body', H.OBJ_MESH, 0, (0, 0), (0, 0, 0))]
        for nm, typ, parent, (nch, chi), t in layout:
            o = bytearray(H.OBJECT_SIZE)
            struct.pack_into('>IIIIiII', o, 0, name(nm), typ, 0, 0, parent, nch, chi)
            struct.pack_into('>9f', o, 0x1C, *t, 0, 0, 0, 1, 1, 1)
            struct.pack_into('>9f', o, 0x40, *t, 0, 0, 0, 1, 1, 1)
            refs = (0, 0, 0, -1, 0, 0, 0) if typ == H.OBJ_MESH else (-1,) * 7
            struct.pack_into('>7i', o, 0x104, *refs)
            if typ == H.OBJ_MESH:
                struct.pack_into('>6I', o, 0x124, 0, 0, 0, 0, 1, 0)
            objs += o
        put('object', objs, 4)
        # bitmap: 4 x 4 RGB565, one 4x4 block: red top half, blue bottom half
        px = np.zeros((4, 4), '>u2')
        px[:2] = 0xF800
        px[2:] = 0x001F
        bm = bytearray(H.BITMAP_SIZE)
        struct.pack_into('>II', bm, 0, name('tex'), 0)
        bm[8], bm[9] = 4, 16
        struct.pack_into('>hhh', bm, 10, 4, 4, 0)
        struct.pack_into('>IiII', bm, 0x10, 0, -1, 0, 0)
        put('bitmap', bm + tile(px, 4, 4).astype('>u2').tobytes(), 1)
        # motion: Head rotY linear 0 -> 90 over 10 frames (+ an optional extra track)
        tracks = [(2, 0, name('Head'), 0, 29, H.CURVE_LINEAR, 2, 0)]
        keys = struct.pack('>4f', 0, 0, 10, 90)
        if extra_track:
            tracks.append(extra_track[0])
            keys += extra_track[1]
        mot = struct.pack('>IIIf', name('MayaConverter'), len(tracks), 0, 10.0)
        for tr in tracks:
            mot += struct.pack('>BBHHHHHI', *tr)
        put('motion', mot + keys, 1)
        # cenv: Hips owns v0, v1 rigidly; v2 is blended 0.25 Hips / 0.75 Head (dual)
        cenv = struct.pack('>9I', name('body'), 0, 12, 28, 1, 1, 0, 3, 0)
        data = struct.pack('>IHHHH', 1, 0, 2, 0, 2)                    # single
        data += struct.pack('>IIII', 1, 2, 1, 0)                         # dual -> weights @0
        data += struct.pack('>fHHHH', 0.25, 2, 1, 2, 1)                  # weight: 0.25 on target1
        put('cenv', cenv + data, 1)
        put('symbol', struct.pack('>%dI' % len(symbols), *symbols), len(symbols))
        put('string', names, len(names))
        blob[:8] = H.MAGIC
        for i, s in enumerate(H.SECTIONS):
            o, n = sec.get(s, (0, 0))
            struct.pack_into('>ii', blob, 8 + 8 * i, o, n)
        return bytes(blob)


# ---------------------------------------------------------------------------------------------
# extract_wii_ddr_data
# ---------------------------------------------------------------------------------------------
class TestCodecs(unittest.TestCase):
    def test_lz_literals_and_backref(self):
        raw = bytes(range(40))
        out, used = W.decode_lz(lz_literals(raw), len(raw))
        self.assertEqual(out, raw)
        # 3 literals 'abc', then a pair copying 6 bytes from ring position 958 (the start)
        stream = bytes([0b00000111]) + b'abc' + bytes([958 & 0xFF, ((958 >> 8) << 6) | 3])
        out, _ = W.decode_lz(stream, 9)
        self.assertEqual(out, b'abcabcabc')

    def test_slide(self):
        # u32 size, flags: 3 literals then a pair {len 1 -> 3 bytes, dist 2 -> 3 back}
        stream = struct.pack('>I', 6) + struct.pack('>I', 0xE0000000) + b'xyz' + struct.pack('>H', (1 << 12) | 2)
        out, _ = W.decode_slide(stream, 6)
        self.assertEqual(out, b'xyzxyz')

    def test_rle(self):
        out, _ = W.decode_rle(bytes([4, 0x41, 0x83]) + b'xyz', 7)
        self.assertEqual(out, b'AAAAxyz')

    def test_pack_round_trip(self):
        a, b, c = b'HSFV037\0' + bytes(200), bytes(range(100)) * 3, b'raw bytes'
        blob = pack([(W.CODEC_ZLIB, a), (W.CODEC_LZ, b), (W.CODEC_NONE, c)])
        self.assertTrue(W.is_pack(blob))
        spans = W.read_pack(blob)
        self.assertEqual([W.decode_entry(blob, s, e)[0] for s, e in spans], [a, b, c])
        self.assertEqual([W.decode_entry(blob, s, e)[1] for s, e in spans], [7, 1, 0])

    def test_pack_rejects_random(self):
        rnd = np.random.default_rng(1).integers(0, 256, 4096, dtype=np.uint8).tobytes()
        self.assertFalse(W.is_pack(rnd))
        self.assertFalse(W.is_pack(b'\0\0\0\0'))

    def test_nintendo_lz10(self):
        raw = b'ABCABCABCABC'
        # header, flags 0b00010000: 3 literals then one ref (len 9 -> 6+3, dist 3)
        stream = struct.pack('<I', 0x10 | (len(raw) << 8)) + bytes([0b00010000]) + b'ABC' + bytes([(6 << 4) | 0, 2])
        self.assertEqual(W.decode_nlz(stream), raw)


class TestGx(unittest.TestCase):
    def test_sizes(self):
        self.assertEqual(W.gx_size(W.GX_RGBA8, 4, 4), 64)
        self.assertEqual(W.gx_size(W.GX_CMPR, 8, 8), 32)
        self.assertEqual(W.gx_size(W.GX_I4, 9, 8), 64)   # padded to 16 x 8

    def test_rgb565_rgb5a3_ia8_i8(self):
        px = np.arange(16, dtype=np.uint32).reshape(4, 4)
        v565 = (px << 11).astype('>u2')
        out = W.decode_gx(tile(v565, 4, 4).astype('>u2').tobytes(), 4, 4, W.GX_RGB565)
        self.assertEqual(out[1, 2, 0], ((6 << 3) | (6 >> 2)))
        self.assertTrue((out[..., 3] == 255).all())
        v = np.full((4, 4), 0x8000 | (31 << 10), '>u2')
        v[0, 0] = (3 << 12) | (15 << 8)                                 # translucent red (A3 = 3)
        out = W.decode_gx(tile(v, 4, 4).astype('>u2').tobytes(), 4, 4, W.GX_RGB5A3)
        self.assertEqual(tuple(out[0, 0]), (255, 0, 0, (3 << 5) | (3 << 2) | (3 >> 1)))
        self.assertEqual(tuple(out[3, 3]), (255, 0, 0, 255))
        ia = np.full((4, 4), 0x80FF, '>u2')
        out = W.decode_gx(tile(ia, 4, 4).astype('>u2').tobytes(), 4, 4, W.GX_IA8)
        self.assertEqual(tuple(out[2, 1]), (255, 255, 255, 0x80))
        i8 = np.arange(32, dtype=np.uint8).reshape(4, 8)
        out = W.decode_gx(tile(i8, 8, 4).tobytes(), 8, 4, W.GX_I8)
        self.assertEqual(out[3, 7, 0], 31)

    def test_i4_nibble_order(self):
        raw = bytes([0x1F] + [0] * 31)
        out = W.decode_gx(raw, 8, 8, W.GX_I4)
        self.assertEqual((out[0, 0, 0], out[0, 1, 0]), (0x11, 0xFF))

    def test_rgba8_two_passes(self):
        blk = bytearray(64)
        blk[0], blk[1] = 0x40, 0x10    # pixel 0: A, R
        blk[32], blk[33] = 0x20, 0x30  # pixel 0: G, B
        out = W.decode_gx(bytes(blk), 4, 4, W.GX_RGBA8)
        self.assertEqual(tuple(out[0, 0]), (0x10, 0x20, 0x30, 0x40))

    def test_cmpr(self):
        # one 8x8 block, four DXT1 sub-blocks: c0 = white > c1 = black, every index 1 (black)
        # except sub-block 1 (top right), index 0 (white)
        sub_black = struct.pack('>HH', 0xFFFF, 0x0000) + bytes([0x55] * 4)
        sub_white = struct.pack('>HH', 0xFFFF, 0x0000) + bytes([0x00] * 4)
        out = W.decode_gx(sub_black + sub_white + sub_black + sub_black, 8, 8, W.GX_CMPR)
        self.assertEqual(tuple(out[0, 0]), (0, 0, 0, 255))
        self.assertEqual(tuple(out[0, 7]), (255, 255, 255, 255))
        self.assertEqual(tuple(out[7, 7]), (0, 0, 0, 255))

    def test_c8_palette(self):
        pal = W.decode_palette(struct.pack('>2H', 0x7000, 0xFC00), W.TL_RGB5A3)
        idx = np.ones((4, 8), np.uint8)
        idx[0, 0] = 0
        out = W.decode_gx(tile(idx, 8, 4).tobytes(), 8, 4, W.GX_C8, pal)
        self.assertEqual(out[0, 0, 3], (7 << 5) | (7 << 2) | (7 >> 1))
        self.assertEqual(tuple(out[1, 1]), (255, 0, 0, 255))

    def test_png(self):
        with tempfile.TemporaryDirectory() as d:
            p = os.path.join(d, 'x.png')
            W.write_png(p, 2, 1, bytes([1, 2, 3, 4, 5, 6, 7, 8]))
            data = open(p, 'rb').read()
            self.assertEqual(data[:8], b'\x89PNG\r\n\x1a\n')
            self.assertEqual(struct.unpack('>II', data[16:24]), (2, 1))


class TestKinds(unittest.TestCase):
    def ssq(self):
        tempo = struct.pack('<IHHI', 0x24, 1, 150, 3) + bytes(0x24 - 12)
        return tempo + struct.pack('<IHHI', 16, 2, 1, 0) + bytes(4) + struct.pack('<I', 0)

    def sprite(self):
        # 1 bank (1 frame), 1 pattern (1 layer), 1 bitmap: 8x4 I8
        bank = struct.pack('>hhI', 1, 0, 0x14 + 8)
        frame = struct.pack('>6h', 0, 10, 0, 0, 0, 0)
        pat_at = 0x14 + 8 + 12
        layer_at = pat_at + 16
        pat = struct.pack('>5h2xI', 1, 4, 2, 8, 4, layer_at)
        layer = struct.pack('>BBh6h8h', 255, 0, 0, 0, 0, 8, 4, 0, 0, *([0] * 8))
        bmp_at = layer_at + 32
        data_at = bmp_at + 20
        bmp = struct.pack('>BBhhhIII', 8, 7, 0, 8, 4, 32, 0, data_at)
        head = struct.pack('>hhhhIII', 1, 1, 1, 0, 0x14, pat_at, bmp_at)
        return head + bank + frame + pat + layer + bmp + bytes(range(32))

    def test_kinds(self):
        self.assertEqual(W.kind_of(HsfWriter().build()), 'hsf')
        self.assertEqual(W.kind_of(self.ssq()), 'ssq')
        self.assertEqual(W.kind_of(self.sprite()), 'spr')
        self.assertEqual(W.kind_of(bytes(64)), 'bin')

    def test_sprite_parse_and_decode(self):
        blob = self.sprite()
        spr = W.parse_sprite(blob)
        self.assertEqual(len(spr['banks'][0]), 1)
        self.assertEqual(spr['patterns'][0]['size'], (8, 4))
        b = spr['bitmaps'][0]
        self.assertEqual((b['width'], b['height'], b['format']), (8, 4, 7))
        rgba = W.sprite_bitmap_rgba(blob, b)
        self.assertEqual(rgba.shape, (4, 8, 4))
        self.assertEqual(rgba[3, 7, 0], 31)

    def test_tpl(self):
        img = struct.pack('>HHII', 4, 4, W.GX_RGB565, 0x40) + bytes(0x40 - 12 - 0x14)
        blob = W.TPL_MAGIC + struct.pack('>III', 1, 0xC, 0x14) + struct.pack('>I', 0)
        blob = blob[:0x14] + struct.pack('>HHII', 4, 4, W.GX_RGB565, 0x40)
        blob = blob.ljust(0x40, b'\0') + struct.pack('>H', 0xF800) * 16
        imgs = W.tpl_images(blob)
        self.assertEqual(imgs[0].shape, (4, 4, 4))
        self.assertEqual(tuple(imgs[0][0, 0]), (255, 0, 0, 255))
        del img

    def test_u8(self):
        # root dir (3 nodes), dir "d", file "d/f" with data "hi"
        names = b'\0d\0f\0'
        nodes = struct.pack('>III', 0x01000000, 0, 3) + struct.pack('>III', 0x01000001, 0, 3)
        data_off = 0x20 + 36 + len(names)
        nodes += struct.pack('>III', 0x00000003, data_off, 2)
        blob = W.U8_MAGIC + struct.pack('>III', 0x20, 36 + len(names), data_off) + bytes(16) + nodes + names + b'hi'
        self.assertEqual(W.parse_u8(blob), [('d/f', b'hi')])

    def test_messages(self):
        msg = struct.pack('>HH', 255, 255) + b'Set\x10Game\nOK\x85\0\0'
        group = struct.pack('>II', 1, 4) + msg
        blob = struct.pack('>II', 1, 4) + group
        g = W.parse_messages(blob)
        self.assertEqual(W.decode_message_text(g[0][0][2]), 'Set Game\nOK{85}')


class TestAudio(unittest.TestCase):
    def test_dsp_decode(self):
        # coefs 0: every sample = nibble << scale (scale 1 -> x2)
        frame = bytes([0x01]) + bytes([0x12, 0x3F, 0x70, 0x00, 0x00, 0x00, 0x00])
        out = W.dsp_decode(frame, 14, [0] * 16)
        self.assertEqual(out[:6], [2, 4, 6, -2, 14, 0])
        # predictor 1 with c1 = 2048 (1.0): a running sum
        coefs = [0, 0, 2048, 0] + [0] * 12
        out = W.dsp_decode(bytes([0x10, 0x11]) + bytes(6), 3, coefs)
        self.assertEqual(out, [1, 2, 2])

    def test_nibbles(self):
        self.assertEqual(W.nibbles_to_samples(16), 14)
        self.assertEqual(W.nibbles_to_samples(18), 14)
        self.assertEqual(W.nibbles_to_samples(20), 16)

    def test_rwsd_waves_and_wav(self):
        # RWSD with a WAVE section holding one mono ADPCM wave of 14 samples
        info = bytearray(0x5C + 0x30)
        info[0], info[1], info[2] = 2, 0, 1
        struct.pack_into('>H', info, 4, 32000)
        struct.pack_into('>IIII', info, 8, 2, 16, 0x1C, 0)     # loop start, end nibbles, ctab, data
        struct.pack_into('>I', info, 0x1C, 0x20)               # channel 0 info @0x20
        struct.pack_into('>II', info, 0x20, 0, 0x5C)          # data offset 0, adpcm info @0x5C
        wave_sec = b'WAVE' + struct.pack('>II', 12 + 4 + len(info), 1) + struct.pack('>I', 16) + bytes(info)
        rwsd = bytearray(b'RWSD' + struct.pack('>HHIHH', 0xFEFF, 0x0102, 0, 0x20, 2))
        rwsd += struct.pack('>IIII', 0x20, 0, 0x20, len(wave_sec))
        rwsd = rwsd.ljust(0x20, b'\0') + wave_sec
        waves = W.rwsd_waves(bytes(rwsd), bytes([0x01, 0x12]) + bytes(6))
        self.assertEqual(len(waves), 1)
        w = waves[0]
        self.assertEqual((w['channels'], w['rate'], w['samples']), (1, 32000, 14))
        with tempfile.TemporaryDirectory() as d:
            p = os.path.join(d, 'w.wav')
            W.wave_to_wav(p, w)
            data = open(p, 'rb').read()
            self.assertEqual(data[:4], b'RIFF')
            self.assertEqual(struct.unpack('<2h', data[44:48]), (2, 4))

    def test_rwsd_1_3_waves_in_rwar(self):
        # FuruFuru Party / MUSIC FIT: an RWSD 1.3 with only a DATA block; its wave is an RWAV
        # inside the RWAR that fills the group's wave block
        info = bytearray(0x5C + 0x30)
        info[0], info[1], info[2] = 2, 0, 1
        struct.pack_into('>H', info, 4, 32000)
        struct.pack_into('>IIII', info, 8, 2, 16, 0x1C, 0)
        struct.pack_into('>I', info, 0x1C, 0x20)
        struct.pack_into('>II', info, 0x20, 0, 0x5C)
        samples = bytes([0x01, 0x12]) + bytes(6)
        info_blk = b'INFO' + struct.pack('>I', 8 + len(info)) + bytes(info)
        data_blk = b'DATA' + struct.pack('>I', 8 + len(samples)) + samples
        rwav = b'RWAV' + struct.pack('>HHIHH', 0xFEFF, 0x0102, 0, 0x20, 2)
        rwav += struct.pack('>IIII', 0x20, len(info_blk), 0x20 + len(info_blk), len(data_blk))
        rwav += info_blk + data_blk
        tabl = b'TABL' + struct.pack('>II', 24, 1) + struct.pack('>III', 0x01000000, 8, len(rwav))
        rwar = b'RWAR' + struct.pack('>HHIHH', 0xFEFF, 0x0100, 0, 0x20, 2)
        rwar += struct.pack('>IIII', 0x20, len(tabl), 0x20 + len(tabl), 8 + len(rwav))
        rwar += tabl + b'DATA' + struct.pack('>I', 8 + len(rwav)) + rwav
        rwsd = bytearray(b'RWSD' + struct.pack('>HHIHH', 0xFEFF, 0x0103, 0, 0x20, 1))
        rwsd += struct.pack('>IIII', 0x20, 8, 0, 0)
        rwsd += b'DATA' + struct.pack('>I', 8)
        waves = W.rwsd_waves(bytes(rwsd), rwar)
        self.assertEqual(len(waves), 1)
        self.assertEqual((waves[0]['channels'], waves[0]['rate'], waves[0]['samples']), (1, 32000, 14))


class TestDolTables(unittest.TestCase):
    def dol(self):
        base = 0x80100000
        body = bytearray(0x400)
        strs = {'data/arrow.bin': 0x100, 'data/boot.bin': 0x110, '1,2 Step': 0x120, 'Clocks': 0x130}
        for s, o in strs.items():
            body[o:o + len(s) + 1] = s.encode() + b'\0'
        tab = 0x200
        for k, s in enumerate(['data/arrow.bin', 'data/boot.bin'] + ['data/boot.bin'] * 8):
            struct.pack_into('>II', body, tab + 8 * k, base + strs[s], 0xFFFFFFFF)
        songs = 0x300
        struct.pack_into('>6HI', body, songs, 4, 1, 0, 0, 0, 113, base + strs['1,2 Step'])
        struct.pack_into('>6HI', body, songs + 16, 4, 2, 1, 1, 0, 145, base + strs['Clocks'])
        hdr = bytearray(0x100)
        struct.pack_into('>I', hdr, 7 * 4, 0x100)        # data section 0 offset
        struct.pack_into('>I', hdr, 0x48 + 7 * 4, base)   # address
        struct.pack_into('>I', hdr, 0x90 + 7 * 4, len(body))
        return W.Dol(bytes(hdr + body))

    def test_tables(self):
        dol = self.dol()
        dirs = W.dol_data_dirs(dol)
        self.assertEqual(dirs[0], (0, 'data/arrow.bin'))
        self.assertEqual(len(dirs), 10)
        songs = W.dol_songs(dol)
        self.assertEqual([(s['song'], s['bpm_hi'], s['title']) for s in songs], [(1, 113, '1,2 Step'), (2, 145, 'Clocks')])

    def test_danceview_bars(self):
        rel = bytes(16) + struct.pack('>3I', 0x00060001, 0x00060002, 0x00060003) + struct.pack('>3I', 0x3000, 0x2000,
                                                                                                0x800)
        self.assertEqual(W.danceview_clip_bars(rel), {1: 3.0, 2: 2.0, 3: 0.5})


# ---------------------------------------------------------------------------------------------
# hsf_dump
# ---------------------------------------------------------------------------------------------
class TestHsf(unittest.TestCase):
    def setUp(self):
        self.m = H.parse_hsf(HsfWriter().build())

    def test_parse(self):
        m = self.m
        self.assertEqual([o['name'] for o in m['objects']], ['Reference', 'Hips', 'Head', 'body'])
        self.assertEqual(m['objects'][0]['children'], [1, 3])
        self.assertEqual(m['objects'][2]['parent'], 1)
        self.assertEqual(m['materials'][0]['attributes'], [0])
        self.assertEqual(H.mesh_bitmap(m, 0), 0)
        self.assertEqual(len(m['face'][0]['faces']), 1)
        rgba = H.decode_bitmap(m['bitmaps'][0])
        self.assertEqual(tuple(rgba[0, 0]), (255, 0, 0, 255))
        self.assertEqual(tuple(rgba[3, 3]), (0, 0, 255, 255))
        self.assertEqual(m['motions'][0]['tracks'][0]['target'], 'Head')

    def test_gx_order_triangle(self):
        tris, corners, mats = H.mesh_corners(self.m, self.m['objects'][3])
        self.assertEqual(tris, [(0, 1, 2)])
        self.assertEqual([c[0] for c in corners], [0, 2, 1])   # corners (0, 2, 1)

    def test_envelopes_and_skinning(self):
        m = self.m
        body = m['objects'][3]
        w = H.mesh_vertex_weights(m, body)
        self.assertEqual(w[0], [(1, 1.0)])
        self.assertEqual(w[2], [(1, 0.25), (2, 0.75)])
        rest = H.rest_worlds(m)
        np.testing.assert_allclose(rest[2][:3, 3], (0, 15, 0))
        np.testing.assert_allclose(H.skinned_positions(m, body, rest, rest), [[0, 10, 0], [1, 10, 0], [1, 15, 0]],
                                   atol=1e-6)
        posed = H.posed_worlds(m, m['motions'][0], 10.0)                      # Head rotY 90
        p = H.skinned_positions(m, body, posed, rest)
        # v2 = 0.25 * rest (Hips static) + 0.75 * rotated about the Head ((1, 15, 0) -> (0, 15, -1))
        np.testing.assert_allclose(p[2], 0.25 * np.array([1, 15, 0]) + 0.75 * np.array([0, 15, -1]), atol=1e-6)

    def test_rig_and_series(self):
        m = self.m
        self.assertEqual(H.rig_joints(m), [('Hips', None), ('Head', 'Hips')])
        jw = H.joint_worlds(m, m['motions'][0], [0.0, 5.0, 10.0])
        ref = H.posed_worlds(m, m['motions'][0], 5.0)
        np.testing.assert_allclose(jw['Head'][1], ref[2], atol=1e-12)
        ws = H.object_worlds_series(m, m['motions'][0], [5.0])
        np.testing.assert_allclose(ws[0, 2], ref[2], atol=1e-12)
        self.assertEqual(H.animated_objects(m, m['motions'][0]), {2})

    def test_game_mesh(self):
        pos, nrm, uv, col, weights, tris, mats, objs = H.game_mesh(self.m)
        self.assertEqual(len(pos), 3)
        np.testing.assert_allclose(pos[0], np.array([0, 10, 0]) * H.GAME_SCALE)
        self.assertEqual(weights[0], [('Hips', 1.0)])
        t, flipped = H.consistent_winding(pos, nrm, tris)
        self.assertEqual(len(t), 1)
        g = np.cross(pos[t[0, 1]] - pos[t[0, 0]], pos[t[0, 2]] - pos[t[0, 0]])
        self.assertGreater(float(g @ nrm[t[0, 0]]), 0)

    def test_cull_winding(self):
        # a back-to-back pair (HP2 STG021's fan blades): front quad normal +y, back quad -y, the
        # same positions, both in GX order (geometric normal OPPOSITE the vertex normal)
        pos = np.array([[0, 0, 0], [0, 0, -1], [1, 0, 0]] * 2, dtype=float)
        nrm = np.array([[0, 1, 0]] * 3 + [[0, -1, 0]] * 3, dtype=float)
        tris = np.array([[0, 1, 2], [3, 5, 4]])
        out = H.cull_winding(pos, nrm, tris, False)
        g = np.cross(pos[out[:, 1]] - pos[out[:, 0]], pos[out[:, 2]] - pos[out[:, 0]])
        # culled: the GX order reversed -- each face's World front is its own side, never both
        np.testing.assert_array_equal(np.sign(g[:, 1]), [1, -1])
        # ... whatever the normals say (a stray inverted normal keeps the Wii's visible side)
        flipped_n = nrm.copy()
        flipped_n[:3] *= -1
        np.testing.assert_array_equal(H.cull_winding(pos, flipped_n, tris, False), out)
        # two-sided: the shading normal's side (consistent_winding)
        np.testing.assert_array_equal(H.cull_winding(pos, flipped_n, tris, True)[0], [0, 1, 2])
        self.assertEqual(len(H.cull_winding(pos, nrm, np.zeros((0, 3), int), False)), 0)
    def test_curves(self):
        lin = dict(curve=H.CURVE_LINEAR, keys=np.array([[0, 0], [10, 10.0]]))
        np.testing.assert_allclose(H.sample_curve(lin, [0, 2.5, 10, 20]), [0, 2.5, 10, 10])
        step = dict(curve=H.CURVE_STEP, keys=np.array([[0, 1.0], [5, 2.0]]))
        np.testing.assert_allclose(H.sample_curve(step, [0, 4.9, 5, 9]), [1, 1, 2, 2])
        # Hermite with zero slopes = smoothstep between the keys
        her = dict(curve=H.CURVE_HERMITE, keys=np.array([[0, 0, 0, 0], [10, 1.0, 0, 0]]))
        self.assertAlmostEqual(H.curve_value(her, 5.0), 0.5)
        self.assertAlmostEqual(H.curve_value(her, 2.5), 3 * 0.25 ** 2 - 2 * 0.25 ** 3)
        # slopes are NOT scaled by the segment length (hsfmotion.c GetBezier)
        her2 = dict(curve=H.CURVE_HERMITE, keys=np.array([[0, 0, 1.0, 0], [10, 0, 0, 0]]))
        self.assertAlmostEqual(H.curve_value(her2, 5.0), 0.5 ** 3 - 2 * 0.25 + 0.5)
        # MayaConverter pre-roll: key 1 lies BEFORE key 0; the game scans, it does not bisect
        pre = dict(curve=H.CURVE_LINEAR, keys=np.array([[0, 0.125], [-84, 0.125], [12, 0.0], [112, -0.125]]))
        self.assertAlmostEqual(H.curve_value(pre, 6.0), 0.125 + (6 + 84) * (-0.125) / 96)
        self.assertAlmostEqual(H.curve_value(pre, 62.0), -0.0625)
        const = dict(curve=H.CURVE_CONST, value=7.0, keys=np.array([[0, 7.0]]))
        self.assertEqual(H.curve_value(const, 3.0), 7.0)

    def test_extra_tracks_and_kinds(self):
        uv_track = (H.TRACK_ATTRIBUTE, 0, 0xFFFF, 0, 8, H.CURVE_LINEAR, 2, 16)
        m = H.parse_hsf(HsfWriter().build((uv_track, struct.pack('>4f', 0, 0, 10, 1.0))))
        self.assertIn(0, H.attribute_tracks(m['motions'][0]))
        mat = dict(flags=H.MATERIAL_FLAG_ADDCOL, pass_=0, inv_alpha=0.0)
        self.assertEqual(H.material_kind(mat), 'add')
        self.assertEqual(H.material_kind(dict(mat, flags=0, pass_=1)), 'ble')
        self.assertEqual(H.material_kind(dict(mat, flags=0)), 'dec')

    def test_world_conversion_round_trip(self):
        sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
        import anm_dump as A
        m = self.m
        joints = H.rig_joints(m)
        names = [n for n, _ in joints]
        parents = [names.index(p) if p else -1 for _, p in joints]
        binds = H.game_bind_matrices(m)
        frames = list(range(0, 11, 2))
        jw = H.joint_worlds(m, m['motions'][0], frames)
        worlds = np.stack([np.transpose(jw[n], (0, 2, 1)) for n in names], 1)
        worlds[:, :, 3, :3] *= H.GAME_SCALE
        spec, wq = H.worlds_to_anm_spec(worlds, names, parents, [binds[n] for n in names], binds, frames, frames[-1])
        parsed = A.parse_anm(A.write_anm(spec))
        for k, f in enumerate(frames):
            pose = A.evaluate_pose(parsed, f, parents)
            for b in range(len(names)):
                w = np.array(pose[b]['world'], dtype=float).reshape(4, 4)
                self.assertLess(float(np.abs(w[3, :3] - wq[k, b, 3, :3]).max()), 1e-4)

    def test_survey_finds_nothing_wrong(self):
        with tempfile.TemporaryDirectory() as d:
            open(os.path.join(d, 'a.hsf'), 'wb').write(HsfWriter().build())
            files, problems = H.survey([d])
            self.assertEqual((files, problems), (1, []))


if __name__ == '__main__':
    unittest.main()
