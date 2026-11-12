"""Host tests for scripts/fix_vertex_colour_srgb.py (the in-place COLOR0 sRGB undo)."""
import json
import os
import struct
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import fix_vertex_colour_srgb as F  # noqa: E402
import ktmdl_dump as K  # noqa: E402

IDENTITY = [1.0, 0, 0, 0, 0, 1.0, 0, 0, 0, 0, 1.0, 0, 0, 0, 0, 1.0]


def synthetic_model(colours):
    """A one-mesh KTMDL whose vertices carry the given (r, g, b, a) COLOR0 bytes."""
    stride = 16
    vdata = b''.join(struct.pack('<3f', float(i), 0.0, 0.0) + bytes((b, g, r, a))
                     for i, (r, g, b, a) in enumerate(colours))
    n = len(colours)
    tris = [(0, i, i + 1) for i in range(1, n - 1)] or [(0, 0, 0)]
    idx = b''.join(struct.pack('<3H', *t) for t in tris)
    spec = dict(
        bones=[dict(identity=K.pack_identity('root'), bind=list(IDENTITY), inverse_bind=None,
                    aabb_min=[0, 0, 0], aabb_max=[1, 1, 1], parent=-1, flags=None)],
        palette=[0],
        meshes=[dict(flags=1, flags2=0, primitive_raw=1, material=0, node=0, texture_slots=[],
                     texture_slot_count=0, bounding_sphere=[0, 0, 0, 1],
                     elements=[(0, 0, 2, 0x10), (0, 12, 0x12, 0x13)], stride=stride,
                     vertex_count=n, vertex_data=vdata, index_count=3 * len(tris), index_data=idx)],
        info=dict(bbox_max=[1, 1, 1, 1], bbox_min=[0, 0, 0, 1]),
        materials=[dict(identity=K.pack_identity('mat'), shader='mdl_bg_constant_vc', params=[[1.0, 1.0, 1.0, 1.0]])],
        debug=dict(texture_names=[], shader_names=['mdl_bg_constant_vc']),
    )
    return K.write_model(spec)


def colours_of(data):
    m = K.parse_model(data)
    return [tuple(v['COLOR0']) for me in m['meshes'] for v in K.read_vertices(m, me)]


def encoded(v):
    return int(round(255 * F.srgb_encode(v)))


class LutTests(unittest.TestCase):
    def test_fixed_points(self):
        self.assertEqual(F.DECODE_LUT[0], 0)
        self.assertEqual(F.DECODE_LUT[255], 255)

    def test_monotonic(self):
        self.assertTrue(all(a <= b for a, b in zip(F.DECODE_LUT, F.DECODE_LUT[1:])))

    def test_inverts_the_ports_encoding_within_one(self):
        for s in range(256):                        # a port's value s/255 shipped as encoded(s/255)
            self.assertLessEqual(abs(F.DECODE_LUT[encoded(s / 255.0)] - s), 1, s)

    def test_known_values(self):
        self.assertEqual(F.DECODE_LUT[encoded(0.5)], 128)   # the 0.5 -> 188 washout
        self.assertEqual(F.DECODE_LUT[188], 128)


class PatchTests(unittest.TestCase):
    def test_rgb_decoded_alpha_kept(self):
        src = [(255, 255, 255, 255), (0, 0, 0, 0), (188, 124, 46, 153), (255, 188, 0, 77)]
        new, st = F.fix_model_bytes(synthetic_model(src))
        got = colours_of(new)
        self.assertEqual(got[0], (255, 255, 255, 255))
        self.assertEqual(got[1], (0, 0, 0, 0))
        self.assertEqual(got[2], (F.DECODE_LUT[188], F.DECODE_LUT[124], F.DECODE_LUT[46], 153))
        self.assertEqual(got[3], (255, F.DECODE_LUT[188], 0, 77))
        self.assertEqual((st['vertices'], st['changed'], st['channels']), (4, 2, 4))

    def test_only_colour_bytes_change(self):
        data = synthetic_model([(10, 100, 200, 255)] * 3)
        new, _ = F.fix_model_bytes(data)
        self.assertEqual(len(new), len(data))
        diff = [i for i, (a, b) in enumerate(zip(data, new)) if a != b]
        m = K.parse_model(data)
        vb = m['meshes'][0]['vertex_buffers'][0]
        allowed = {vb['data_offset'] + v * 16 + 12 + k for v in range(3) for k in range(3)}
        self.assertTrue(diff and set(diff) <= allowed)

    def test_non_round_trip_is_refused(self):
        data = bytearray(synthetic_model([(10, 20, 30, 255)] * 3))
        data += b'\0' * 16                          # trailing junk the writer would not produce
        with self.assertRaises(ValueError):
            F.fix_model_bytes(bytes(data))


class RootTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = self.tmp.name
        d = os.path.join(self.root, 'Stage 01', 'mapset_x', 'gm_x_bg')
        os.makedirs(d)
        self.path = os.path.join(d, 'gm_x_bg.model')
        with open(self.path, 'wb') as f:
            f.write(synthetic_model([(188, 124, 46, 255)] * 3))
        self.quiet = lambda *_a: None

    def tearDown(self):
        self.tmp.cleanup()

    def test_dry_run_writes_nothing(self):
        before = open(self.path, 'rb').read()
        t = F.fix_root(self.root, dry_run=True, log=self.quiet)
        self.assertEqual(t['changed'], 3)
        self.assertEqual(open(self.path, 'rb').read(), before)
        self.assertFalse(os.path.exists(os.path.join(self.root, F.MANIFEST)))

    def test_second_run_is_a_no_op(self):
        F.fix_root(self.root, log=self.quiet)
        once = open(self.path, 'rb').read()
        self.assertEqual(colours_of(once)[0][:3], (F.DECODE_LUT[188], F.DECODE_LUT[124], F.DECODE_LUT[46]))
        man = json.load(open(os.path.join(self.root, F.MANIFEST)))
        self.assertEqual(list(man), ['Stage 01/mapset_x/gm_x_bg/gm_x_bg.model'])
        t = F.fix_root(self.root, log=self.quiet)
        self.assertEqual((t['skipped'], t['patched']), (1, 0))
        self.assertEqual(open(self.path, 'rb').read(), once)

    def test_changed_after_patch_is_refused(self):
        F.fix_root(self.root, log=self.quiet)
        fresh = synthetic_model([(128, 50, 10, 255)] * 3)   # e.g. re-ported with exact bytes
        with open(self.path, 'wb') as f:
            f.write(fresh)
        t = F.fix_root(self.root, log=self.quiet)
        self.assertEqual(t['refused'], 1)
        self.assertEqual(open(self.path, 'rb').read(), fresh)
        t = F.fix_root(self.root, force=True, log=self.quiet)
        self.assertEqual(t['patched'], 1)

    def test_cli_refuses_to_write_without_legacy_port(self):
        before = open(self.path, 'rb').read()
        self.assertEqual(F.main([self.root]), 2)
        self.assertEqual(open(self.path, 'rb').read(), before)


if __name__ == '__main__':
    unittest.main()
