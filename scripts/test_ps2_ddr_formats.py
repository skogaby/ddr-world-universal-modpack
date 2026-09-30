"""Host-only tests for scripts/extract_ps2_ddr_data.py (no game data needed).

Run: (cd scripts && python3 -m unittest -q test_ps2_ddr_formats)
"""
import random
import struct
import unittest

import extract_ps2_ddr_data as P


def lz_store(data):
    """A store-only Bemani LZ encoder: literal runs (8-70 bytes), single literals, end code."""
    cmds, i = [], 0
    while len(data) - i >= 8:
        n = min(70, len(data) - i)
        cmds.append((1, bytes([0xB8 + n]) + data[i:i + n]))
        i += n
    cmds += [(0, data[k:k + 1]) for k in range(i, len(data))]
    cmds.append((1, b"\xff"))
    out = bytearray()
    for g in range(0, len(cmds), 8):
        group = cmds[g:g + 8]
        out.append(sum(flag << k for k, (flag, _) in enumerate(group)))
        for _flag, body in group:
            out += body
    return bytes(out)


def lz_rle(ch):
    """69 bytes of `ch` in 7: a literal, then two 34-byte distance-1 copies, then the end code."""
    return b"\x0e" + ch + b"\x7c\x01\x7c\x01\xff"


def tcb(width, height, image_type, pixels, palette, clut_type=3):
    """A minimal TCB: file header, 0x30-byte picture header, GIF-tag-prefixed pixels and CLUT."""
    image = b"\x00" * 16 + pixels
    clut = b"\x00" * 16 + b"".join(struct.pack("<I", c) for c in palette)
    header = struct.pack("<3I2H4B2H", 0x30 + len(image) + len(clut), len(clut), len(image), 0x30,
                         len(palette), clut_type, 0, image_type, 0, width, height)
    header += b"\x00" * (0x30 - len(header))
    return P.TCB_MAGIC + header + image + clut


def elf_with(table_va, payload):
    """A 32-bit LE ELF with one PT_LOAD segment holding `payload` at `table_va`."""
    ehdr = bytearray(0x34)
    ehdr[:6] = b"\x7fELF\x01\x01"
    struct.pack_into("<I", ehdr, 0x1C, 0x34)
    struct.pack_into("<HH", ehdr, 0x2A, 32, 1)
    phdr = struct.pack("<8I", 1, 0x54, table_va, table_va, len(payload), len(payload), 5, 16)
    return bytes(ehdr) + phdr + payload


def toc_entry(fid, sector, sectors):
    return struct.pack("<Q", fid | sector << 16 | sectors << 40)


class LzTests(unittest.TestCase):
    def test_literals_and_end(self):
        self.assertEqual(P.lz_decode(b"\x00ABCDEFGH\x01\xff"), (b"ABCDEFGH", 11, True))

    def test_long_copy(self):
        # 4 literals, then len 4 / distance 4 ((4 - 3) << 2, 4), then end
        self.assertEqual(P.lz_decode(b"\x30ABCD\x04\x04\xff")[0], b"ABCDABCD")

    def test_short_overlapping_copy(self):
        # 'A', then short copy distance 1, length 3 (0x90), then end
        self.assertEqual(P.lz_decode(b"\x06A\x90\xff")[0], b"AAAA")

    def test_literal_run(self):
        self.assertEqual(P.lz_decode(b"\x03\xc012345678\xff")[0], b"12345678")

    def test_copy_before_start_reads_zero_window(self):
        self.assertEqual(P.lz_decode(b"\x03\x04\x10\xff")[0], b"\x00" * 4)

    def test_truncated_stream_is_not_clean(self):
        _data, _stop, clean = P.lz_decode(b"\x00ABC")
        self.assertFalse(clean)

    def test_store_encoder_round_trip(self):
        data = bytes(range(256)) * 3
        self.assertEqual(P.lz_decode(lz_store(data))[0], data)

    def test_member_requires_zero_padding(self):
        stream = lz_store(b"x" * 100)
        self.assertIsNotNone(P.lz_member(stream + b"\x00" * 7, 0, len(stream) + 7))
        self.assertIsNone(P.lz_member(stream + b"\x01", 0, len(stream) + 1))


class TocTests(unittest.TestCase):
    def setUp(self):
        self.va = 0x200000
        table = toc_entry(1, 0, 4) + toc_entry(2, 4, 2) + toc_entry(3, 8, 1) + toc_entry(0xFFFF, 0xFFFF, 0)
        self.elf = elf_with(self.va, table)

    def test_read_toc(self):
        self.assertEqual(P.read_toc(self.elf, self.va, 3, 16), [(1, 0, 4), (2, 4, 2), (3, 8, 1)])

    def test_missing_terminator_is_rejected(self):
        with self.assertRaises(P.ArchiveError):
            P.read_toc(self.elf, self.va, 2, 16)

    def test_entry_outside_span_is_rejected(self):
        with self.assertRaises(P.ArchiveError):
            P.read_toc(self.elf, self.va, 3, 8)

    def test_gaps(self):
        self.assertEqual(P.gaps([(1, 0, 4), (2, 4, 2), (3, 8, 1)], 12), [(6, 2), (9, 3)])

    def test_find_toc(self):
        table = b"".join(toc_entry(i * 2 + 1, i, 1) for i in range(20)) + toc_entry(0xFFFF, 0xFFFF, 0)
        va, count, terminated, contiguous = P.find_toc_candidates(elf_with(0x300000, b"\xaa" * 6 + table), 64)[0]
        self.assertEqual((va, count, terminated, contiguous), (0x300006, 20, True, 1.0))


class ContainerTests(unittest.TestCase):
    def test_pairs_unsorted_with_empty_slot(self):
        blob = struct.pack("<6I", 0x28, 4, 0x18, 0x10, 0, 0) + b"A" * 16 + b"BBBB"
        self.assertEqual(P.parse_pairs(blob), [(0, 0x28, 0x2C), (1, 0x18, 0x28)])

    def test_pairs_reject_overlap(self):
        blob = struct.pack("<4I", 0x10, 8, 0x14, 4) + b"\x00" * 16
        self.assertIsNone(P.parse_pairs(blob))

    def test_offsets_unsorted_with_null_slot(self):
        blob = struct.pack("<3I", 0x10, 0, 0x0C) + b"xxxx" + b"yyyy"
        self.assertEqual(P.parse_offsets(blob), [(0, 0x10, 0x14), (2, 0x0C, 0x10)])

    def test_counted(self):
        blob = struct.pack("<4I", 2, 0x10, 0x14, 0x18) + b"aaaabbbb"
        self.assertEqual(P.parse_counted(blob), [(0, 0x10, 0x14), (1, 0x14, 0x18)])

    def test_lz_sequence(self):
        blob = lz_rle(b"a") + b"\x00" * (0x40 - 7) + lz_rle(b"b")
        members = P.parse_lz_sequence(blob)
        self.assertIsNotNone(members)
        self.assertEqual([(start, stop) for _slot, start, stop in members or []], [(0, 7), (0x40, 0x47)])

    def test_lz_sequence_needs_expanding_streams(self):
        a = lz_store(b"a" * 80)  # store-only streams never expand, like most random data
        self.assertIsNone(P.parse_lz_sequence(a + b"\x00" * (0x60 - len(a)) + lz_store(b"b" * 90)))

    def test_unpack_nested_lz_member(self):
        image = tcb(2, 1, 5, b"\x00\x01", [0x80000000, 0x800000FF])
        member = lz_store(image)
        blob = struct.pack("<2I", 8, len(member)) + member
        tree = P.unpack(blob)
        self.assertEqual(tree.kind, "pairs")
        self.assertEqual((tree.children[0].kind, tree.children[0].compressed), ("tcb", True))
        self.assertEqual(tree.children[0].data, image)

    def test_random_data_is_not_a_container(self):
        rng = random.Random(1234)
        for _ in range(200):
            blob = bytes(rng.randrange(256) for _ in range(rng.randrange(16, 4096)))
            self.assertEqual(P.unpack(blob).kind, "unknown")

    def test_split_hidden(self):
        member = lz_store(tcb(2, 1, 5, b"\x00\x01", [0, 0]))
        first = struct.pack("<2I", 8, len(member)) + member
        first += b"\x00" * (-len(first) % P.SECTOR)
        blob = first + b"\x00" * P.SECTOR + b"\x99" * 100 + b"\x00" * (P.SECTOR - 100)
        self.assertEqual(P.split_hidden(blob), [(0, len(first)), (len(first) + P.SECTOR, P.SECTOR)])


class LeafTests(unittest.TestCase):
    def test_tcb_8bpp_csm1_palette(self):
        stored = [0x80000000] * 256
        stored[16] = 0x800000FF  # CSM1 stores logical entry 8 at position 16
        w, h, rgba = P.decode_tcb(tcb(1, 1, 5, b"\x08", stored))
        self.assertEqual((w, h, rgba), (1, 1, b"\xff\x00\x00\xff"))

    def test_tcb_4bpp_low_nibble_first(self):
        pal = [0x80000000] * 16
        pal[1], pal[2] = 0x8000FF00, 0x40FF0000
        _w, _h, rgba = P.decode_tcb(tcb(2, 1, 4, b"\x21", pal))
        self.assertEqual(rgba, b"\x00\xff\x00\xff" + b"\x00\x00\xff\x80")

    def test_sniff(self):
        self.assertEqual(P.sniff(b"Svag" + bytes(12)), ("svag", "svag"))
        self.assertEqual(P.sniff(b"\x00\x00\x01\xb3" + bytes(12)), ("mpeg1", "m1v"))
        cmd = bytes(8) + struct.pack("<I", 2) + bytes(20) + struct.pack("<6I", 0x18, 1, 0x1000, 0x18, 1, 0x1000)
        self.assertEqual(P.sniff(cmd + bytes(0x40)), ("dancer_mesh", "cmd"))
        self.assertEqual(P.sniff(struct.pack("<HHII", 0x53, 1, 8, 16) + bytes(8)), ("dancer_motion", "cmm"))

    def test_svag_adpcm_frame(self):
        frame = bytes([0x00, 0x00, 0x21]) + bytes(13)  # shift 0, filter 0: nibbles 1, 2 -> 4096, 8192
        svag = struct.pack("<4s4I", b"Svag", 16, 44100, 1, 0) + bytes(P.SECTOR - 20) + frame
        rate, channels, pcm = P.decode_svag(svag)
        self.assertEqual((rate, channels), (44100, 1))
        self.assertEqual(struct.unpack_from("<2h", pcm), (4096, 8192))


if __name__ == "__main__":
    unittest.main()
