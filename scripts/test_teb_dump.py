#!/usr/bin/env python3
"""Unit tests for scripts/teb_dump.py (the zan `CzanEff` TEB decoder + reference simulator,
docs/wii_ddr_zan_effects_research.md) on a synthetic bank built here -- no game data. The DLL's
port (src/mods/background_dancers/flight_fx.rs) carries the same cases in Rust.

    cd scripts && python3 -m unittest -q test_teb_dump
"""
import math
import os
import struct
import sys
import unittest

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import teb_dump as T  # noqa: E402


class Writer:
    """Big-endian blocks at TEB-relative offsets."""

    def __init__(self):
        self.b = bytearray()

    def zeros(self, n):
        while len(self.b) % 4:
            self.b.append(0)
        o = len(self.b)
        self.b += bytes(n)
        return o

    def put(self, o, fmt, *v):
        struct.pack_into('>' + fmt, self.b, o, *v)

    def f32s(self, vals):
        o = self.zeros(4 * len(vals))
        self.put(o, '%df' % len(vals), *vals)
        return o

    def tracks(self, t0, t1, keys, spline=False):
        head = self.zeros(8)
        table = self.zeros(0x14)
        kp = self.zeros(0x20 * len(keys))
        for i, (t, pos, quat) in enumerate(keys):
            self.put(kp + 0x20 * i, '8f', t, *pos, *quat)
        self.put(head, '2I', 1, table)
        self.put(table, '2I2fB', len(keys), kp, t0, t1, int(spline))
        return head


def part(w, flags=T.P_EMITTER | T.P_DRAW, shape=0, life=1.0, interval=0.1, mx=4, per=1, speed=0.0,
         spread=(0, 0, 0), billboard=0, colour=(200, 100, 50, 128), tex=3, ribbon=None, track=(0.0, 100.0)):
    head = w.zeros(0x30)
    w.put(head, 'I', flags)
    w.put(head + 0x2C, 'B', shape)
    w.put(head + 0x28, 'I', w.tracks(track[0], track[1], [(track[0], (0, 0, 0), (0, 0, 0, 0)), (track[1], (0, 0, 0), (0, 0, 0, 0))]))
    e = w.f32s([life, 0.0, interval, *spread, 0.0, 0.0, 0.0, speed, 0.0])
    w.zeros(8)
    w.put(e + 0x2C, '2h', mx, per)
    w.put(e + 0x32, 'B', billboard)
    w.put(head + 4, 'I', e)
    c = w.zeros(4)
    w.put(c, '4B', *colour)
    d = w.zeros(0x30)
    w.put(d, '2I4f', 0, c, -0.5, -0.5, 0.5, 0.5)
    w.put(d + 0x26, 'B', 2)
    w.put(d + 0x28, 'h', tex)
    w.put(head + 12, 'I', d)
    if ribbon:
        r = w.zeros(8)
        w.put(r, 'fBxh', ribbon[0], ribbon[1], ribbon[2])
        w.put(head + 20, 'I', r)
    return head


def bank(parts_of_effects, loop=False, span=2.0):
    """parts_of_effects: per effect, a list of (parent node, part kwargs)."""
    w = Writer()
    w.b += b'TEB\0' + bytes(12)
    w.b += struct.pack('>2I', 0x18, len(parts_of_effects) << 16)
    table = w.zeros(0x10 * len(parts_of_effects))
    for e, parts in enumerate(parts_of_effects):
        n = len(parts) + 1
        lst = w.zeros(8 * n)
        root = w.zeros(8)
        w.put(root, '2I', int(loop), w.tracks(0.0, span, [(0.0, (0, 0, 0), (0, 0, 0, 0)), (span, (0, 0, 0), (0, 0, 0, 0))]))
        w.put(lst, '4BI', 0, 0, 0, 0, root)
        last = {}
        for i, (parent, kw) in enumerate(parts):
            idx = i + 1
            w.put(lst + 8 * idx, '4BI', 0, 0, 1, 0, part(w, **kw))
            if parent in last:
                w.put(lst + 8 * last[parent] + 1, 'B', idx)
            else:
                w.put(lst + 8 * parent, 'B', idx)
            last[parent] = idx
        w.put(table + 0x10 * e, 'I', lst)
        w.put(table + 0x10 * e + 8, 'B', n)
    return bytes(w.b)


def frames(effect, n, attach=None, **kw):
    a = attach or (lambda i: np.eye(4))
    return list(T.simulate(effect, a, n, **kw))


class TestParse(unittest.TestCase):
    def test_nodes_parents_and_blocks(self):
        b = bank([[(0, {}), (0, dict(shape=0, tex=5)), (2, dict(ribbon=(0.5, 10, 7), flags=T.P_EMITTER | T.P_DRAW | T.P_RIBBON))]], loop=True)
        fx = T.parse_teb(b)
        e = fx['effects'][0]
        self.assertTrue(e['root']['loop'])
        self.assertEqual([nd['parent'] for nd in e['nodes']], [None, 0, 0, 2])
        self.assertEqual(T.node_order(e), [0, 1, 2, 3])
        p = e['nodes'][3]['part']
        self.assertEqual(p['emitter']['max'], 4)
        self.assertEqual(p['draw']['tex'], 3)
        self.assertEqual(p['draw']['colour'], (200, 100, 50, 128))
        self.assertEqual(p['ribbon']['segments'], 10)
        self.assertAlmostEqual(p['ribbon']['width'], 0.5)
        self.assertEqual(e['nodes'][2]['part']['draw']['tex'], 5)

    def test_bad_magic(self):
        with self.assertRaises(ValueError):
            T.parse_teb(b'TEX\0' + bytes(32))


class TestEval(unittest.TestCase):
    def test_keys(self):
        self.assertEqual(T.key_eval([], 0.5), 1.0)
        k = [(0.0, 0.0), (0.5, 1.0), (1.0, 0.0)]
        self.assertAlmostEqual(T.key_eval(k, 0.25), 0.5)
        self.assertAlmostEqual(T.key_eval(k, 0.75), 0.5)
        self.assertEqual(T.key_eval(k, -1.0), 0.0)
        c = [(255, 0, 0, 0, 0), (0, 0, 255, 0, 50), (0, 255, 0, 0, 100)]
        np.testing.assert_allclose(T.colour_eval(c, 25.0), [127.5, 0, 127.5])
        np.testing.assert_allclose(T.colour_eval(c, 150.0), [0, 255, 0])

    def test_tracks(self):
        s = math.sqrt(0.5)
        tr = [dict(t0=0.0, t1=1.0, spline=0, keys=[(0.0, np.zeros(3), np.zeros(4)), (1.0, np.array([2.0, 4.0, 6.0]), np.array([0, s, 0, s]))])]
        m, active = T.track_local(tr, 0.5)
        self.assertTrue(active)
        np.testing.assert_allclose(m[:3, 3], [1, 2, 3])
        np.testing.assert_allclose(m[:3, 0], [s, 0, -s], atol=1e-6)   # 45 deg about y
        self.assertEqual(T.track_local(tr, 1.5), (None, False))
        sp = [dict(t0=0.0, t1=3.0, spline=1, keys=[(float(i), np.array([float(i * i), 0, 0]), np.zeros(4)) for i in range(4)])]
        for i in range(3):
            self.assertAlmostEqual(T.track_local(sp, float(i))[0][0, 3], i * i, places=5)


class TestSimulate(unittest.TestCase):
    def test_spawn_rate_cap_and_death(self):
        fx = T.parse_teb(bank([[(0, dict(life=0.25, interval=0.1, mx=3, track=(0.0, 1.0)))]]))
        n = [len(f) for f in frames(fx['effects'][0], 150)]
        self.assertEqual(n[0], 1)
        self.assertTrue(all(x <= 3 for x in n))
        self.assertTrue(all(2 <= x <= 3 for x in n[20:60]), n)
        self.assertEqual(n[85], 0)

    def test_billboard_faces_the_camera(self):
        fx = T.parse_teb(bank([[(0, dict(mx=1))]]))
        vr = np.array([[0, 0, -1.0], [0, 1.0, 0], [1.0, 0, 0]])   # a camera on +x
        s = frames(fx['effects'][0], 1, view_rot=lambda i: vr)[0][0]
        z = s['axes'][:, 2]
        np.testing.assert_allclose(z / np.linalg.norm(z), [1, 0, 0], atol=1e-6)
        self.assertAlmostEqual(np.linalg.norm(s['axes'][:, 0]), T.EFFECT_SCALE)

    def test_scroll_streams_the_ribbon(self):
        kw = dict(flags=T.P_EMITTER | T.P_DRAW | T.P_RIBBON | T.P_IMMORTAL, mx=1, ribbon=(0.5, 30, 7))
        fx = T.parse_teb(bank([[(0, kw)]]))
        side = lambda i: np.array([100.0, 0, 0])   # noqa: E731
        f = frames(fx['effects'][0], 20, cam_pos=side, scroll=T.FLIGHT_SCROLL)[-1]
        pts = f[0]['ribbon']['points']
        self.assertEqual(len(pts), 20)
        mid = [(l + r) / 2 for l, r in pts]
        np.testing.assert_allclose(mid[-1], [0, 0, 0], atol=1e-6)
        self.assertAlmostEqual(mid[0][2], -3.0 * 19, places=4)    # oldest 19 frames x 3 units behind
        self.assertAlmostEqual(np.linalg.norm(pts[1][0] - mid[1]), 5.0, places=4)   # width x scale
        still = frames(fx['effects'][0], 20, cam_pos=side)[-1][0]['ribbon']['points']
        self.assertTrue(all(np.linalg.norm(l) < 1e-6 for l, _r in still))


if __name__ == '__main__':
    unittest.main()
