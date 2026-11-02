"""Host-only tests for the K3D (DDR ULTRAMIX / Universe) `.ddm` revision 0x112 reader in
scripts/ultramix_k3d_dump.py and the XDVDFS image reader in scripts/xdvdfs_extract.py (no game
data needed): synthetic files built by the writers below, which mirror the layouts in the module
docs / docs/dancing_stage_unleashed_dancers_port_feasibility.md sections 13-14.

Run: (cd scripts && python3 -m unittest -q test_k3d_formats)
"""
import os
import struct
import tempfile
import unittest

import numpy as np

import ultramix_k3d_dump as U
import xdvdfs_extract as X


# ---------------------------------------------------------------------------
# writers
# ---------------------------------------------------------------------------
def pad(b, n):
    return b + bytes(n - len(b))


def ddm4(materials, bones, indices, vertices, pad_indices):
    """A revision-0x112 `srdd`: materials = [(name, [(palette, first_index, triangles)])],
    bones = [name], vertices = [16 floats]."""
    out = b'srdd' + struct.pack('<II', U.DDM4_REVISION, len(materials))
    for name, groups in materials:
        out += pad(name.encode(), 0x40) + struct.pack('<II', 3, 0xA0000002) + struct.pack('<17f', *([0.0] * 17))
        out += pad(b'tex_' + name.encode(), 0x100) + struct.pack('<I', len(groups))
        for palette, first, tris in groups:
            out += struct.pack('<I', len(palette)) + pad(struct.pack('<%dH' % len(palette), *palette), 60)
            out += struct.pack('<II', first, tris)
    out += struct.pack('<I', len(bones))
    for i, name in enumerate(bones):
        out += struct.pack('<16f', *np.eye(4).ravel()) + pad(name.encode(), 0x40) + struct.pack('<i', i)
    out += struct.pack('<I', len(indices)) + struct.pack('<%dH' % len(indices), *indices)
    if pad_indices and len(indices) % 2:
        out += bytes(2)
    out += struct.pack('<I', len(vertices))
    for v in vertices:
        out += struct.pack('<16f', *v)
    return out


def vertex(x, slots, weights):
    return [x, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0] + [float(s) for s in slots] + list(weights)


BONES = ['Hips', 'Spine', 'Spine1', 'Spine2']


class Ddm4Test(unittest.TestCase):
    def test_umx4_layout_one_group_per_material(self):
        data = ddm4([('a', [((0, 1), 0, 1)]), ('b', [((2, 3), 3, 1)])], BONES, [0, 1, 2, 3, 4, 5],
                    [vertex(i, (i % 2, -1, -1, -1), (1, 0, 0, 0)) for i in range(6)], pad_indices=False)
        m = U.parse_ddm(data)
        self.assertEqual([mt['palette'] for mt in m['materials']], [(0, 1), (2, 3)])
        self.assertEqual([len(mt['groups']) for mt in m['materials']], [1, 1])
        regs, ws = U.vertex_influences(m)
        self.assertEqual(regs[:, 0].tolist(), [0, 1, 0, 3, 2, 3])
        self.assertTrue(np.allclose(ws.sum(1), 1.0))

    def test_universe_groups_resolve_their_own_palettes(self):
        # one material, two groups: slot 0 means bone 1 in group 0 and bone 3 in group 1
        data = ddm4([('cloth', [((1, 0), 0, 1), ((3, 2), 3, 1)])], BONES, [0, 1, 2, 3, 4, 5],
                    [vertex(i, (0, 1, -1, -1), (0.75, 0.25, 0, 0)) for i in range(6)], pad_indices=False)
        m = U.parse_ddm(data)
        mt = m['materials'][0]
        self.assertEqual((mt['first_index'], mt['triangles']), (0, 2))
        regs, ws = U.vertex_influences(m)
        self.assertEqual(regs[:3, :2].tolist(), [[1, 0]] * 3)
        self.assertEqual(regs[3:, :2].tolist(), [[3, 2]] * 3)
        self.assertTrue(np.allclose(ws[:, :2], [0.75, 0.25]))

    def test_odd_index_count_is_padded(self):
        tris = [0, 1, 2, 2, 1, 3, 3, 1, 4]   # 9 indices: 2 bytes of padding follow
        data = ddm4([('a', [((0,), 0, 3)])], BONES, tris,
                    [vertex(i, (0, -1, -1, -1), (1, 0, 0, 0)) for i in range(5)], pad_indices=True)
        m = U.parse_ddm(data)
        self.assertEqual(list(m['indices']), tris)
        self.assertEqual(m['vertex_count'], 5)

    def test_shared_vertex_between_groups_is_refused(self):
        data = ddm4([('a', [((0,), 0, 1), ((1,), 3, 1)])], BONES, [0, 1, 2, 2, 3, 4],
                    [vertex(i, (0, -1, -1, -1), (1, 0, 0, 0)) for i in range(5)], pad_indices=False)
        with self.assertRaises(ValueError):
            U.vertex_influences(U.parse_ddm(data))

    def test_gap_between_groups_is_refused(self):
        data = ddm4([('a', [((0,), 0, 1), ((1,), 6, 1)])], BONES, list(range(9)),
                    [vertex(i, (0, -1, -1, -1), (1, 0, 0, 0)) for i in range(9)], pad_indices=True)
        with self.assertRaises(ValueError):
            U.parse_ddm(data)

    def test_zero_normal_borrows_its_neighbours(self):
        verts = [vertex(i, (0, -1, -1, -1), (1, 0, 0, 0)) for i in range(3)]
        verts[1][3:6] = [0.0, 0.0, 0.0]
        data = ddm4([('a', [((0,), 0, 1)])], BONES, [0, 1, 2], verts, pad_indices=True)
        _, nrm, _, _, _ = U.game_mesh(U.parse_ddm(data))
        self.assertTrue(np.isfinite(nrm).all())
        self.assertTrue(np.allclose(nrm[1], nrm[0]))


class HierarchyTest(unittest.TestCase):
    def test_universe_table_is_picked_by_its_joints(self):
        h = U.k3d_hierarchy(['Hips', 'Spine', 'leftForearm', 'leftForearmRoll', 'leftHand', 'leftArm',
                             'leftShoulder', 'Spine1', 'Spine2'])
        self.assertEqual(h['leftHand'], 'leftForearmRoll')
        self.assertEqual(U.k3d_hierarchy(['Hips', 'leftArm', 'leftForearm', 'leftHand'])['leftHand'], 'leftForearm')

    def test_universe_table_is_closed(self):
        for child, parent in U.HIERARCHY_UNIVERSE.items():
            self.assertTrue(parent is None or parent in U.HIERARCHY_UNIVERSE, (child, parent))
        self.assertEqual(sum(1 for p in U.HIERARCHY_UNIVERSE.values() if p is None), 1)


# ---------------------------------------------------------------------------
# XDVDFS
# ---------------------------------------------------------------------------
def xdvdfs_dir(entries):
    """One directory sector from [(name, sector, size, attr)] as a right-leaning tree."""
    out = b''
    offs = []
    for name, _, _, _ in entries:
        offs.append(len(out))
        out += bytes((14 + len(name) + 3) // 4 * 4)
    buf = bytearray(out)
    for i, (name, sector, size, attr) in enumerate(entries):
        right = offs[i + 1] // 4 if i + 1 < len(entries) else 0
        struct.pack_into('<HHIIBB', buf, offs[i], 0, right, sector, size, attr, len(name))
        buf[offs[i] + 14:offs[i] + 14 + len(name)] = name.encode()
    return bytes(buf) + b'\xff' * (X.SECTOR - len(buf))


def xdvdfs_image(base):
    """Volume at sector 32; root at 33 (a/ dir + b.bin); a/ at 34 (c.txt); data at 35 / 36."""
    img = bytearray(base + 40 * X.SECTOR)
    vol = X.MAGIC + struct.pack('<II', 33, X.SECTOR)
    img[base + 32 * X.SECTOR:base + 32 * X.SECTOR + len(vol)] = vol
    root = xdvdfs_dir([('a', 34, X.SECTOR, X.ATTR_DIRECTORY), ('b.bin', 35, 5, 0)])
    sub = xdvdfs_dir([('c.txt', 36, 3, 0)])
    img[base + 33 * X.SECTOR:base + 34 * X.SECTOR] = root
    img[base + 34 * X.SECTOR:base + 35 * X.SECTOR] = sub
    img[base + 35 * X.SECTOR:base + 35 * X.SECTOR + 5] = b'hello'
    img[base + 36 * X.SECTOR:base + 36 * X.SECTOR + 3] = b'abc'
    return bytes(img)


class XdvdfsTest(unittest.TestCase):
    def check(self, base):
        with tempfile.TemporaryDirectory() as d:
            path = os.path.join(d, 'disc.iso')
            with open(path, 'wb') as f:
                f.write(xdvdfs_image(base))
            image = X.Image(path)
            self.assertEqual(image.base, base)
            listing = [(p, s, n, isdir) for p, s, n, isdir in image.walk()]
            self.assertEqual([p for p, _, _, _ in listing], ['a', 'a/c.txt', 'b.bin'])
            self.assertEqual(image.read(35, 5), b'hello')
            image.f.close()

    def test_plain_xiso(self):
        self.check(0)

    def test_xgd2_partition_offset(self):
        # a sparse image: the reader only touches the partition, so the hole can stay small by
        # shifting the expectation instead of writing 254 MB
        X.PARTITION_OFFSETS, saved = (0x4000, 0), X.PARTITION_OFFSETS
        try:
            self.check(0x4000)
        finally:
            X.PARTITION_OFFSETS = saved


if __name__ == '__main__':
    unittest.main()
