#!/usr/bin/env python3
"""Host tests for the zan-engine decoder (scripts/zan_dump.py) used by the DanceDanceRevolution
FuruFuru Party (HOTTEST PARTY 2), MUSIC FIT (HOTTEST PARTY 3) and HOTTEST PARTY 4 / 5 ports, and for the zan leg of
scripts/extract_wii_ddr_data.py: `WII\\0` archives (named and unnamed, nested, the packed name
strides), ZMB models (texture / material / node blocks, strips, skin by joint name, rigid and
skinned submeshes), ZAB motions (the row-vector quaternion convention), cameras (the 0x34
signature), material modes, the overlay bake, the choreography helpers (chaining, bars, chunks,
dealing), SSQ tempo, the stage instancing rule, the UV-offset keys and the HOTTEST PARTY 4 / 5
additions (the packed texture-count word, the second camera signature, `STG<nnn>_MDL.bin` stages,
colour-group-92 screens, the stage signature / port planner). Everything is built
synthetically -- no disc needed.

Run: scripts/validate_wii_ddr_tools.sh (or `python3 -m unittest test_zan_formats` in scripts/).
"""
import math
import os
import struct
import sys
import tempfile
import types
import unittest

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import extract_wii_ddr_data as W  # noqa: E402
import zan_dump as Z  # noqa: E402


# ---------------------------------------------------------------------------------------------
# builders
# ---------------------------------------------------------------------------------------------
def archive(members, name_words=0, stride=None):
    """A `WII\\0` archive of [(name, bytes)] (names written when name_words > 0, at `stride`)."""
    n = len(members)
    stride = stride or name_words * 4
    head = 0x10 + 8 * n + (n * stride if name_words else 0)
    head = (head + 0x1F) & ~0x1F
    offs, body, pos = [], b'', head
    for _name, blob in members:
        offs.append((pos, len(blob)))
        pad = (-len(blob)) % 0x20
        body += blob + b'\0' * pad
        pos += len(blob) + pad
    out = struct.pack('>4sfII', b'WII\0', 1.0, n, name_words)
    out += b''.join(struct.pack('>II', o, s) for o, s in offs)
    if name_words:
        for name, _b in members:
            out += (name or '').encode().ljust(stride, b'\0')
    return out.ljust(head, b'\0') + body


class Blob:
    """Append-only big-endian buffer with offset fix-ups."""

    def __init__(self, size=0x30):
        self.b = bytearray(size)

    def add(self, data, align=4):
        while len(self.b) % align:
            self.b.append(0)
        o = len(self.b)
        self.b += data
        return o

    def put(self, off, fmt, *vals):
        struct.pack_into(fmt, self.b, off, *vals)


def zmb(nodes, materials, textures=(), material_version=1.0):
    """A ZMB: nodes [dict(name, parent, local (4x4 row), subs [dict(material, flags, pos, nrm, uv,
    col, strips [(pos idx, nrm idx, col idx, uv0 idx[, uv1 idx])], skin [[(joint, w)]] or None)])],
    materials [dict(flags (4 bytes), tex [indices], layer (material dict) or None)]."""
    B = Blob()
    B.b[0:8] = Z.ZMB_MAGIC
    B.put(0x14, '>f', 1.0)
    # textures
    tb = B.add(struct.pack('>HHfII', 0, len(textures), 0.0, 0, 0))
    names = B.add(b''.join(t.encode().ljust(0x20, b'\0') for t in textures))
    B.put(tb + 8, '>I', names)
    B.put(0x18, '>I', tb)
    # materials (+ layers after them)
    size = Z.MATERIAL_SIZE[material_version]
    mb = B.add(struct.pack('>IfII', len(materials), material_version, 0, 0))
    allm = list(materials)
    for m in materials:
        if m.get('layer'):
            allm.append(m['layer'])
    mo = B.add(b'\0' * (size * len(allm)))
    B.put(mb + 8, '>I', mo)
    for i, m in enumerate(allm):
        o = mo + size * i
        B.put(o, '>IIIf', 0x959595FF, 0x959595FF, 0xFF, 0.0)
        B.b[o + 0x10:o + 0x14] = bytes(m['flags'])
        tl = B.add(b''.join(struct.pack('>I', t) for t in m['tex']))
        B.put(o + 0x14, '>II', m.get('ntex_word', len(m['tex'])), tl)
        if m.get('group'):
            B.put(o + 0x28, '>H', m['group'])
        if m.get('layer'):
            B.put(o + 0x28, '>HHI', m.get('group', 0), 1, mo + size * allm.index(m['layer']))
        if material_version == 3.0 and m.get('uv'):
            keys = m['uv']
            ko = B.add(b''.join(struct.pack('>fff4B', t, u, v, *fl) for t, u, v, fl in keys))
            B.put(o + 0x1C, '>I', 1)
            B.put(o + 0x38, '>II', len(keys), ko)
    B.put(0x1C, '>I', mb)
    # nodes
    nb = B.add(struct.pack('>IfII', len(nodes), 2.0, 0, 0))
    no = B.add(b'\0' * (Z.NODE_SIZE * len(nodes)))
    B.put(nb + 8, '>I', no)
    B.put(0x20, '>I', nb)
    for i, nd in enumerate(nodes):
        r = no + Z.NODE_SIZE * i
        B.b[r:r + 0x30] = nd['name'].encode('shift_jis').ljust(0x30, b'\0')
        B.put(r + 0x30, '>16f', *np.asarray(nd.get('local', np.eye(4)), dtype=float).reshape(16))
        B.put(r + 0x94, '>i', nd['parent'])
        subs = nd.get('subs', [])
        B.put(r + 0x98, '>HH', nd.get('flags', 0), len(subs))
        if subs:
            so = B.add(b'\0' * (Z.SUBMESH_SIZE * len(subs)))
            B.put(r + 0x9C, '>I', so)
            for j, sm in enumerate(subs):
                two = sm['flags'] & 0x10000
                rec = 0x20 if two else 0x14
                pk = B.add(b'\0' * (rec * len(sm['strips'])))
                for q, strip in enumerate(sm['strips']):
                    cols = list(zip(*strip))
                    ptrs = [B.add(b''.join(struct.pack('>I', v) for v in c)) for c in cols]
                    B.put(pk + rec * q, '>HH', 1, len(strip))
                    for k, p in enumerate(ptrs):
                        B.put(pk + rec * q + 4 + 4 * k, '>I', p)
                pos = B.add(struct.pack('>%df' % (3 * len(sm['pos'])), *np.ravel(sm['pos'])))
                nrm = B.add(struct.pack('>%df' % (3 * len(sm['nrm'])), *np.ravel(sm['nrm'])))
                uv = B.add(struct.pack('>%df' % (2 * len(sm['uv'])), *np.ravel(sm['uv'])))
                col = B.add(bytes(np.asarray(sm['col'], dtype=np.uint8).ravel()))
                skin = 0
                if sm.get('skin'):
                    recs = []
                    for ws in sm['skin']:
                        inf = B.add(b''.join(j_.encode().ljust(0x3C, b'\0') + struct.pack('>f', w) for j_, w in ws))
                        recs.append(struct.pack('>II', len(ws), inf))
                    skin = B.add(b''.join(recs))
                B.put(so + Z.SUBMESH_SIZE * j, '>8I6I', sm['material'], sm['flags'], len(sm['strips']), len(sm['pos']),
                      len(sm['skin']) if sm.get('skin') else 0, len(sm['nrm']), len(sm['uv']), len(sm['col']),
                      pk, pos, skin, nrm, uv, col)
    return bytes(B.b)


def zab(length, bones):
    """A ZAB: bones [(name, {kind: [(frame, values)]})]."""
    B = Blob(0x30)
    B.b[0:8] = Z.ZAB_MAGIC
    B.put(0x08, '>fII', 1.0, len(bones), length)
    B.put(0x1C, '>iI', -1, 0x30)
    table = B.add(b'\0' * (0x40 * len(bones)))
    assert table == 0x30
    for i, (name, chans) in enumerate(bones):
        r = table + 0x40 * i
        B.b[r:r + 0x30] = name.encode().ljust(0x30, b'\0')
        ch = B.add(b'\0' * (0x10 * len(chans)))
        B.put(r + 0x30, '>iIII', -1, len(chans), 0x111, ch)
        for c, (kind, keys) in enumerate(sorted(chans.items())):
            nv = len(keys[0][1])
            data = B.add(b''.join(struct.pack('>I%df' % nv, f, *v) for f, v in keys))
            B.put(ch + 0x10 * c, '>4I', kind, 4 + 4 * nv, len(keys), data)
    return bytes(B.b)


def cam(length, pos, aim, fov=45.0, signature=None):
    """A camera: 2 keys per track."""
    B = Blob(0x40)
    B.put(0, '>f', length)
    B.b[0x34:0x40] = signature or Z.CAM_SIGNATURE
    tracks = [
        [(0.0, *pos[0]), (length, *pos[1])],
        [(0.0, 0.0, 0.0, 0.0, 1.0), (length, 0.0, 0.0, 0.0, 1.0)],
        [(0.0, fov), (length, fov)],
        [(0.0, *aim[0]), (length, *aim[1])],
        [(0.0, 1.0), (length, 1.0)],
        [(0.0, 5000.0), (length, 5000.0)],
    ]
    for i, keys in enumerate(tracks):
        o = B.add(b''.join(struct.pack('>%df' % len(k), *k) for k in keys))
        B.put(4 + 8 * i, '>II', len(keys), o)
    return bytes(B.b)


def trs(t=(0, 0, 0), s=1.0):
    m = np.eye(4)
    m[:3, :3] *= s
    m[3, :3] = t
    return m


def quat_y(deg):
    a = math.radians(deg) / 2
    return (0.0, math.sin(a), 0.0, math.cos(a))


# ---------------------------------------------------------------------------------------------
# tests
# ---------------------------------------------------------------------------------------------
class TestArchive(unittest.TestCase):
    def test_unnamed_nested(self):
        inner = archive([(None, b'ZMB GC\0\0' + b'\0' * 24), (None, b'hello')])
        outer = archive([(None, inner), (None, b'\x00\x20\xaf\x30' + b'\0' * 12)])
        walked = list(Z.walk(outer))
        self.assertEqual([p for p, *_ in walked], ['/#0', '/#0/#0', '/#0/#1', '/#1'])
        self.assertEqual([k for *_, k in walked], ['archive', 'zmb', 'bin', 'tpl'])
        _p, _n, o, s, _k = walked[2]
        self.assertEqual(outer[o:o + s], b'hello')

    def test_names_64(self):
        blob = archive([('DRAW_STG27_01.zmb', b'a' * 3), ('DRAW_STG27_01.tpl', b'b')], name_words=16)
        self.assertEqual([n for n, _o, _s in Z.archive_members(blob)], ['DRAW_STG27_01.zmb', 'DRAW_STG27_01.tpl'])

    def test_names_packed_stride(self):
        # name_words 4 on the disc packs 17..23-character names at a 24-byte stride
        names = ['DRAW_STG07_01.zmb', 'DRAW_STG07_01.tpl', 'BG_STG07.zmb']
        blob = archive([(n, b'x') for n in names], name_words=4, stride=24)
        self.assertEqual([n for n, _o, _s in Z.archive_members(blob)], names)

    def test_extractor_unpacks(self):
        inner = archive([('a.zab', zab(10, [('Hips', {0: [(0, (0, 1, 2))]})]))], name_words=16)
        blob = archive([(None, inner), ('cam01.cam', cam(3.0, [(0, 0, 10)] * 2, [(0, 0, 0)] * 2))], name_words=16)
        with tempfile.TemporaryDirectory() as d:
            rows, stats = [], {}
            W.extract_zan_archive(blob, os.path.join(d, 'x_unpacked'), 'x.bin', types.SimpleNamespace(png=False),
                                  rows, stats, d)
            self.assertTrue(os.path.exists(os.path.join(d, 'x_unpacked', '#000', 'a.zab')))
            self.assertTrue(os.path.exists(os.path.join(d, 'x_unpacked', 'cam01.cam')))
            self.assertTrue(os.path.exists(os.path.join(d, 'x_unpacked', 'archive.json')))
            self.assertEqual(stats, {'zan_zab': 1, 'zan_cam': 1})


def figure():
    """root -> Hips (y 10) -> Spine (y 2), a skinned mesh holder and a rigid prop on Spine."""
    skinned = dict(material=0, flags=1, pos=[(0, 9, 0), (1, 9, 0), (0, 13, 0), (1, 13, 0)],
                   nrm=[(0, 0, 1)], uv=[(0, 0), (1, 0), (0, 1), (1, 1)], col=[(255, 255, 255, 255)],
                   strips=[[(0, 0, 0, 0), (1, 0, 0, 1), (2, 0, 0, 2), (3, 0, 0, 3)]],
                   skin=[[('Hips', 1.0)], [('Hips', 0.5), ('Spine', 0.5)], [('Spine', 1.0)], [('Spine', 1.0)]])
    rigid = dict(material=1, flags=0, pos=[(0, 0, 0), (1, 0, 0), (0, 1, 0)], nrm=[(0, 0, 1)],
                 uv=[(0, 0), (1, 0), (0, 1)], col=[(255, 0, 0, 128)], strips=[[(0, 0, 0, 0), (1, 0, 0, 1), (2, 0, 0, 2)]])
    nodes = [dict(name='シーン ルート', parent=-1), dict(name='scale', parent=0),
             dict(name='Hips', parent=1, local=trs((0, 10, 0))), dict(name='Spine', parent=2, local=trs((0, 2, 0))),
             dict(name='body', parent=0, subs=[skinned]), dict(name='prop', parent=3, local=trs((0, 1, 0)), subs=[rigid])]
    mats = [dict(flags=(1, 1, 0x03, 0), tex=[0]), dict(flags=(0, 0, 0x83, 0), tex=[1])]
    return Z.parse_zmb(zmb(nodes, mats, ['tex_a.tga', 'tex_b.tga']))


class TestZmb(unittest.TestCase):
    def test_parse(self):
        m = figure()
        self.assertEqual(m['textures'], ['tex_a.tga', 'tex_b.tga'])
        self.assertEqual(len(m['materials']), 2)
        self.assertEqual(m['materials'][1]['textures'], [1])
        body = m['nodes'][m['by_name']['body']]
        sm = body['submeshes'][0]
        self.assertEqual(sm['skin'][1], [('Hips', 0.5), ('Spine', 0.5)])
        self.assertEqual(len(sm['packets']), 1)
        self.assertEqual(sm['packets'][0]['corners'], 4)
        np.testing.assert_allclose(sm['col'][0], [1, 1, 1, 1])

    def test_rest_and_rig(self):
        m = figure()
        W_ = Z.rest_worlds(m)
        np.testing.assert_allclose(W_[m['by_name']['Spine']][3, :3], [0, 12, 0])
        joints = Z.rig_joints(m, keep=['Hips', 'Spine'])
        self.assertEqual(joints, [('Hips', None), ('Spine', 'Hips')])   # no root, wrapper, mesh holders

    def test_strips(self):
        self.assertEqual(Z.strip_triangles(5), [(0, 1, 2), (2, 1, 3), (2, 3, 4)])

    def test_pieces_and_weld(self):
        m = figure()
        pieces = Z.model_pieces(m, {'Hips', 'Spine'})
        skinned = next(p for p in pieces if p['node'] == 'body')
        rigid = next(p for p in pieces if p['node'] == 'prop')
        np.testing.assert_allclose(rigid['pos'][1], [1, 13, 0])            # node-local -> model space
        self.assertEqual(rigid['weights'][0], [('Spine', 1.0)])            # nearest joint ancestor
        pos, nrm, uv, col, wts, tris = Z.weld(skinned, s=1.0)
        self.assertEqual(len(pos), 4)
        self.assertEqual(tris.tolist(), [[0, 1, 2], [2, 1, 3]])
        self.assertEqual(wts[1], [('Hips', 0.5), ('Spine', 0.5)])
        f = trs((5, 0, 0))
        moved = Z.model_pieces(m, {'Hips', 'Spine'}, frame=f)
        np.testing.assert_allclose(next(p for p in moved if p['node'] == 'prop')['pos'][1], [6, 13, 0])

    def test_material_mode(self):
        m = figure()
        self.assertEqual(Z.material_mode(m['materials'][0]), (Z.BLEND_ALPHA, False, True, True))
        self.assertEqual(Z.material_mode(m['materials'][1]), (Z.BLEND_ALPHA, True, False, False))
        self.assertEqual(Z.material_mode(dict(flags=bytes((1, 0, 0x81, 0))))[0], Z.BLEND_ADD)

    def test_layer_overlay(self):
        lay = dict(flags=(1, 0, 0x83, 0), tex=[2])
        nodes = [dict(name='root', parent=-1), dict(name='head', parent=0, subs=[dict(
            material=0, flags=0x10000, pos=[(0, 0, 0), (1, 0, 0), (0, 1, 0), (1, 1, 0)], nrm=[(0, 0, 1)],
            uv=[(0, 0), (1, 0), (0, 1), (1, 1), (0, 0), (1, 0), (0, 1), (1, 1)], col=[(255, 255, 255, 255)],
            strips=[[(0, 0, 0, 0, 4), (1, 0, 0, 1, 5), (2, 0, 0, 2, 6), (3, 0, 0, 3, 7)]])])]
        m = Z.parse_zmb(zmb(nodes, [dict(flags=(1, 0, 0, 0), tex=[0], layer=lay)]))
        self.assertIsNotNone(Z.material_layer(m, 0))
        self.assertEqual(Z.material_layer(m, 0)['textures'], [2])
        piece = Z.model_pieces(m, set(), default_joint='mii_head')[0]
        base = np.zeros((8, 8, 4), np.uint8)
        base[..., 0] = 200
        base[..., 3] = 255
        over = np.zeros((4, 4, 4), np.uint8)
        over[1:3, 1:3] = (0, 0, 255, 255)
        out = Z.bake_overlay(base, over, piece, scale=2)
        self.assertEqual(out.shape, (8, 8, 4))
        self.assertEqual(tuple(out[0, 0]), (200, 0, 0, 255))     # the face shows through
        self.assertEqual(tuple(out[3, 3]), (0, 0, 255, 255))     # the eye on top


class TestZab(unittest.TestCase):
    def test_pose_row_quaternion(self):
        m = figure()
        mo = Z.parse_zab(zab(20, [('Hips', {0: [(0, (0, 10, 0)), (20, (0, 10, 0))],
                                            1: [(0, quat_y(0)), (20, quat_y(90))],
                                            2: [(0, (1, 1, 1)), (20, (1, 1, 1))]})]))
        self.assertEqual(mo['length'], 20)
        w = Z.posed_worlds(m, mo, [0.0, 20.0])
        spine = m['by_name']['Spine']
        np.testing.assert_allclose(w[0, spine, 3, :3], [0, 12, 0], atol=1e-6)
        # 90 degrees about +y (row-vector): the local +x axis lands on -z
        np.testing.assert_allclose(w[1, m['by_name']['Hips'], 0, :3], [0, 0, -1], atol=1e-6)
        np.testing.assert_allclose(Z.quat_rows(np.array([quat_y(90)]))[0] @ Z.quat_rows(np.array([quat_y(-90)]))[0],
                                   np.eye(3), atol=1e-9)

    def test_chain(self):
        m = figure()
        a = Z.parse_zab(zab(10, [('Hips', {0: [(0, (0, 10, 0)), (10, (0, 11, 0))]})]))
        b = Z.parse_zab(zab(10, [('Hips', {0: [(0, (0, 11, 0)), (10, (0, 10, 0))]})]))
        c = Z.parse_zab(zab(10, [('Hips', {0: [(0, (5, 10, 0)), (10, (5, 10, 0))]})]))
        self.assertEqual(Z.chain_motions(m, [(0, a), (1, b), (2, c)], ['Hips', 'Spine']), [[0, 1], [2]])


class TestChoreography(unittest.TestCase):
    def test_clip_bars(self):
        self.assertEqual(Z.clip_bars(90, 128), 1)      # a 160-BPM bar in a 128-BPM song
        self.assertEqual(Z.clip_bars(288, 100), 2)     # two 100-BPM bars
        self.assertEqual(Z.clip_bars(576, 113), 4)
        self.assertEqual(Z.clip_bars(176, 148), 2)

    def test_tempo(self):
        tps, n = 150, 3
        mo = (0, 4096 * 4, 4096 * 8)
        td = (0, 4 * 4 * 60 * 150 // 120, 4 * 4 * 60 * 150 // 120 + 4 * 4 * 60 * 150 // 150)
        body = struct.pack('<%di' % n, *mo) + struct.pack('<%di' % n, *td)
        blob = struct.pack('<IHHI', 12 + len(body), 1, tps, n) + body
        segs = Z.ssq_tempo(blob)
        self.assertAlmostEqual(segs[0][2], 120.0, 3)
        self.assertAlmostEqual(segs[1][2], 150.0, 3)
        self.assertEqual(Z.dominant_bpm(segs), 120)

    def test_chunks(self):
        runs = Z.chunk_take([(i, 1.0) for i in range(19)])
        self.assertEqual([len(r) for r in runs], [8, 11])          # a 3-bar tail joins the last run
        runs = Z.chunk_take([(i, 1.0) for i in range(22)])
        self.assertEqual([len(r) for r in runs], [8, 8, 6])

    def test_deal(self):
        hands = Z.deal_rotating(range(10), 4, 3, seed=1)
        self.assertTrue(all(len(h) == len(set(h)) == 3 for h in hands))
        self.assertEqual(set().union(*hands), set(range(10)))
        self.assertEqual(hands, Z.deal_rotating(range(10), 4, 3, seed=1))

    def test_piece_class(self):
        body = Z.parse_zmb(zmb([dict(name='Hips', parent=-1, local=trs((0, Z.HIPS_UNITS, 0))),
                                dict(name='Head', parent=0, local=trs((0, 5, 0)))], []))
        level = (math.sin(math.radians(45)), 0.0, 0.0, math.cos(math.radians(45)))   # 90 deg about x
        stand = (0.0, 0.0, 0.0, 1.0)

        def piece(t0, t1, q, length=84):
            return Z.parse_zab(zab(length, [('Hips', {Z.CH_T: [(0, t0), (length, t1)],
                                                      Z.CH_R: [(0, q), (length, q)]})]))
        h = Z.HIPS_UNITS
        self.assertEqual(Z.piece_class(body, piece((0, h, 0), (0, h, 0), stand)), 'dance')
        # flight: the Hips at the origin, the body level (MUSIC FIT 046's pieces)
        self.assertEqual(Z.piece_class(body, piece((0, 0.1, 1), (0, 0.1, 1), level)), 'flight')
        # lying on the floor at standing hip height is not flight
        self.assertEqual(Z.piece_class(body, piece((0, h, 0), (0, h, 0), level)), 'dance')
        # the take-off: standing, then a leap to 7x the hip height
        self.assertEqual(Z.piece_class(body, piece((0, h, -21), (0, 7 * h, 38), stand, 600)), 'takeoff')


def dol(data, addr=0x80001000):
    """A minimal DOL: one data section `data` at `addr`."""
    h = [0] * 64
    h[7], h[18 + 7], h[36 + 7] = 0x100, addr, len(data)       # data section 0 (slot 7)
    return struct.pack('>64I', *h) + data


class TestSkinTones(unittest.TestCase):
    def test_skin_material(self):
        mats = [dict(flags=(1, 0, 0, 0), tex=[0]), dict(flags=(1, 0, 0, 0), tex=[0])]
        blob = bytearray(zmb([dict(name='root', parent=-1)], mats))
        m = Z.parse_zmb(bytes(blob))
        self.assertIsNone(Z.skin_material(m))
        struct.pack_into('>I', blob, m['materials'][1]['offset'] + 0x28, 0x00020000)   # colour group 2
        self.assertEqual(Z.skin_material(Z.parse_zmb(bytes(blob))), 1)

    def test_table(self):
        base = 0x80001000
        arrays = b''
        ptr = []
        for r in range(6):                         # six 16-entry RGB rows of 0x30 bytes
            ptr.append(base + len(arrays))
            row = b''.join(bytes([0x10 * (r + 1), i + 1, 0x20]) for i in range(15)) + b'\0\0\0'
            arrays += row
        # groups CHR01.. / CHR21.. / CHR41..: variants 1/2 -> row a, 3/4 -> row b; Mii group empty
        block = [ptr[0], ptr[0], ptr[1], ptr[1], 0, 0, ptr[2], ptr[2], ptr[3], ptr[3], 0, 0,
                 ptr[4], ptr[4], ptr[5], ptr[5], 0, 0] + [0] * 6
        data = arrays + struct.pack('>24I', *block)
        t = Z.skin_tone_table(dol(data, base))
        self.assertEqual(sorted(t), list(range(1, 16)) + list(range(21, 36)) + list(range(41, 56)))
        self.assertEqual(t[1], [(0x10, 1, 0x20)] * 2 + [(0x20, 1, 0x20)] * 2)
        self.assertEqual(t[23][0], (0x30, 3, 0x20))
        self.assertEqual(t[55][2], (0x60, 15, 0x20))
        with self.assertRaises(ValueError):
            Z.skin_tone_table(dol(arrays))


class TestStage(unittest.TestCase):
    def test_instances(self):
        prop = dict(stem='OBJA_Z_STG27_board01_NC', kind='obj')
        other = dict(stem='OBJB_N_STG27_board01x', kind='obj')
        col = Z.parse_zmb(zmb([dict(name='root', parent=-1), dict(name='OBJSET_STG27_board01_01', parent=0),
                               dict(name='OBJSET_STG27_board01_02', parent=0), dict(name='EFF_01_01', parent=0)], []))
        out = Z.stage_instances(dict(models=[prop, other], col=(col, None)))
        self.assertEqual(out['OBJA_Z_STG27_board01_NC'], ['OBJSET_STG27_board01_01', 'OBJSET_STG27_board01_02'])
        self.assertEqual(out['OBJB_N_STG27_board01x'], [])

    def test_uv_keys(self):
        # flags byte 0 / 1 = the u / v axis: 0xFF ends the axis' keys, 1 = hold, else linear
        mat = dict(flags=(1, 0, 0x83, 0), tex=[0],
                   uv=[(0.0, 0.0, 0.0, (0xFF, 1, 0xFF, 1)), (1.0, 0.5, -1.0, (0xFF, 1, 0xFF, 1)),
                       (2.0, 1.0, -2.0, (0xFF, 1, 0xFF, 1))])
        blob = zmb([dict(name='root', parent=-1)], [mat], material_version=3.0)
        m = Z.parse_zmb(blob)
        n, ptr = Z.material_uv_keys(m, m['materials'][0])
        keys, flags = Z.uv_keys(blob, n, ptr)
        self.assertEqual(Z.uv_axis_counts(flags), (0, 3))
        v = Z.sample_uv(keys, flags, [0.5, 1.5, 2.5])
        np.testing.assert_allclose(v[:, 0], [0.0, 0.0, 0.0])        # no u keys: key 0's value
        np.testing.assert_allclose(v[:, 1], [0.0, -1.0, 0.0])       # v holds, wraps at 2 s

    def test_uv_linear_sign_and_phase(self):
        keys = np.array([[0.0, 0.0, 0.0], [2.0, 1.0, -1.0]])
        flags = np.array([[0, 2, 0, 2], [0, 2, 0, 2]], dtype=np.uint8)
        np.testing.assert_allclose(Z.sample_uv(keys, flags, [0.5, 2.5]), [[0.25, -0.25], [0.25, -0.25]])
        # the game draws s' = s - u, t' = t + v (FUN_800e9490)
        np.testing.assert_allclose(Z.texmtx_offset(keys, flags, [0.5]), [[-0.25, -0.25]])
        np.testing.assert_allclose(Z.scroll_offset(0.01, 0.02, [10]), [[-0.1, 0.2]])
        # a key set starting late runs from T = P (FUN_800e921c): keys 1..3 s, P = 2 -> at t = 0
        # the clock reads 2 s
        late = np.array([[1.0, 0.0, 0.0], [3.0, 2.0, 0.0]])
        np.testing.assert_allclose(Z.sample_uv(late, flags, [0.0])[0, 0], 1.0)

    def test_flip_book(self):
        mt = dict(textures=[6, 7, 8, 7, 6], frames=[34, 36, 136, 138, 200])
        tex, ends, period = Z.flip_book(mt)
        self.assertEqual((tex, period), ([6, 7, 8, 7, 6], 200))
        # entry i shows until ends[i]
        idx = Z.flip_index(ends, [0, 33, 34, 35, 36, 135, 136, 138, 199, 200, 234])
        np.testing.assert_array_equal(idx, [0, 0, 1, 1, 2, 2, 3, 4, 4, 0, 1])
        self.assertIsNone(Z.flip_book(dict(textures=[1], frames=[])))
        self.assertIsNone(Z.flip_book(dict(textures=[1, 2], frames=[5])))

    def test_camera(self):
        blob = cam(3.0, [(0, 10, 30), (0, 10, 60)], [(0, 8, 0), (0, 8, 0)], fov=54.0)
        self.assertEqual(Z.kind_of(blob), 'cam')
        c = Z.parse_cam(blob)
        self.assertAlmostEqual(Z.cam_length(c), 3.0)
        s = Z.cam_samples(c, [1.5])
        np.testing.assert_allclose(s['pos'][0], [0, 10, 45])
        self.assertAlmostEqual(float(s['fov'][0, 0]), 54.0, 5)


def quad(material=0):
    return dict(material=material, flags=0, pos=[(0, 0, 0), (1, 0, 0), (0, 1, 0), (1, 1, 0)], nrm=[(0, 0, 1)],
                uv=[(0, 0), (1, 0), (0, 1), (1, 1)], col=[(255, 255, 255, 255)],
                strips=[[(0, 0, 0, 0), (1, 0, 0, 1), (2, 0, 0, 2), (3, 0, 0, 3)]])


def tpl1():
    """A TPL with one 4x4 RGBA8 image."""
    head = struct.pack('>III', 0x0020AF30, 1, 0x0C) + struct.pack('>II', 0x14, 0)
    img = struct.pack('>HHIIIIIIfBBBB', 4, 4, 6, 0x40, 0, 0, 1, 1, 0.0, 0, 0, 0, 0)
    return (head + img).ljust(0x40, b'\0') + bytes(4 * 4 * 4)


def stage_blob(prefix, offset=0.0, layout='hp4'):
    """A stage file: DRAW + BG models (+ TPL), named as `prefix`, in HP4's `_MDL.bin` layout or
    HP2 / HP3's /#0."""
    nodes = [dict(name='root', parent=-1, local=trs((offset, 0, 0)), subs=[quad()])]
    mats = [dict(flags=(1, 0, 0, 0), tex=[0])]
    members = [('DRAW_%s_01.zmb' % prefix, zmb(nodes, mats, ['a.tga'])), ('DRAW_%s_01.tpl' % prefix, tpl1()),
               ('BG_%s.zmb' % prefix, zmb(nodes, mats, ['b.tga'])), ('BG_%s.tpl' % prefix, tpl1())]
    cams = archive([(None, cam(3.0, [(0, 1, 9), (0, 1, 9)], [(0, 1, 0), (0, 1, 0)],
                                signature=Z.CAM_SIGNATURES[1]))])
    if layout == 'hp4':
        return archive([('%s_MDL.bin' % prefix, archive(members, 16)), ('%s_CAM.bin' % prefix, cams)], 16)
    return archive([(None, archive(members, 16)), (None, cams)])


class TestHottestParty45(unittest.TestCase):
    def test_packed_texture_count(self):
        # HP4 / HP5 eye / mouth layers: 0x00010107 = purpose 1 (eye), flag 1, 7 frames
        lay = dict(flags=(1, 0, 0x83, 0), tex=[2, 3, 4, 5, 6, 7, 8], ntex_word=0x00010107)
        m = Z.parse_zmb(zmb([dict(name='root', parent=-1)], [dict(flags=(1, 0, 0, 0), tex=[0], layer=lay)]))
        self.assertEqual(m['layers'][m['materials'][0]['layer_offsets'][0]]['textures'], [2, 3, 4, 5, 6, 7, 8])
        one = Z.parse_zmb(zmb([dict(name='root', parent=-1)], [dict(flags=(1, 0, 0, 0), tex=[9], ntex_word=0x101)]))
        self.assertEqual(one['materials'][0]['textures'], [9])

    def test_second_camera_signature(self):
        blob = cam(3.0, [(0, 1, 9), (0, 1, 9)], [(0, 1, 0), (0, 1, 0)], signature=Z.CAM_SIGNATURES[1])
        self.assertEqual(Z.kind_of(blob), 'cam')
        self.assertAlmostEqual(Z.cam_length(Z.parse_cam(blob)), 3.0)

    def test_mdl_archive_stage(self):
        src = Z.stage_sources(stage_blob('STG001'))
        self.assertEqual([e['stem'] for e in src['models']], ['BG_STG001', 'DRAW_STG001_01'])
        self.assertEqual(len(src['cams']), 1)
        self.assertEqual(len(src['models'][0]['textures']), 1)
        # the signature ignores names and layout: the same content under other names matches
        same = Z.stage_sources(stage_blob('STG101', layout='hp3'))
        moved = Z.stage_sources(stage_blob('STG001', offset=5.0))
        self.assertEqual(Z.stage_signature(src), Z.stage_signature(same))
        self.assertNotEqual(Z.stage_signature(src), Z.stage_signature(moved))

    def test_long_flip_book(self):
        # MUSIC FIT STG102 / HP5 STG426 list 253 / 77 flip-book entries (the old 64 cap dropped them)
        tex = [k % 18 for k in range(253)]
        m = Z.parse_zmb(zmb([dict(name='root', parent=-1)], [dict(flags=(1, 0, 0, 0), tex=tex)]))
        self.assertEqual(m['materials'][0]['textures'], tex)

    def test_stage_params(self):
        def prm(kind, name=b''):
            rec = bytearray(0xE0) + name + (b'\0' if name else b'')
            rec[0:8] = b'ZAR\0WII\0'
            rec[0xC0] = kind
            return bytes(rec)
        blob = archive([('STG013_MDL.bin', archive([])), ('STG013_Prm.bin', archive([(None, prm(3, b'single02'))]))], 16)
        self.assertEqual(Z.stage_params(blob), dict(type=3, movie='single02'))
        plain = archive([('STG001_Prm.bin', archive([(None, prm(0)[:0xE1])]))], 16)
        self.assertEqual(Z.stage_params(plain), dict(type=0, movie=None))
        self.assertIsNone(Z.stage_params(archive([(None, archive([]))])))

    def test_screen_group(self):
        m = Z.parse_zmb(zmb([dict(name='root', parent=-1)], [dict(flags=(1, 0, 0, 0), tex=[0], group=92),
                                                            dict(flags=(1, 0, 0, 0), tex=[0], group=24)]))
        self.assertEqual([Z.material_group(mt) for mt in m['materials']], [92, 24])
        self.assertEqual([Z.is_screen_material(mt) for mt in m['materials']], [True, False])

    def test_plan_stage_ports(self):
        with tempfile.TemporaryDirectory() as d:
            old, new = os.path.join(d, 'old'), os.path.join(d, 'new')
            os.makedirs(old)
            os.makedirs(new)
            open(os.path.join(old, 'STG101.bin'), 'wb').write(stage_blob('STG101', 1.0, 'hp3'))
            open(os.path.join(new, 'STG001.bin'), 'wb').write(stage_blob('STG001', 1.0))      # = old STG101
            open(os.path.join(new, 'STG002.bin'), 'wb').write(stage_blob('STG002', 2.0))
            open(os.path.join(new, 'STG011.bin'), 'wb').write(stage_blob('STG011', 2.0))      # = STG002
            open(os.path.join(new, 'STG000.bin'), 'wb').write(archive([(None, archive([]))]))
            open(os.path.join(new, 'STG002_S.bin'), 'wb').write(stage_blob('STG002', 3.0))   # not a stage file
            port, skipped, sigs = Z.plan_stage_ports(new, [('OLD', old, None)])
        self.assertEqual(port, ['STG002'])
        self.assertEqual(skipped, {'STG000': 'draws nothing', 'STG001': 'same as OLD STG101',
                                   'STG011': 'duplicate of STG002'})
        self.assertEqual(sorted(sigs.values()), ['duplicate of STG002', 'same as OLD STG101'])


if __name__ == '__main__':
    unittest.main()
