"""Host-only tests for scripts/tzm_dump.py (no game data needed): synthetic TZM packs built by
the writers below, which mirror the layouts in the module doc / docs/ps2_ddr_filedata_research.md
§7.4.

Run: (cd scripts && python3 -m unittest -q test_tzm_dump)
"""
import math
import struct
import unittest

import numpy as np

import tzm_dump as Z
from test_ps2_ddr_formats import tgcd, tgcd_literal


def tgcd_store(data):
    """A store-only TGCD stream (literal runs of <= 0x7FF0 bytes)."""
    ops = [tgcd_literal(data[i:i + 0x7FF0]) for i in range(0, len(data), 0x7FF0)] or [tgcd_literal(b'')]
    return tgcd(ops, len(data))


def pad(b, n):
    return b + bytes(n - len(b))


# ---------------------------------------------------------------------------
# writers
# ---------------------------------------------------------------------------
def tzm(chunks):
    """A TZM container: directory of 0x5C entries, chunks on 0x800 sectors."""
    head = struct.pack('<III', Z.TZM_MAGIC, 0x10000, len(chunks))
    dir_size = 12 + Z.DIR_ENTRY * len(chunks)
    sector = (dir_size + Z.SECTOR - 1) // Z.SECTOR
    entries, body = b'', b''
    for name, data in chunks:
        nsec = (len(data) + Z.SECTOR - 1) // Z.SECTOR
        entries += pad(name.encode(), 0x40) + bytes(16) + struct.pack('<III', sector, nsec, len(data))
        body += pad(data, nsec * Z.SECTOR)
        sector += nsec
    return pad(head + entries, (dir_size + Z.SECTOR - 1) // Z.SECTOR * Z.SECTOR) + body


def imagelist(names):
    return struct.pack('<II', 0x9135AB5C, len(names)) + b''.join(pad(n.encode(), 0x50) for n in names)


def texture(name, w, h, clut, indices, bpp=8):
    """A texture chunk: header, LINEAR CLUT (16 or 256 RGBA32 entries), TGCD index stream."""
    if bpp == 4:
        flat = [int(x) for x in np.asarray(indices).ravel()]
        stream = bytes((flat[i] & 0xF) | (flat[i + 1] << 4) for i in range(0, len(flat), 2))
    else:
        stream = bytes(int(x) for x in np.asarray(indices).ravel())
    clut_bytes = b''.join(struct.pack('<4B', *c) for c in clut)
    tg = tgcd_store(stream)
    head = struct.pack('<I', 0x1234) + pad(name.encode(), 0x40) + bytes(0x10) + struct.pack('<HH', w, h)
    head += bytes(4) + struct.pack('<5I', len(clut_bytes) + len(tg), len(clut_bytes), len(tg), w * bpp // 8, 1)
    assert len(head) == 0x70
    return head + clut_bytes + tg


def node(name, T=(0, 0, 0), R=(0, 0, 0), S=(1, 1, 1), T2=(0, 0, 0), R2=(0, 0, 0), S2=(1, 1, 1),
         parent=-1, first_child=-1, next_sibling=-1, prev_sibling=-1, mesh_slot=None):
    """A bone record, or (mesh_slot given, -1 = none) an object record."""
    rec = pad(name.encode(), 0x40) + bytes(0x10)
    for v in (T, R, S, T2, R2, S2):
        rec += struct.pack('<4f', *v, 0.0)
    if mesh_slot is None:
        rec += struct.pack('<8i', parent, first_child, next_sibling, prev_sibling, 0, 3, 0, 0)
    else:
        rec += struct.pack('<2i', mesh_slot, -0x80000000) + struct.pack('<6i', parent, first_child, next_sibling, prev_sibling, 0, 0)
    assert len(rec) == Z.NODE_SIZE
    return rec


def mesh(fmt, verts, palette=(), next_mesh=-1, material='DefaultLib.Material'):
    """A mesh record from per-vertex dicts (pos, [weights, bones], normal, [colour], uv, flag)."""
    rows = []
    for v in verts:
        r = struct.pack('<4f', *v['pos'], 0.0)
        if fmt & Z.FMT_WEIGHTS:
            w = list(v.get('weights', (1.0, 0.0, 0.0)))
            b = list(v.get('bones', (0, 0, 0, 0)))
            r += struct.pack('<3f', *w) + struct.pack('<4B', *b)
        r += struct.pack('<4f', *v.get('normal', (0.0, 1.0, 0.0)), 0.0)
        if fmt & Z.FMT_COLOUR:
            r += struct.pack('<4f', *v.get('colour', (128.0, 128.0, 128.0, 128.0)))
        r += struct.pack('<3f', *v.get('uv', (0.0, 0.0)), 1.0) + struct.pack('<I', v.get('flag', 0))
        rows.append(r)
    stride = len(rows[0])
    skinned = bool(fmt & Z.FMT_WEIGHTS)
    pal = list(palette) + [-1] * (9 - len(palette))
    hdr = struct.pack('<24i', -1, fmt, len(verts), stride, stride * len(verts), 4, len(verts) - 2,
                      len(verts) - 2 if skinned else 3 * (len(verts) - 2), 2 if skinned else 0,
                      2 if skinned else 0, *pal, len(palette), next_mesh, 0, 0, 0)
    return pad(material.encode(), 0x18) + bytes(0x38) + hdr + b''.join(rows) + (bytes(2 * stride) if skinned else b'')


def model(objects, object_meshes, meshes, bones):
    """A MODEL chunk: 0x30 header with (count, offset + 0x30) per section, then the TGCD."""
    root = pad(b'globalSRT', 0x40) + bytes(0x18) + b'\xCC' * 8
    body = root
    off_objects = len(body)
    body += b''.join(objects)
    off_om = len(body)
    body += b''.join(struct.pack('<4i', m, -1, 0, 0) for m in object_meshes)
    off_tab = len(body)
    tab_size = 4 * len(meshes)
    offs, cur = [], tab_size
    for m in meshes:
        offs.append(cur)
        cur += len(m)
    body += struct.pack('<%dI' % len(meshes), *offs) if meshes else b''
    body += b''.join(meshes)
    off_bones = len(body)
    body += b''.join(bones)
    sec = [(1, 0), (len(objects), off_objects), (len(object_meshes), off_om), (len(meshes), off_tab), (len(bones), off_bones)]
    head = struct.pack('<II', 0xF563B872, 1) + b''.join(struct.pack('<II', c, o + Z.MODEL_HEADER) for c, o in sec)
    return head + tgcd_store(body)


def track(name, kind, keys, flag=0):
    k = np.asarray(keys, dtype='<f4')
    return pad(name.encode(), 0x50) + struct.pack('<5I', kind, 6, flag, k.nbytes, len(k)) + k.tobytes()


def motion(records):
    """A MOTION chunk from [(name, first, last, fps, [track bytes])]: headers first, then each
    record's offset table + tracks in record order."""
    heads, datas = [], []
    table = Z.MOTION_RECORD * len(records)
    for name, first, last, fps, tracks in records:
        offs, cur = [], 4 * len(tracks)
        for t in tracks:
            offs.append(cur)
            cur += len(t)
        data = struct.pack('<%dI' % len(tracks), *offs) + b''.join(tracks)
        n30 = (last - first + 1) // 2 if fps == 30 else last - first + 1
        head = pad(name.encode(), 0x50) + struct.pack('<I5i2f', len(tracks), first, last, 1, n30, 0, fps, fps / 60.0)
        head += struct.pack('<3I', table + Z.MOTION_HEADER, len(data), 1)
        assert len(head) == Z.MOTION_RECORD
        heads.append(head)
        datas.append(data)
        table += len(data)
    return struct.pack('<II', 0xF563B872, len(records)) + tgcd_store(b''.join(heads) + b''.join(datas))


def quat_z(a):
    return (0.0, 0.0, math.sin(a / 2), math.cos(a / 2))


def materiallist(mats):
    """A MATERIALLIST chunk from [(name, [texture names])]: 0x10 header + 1020-byte records."""
    out = struct.pack('<II', 0x06849AB0, len(mats)) + b'\xCD' * 8
    for name, texs in mats:
        rec = bytearray(1020)
        rec[0:len(name)] = name.encode()
        for i, o in enumerate((0x50, 0x370)):
            if i < len(texs):
                rec[o:o + len(texs[i])] = texs[i].encode()
        struct.pack_into('<4f', rec, 0xA0, 1.0, 0.5, 0.25, 1.0)
        out += bytes(rec)
    return out


# ---------------------------------------------------------------------------
# fixtures: a two-bone "arm" with one skinned strip mesh and one rigid coloured mesh
# ---------------------------------------------------------------------------
def arm_model():
    objects = [node('globalSRT', mesh_slot=-1, first_child=1), node('body', mesh_slot=0, parent=0, first_child=2),
               node('gem', mesh_slot=1, parent=1, T=(0, 0, 2), R=(0, 0, math.pi / 2))]
    bones = [node('globalSRT', first_child=1),
             node('Hip', T=(0, 10, 0), T2=(0, 1, 0), parent=0, first_child=2),      # bind frame 9 below the root frame
             node('Spine', T=(2, 0, 0), R=(0, 0, math.pi / 2), T2=(0, 3, 0), R2=(0, 0, math.pi / 2), parent=1)]
    # a strip of two triangles then an ADC restart with one more triangle
    verts = [dict(pos=(0, 1, 0), bones=(0, 1, 0, 0), weights=(1.0, 0.0, 0.0), uv=(0.0, 0.0), normal=(0, 0, 1)),
             dict(pos=(1, 1, 0), bones=(0, 1, 0, 0), weights=(0.5, 0.5, 0.0), uv=(0.5, 0.0), normal=(0, 0, 1)),
             dict(pos=(0, 3, 0), bones=(1, 0, 0, 0), weights=(1.0, 0.0, 0.0), uv=(0.0, 1.0), normal=(0, 0, 1)),
             dict(pos=(1, 3, 0), bones=(1, 0, 0, 0), weights=(1.0, 0.0, 0.0), uv=(0.5, 1.0), normal=(0, 0, 1)),
             dict(pos=(5, 1, 0), bones=(0, 0, 0, 0), uv=(0.6, 0.0), normal=(0, 0, -1), flag=Z.ADC),
             dict(pos=(6, 1, 0), bones=(0, 0, 0, 0), uv=(0.7, 0.0), normal=(0, 0, -1), flag=Z.ADC),
             dict(pos=(5, 2, 0), bones=(0, 0, 0, 0), uv=(0.6, 0.5), normal=(0, 0, -1))]
    skinned = mesh(0x11A, verts, palette=(1, 2))
    gem = mesh(0x152, [dict(pos=(0, 0, 0), colour=(64, 128, 0, 76.8)), dict(pos=(1, 0, 0), colour=(64, 128, 0, 76.8)),
                       dict(pos=(0, 1, 0), colour=(64, 128, 0, 76.8))])
    return model(objects, [0, 1], [skinned, gem], bones)


def arm_motion():
    keys_rot = [quat_z(0.0), quat_z(0.5), quat_z(1.0)]
    keys_trn = [(0, 10, 0, 0), (0, 10.5, 0, 0), (0, 11, 0, 0)]
    rec = ('CLIP', 1, 6, 30.0, [track('globalSRT', Z.KIND_ROTATION, [(0, 0, 0, 1)], flag=2),
                                track('Hip', Z.KIND_ROTATION, keys_rot), track('Hip', Z.KIND_TRANSLATION, keys_trn),
                                track('Spine', Z.KIND_ROTATION, [quat_z(math.pi / 2)] * 3)])
    cam = ('CAM', 1, 3, 60.0, [track('cam', Z.KIND_SRT, [(1, 1, 1, 0, 0, 0, 1, 0, 5, 0)] * 3)])
    return motion([rec, cam])


def arm_pack():
    clut = [(i, 255 - i, 0, 128 if i else 0) for i in range(256)]
    idx = np.arange(64, dtype=np.uint8).reshape(8, 8)
    small = [(0, 0, 0, 128)] * 15 + [(255, 255, 255, 128)]
    return tzm([('IMAGELIST', imagelist(['tex_arm_png', 'tex_flag_png'])),
                ('tex_arm_png', texture('tex_arm_png', 8, 8, clut, idx)),
                ('tex_flag_png', texture('tex_flag_png', 4, 2, small, [[0, 15, 0, 15], [15, 0, 15, 0]], bpp=4)),
                ('MODEL', arm_model()), ('MOTION', arm_motion())])


# ---------------------------------------------------------------------------
class ContainerTests(unittest.TestCase):
    def test_directory_and_sectors(self):
        chunks = Z.parse_tzm(arm_pack())
        self.assertEqual([n for n, _ in chunks], ['IMAGELIST', 'tex_arm_png', 'tex_flag_png', 'MODEL', 'MOTION'])
        self.assertEqual(Z.parse_imagelist(dict(chunks)['IMAGELIST']), ['tex_arm_png', 'tex_flag_png'])

    def test_bad_magic_and_overrun_are_rejected(self):
        with self.assertRaises(ValueError):
            Z.parse_tzm(b'\0' * 64)
        data = bytearray(arm_pack())
        struct.pack_into('<I', data, 12 + 0x58, 0x7FFFFFFF)  # first chunk's size
        with self.assertRaises(ValueError):
            Z.parse_tzm(bytes(data))

    def test_chunk_payload_without_tgcd(self):
        head, dec = Z.chunk_payload(b'RAW' + bytes(64))
        self.assertIsNone(dec)


class TextureTests(unittest.TestCase):
    def test_8bpp_linear_clut_and_alpha(self):
        tex = Z.textures_of(Z.parse_tzm(arm_pack()))['tex_arm_png']
        self.assertEqual((tex['width'], tex['height'], tex['bpp']), (8, 8, 8))
        self.assertEqual(tex['indices'][1, 2], 10)
        self.assertEqual(tex['rgba'][1, 2].tolist(), [10, 245, 0, 255])  # alpha 0x80 -> 255
        self.assertEqual(tex['rgba'][0, 0, 3], 0)

    def test_4bpp_nibbles_low_first(self):
        tex = Z.textures_of(Z.parse_tzm(arm_pack()))['tex_flag_png']
        self.assertEqual(tex['bpp'], 4)
        self.assertEqual(tex['indices'].tolist(), [[0, 15, 0, 15], [15, 0, 15, 0]])
        self.assertEqual(tex['rgba'][0, 1].tolist(), [255, 255, 255, 255])


class ModelTests(unittest.TestCase):
    def setUp(self):
        self.m = Z.parse_model(dict(Z.parse_tzm(arm_pack()))['MODEL'])

    def test_nodes_are_name_first_records_with_links(self):
        self.assertEqual([o['name'] for o in self.m['objects']], ['globalSRT', 'body', 'gem'])
        self.assertEqual([b['name'] for b in self.m['bones']], ['globalSRT', 'Hip', 'Spine'])
        hip = self.m['bones'][1]
        self.assertEqual((hip['parent'], hip['first_child']), (0, 2))
        np.testing.assert_allclose(hip['T'], (0, 10, 0))
        np.testing.assert_allclose(hip['T2'], (0, 1, 0))
        self.assertEqual(self.m['root'], 'globalSRT')

    def test_object_mesh_chain_and_formats(self):
        self.assertEqual(self.m['object_meshes'], [0, 1])
        self.assertEqual(self.m['mesh_object'], {0: 1, 1: 2})
        self.assertEqual([(o['mesh_slot'], o['parent']) for o in self.m['objects']], [(-1, -1), (0, 0), (1, 1)])
        skinned, gem = self.m['meshes']
        self.assertEqual((skinned['format'], skinned['stride'], skinned['count'], skinned['palette']), (0x11A, 64, 7, [1, 2]))
        self.assertEqual((gem['format'], gem['stride'], gem['count']), (0x152, 64, 3))
        self.assertIsNone(gem['weights'])
        np.testing.assert_allclose(gem['colours'][0], (0.5, 1.0, 0.0, 0.6))
        self.assertEqual(skinned['end'] + 0, self.m['sections'][3][1] - Z.MODEL_HEADER + 8 + len(mesh(0x11A, [dict(pos=(0, 0, 0))] * 7, palette=(1, 2))))

    def test_strip_triangles_honour_the_adc_restart(self):
        tris = self.m['meshes'][0]['triangles'].tolist()
        self.assertEqual(tris, [[0, 1, 2], [1, 2, 3], [4, 5, 6]])
        self.assertEqual(Z.strip_triangles([0, 0]).shape, (0, 3))

    def test_weights_and_palette_slots(self):
        skin = Z.mesh_skin(self.m, 0)
        self.assertEqual(skin[0], [(1, 1.0)])
        self.assertEqual(skin[1], [(1, 0.5), (2, 0.5)])
        self.assertEqual(skin[2], [(2, 1.0)])
        self.assertEqual(Z.mesh_skin(self.m, 1), [[], [], []])

    def test_slot_beyond_palette_is_rejected(self):
        bad = mesh(0x11A, [dict(pos=(0, 0, 0), bones=(5, 0, 0, 0))] * 3, palette=(1,))
        chunk = model([node('globalSRT', mesh_slot=-1), node('b', mesh_slot=0, parent=0)], [0], [bad],
                      [node('globalSRT'), node('Hip', parent=0)])
        with self.assertRaises(ValueError):
            Z.parse_model(chunk)

    def test_object_transform_reaches_the_mesh(self):
        pos, nrm = Z.mesh_bind_positions(self.m, 1)  # gem: T (0,0,2), Rz 90 deg under the identity body
        np.testing.assert_allclose(pos[1], (0, 1, 2), atol=1e-6)
        np.testing.assert_allclose(nrm[0], (-1, 0, 0), atol=1e-6)

    def test_object_hierarchy_composes_like_a_stage(self):
        # a layer root, a rotated group, a child offset along the group's local X
        objects = [node('add', mesh_slot=-1, first_child=1),
                   node('grp', mesh_slot=-1, parent=0, R=(0, 0, math.pi / 2), first_child=2),
                   node('leaf', mesh_slot=0, parent=1, T=(3, 0, 0), S=(2, 2, 2))]
        leaf = mesh(0x152, [dict(pos=(1, 0, 0)), dict(pos=(0, 0, 0)), dict(pos=(0, 0, 1))])
        m = Z.parse_model(model(objects, [0], [leaf], []))
        self.assertEqual(m['mesh_object'], {0: 2})
        pos, _ = Z.mesh_bind_positions(m, 0)
        np.testing.assert_allclose(pos[0], (0, 5, 0), atol=1e-6)  # (1,0,0)*2 + (3,0,0) -> Rz90 -> (0, 5, 0)

    def test_consistent_winding_flips_against_the_normals(self):
        pos = np.array([(0, 0, 0), (1, 0, 0), (0, 1, 0)], dtype=float)
        nrm = np.array([(0, 0, -1)] * 3, dtype=float)
        tris, flipped = Z.consistent_winding(pos, nrm, np.array([[0, 1, 2]]))
        self.assertEqual((tris.tolist(), flipped), ([[0, 2, 1]], 1))


class MotionTests(unittest.TestCase):
    def setUp(self):
        self.recs = Z.parse_motion(dict(Z.parse_tzm(arm_pack()))['MOTION'])

    def test_two_records_headers_first_then_tables(self):
        self.assertEqual([r['name'] for r in self.recs], ['CLIP', 'CAM'])
        clip, cam = self.recs
        self.assertEqual((clip['first'], clip['last'], clip['fps'], clip['ntracks']), (1, 6, 30.0, 4))
        self.assertEqual([(t['name'], t['kind'], t['flag'], t['keys'].shape) for t in clip['tracks']],
                         [('globalSRT', 2003, 2, (1, 4)), ('Hip', 2003, 0, (3, 4)), ('Hip', 2004, 0, (3, 4)), ('Spine', 2003, 0, (3, 4))])
        self.assertEqual(cam['tracks'][0]['keys'].shape, (3, 10))
        self.assertEqual(Z.clip_frames(clip), 3)

    def test_clip_worlds_compose_the_hierarchy(self):
        m = Z.parse_model(dict(Z.parse_tzm(arm_pack()))['MODEL'])
        W = Z.clip_worlds(m['bones'], self.recs[0], 2)
        np.testing.assert_allclose(W[1][:3, 3], (0, 11, 0), atol=1e-6)             # Hip translation key 2
        # Spine: local T (2, 0, 0) rotated by the Hip's Rz(1.0) then its own Rz(90) on top
        np.testing.assert_allclose(W[2][:3, 3], (2 * math.cos(1.0), 11 + 2 * math.sin(1.0), 0), atol=1e-6)
        np.testing.assert_allclose(W[2][:3, :3], Z.rot_z(1.0 + math.pi / 2), atol=1e-6)


class StageTests(unittest.TestCase):
    def test_materiallist_and_truncated_mesh_names(self):
        ml = Z.parse_materiallist(materiallist([('DefaultLib.Scene_Material', []),
                                                ('DefaultLib.add_tex1_uvani', ['st003_01_png']),
                                                ('DefaultLib.glo_tex1', ['st003_02t_png', 'st003_02g_png'])]))
        self.assertEqual(ml['DefaultLib.glo_tex1']['textures'], ['st003_02t_png', 'st003_02g_png'])
        self.assertEqual(ml['DefaultLib.Scene_Material']['textures'], [])
        np.testing.assert_allclose(ml['DefaultLib.glo_tex1']['diffuse'], (1.0, 0.5, 0.25, 1.0))
        self.assertEqual(Z.material_for(ml, 'DefaultLib.add_tex1_uvan')['textures'], ['st003_01_png'])  # 0x18-byte field
        self.assertIsNone(Z.material_for(ml, 'DefaultLib.nothing'))

    def test_srt_keys_and_animated_names(self):
        self.assertEqual(Z.srt_key([2, 3, 4, 0, 0, 0, 1, 5, 6, 7]), ([2, 3, 4], [0, 0, 0, 1], [5, 6, 7]))
        self.assertEqual(Z.srt_key([0, 0, 0, 1, 5, 6, 7]), ([1.0, 1.0, 1.0], [0, 0, 0, 1], [5, 6, 7]))
        self.assertEqual(Z.srt_key([5, 6, 7, 0]), ([1.0, 1.0, 1.0], [0.0, 0.0, 0.0, 1.0], [5, 6, 7]))
        rec = ('stage', 1, 4, 60.0, [track('spin', 3, [(1, 1, 1) + quat_z(0.0) + (0, 0, 0), (1, 1, 1) + quat_z(1.0) + (0, 0, 0)]),
                                     track('still', 2, [(0, 0, 0, 1, 1, 2, 3)] * 2),
                                     track('DefaultLib.tex', 503, [(1.0,)], flag=2)])
        r = Z.parse_motion(motion([rec]))[0]
        self.assertEqual(sorted(Z.object_tracks(r)), ['spin', 'still'])
        self.assertEqual(Z.animated_names(r), {'spin'})

    def test_object_worlds_at_follows_the_tracks(self):
        objects = [node('add', mesh_slot=-1, first_child=1),
                   node('spin', mesh_slot=-1, parent=0, T=(0, 0, 9), first_child=2),
                   node('leaf', mesh_slot=0, parent=1, T=(2, 0, 0))]
        m = Z.parse_model(model(objects, [0], [mesh(0x152, [dict(pos=(0, 0, 0))] * 3)], []))
        rec = ('stage', 1, 2, 60.0, [track('spin', 3, [(1, 1, 1) + quat_z(0.0) + (0, 0, 9), (2, 2, 2) + quat_z(math.pi / 2) + (0, 0, 9)])])
        r = Z.parse_motion(motion([rec]))[0]
        w0 = Z.object_worlds_at(m, r, 0)
        np.testing.assert_allclose(w0[2][:3, 3], (2, 0, 9), atol=1e-9)
        w1 = Z.object_worlds_at(m, r, 1)
        np.testing.assert_allclose(w1[2][:3, 3], (0, 4, 9), atol=1e-5)   # scaled 2 then turned 90 deg about Z (f32 keys)
        u1 = Z.object_worlds_at(m, r, 1, unit_scale=True)
        np.testing.assert_allclose(u1[2][:3, 3], (0, 2, 9), atol=1e-5)
        np.testing.assert_allclose(u1[2][:3, :3], Z.rot_z(math.pi / 2), atol=1e-5)
        self.assertEqual(Z.object_chain(m, 2), [0, 1, 2])


class MathTests(unittest.TestCase):
    def test_euler_xyz_matches_quaternion(self):
        r = (0.3, -0.7, 1.1)
        m = Z.euler_xyz(r)
        q = Z.rowmat_to_quat(m.T)
        np.testing.assert_allclose(Z.quat_mat(q), m, atol=1e-9)

    def test_skinning_at_bind_is_the_object_space_mesh(self):
        m = Z.parse_model(dict(Z.parse_tzm(arm_pack()))['MODEL'])
        binds = [Z.bind_matrix(b) for b in m['bones']]
        parts = Z.skin_positions(m, binds, binds)
        np.testing.assert_allclose(parts[0][0], m['meshes'][0]['positions'], atol=1e-9)

    def test_game_space_shift_scale_and_rigid_reframing(self):
        m = Z.parse_model(dict(Z.parse_tzm(arm_pack()))['MODEL'])
        self.assertEqual(Z.character_scale(m), 1.0)
        np.testing.assert_allclose(Z.bind_shift(m), (0, 0, 0), atol=1e-6)  # the gem's lowest vertex is on y = 0 already
        s = Z.unit_scale(m)
        pos, nrm, uv, col, weights, tris, src = Z.game_mesh(m)
        np.testing.assert_allclose(pos[0], np.array((0, 1, 0)) * s, atol=1e-9)
        self.assertEqual(weights[1], [('Hip', 0.5), ('Spine', 0.5)])
        self.assertEqual(col.shape, (10, 4))
        np.testing.assert_allclose(col[0], (1, 1, 1, 1))
        self.assertEqual(sorted(src.tolist()), [0, 0, 0, 1])
        names, index = Z.rig_bones(m)
        self.assertEqual(names, [('globalSRT', None), ('Hip', 'globalSRT'), ('Spine', 'Hip')])
        binds = Z.game_bind_matrices(m)
        np.testing.assert_allclose(binds['Hip'][3, :3], np.array((0, 1, 0)) * s, atol=1e-9)
        lifted = Z.parse_model(dict(Z.parse_tzm(arm_pack()))['MODEL'])
        lifted['meshes'][0]['positions'][:, 1] -= 3.0  # lowest vertex 2 units under the floor -> lifted by 2
        np.testing.assert_allclose(Z.bind_shift(lifted), (0, 2, 0), atol=1e-9)
        np.testing.assert_allclose(Z.game_bind_matrices(lifted)['Hip'][3, :3], np.array((0, 3, 0)) * s, atol=1e-9)
        # a rigid re-framing of every target bind leaves the skinning product unchanged
        rot = np.eye(4)
        rot[:3, :3] = Z.rot_z(0.4).T
        order = [n for n, _ in names]
        target = [rot @ binds[n] for n in order]
        rec = Z.parse_motion(dict(Z.parse_tzm(arm_pack()))['MOTION'])[0]
        worlds = Z.clip_game_worlds(m, rec, order, target)
        self.assertEqual(worlds.shape, (3, 3, 4, 4))
        plain = Z.clip_game_worlds(m, rec, order, [binds[n] for n in order])
        for f in range(3):
            for b in range(3):
                np.testing.assert_allclose(np.linalg.inv(target[b]) @ worlds[f, b],
                                           np.linalg.inv(binds[order[b]]) @ plain[f, b], atol=1e-9)
        spec, _ = Z.clip_to_anm_spec(m, rec, order, [-1, 0, 1], target)
        self.assertEqual(spec['frame_count'], 4)  # 3 keys at 30 fps -> frames 0, 2, 4
        kinds = sorted({t['kind'] for t in spec['tracks']})
        self.assertEqual(kinds, [0x1C, 0x1D])
        self.assertEqual(len(spec['tracks']), 6)

    def test_scale_node_folds_into_the_unit_scale_and_drops_out_of_the_rig(self):
        bones = [node('globalSRT', first_child=1), node('SCALE', S=(0.6, 0.6, 0.6), S2=(0, 0, 0), parent=0, first_child=2),
                 node('Hip', T=(0, 10, 0), T2=(0, 1, 0), parent=1)]
        chunk = model([node('globalSRT', mesh_slot=-1), node('b', mesh_slot=0, parent=0)], [0],
                      [mesh(0x11A, [dict(pos=(0, 0, 0), bones=(0, 0, 0, 0))] * 3, palette=(2,))], bones)
        m = Z.parse_model(chunk)
        self.assertAlmostEqual(Z.character_scale(m), 0.6)
        self.assertAlmostEqual(Z.unit_scale(m), Z.GAME_SCALE * 0.6)
        self.assertEqual(Z.rig_bones(m)[0], [('globalSRT', None), ('Hip', 'globalSRT')])
        np.testing.assert_allclose(Z.bind_matrix(m['bones'][1]), np.eye(4))
        W = Z.rest_worlds(m['bones'])
        np.testing.assert_allclose(W[2][:3, 3], (0, 6, 0), atol=1e-9)  # the SCALE node scales the Hip's offset
        # the game-space worlds carry the scale ONCE (through unit_scale), never through the node
        rec = ('CLIP', 1, 2, 30.0, [track('Hip', Z.KIND_TRANSLATION, [(0, 10, 0, 0)])])
        recs = Z.parse_motion(motion([rec]))
        order = ['globalSRT', 'Hip']
        binds = Z.game_bind_matrices(m)
        worlds = Z.clip_game_worlds(m, recs[0], order, [binds[n] for n in order])
        np.testing.assert_allclose(worlds[0, 1][3, :3], (0, 10 * Z.GAME_SCALE * 0.6, 0), atol=1e-9)
        np.testing.assert_allclose(worlds[0, 1][:3, :3], np.eye(3), atol=1e-9)


def fkey(time, value, interp=Z.FCURVE_BEZIER, hl=None, hr=None, hl_value=None, hr_value=None):
    """One 7-float fcurve key; the interpolation code is a u32 stored in the float slot, the
    handles default to +-1/3 of a 60-frame interval with flat tangents (what XSI wrote)."""
    code = struct.unpack('<f', struct.pack('<I', interp))[0]
    return (time, time - 20.0 if hl is None else hl, time + 20.0 if hr is None else hr, code,
            value, value if hl_value is None else hl_value, value if hr_value is None else hr_value)


class FcurveTests(unittest.TestCase):
    def test_interpolation_code_is_a_u32_in_the_float_slot(self):
        self.assertEqual(Z.fcurve_interpolation(fkey(1, 0, Z.FCURVE_LINEAR)), 1)
        self.assertEqual(Z.fcurve_interpolation(fkey(1, 0, Z.FCURVE_BEZIER)), 2)
        # the same through a float32 track array, as parse_motion delivers them
        keys = np.asarray([fkey(1, 0, 2)], dtype='<f4').astype(float)
        self.assertEqual(Z.fcurve_interpolation(keys[0]), 2)

    def test_linear_and_hold_outside_the_range(self):
        keys = [fkey(1, 0.0, Z.FCURVE_LINEAR), fkey(240, -2.0, Z.FCURVE_LINEAR)]
        self.assertEqual(Z.fcurve_value(keys, -5), 0.0)
        self.assertEqual(Z.fcurve_value(keys, 1), 0.0)
        self.assertAlmostEqual(Z.fcurve_value(keys, 120.5), -2.0 * 119.5 / 239.0)
        self.assertEqual(Z.fcurve_value(keys, 240), -2.0)
        self.assertEqual(Z.fcurve_value(keys, 999), -2.0)

    def test_flat_bezier_is_a_smooth_step_and_solves_time_exactly(self):
        # flat tangents at +-1/3 of the interval: the value follows the cubic 3s^2 - 2s^3 of the
        # time fraction (the classic ease), symmetric about the midpoint
        keys = [fkey(1, 0.0, hl=-19, hr=21), fkey(61, 1.0, hl=41, hr=81)]
        self.assertAlmostEqual(Z.fcurve_value(keys, 31), 0.5, places=4)
        self.assertAlmostEqual(Z.fcurve_value(keys, 16), 3 * 0.25 ** 2 - 2 * 0.25 ** 3, places=3)
        self.assertAlmostEqual(Z.fcurve_value(keys, 46), 3 * 0.75 ** 2 - 2 * 0.75 ** 3, places=3)
        self.assertTrue(0.0 < Z.fcurve_value(keys, 2) < 0.01)

    def test_bezier_with_overshooting_handles_is_a_real_cubic(self):
        # stage010 glo_tex2's fourth key: value 0 with handle values -0.176 / 0.293
        k0 = fkey(104, 0.0, hl=96, hr=117.333, hl_value=-0.175781, hr_value=0.292969)
        k1 = fkey(144, 1.0, hl=130.667, hr=157.333)
        s = 0.5
        val, tim = Z._bezier(s, k0, k1)
        self.assertAlmostEqual(tim, 0.125 * 104 + 0.375 * 117.333 + 0.375 * 130.667 + 0.125 * 144, places=3)
        self.assertAlmostEqual(val, 0.375 * 0.292969 + 0.375 * 1.0 + 0.125 * 1.0, places=6)
        self.assertAlmostEqual(Z.fcurve_value([k0, k1], tim, tol=1e-6), val, places=4)

    def test_material_animation_channels_and_defaults(self):
        uv = [fkey(1, 0.0, Z.FCURVE_LINEAR), fkey(240, -2.0, Z.FCURVE_LINEAR)]
        col = [fkey(1, 0.5), fkey(120, 1.0), fkey(240, 0.5)]
        glow = [fkey(1, 0.0), fkey(120, 1.0), fkey(240, 0.0)]
        tracks = [track('DefaultLib.roll', 503, uv, flag=3), track('DefaultLib.roll', 503, [(0.0,)], flag=2),
                  track('DefaultLib.roll', 500, [(1.0,)], flag=2), track('DefaultLib.roll', 1302, [(0.0,)], flag=2),
                  track('DefaultLib.blink', 504, col, flag=3), track('DefaultLib.glo', 1302, glow, flag=3),
                  track('DefaultLib.spin', 501, [fkey(1, 0.0), fkey(240, 1.0)], flag=3),
                  track('DefaultLib.plain', 503, [(0.0,)], flag=2)]
        # the writer's `n` is 6 for every track; give the components their real n values
        rec_bytes = motion([('stage', 1, 240, 60.0, tracks)])
        rec = Z.parse_motion(rec_bytes)[0]
        ns = {(t['name'], t['kind'], t['flag']): n for (t, n) in zip(rec['tracks'], (8, 9, 8, 7, 8, 7, 8, 8))}
        for t in rec['tracks']:
            t['n'] = ns[(t['name'], t['kind'], t['flag'])]
        mt = Z.material_tracks(rec)
        self.assertEqual(sorted(mt), ['DefaultLib.blink', 'DefaultLib.glo', 'DefaultLib.plain', 'DefaultLib.roll', 'DefaultLib.spin'])
        self.assertEqual(sorted(mt['DefaultLib.roll']), [(500, 0), (503, 0), (503, 1), (1302, 0)])
        anim = Z.material_animation(rec)
        self.assertEqual(sorted(anim), ['DefaultLib.blink', 'DefaultLib.glo', 'DefaultLib.roll', 'DefaultLib.spin'])
        roll = anim['DefaultLib.roll']
        self.assertEqual(roll['uv_offset'].shape, (240, 2))
        np.testing.assert_allclose(roll['uv_offset'][0], (0.0, 0.0))
        np.testing.assert_allclose(roll['uv_offset'][-1], (-2.0, 0.0), atol=1e-6)
        self.assertIsNone(roll['colour'])
        self.assertIsNone(roll['glow'])
        blink = anim['DefaultLib.blink']
        self.assertEqual(blink['colour'].shape, (240, 4))
        np.testing.assert_allclose(blink['colour'][0], (0.5, 1.0, 1.0, 1.0))   # untracked g/b/a default to 1
        self.assertAlmostEqual(blink['colour'][119, 0], 1.0, places=3)
        glo = anim['DefaultLib.glo']
        self.assertEqual(glo['glow'].shape, (240,))
        self.assertAlmostEqual(glo['glow'][119], 1.0, places=3)
        self.assertAlmostEqual(glo['glow'][60], 0.5, places=1)   # frame 61, just past the 60.5 midpoint
        self.assertEqual(anim['DefaultLib.spin']['unsupported'], ['kind 501 component 0'])
        self.assertEqual(Z.record_frames(rec), list(range(1, 241)))


class CameraTests(unittest.TestCase):
    def camera_record(self, first=1):
        pos = [(0.0, 20.0, 30.0 - f, 0.0) for f in range(240 - first + 1)]
        tracks = [track('Camera_001', Z.KIND_CAM_SRT, [(1, 1, 1, 0, 0, 0, 1, 0, 0, 0)], flag=2),
                  track('Camera_001', Z.KIND_CAM_POSITION, pos),
                  track('Camera_001', Z.KIND_CAM_INTEREST, [(0.0, 9.74, 1.81, 0.0)], flag=2),
                  track('Camera_001', Z.KIND_CAM_FOV, [(Z.XSI_DEFAULT_FOV,)], flag=2),
                  track('Camera_001', Z.KIND_CAM_ROLL, [(0.0,)], flag=2),
                  track('Camera_neu', Z.KIND_CAM_SRT, [(2, 2, 2, 0, 0, 0, 1, 0, 0, 5)], flag=2),
                  track('Camera_neu', Z.KIND_CAM_POSITION, [(0.0, 20.0, 30.0, 0.0)], flag=2),
                  track('Camera_neu', Z.KIND_CAM_INTEREST, [(0.0, 9.74, 1.81, 0.0)], flag=2),
                  track('Camera_neu', Z.KIND_CAM_FOV, [(1.0,)], flag=2),
                  track('Camera_neu', Z.KIND_CAM_ROLL, [(0.5,)], flag=2)]
        return Z.parse_motion(motion([('camera001', first, 240, 60.0, tracks)]))[0]

    def test_camera_tracks_and_samples(self):
        rec = self.camera_record()
        self.assertEqual(sorted(Z.camera_tracks(rec)), ['Camera_001', 'Camera_neu'])
        pos, aim, fov, roll = Z.camera_samples(rec, 'Camera_001')
        self.assertEqual(pos.shape, (240, 3))
        np.testing.assert_allclose(pos[0], (0, 20, 30))
        np.testing.assert_allclose(pos[239], (0, 20, 30 - 239))
        np.testing.assert_allclose(aim[5], (0, 9.74, 1.81), atol=1e-6)
        self.assertAlmostEqual(float(fov[0]), Z.XSI_DEFAULT_FOV, places=6)
        # the camera null's SRT (scale 2, +5 in z) moves position and interest alike
        pos, aim, fov, roll = Z.camera_samples(rec, 'Camera_neu')
        np.testing.assert_allclose(pos[0], (0, 40, 65), atol=1e-5)
        np.testing.assert_allclose(aim[0], (0, 19.48, 8.62), atol=1e-5)
        self.assertAlmostEqual(float(roll[0]), 0.5, places=6)

    def test_negative_lead_in_frames_are_dropped(self):
        rec = self.camera_record(first=-22)
        self.assertEqual(Z.record_frames(rec), list(range(0, 241)))
        pos, _, _, _ = Z.camera_samples(rec, 'Camera_001')
        self.assertEqual(len(pos), 241)
        np.testing.assert_allclose(pos[0], (0, 20, 30 - 22))   # key 22 = record frame 0

    def test_look_at_rows_is_the_game_camera_frame(self):
        r = Z.look_at_rows((0, 2, 3), (0, 1, 0))
        fwd = np.array([0, -1, -3.0]) / math.sqrt(10)
        np.testing.assert_allclose(r[2], -fwd, atol=1e-9)          # row 2 = backward (local +Z)
        np.testing.assert_allclose(r[0], (1, 0, 0), atol=1e-9)     # right = +X
        self.assertGreater(r[1][1], 0.9)                           # up leans to +Y
        np.testing.assert_allclose(r @ r.T, np.eye(3), atol=1e-9)
        self.assertAlmostEqual(np.linalg.det(r), 1.0)
        # roll turns right/up about the view axis, keeps the forward
        rr = Z.look_at_rows((0, 2, 3), (0, 1, 0), roll=math.pi / 2)
        np.testing.assert_allclose(rr[2], r[2], atol=1e-9)
        np.testing.assert_allclose(rr[1], r[0], atol=1e-9)
        # straight down: no NaNs
        d = Z.look_at_rows((0, 5, 0), (0, 0, 0))
        self.assertTrue(np.isfinite(d).all())

    def test_fov_mapping_keeps_supernovas_vertical_extent(self):
        deg = Z.sn_fov_to_camanm(Z.XSI_DEFAULT_FOV)
        # the game: H = tan(deg/2) * 4/3; t' = tan(atan(1/H) / 2); vertical half-tangent t' * 9/16
        h = math.tan(math.radians(deg) / 2) * 4 / 3
        tp = math.tan(0.5 * math.atan(1 / h))
        self.assertAlmostEqual(math.degrees(2 * math.atan(tp * 9 / 16)), 41.53, places=2)   # = 53.638 deg horizontal on 4:3
        self.assertAlmostEqual(math.degrees(2 * math.atan(tp)), 67.97, places=1)
        deg_h = Z.sn_fov_to_camanm(Z.XSI_DEFAULT_FOV, keep='horizontal')
        h = math.tan(math.radians(deg_h) / 2) * 4 / 3
        tp = math.tan(0.5 * math.atan(1 / h))
        self.assertAlmostEqual(math.degrees(2 * math.atan(tp)), 53.638, places=2)
        self.assertAlmostEqual(Z.world_camanm_fov(math.tan(math.radians(41.53) / 2) * 4 / 3 * 0 + 0.6152, 4 / 3), 41.53, places=1)

    def test_camanm_spec_round_trips_through_anm_dump(self):
        import anm_dump as A
        rec = self.camera_record()
        spec, times, (pos_m, aim_m) = Z.camera_to_camanm_spec(rec, 'Camera_001', Z.GAME_SCALE)
        self.assertEqual(times, list(range(240)))
        self.assertEqual(spec['frame_count'], 239)
        self.assertEqual(spec['fps'], 60)
        data = A.write_anm(spec)
        parsed = A.parse_anm(data)
        cam = next(c for c in parsed['chunks'] if c['type'] == 4)
        self.assertEqual(parsed['header']['fps_or_one'], 60)
        self.assertEqual([t['key_count'] for t in cam['tracks']], [240, 240, 1, 1, 1, 1])
        for f in (0, 100, 239):
            q = A.sample_track(data, cam['tracks'][0], float(f))
            p = np.array(A.sample_track(data, cam['tracks'][1], float(f))) * 0.01
            rows = np.array(A.quat_to_rowmat(q))
            np.testing.assert_allclose(p, pos_m[f], atol=1e-4)
            want = (aim_m[f] - pos_m[f]) / np.linalg.norm(aim_m[f] - pos_m[f])
            np.testing.assert_allclose(-rows[2], want, atol=1e-4)      # target = eye - R.row2
            self.assertGreater(rows[1][1], 0.9)                         # up = R.row1 ~ +Y
        self.assertAlmostEqual(A.sample_track(data, cam['tracks'][2], 0.0)[0], Z.sn_fov_to_camanm(Z.XSI_DEFAULT_FOV), places=4)
        self.assertAlmostEqual(A.sample_track(data, cam['tracks'][5], 0.0)[0], 4 / 3, places=6)
        # a static shot collapses to single keys
        spec2, _, _ = Z.camera_to_camanm_spec(rec, 'Camera_neu', Z.GAME_SCALE)
        self.assertEqual([len(t['keys']) for t in spec2['camera']], [1, 1, 1, 1, 1, 1])
        self.assertEqual(spec2['frame_count'], 239)
        # a 29.97 fps record spaces the keys two frames apart
        rec2 = self.camera_record()
        rec2['fps'] = 29.97
        spec3, times3, _ = Z.camera_to_camanm_spec(rec2, 'Camera_001', Z.GAME_SCALE)
        self.assertEqual(times3[:3], [0, 2, 4])
        self.assertEqual(spec3['frame_count'], 478)


if __name__ == '__main__':
    unittest.main()
