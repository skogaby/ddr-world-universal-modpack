"""Host-only tests for scripts/anm_dump.py's writer (no game data needed): the `.sanm` material
animation shape (type-14 kind-8 float tracks + type-15 material targets,
docs/3d_model_format_research.md §7) round-trips through the parser and evaluates like the
game's per-frame sampler.

Run: (cd scripts && python3 -m unittest -q test_anm_dump)
"""
import unittest

import anm_dump as A


def sanm_spec():
    return dict(
        frame_count=240, flag=1, fps=60,
        material_tracks=[
            dict(kind=8, target=0, sub=2, times=[0, 240], keys=[(0.0,), (-2.0,)]),          # offU ramp
            dict(kind=8, target=0, sub=3, times=[0], keys=[(0.0,)]),                       # offV constant
            dict(kind=8, target=1, sub=4, times=[0, 120, 240], keys=[(1.0,), (0.0,), (1.0,)]),
            dict(kind=8, target=1, sub=5, times=[0, 120, 240], keys=[(1.0,), (0.0,), (1.0,)]),
            dict(kind=8, target=1, sub=6, times=[0, 120, 240], keys=[(1.0,), (0.0,), (1.0,)]),
        ],
        material_targets=[
            dict(identity=0x32984D471C75C80D, identity2=0, hash=0x4DD8548B, flags=0x2000),
            dict(identity=0x42171DC71C75C88D, identity2=0x330A6152C33CE4D4, hash=0x8890DA89, flags=0x2000),
        ],
    )


class SanmWriterTests(unittest.TestCase):
    def test_round_trip_through_the_parser(self):
        spec = sanm_spec()
        data = A.write_anm(spec)
        parsed = A.parse_anm(data)
        self.assertEqual(parsed["header"], dict(frame_count=240, flag=1, fps_or_one=60, d=1))
        self.assertEqual([c["type"] for c in parsed["chunks"]], [14, 15])
        tracks = parsed["chunks"][0]["tracks"]
        self.assertEqual([(t["kind"], t["target"], t["sub"], t["key_count"], t["times"]) for t in tracks],
                         [(8, 0, 2, 2, [0, 240]), (8, 0, 3, 1, [0]), (8, 1, 4, 3, [0, 120, 240]),
                          (8, 1, 5, 3, [0, 120, 240]), (8, 1, 6, 3, [0, 120, 240])])
        self.assertEqual(A.decode_track(data, tracks[0]), [(0.0,), (-2.0,)])
        entries = parsed["chunks"][1]["entries"]
        self.assertEqual([(e["identity"], e["identity2"], e["u32"], e["flags"]) for e in entries],
                         [(0x32984D471C75C80D, 0, 0x4DD8548B, 0x2000), (0x42171DC71C75C88D, 0x330A6152C33CE4D4, 0x8890DA89, 0x2000)])
        self.assertEqual(entries[0]["name"], "jxst00104m")
        # every offset the parser followed stayed inside the file, 16-byte aligned values
        for t in tracks:
            self.assertEqual(t["values"] % 16, 0)
            self.assertLessEqual(t["values"] + 4 * t["key_count"], len(data))
        # spec -> file -> spec -> file is stable
        again = A.write_anm(A.anm_to_spec(parsed))
        self.assertEqual(again, data)

    def test_evaluate_materials_samples_like_the_game(self):
        data = A.write_anm(sanm_spec())
        parsed = A.parse_anm(data)
        m0 = A.evaluate_materials(parsed, 0.0)
        self.assertEqual(m0, {0: {2: 0.0, 3: 0.0}, 1: {4: 1.0, 5: 1.0, 6: 1.0}})
        m = A.evaluate_materials(parsed, 60.0)
        self.assertAlmostEqual(m[0][2], -0.5, places=6)
        self.assertAlmostEqual(m[1][4], 0.5, places=6)
        m = A.evaluate_materials(parsed, 180.0)
        self.assertAlmostEqual(m[0][2], -1.5, places=6)
        self.assertAlmostEqual(m[1][5], 0.5, places=6)
        # past the last key: held
        self.assertAlmostEqual(A.evaluate_materials(parsed, 999.0)[0][2], -2.0, places=6)

    def test_uniform_material_keys_and_the_camera_shape_still_write(self):
        spec = dict(frame_count=3, flag=1, fps=60,
                    material_tracks=[dict(kind=8, target=0, sub=4, keys=[(1.0,), (0.5,), (0.25,), (0.0,)])],
                    material_targets=[dict(identity=1)])
        parsed = A.parse_anm(A.write_anm(spec))
        t = parsed["chunks"][0]["tracks"][0]
        self.assertIsNone(t["times"])
        self.assertAlmostEqual(A.sample_track(parsed["data"], t, 1.5)[0], 0.375)
        self.assertEqual(parsed["chunks"][1]["entries"][0]["flags"], 0x2000)   # the stock default
        cam = dict(frame_count=1, flag=0, fps=60, camera=[dict(kind=1, target=0, keys=[(0, 0, 0, 1)]), None, None, None, None, None])
        self.assertEqual([c["type"] for c in A.parse_anm(A.write_anm(cam))["chunks"]], [4])


if __name__ == "__main__":
    unittest.main()
