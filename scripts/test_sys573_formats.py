"""Host-only tests for the System 573 tools (no game data needed):
extract_sys573_data.py, sys573_dancer_dump.py, sys573_video.py.

Run: (cd scripts && python3 -m unittest -q test_sys573_formats)
"""
import math
import struct
import unittest

import numpy as np

import extract_sys573_data as X
import sys573_dancer_dump as D
import sys573_video as V


class NameHashTests(unittest.TestCase):
    def test_known_table_hashes(self):
        # values read from the DDR 3rd/4th/5th/MAX file tables
        self.assertEqual(X.name_hash('data/mdb/mdb.bin'), 0xCC8A6B44)
        self.assertEqual(X.name_hash('data/mp3/mp3_tab.bin'), 0x93FF4E63)
        self.assertEqual(X.name_hash('data/chara/afro/afro.cmd'), 0x28BA391B)

    def test_incremental_equals_whole(self):
        h = X.hash_continue(X.hash_continue(0, 'data/chara/'), 'robo/robo.ctx')
        self.assertEqual(h, X.name_hash('data/chara/robo/robo.ctx'))

    def test_only_low_six_bits_count(self):
        # the aliasing the name solver has to live with ('qp' vs 'q0')
        self.assertEqual(X.name_hash('data/chara/qp/qp.cmd'), X.name_hash('data/chara/q0/q0.cmd'))

    def test_layout_solver_recovers_directory_names(self):
        sv = X._solver('data/anime/{X}/{X}.anm', 5)
        for name in ('mbsa1', 'mnor2', 'mfjc1'):
            bits = X._solve(sv, X.name_hash('data/anime/%s/%s.anm' % (name, name)))
            self.assertIn(name, X._spellings(bits, 5))
            self.assertEqual(X._pick_spelling(X._spellings(bits, 5), set(), True), name)

    def test_solver_rejects_underdetermined_lengths(self):
        self.assertIsNone(X._solver('data/mdb/{X}/{X}.csq', 6))  # 36 unknown bits > 32


class LzAndCryptoTests(unittest.TestCase):
    def test_literal_run_near_and_far_copies(self):
        # control 0x0F: token 0 literal run (0xC0 = 8 bytes), token 1 near copy (0x81: dist 2,
        # len 2), token 2 far copy (0x04 0x0A: dist 10, len 4), token 3 end
        stream = bytes([0x0F, 0xC0]) + b'ABCDEFGH' + bytes([0x81, 0x04, 0x0A, 0xFF])
        out, used = X.decode_lz(stream)
        self.assertEqual(out, b'ABCDEFGHGHABCD')
        self.assertEqual(used, len(stream))

    def test_plain_literals(self):
        out, _ = X.decode_lz(bytes([0x04]) + b'xy' + bytes([0xFF]))  # bits 0,1 literal; bit 2 end
        self.assertEqual(out, b'xy')

    def test_truncated_stream_raises(self):
        with self.assertRaises(ValueError):
            X.decode_lz(bytes([0x01, 0xC4, 0x41]))

    def test_decrypt_is_an_involution(self):
        data = bytes(range(256)) * 3
        for k in (0x3A, 0x99, 0):
            self.assertEqual(X.decrypt(X.decrypt(data, k), k), data)

    def test_config_xor(self):
        self.assertEqual(X.config_decrypt(X.config_decrypt(b'conversion /a:/b')), b'conversion /a:/b')


def _key_block(keys, value_shift=0):
    """A .cmm channel key block: one segment referenced by all 8 index slots (shift 8)."""
    blk = bytearray(0x40)
    struct.pack_into('<H', blk, 2, 4)
    blk[0xD] = 4
    struct.pack_into('<H', blk, 6, value_shift)
    for i in range(8):
        struct.pack_into('<H', blk, 0x16 + 4 * i, 0x40)
    for t, v in keys:
        blk += struct.pack('<Hh', t, v)
    return bytes(blk)


class MotionTests(unittest.TestCase):
    def test_linear_and_step_sampling(self):
        blk = _key_block([(0, 0), (960, 960), (1920, -960), (0xFFFF, -960)])
        self.assertEqual(D.sample(blk, 0, 480), 480)
        self.assertEqual(D.sample(blk, 0, 960), 960)
        self.assertEqual(D.sample(blk, 0, 1440), 0)
        self.assertEqual(D.sample(blk, 0, 1919), -958)  # C truncating division
        self.assertEqual(D.sample(blk, 0, 480, step=True), 0)

    def test_value_shift(self):
        self.assertEqual(D.sample(_key_block([(0, 64), (1920, 64), (0xFFFF, 64)], 5), 0, 100), 2)

    def test_rotation_matches_psx_rotmatrixzyx(self):
        # RotMatrixZYX(vx=0, vy, vz) as decompiled from 3rdMIX PLUS (0x8003dbf0), rows m[i][j]
        vy, vz = 700, -1300
        sy, cy = math.sin(vy * D.TAU), math.cos(vy * D.TAU)
        sz, cz = math.sin(vz * D.TAU), math.cos(vz * D.TAU)
        ref = np.array([[cy * cz, -sz, sy * cz], [sz * cy, cz, sy * sz], [-sy, 0, cy]])
        np.testing.assert_allclose(D._rz(vz) @ D._ry(vy), ref, atol=1e-12)
        # RotMatrixX (0x8003de80) left-multiplies: rows 1/2 = c*r1 - s*r2, s*r1 + c*r2
        vx = 333
        c, s = math.cos(vx * D.TAU), math.sin(vx * D.TAU)
        m = ref.copy()
        m[1], m[2] = c * ref[1] - s * ref[2], s * ref[1] + c * ref[2]
        np.testing.assert_allclose(D._rx(vx) @ ref, m, atol=1e-12)

    def test_routines_group_lettered_clips(self):
        clips = {n: None for n in ['hiphop1a', 'hiphop1b', 'hiphop1c', 'hiphop1n', 'sino_a', 'sino_b', 'sino_c',
                                   'normal_00', 'normal_f2', 'ex_b']}
        r = D.routines(clips)
        self.assertEqual(r['hiphop1'], ['hiphop1a', 'hiphop1b', 'hiphop1c', 'hiphop1n'])
        self.assertEqual(r['sino_'], ['sino_a', 'sino_b', 'sino_c'])
        self.assertEqual(r['normal_00'], ['normal_00'])
        self.assertEqual(r['normal_f2'], ['normal_f2'])
        self.assertEqual(r['ex_b'], ['ex_b'])

    def test_hierarchy_is_a_tree_rooted_at_hips(self):
        for j, p in enumerate(D.PARENT):
            self.assertLess(p, j)
        self.assertEqual(D.PARENT[D.HEAD], 12)


class DrawSelectionTests(unittest.TestCase):
    def setUp(self):
        groups = {j: [j] for j in range(13)}
        groups[13] = [13, 14, 15]
        groups[15] = [20, 21, 22]
        self.ch = dict(groups=groups)

    def test_head_draws_base_plus_face(self):
        vis = D.visible_objects(self.ch, {D.HEAD: 2})
        self.assertIn((15, 20), vis)
        self.assertIn((15, 22), vis)

    def test_selectors_clamp_to_the_joints_objects(self):
        vis = dict(D.visible_objects(self.ch, {13: 9}))
        self.assertEqual(vis[13], 15)

    def test_default_pose_shows_first_face(self):
        vis = D.visible_objects(self.ch, {})
        self.assertIn((15, 21), vis)


class GltfTests(unittest.TestCase):
    def test_quaternion_roundtrip(self):
        r = D._rx(500) @ D._rz(-900) @ D._ry(2100)
        x, y, z, w = D._quat(r)
        back = np.array([[1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)],
                         [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
                         [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)]])
        np.testing.assert_allclose(back, r, atol=1e-9)

    def test_axis_conversion_is_proper(self):
        self.assertAlmostEqual(np.linalg.det(D.CONV), 1.0)


def _synthetic_character():
    """16 joints, one flat-colour triangle each; 3 hand shapes on hand_L, head + 2 faces."""
    tri = dict(verts=np.array([[0.0, 0, 0], [10, 0, 0], [0, 10, 0]]), normals=np.array([[0.0, 0, -1]]),
               tris=[((0, 1, 2), (0, 0, 0))], uvs=None, color=(0x40, 0x80, 0xC0), kind=0x30)
    groups = {j: [j] for j in range(16)}
    groups[13] = [13, 16, 17]
    groups[15] = [15, 18, 19]
    objects = [[dict(tri)] for _ in range(20)]
    rest = [np.array([0.0, -1000.0 if j == 0 else 100.0, 0.0]) for j in range(16)]
    return dict(groups=groups, objects=objects, rest=rest, texture=np.zeros((256, 256, 4), np.uint8))


class WorldConversionTests(unittest.TestCase):
    def setUp(self):
        self.ch = _synthetic_character()

    def test_bones_are_parent_first_with_helpers(self):
        bones = D.world_bones(self.ch)
        names = [b[0] for b in bones]
        self.assertEqual(names[:2], ['root', 'hips'])
        self.assertEqual(names[17:], ['hand_L_alt0', 'hand_L_alt1', 'hand_L_alt2', 'head_alt1', 'head_alt2'])
        for name, parent, _j, _oi in bones:
            if parent:
                self.assertLess(names.index(parent), names.index(name))
        self.assertEqual(D.object_bone(self.ch, 15), 'head')      # head base stays on head
        self.assertEqual(D.object_bone(self.ch, 18), 'head_alt1')
        self.assertEqual(set(D.WORLD_ROLE_ALIASES.values()) - set(names), set())

    def test_binds_are_rest_joints_in_game_space(self):
        b = D.world_binds(self.ch)
        np.testing.assert_allclose(b['hips'][3, :3], [0.0, 1.0, 0.0])  # PSX y -1000 mm -> +1 m
        np.testing.assert_allclose(b['thigh_L'][3, :3], [0.0, 0.9, 0.0])
        np.testing.assert_allclose(b['hips'][:3, :3], np.eye(3))

    def test_flat_colours_become_swatches(self):
        img, uv = D.world_atlas(self.ch)
        self.assertEqual(img.shape, (D.ATLAS_H, D.ATLAS_W, 4))
        u, v = uv[(0x40, 0x80, 0xC0)]
        self.assertEqual(tuple(img[int(v), int(u)]), (0x80, 0xFF, 0xFF, 255))  # doubled, clamped

    def test_mesh_winding_is_reversed_and_rigid(self):
        pos, _nrm, uv, bone, tris = D.world_mesh(self.ch)
        self.assertEqual(len(bone), 3 * 20)
        self.assertEqual(tuple(tris[0]), (0, 2, 1))
        self.assertTrue(np.all((uv >= 0) & (uv <= 1)))

    def test_helper_scale_steps(self):
        names = [b[0] for b in D.world_bones(self.ch)]
        idx = {n: i for i, n in enumerate(names)}
        parents = [idx[b[1]] if b[1] else -1 for b in D.world_bones(self.ch)]
        binds = D.world_binds(self.ch)
        rest_local, _ = D.local_pose(dict(last=1920, tracks=[dict(trans=False, chans={})] * 17, data=b''), 0,
                                     self.ch['rest'])
        samples = [[2 * i, [m.copy() for m in rest_local], {13: 0 if i < 3 else 2, D.HEAD: 1}] for i in range(6)]
        spec, frames, _w, vis = D.routine_to_anm_spec(self.ch, samples, names, parents, [binds[n] for n in names])
        self.assertEqual(vis['hand_L_alt0'], [True] * 3 + [False] * 3)
        scale = [t for t in spec['tracks'] if t['kind'] == 10 and t['target'] == idx['hand_L_alt0']][0]
        self.assertEqual(scale['times'], [0, 5, 6, 10])
        self.assertEqual([k[0] for k in scale['keys']], [1.0, 1.0, D.HIDDEN_SCALE, D.HIDDEN_SCALE])
        const = [t for t in spec['tracks'] if t['kind'] == 10 and t['target'] == idx['head_alt1']][0]
        self.assertEqual(const['keys'], [(1.0, 1.0, 1.0)])


def _bits_to_frame(bits, qscale=1):
    bits += '0' * (-len(bits) % 16) + '0' * 64
    words = [int(bits[i:i + 16], 2) for i in range(0, len(bits), 16)]
    return struct.pack('<HHHH', len(words), V.FRAME_MAGIC, qscale, 2) + struct.pack('<%dH' % len(words), *words)


class MdecTests(unittest.TestCase):
    def test_vlc_table_is_prefix_free(self):
        codes = sorted(V.VLC) + ['10', '000001']
        for a in codes:
            for b in codes:
                if a != b:
                    self.assertFalse(b.startswith(a), (a, b))

    def test_dc_only_macroblock(self):
        # six blocks: DC 0 then EOB -> flat mid grey
        frame = _bits_to_frame(('0' * 10 + '10') * 6)
        mbs = V.decode_frame(frame)
        self.assertEqual(len(mbs), 1)
        rgb = V.to_rgb(mbs, (16, 16))
        self.assertTrue(np.all(rgb == 128))

    def test_luma_dc_brightens(self):
        dc = format(64, '010b')  # 64 * QUANT[0]=2 = 128 -> +128/8 in the 8x8 IDCT
        frame = _bits_to_frame(('0' * 10 + '10') * 2 + (dc + '10') * 4)
        rgb = V.to_rgb(V.decode_frame(frame), (16, 16))
        self.assertTrue(np.all(rgb == 144))

    def test_rejects_other_payloads(self):
        with self.assertRaises(ValueError):
            V.decode_frame(b'\0' * 16)


if __name__ == '__main__':
    unittest.main()
