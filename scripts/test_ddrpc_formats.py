"""Host-only tests for the DDR for Windows (2002) dancer tools (no game data needed):
scripts/ddrpc_dancer_dump.py. Formats: docs/ddr_pc_dancers_research.md.

Run: (cd scripts && python3 -m unittest -q test_ddrpc_formats)
"""
import struct
import unittest

import numpy as np

import ddrpc_dancer_dump as D


def bmp8(w, h, palette, idx, top_down=False):
    stride = (w + 3) // 4 * 4
    rows = [bytes(idx[y]) + b'\0' * (stride - w) for y in range(h)]
    if not top_down:
        rows = rows[::-1]
    px = b''.join(rows)
    pal = b''.join(bytes([b, g, r, 0]) for r, g, b in palette)
    off = 14 + 40 + len(pal)
    hdr = b'BM' + struct.pack('<IHHI', off + len(px), 0, 0, off)
    hdr += struct.pack('<IiiHHIIiiII', 40, w, -h if top_down else h, 1, 8, 0, len(px), 2835, 2835, len(palette), 0)
    return hdr + pal + px


class BmpTests(unittest.TestCase):
    def test_palette_bottom_up_and_colour_key(self):
        pal = [(10, 20, 30), (248, 0, 248), (200, 100, 0)]
        idx = [[0, 1], [2, 2]]  # row 0 = top of the image
        img = D.decode_bmp(bmp8(2, 2, pal, idx))
        self.assertEqual(img.shape, (2, 2, 4))
        self.assertEqual(tuple(img[0, 0]), (10, 20, 30, 255))
        self.assertEqual(tuple(img[0, 1]), (248, 0, 248, 0))  # the key is transparent
        self.assertEqual(tuple(img[1, 1]), (200, 100, 0, 255))

    def test_top_down_matches_bottom_up(self):
        pal = [(1, 1, 1), (2, 2, 2)]
        idx = [[0, 1, 0], [1, 1, 0]]
        a = D.decode_bmp(bmp8(3, 2, pal, idx))
        b = D.decode_bmp(bmp8(3, 2, pal, idx, top_down=True))
        self.assertTrue((a == b).all())

    def test_24bpp(self):
        px = bytes([30, 20, 10, 60, 50, 40]) + b'\0\0'  # one row, 2 px BGR + pad
        hdr = b'BM' + struct.pack('<IHHI', 54 + len(px), 0, 0, 54)
        hdr += struct.pack('<IiiHHIIiiII', 40, 2, 1, 1, 24, 0, len(px), 0, 0, 0, 0)
        img = D.decode_bmp(hdr + px)
        self.assertEqual(tuple(img[0, 0, :3]), (10, 20, 30))
        self.assertEqual(tuple(img[0, 1, :3]), (40, 50, 60))


class SpaceTests(unittest.TestCase):
    def test_mirror_is_an_involution_and_scales_translation(self):
        m = np.eye(4)
        m[:3, :3] = D._quat_to_mat((0.1, 0.2, 0.3, 0.927))
        m[3, :3] = (1.0, 2.0, 3.0)
        w = D.d3d_to_world(m)
        self.assertTrue(np.allclose(w[3, :3], (0.1, 0.2, -0.3)))
        back = D.MIRROR @ w @ D.MIRROR
        back[3, :3] /= D.SCALE
        self.assertTrue(np.allclose(back, m))

    def test_rotation_stays_proper(self):
        r = D._quat_to_mat((0.0, 0.70710678, 0.0, 0.70710678))
        m = np.eye(4)
        m[:3, :3] = r
        w = D.d3d_to_world(m)
        self.assertAlmostEqual(float(np.linalg.det(w[:3, :3])), 1.0, places=5)

    def test_hierarchy_is_parent_first(self):
        self.assertEqual(D.BONE_NAMES[0], 'root')
        for i, p in enumerate(D.BONE_PARENTS):
            self.assertLess(p, i)
        self.assertEqual(D.BONE_NAMES[D.BONE_PARENTS[D.BONE_NAMES.index('head')]], 'neck')
        self.assertEqual(D.BONE_NAMES[D.BONE_PARENTS[D.BONE_NAMES.index('foot_L')]], 'shin_L')
        self.assertEqual(set(D.BONE_JOINT[1:]), set(range(16)))


class MotionTests(unittest.TestCase):
    def _clip(self, n, hips_x=0.0):
        c = np.tile(np.eye(4), (n, 16, 1, 1))
        for j in range(16):
            c[:, j, 3, 1] = 5.0 + j
        c[:, D.HIPS, 3, 0] = hips_x
        return c

    def test_routine_worlds_timing_and_order(self):
        frames, W = D.routine_worlds([self._clip(60), self._clip(60)], 'inplace')
        self.assertEqual(frames[:3], [0, 2, 4])
        self.assertEqual(frames[-1], 2 * 120)  # 2 measures of 60 + the repeated end pose
        self.assertEqual(W.shape, (121, 17, 4, 4))
        hips_b = D.BONE_NAMES.index('hips')
        self.assertAlmostEqual(W[0, hips_b, 3, 1], (5.0 + D.HIPS) * D.SCALE)

    def test_recentre_moves_hips_to_the_mark(self):
        _f, W = D.routine_worlds([self._clip(60, hips_x=4.0)], 'recentre')
        self.assertAlmostEqual(W[0, D.BONE_NAMES.index('hips'), 3, 0], 0.0)
        _f, W2 = D.routine_worlds([self._clip(60, hips_x=4.0)], 'inplace')
        self.assertAlmostEqual(W2[0, D.BONE_NAMES.index('hips'), 3, 0], 0.4)

    def test_anm_spec_locals_and_constant_collapse(self):
        frames, W = D.routine_worlds([self._clip(60)], 'inplace')
        spec = D.worlds_to_anm_spec(frames, W, D.BONE_PARENTS)
        self.assertEqual(spec['frame_count'], frames[-1])
        self.assertEqual(spec['hierarchy'], D.BONE_PARENTS)
        self.assertEqual(len(spec['tracks']), 2 * 17)
        self.assertTrue(all(len(t['keys']) == 1 for t in spec['tracks']))  # a static pose
        head_b = D.BONE_NAMES.index('head')
        neck_j = D.JOINT_NAMES.index('neck')
        t = [t for t in spec['tracks'] if t['kind'] == 0x1D and t['target'] == head_b][0]
        self.assertAlmostEqual(t['keys'][0][1], (D.JOINT_NAMES.index('head') - neck_j) * D.SCALE)

    def test_retarget_absorbs_bind_reframing(self):
        frames, W = D.routine_worlds([self._clip(60)], 'inplace')
        rest = self._clip(1)[0]
        src = D.world_binds(rest)
        rot = np.eye(4)
        rot[:3, :3] = D._quat_to_mat((0.0, 0.3826834, 0.0, 0.9238795))
        target = [rot @ src[n] for n in D.BONE_NAMES]  # every bind re-framed the same way
        R = D.retarget_worlds(W, D.BONE_NAMES, src, target)
        self.assertTrue(np.allclose(R[0, 1], rot @ W[0, 1]))


class ModelLayoutTests(unittest.TestCase):
    def test_flat_colours_and_material_texture(self):
        ch = dict(name='t', ntex=1, materials=[dict(tex=[]), dict(tex=[0]), dict(tex=[5])],
                  mat_tri_start=[0, 3, 6, 9], tris=np.array([[0, 1, 2], [3, 4, 5], [6, 7, 8]]),
                  colors=np.array([[9, 9, 9, 255]] * 3 + [[255] * 4] * 6, np.uint8))
        self.assertIsNone(D.material_texture(ch, 0))
        self.assertEqual(D.material_texture(ch, 1), 0)
        self.assertIsNone(D.material_texture(ch, 2))  # out of range -> untextured
        self.assertEqual(D.flat_colors(ch), {0: (9, 9, 9, 255), 2: (255, 255, 255, 255)})

    def test_texture_packer_power_of_two(self):
        pos, (w, h) = D._pack_textures([(128, 128)] * 7 + [(16, 16)])
        self.assertEqual((w, h), (512, 256))
        self.assertEqual(len(pos), 8)
        self.assertTrue(all(p is not None for p in pos))


if __name__ == '__main__':
    unittest.main()
