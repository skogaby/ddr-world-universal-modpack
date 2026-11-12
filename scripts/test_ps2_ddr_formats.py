"""Host-only tests for scripts/extract_ps2_ddr_data.py (no game data needed).

Run: (cd scripts && python3 -m unittest -q test_ps2_ddr_formats)
"""
import argparse
import contextlib
import csv
import io
import random
import struct
import tempfile
import unittest
from pathlib import Path

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


def tim2(width, height, image_type, pixels, clut_words, clut_type=3, fmt=0, clut_fmt="I"):
    """A one-picture TIM2 (format 0: picture at 0x10, 1: at 0x80), pixels then CLUT, no GIF tags."""
    clut = b"".join(struct.pack("<" + clut_fmt, c) for c in clut_words)
    header = struct.pack("<3I2H4B2H", 0x30 + len(pixels) + len(clut), len(clut), len(pixels), 0x30,
                         len(clut_words), 0, 1, clut_type, image_type, width, height)
    head = P.TIM2_MAGIC + bytes([4, fmt]) + struct.pack("<H", 1) + bytes(8)
    return head + bytes(0x70 if fmt == 1 else 0) + header + bytes(0x30 - len(header)) + pixels + clut


def tgcd_literal(data):
    return struct.pack("<HH", 0x8000, len(data)) + data + bytes(-len(data) % 16)


def tgcd_copy(back, n, literal):
    """Copy n bytes from `back` into the window (0 = its start), then one literal byte."""
    return struct.pack("<HBB", back, n, literal)


def tgcd(ops, out_size):
    stream = b"".join(ops)
    return (P.TGCD_MAGIC + struct.pack("<7I", out_size, 0x20 + len(stream), 0x7FFF, 0xFFFF, 0xFF, 0x8000,
                                       sum(stream) & 0xFFFFFFFF) + stream)


def vig(body, channels, interleave, flags=0, rate=44100):
    head = P.VIG_MAGIC + struct.pack("<9I", 0, P.SECTOR, len(body), 0, 0, rate, channels, flags, interleave)
    return head + bytes(P.SECTOR - len(head)) + body


def adpcm_frame(*nibbles):
    """One PS-ADPCM frame, shift 0 / filter 0: each nibble n decodes to n << 12."""
    data = bytearray(14)
    for k, nib in enumerate(nibbles):
        data[k // 2] |= nib << (4 * (k % 2))
    return b"\x00\x00" + bytes(data)


def frameinfo_ipu(stream_data=b"DATA", header_size=0x20):
    ipum = b"ipum" + struct.pack("<I2HI", 8 + len(stream_data), 16, 16, 1) + stream_data
    return struct.pack("<I", header_size) + P.FRAMEINFO_TAG + bytes(header_size - 16) + ipum, ipum


def dat_table(table_va, files, entry_size=0x2C, date=(2009, 8, 26, 20, 28)):
    """A DAT table payload for elf_with: u32 count, entries, then the names they point at.

    files: [(name, sector, size, byte_sum, variant)] after the implicit entry 0 (the header).
    """
    rows = [("", 0, P.DAT_HEADER_SIZE, 0, 0)] + list(files)
    names_at = table_va + 4 + len(rows) * entry_size
    names, entries = b"", b""
    for pos, (name, sector, size, byte_sum, variant) in enumerate(rows):
        name_va = names_at + len(names)
        names += name.encode() + b"\x00"
        head = (pos, 1, variant) if entry_size == 0x30 else (pos, 1)
        entries += struct.pack(f"<{entry_size // 4}I", *head, size, sector, name_va, byte_sum, *date)
    return struct.pack("<I", len(rows)) + entries + names


def dat_header(date=(2009, 8, 26, 20, 28)):
    return bytes(12) + struct.pack("<5I", *date)


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
        found = P.find_toc_candidates(elf_with(0x300000, b"\xaa" * 6 + table), 64)[0]
        self.assertEqual(found, (0x300006, 20, True, 1.0, False))

    def test_find_toc_ranks_span_cover_first(self):
        # an unterminated table that tiles the span beats a longer run that doesn't
        tiling = b"".join(toc_entry(i + 1, i * 2, 2) for i in range(20)) + toc_entry(3, 0, 0)
        longer = b"".join(toc_entry(i + 1, 0, 1) for i in range(30)) + toc_entry(3, 0, 0)
        found = P.find_toc_candidates(elf_with(0x300000, longer + tiling), 40)
        self.assertEqual(found[0][:2] + found[0][4:], (0x300000 + len(longer), 20, True))

    def test_unterminated_toc(self):
        table = toc_entry(1, 0, 4) + toc_entry(2, 4, 2) + toc_entry(9, 6, 2) + toc_entry(3, 0x3C0040, 0)
        elf = elf_with(self.va, table)
        self.assertEqual(P.read_toc(elf, self.va, 3, 8, terminated=False), [(1, 0, 4), (2, 4, 2), (9, 6, 2)])
        with self.assertRaises(P.ArchiveError):  # entry 2 still continues the run: count too small
            P.read_toc(elf, self.va, 2, 8, terminated=False)
        with self.assertRaises(P.ArchiveError):  # and it isn't 0xFFFF-terminated
            P.read_toc(elf, self.va, 3, 8)


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

    def test_tim2_8bpp_csm1_palette(self):
        stored = [0x80000000] * 256
        stored[16] = 0x800000FF  # CSM1: logical entry 8 at position 16, as in TCB
        self.assertEqual(P.decode_tim2(tim2(1, 1, 5, b"\x08", stored)), (1, 1, b"\xff\x00\x00\xff"))

    def test_tim2_format1_4bpp_16bit_clut(self):
        pal = [0x8000] * 16
        pal[1], pal[2] = 0x801F, 0x03E0  # opaque red, transparent green
        image = tim2(2, 1, 4, b"\x21", pal, clut_type=1, fmt=1, clut_fmt="H")
        self.assertEqual(P.decode_tim2(image)[2], b"\xff\x00\x00\xff" + b"\x00\xff\x00\x00")

    def test_tim2_direct_32bit(self):
        image = tim2(2, 1, 3, struct.pack("<2I", 0x80102030, 0x00405060), [], clut_type=0)
        self.assertEqual(P.decode_tim2(image)[2], b"\x30\x20\x10\xff" + b"\x60\x50\x40\x00")

    def test_tim2_clut_only_is_not_an_image(self):
        with self.assertRaises(ValueError):
            P.decode_tim2(tim2(0, 0, 0, b"", [0] * 16))

    def test_vig_stereo_interleave(self):
        body = adpcm_frame(1, 2) + adpcm_frame(3, 4)  # L block, then R block, 16 bytes each
        rate, channels, pcm = P.decode_vig(vig(body, 2, 0x10))
        self.assertEqual((rate, channels), (44100, 2))
        self.assertEqual(struct.unpack_from("<4h", pcm), (4096, 12288, 8192, 16384))

    def test_vig_mono_and_encrypted(self):
        self.assertEqual(struct.unpack_from("<h", P.decode_audio(vig(adpcm_frame(5), 1, 0))[2]), (5 << 12,))
        with self.assertRaises(ValueError):
            P.decode_vig(vig(adpcm_frame(5), 1, 0, flags=1))

    def test_sniff_new_formats(self):
        self.assertEqual(P.sniff(tim2(1, 1, 5, b"\x00", [0])), ("tim2", "tm2"))
        self.assertEqual(P.sniff(vig(bytes(16), 1, 0)), ("vig", "vig"))
        self.assertEqual(P.sniff(frameinfo_ipu()[0]), ("ipu_frameinfo", "fipu"))
        self.assertEqual(P.sniff(tgcd([tgcd_literal(b"TIM2")], 4)), ("tgcd", "tm2c"))
        self.assertEqual(P.sniff(b"\x89DTF" + bytes(12)), ("dtf", "dtf"))
        self.assertIsNone(P.sniff(P.TGCD_MAGIC + bytes(28)))  # TGCD magic, not the game's header


class TgcdTests(unittest.TestCase):
    def test_literal_run_and_overlapping_copy(self):
        stream = tgcd([tgcd_literal(b"ABC"), tgcd_copy(0, 5, ord("!"))], 9)
        self.assertEqual(P.tgcd_decode(stream), b"ABCABCAB!")

    def test_window_slides_after_0x7fff_bytes(self):
        head = bytes(range(256)) * 0x81  # 0x8100 bytes: the window now starts at 0x8100 - 0x7FFF = 0x101
        stream = tgcd([tgcd_literal(head), tgcd_copy(0, 2, 0xEE)], len(head) + 3)
        self.assertEqual(P.tgcd_decode(stream)[-3:], bytes([head[0x101], head[0x102], 0xEE]))

    def test_size_mismatch_and_bad_copy_are_rejected(self):
        with self.assertRaises(ValueError):
            P.tgcd_decode(tgcd([tgcd_literal(b"ABCD")], 3))
        with self.assertRaises(ValueError):
            P.tgcd_decode(tgcd([tgcd_literal(b"AB"), tgcd_copy(5, 1, 0)], 4))  # reads unwritten bytes
        with self.assertRaises(ValueError):
            P.tgcd_decode(tgcd([tgcd_literal(b"AB")], 10))  # runs out of stream

    def test_unpack_and_png_see_through_tgcd(self):
        image = tim2(1, 1, 5, b"\x00", [0x80FFFFFF])
        stream = tgcd([tgcd_literal(image)], len(image))
        tree = P.unpack(stream)
        self.assertEqual((tree.kind, tree.wrapper, tree.data), ("tim2", "tgcd", image))
        self.assertEqual(P.decode_image(stream), (1, 1, b"\xff\xff\xff\xff"))

    def test_unpack_frameinfo(self):
        blob, ipum = frameinfo_ipu()
        tree = P.unpack(blob + bytes(64))
        self.assertEqual((tree.kind, tree.wrapper, tree.offset, tree.data), ("ipu", "frameinfo", 0x20, ipum))


class DatTests(unittest.TestCase):
    va = 0x280000
    files = [("LOGO/a.tm2c", 1, 0x900, 7, 0), ("snd/b.vig", 3, 0x10, 8, 0x80000001)]

    def test_read_both_entry_layouts(self):
        for entry_size in (0x2C, 0x30):
            elf = elf_with(self.va, dat_table(self.va, self.files, entry_size))
            entries = P.read_dat_table(elf, self.va, entry_size, 4 * P.SECTOR, dat_header())
            self.assertEqual([(e.name, e.sector, e.size, e.byte_sum) for e in entries],
                             [("", 0, 0x20, 0), ("LOGO/a.tm2c", 1, 0x900, 7), ("snd/b.vig", 3, 0x10, 8)])
            self.assertEqual(entries[2].variant, 0x80000001 if entry_size == 0x30 else 0)
            self.assertEqual(entries[1].date, (2009, 8, 26, 20, 28))

    def test_rejects_wrong_build_overlap_bounds_and_names(self):
        elf = elf_with(self.va, dat_table(self.va, self.files))
        cases = [
            (elf, 4 * P.SECTOR, dat_header((2008, 1, 1, 0, 0))),  # header dated differently
            (elf, 3 * P.SECTOR, None),  # b.vig is past the archive's end
            (elf_with(self.va, dat_table(self.va, self.files + [("c", 2, 4, 0, 0)])), 4 * P.SECTOR, None),  # overlap
            (elf_with(self.va, dat_table(self.va, [("../x", 1, 4, 0, 0)])), 4 * P.SECTOR, None),
            (elf_with(self.va, dat_table(self.va, [("a\\b", 1, 4, 0, 0)])), 4 * P.SECTOR, None),
        ]
        for elf_bytes, size, header in cases:
            with self.assertRaises(P.ArchiveError):
                P.read_dat_table(elf_bytes, self.va, 0x2C, size, header)

    def test_find_dat_tables(self):
        pad = b"\x00" * 0x40
        elf = elf_with(self.va, pad + dat_table(self.va + 0x40, self.files, 0x30))
        found = P.find_dat_tables(elf, {"IMAGE": (4 * P.SECTOR, dat_header()), "SOUND": (P.SECTOR, dat_header())})
        self.assertEqual(found, [(self.va + 0x40, 0x30, 3, ["IMAGE"], "LOGO/a.tm2c")])

    def test_unique_name(self):
        seen = set()
        self.assertEqual([P._unique_name(n, seen, k) for k, n in enumerate(["A/x.TM2", "a/X.tm2", "A/y"])],
                         ["A/x.TM2", "a/X~1.tm2", "A/y"])


def extract_args(game, game_dir, out_dir, **kw):
    opts = dict(unpack=True, png=True, wav=True, exclude=None, ids=None, names=None)
    opts.update(kw)
    return argparse.Namespace(cmd="extract", game=game, game_dir=Path(game_dir), out_dir=Path(out_dir), **opts)


class ExtractTests(unittest.TestCase):
    """cmd_extract end to end on synthetic discs."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.addCleanup(self.tmp.cleanup)
        self.addCleanup(P.GAMES.pop, "_test", None)

    def run_extract(self, **kw):
        with contextlib.redirect_stdout(io.StringIO()):
            P.cmd_extract(extract_args("_test", self.root / "disc", self.root / "out", **kw))
        with open(self.root / "out" / "manifest.csv", newline="") as fh:
            return list(csv.DictReader(fh))

    def make_dat_disc(self, corrupt=False):
        image = tim2(1, 1, 5, b"\x00", [0x80FFFFFF])
        packed = tgcd([tgcd_literal(image)], len(image))
        sound = vig(adpcm_frame(1, 2), 1, 0)
        payloads = [("LOGO/title.TM2c", packed), ("LOGO/title.TM2", image), ("snd/jingle.vig", sound)]
        dat, files = bytearray(dat_header() + bytes(P.SECTOR - 0x20)), []
        for name, data in payloads:
            files.append((name, len(dat) // P.SECTOR, len(data), sum(data), 0))
            dat += data + bytes(-len(data) % P.SECTOR)
        dat += image + bytes(-len(image) % P.SECTOR)  # an unreferenced sector
        if corrupt:
            dat[P.SECTOR * files[2][1] + 0x800] ^= 1
        (self.root / "disc" / "DATA").mkdir(parents=True)
        (self.root / "disc" / "DATA" / "IMAGE.DAT").write_bytes(bytes(dat))
        (self.root / "disc" / "TEST.ELF").write_bytes(elf_with(self.va, dat_table(self.va, files, 0x30)))
        P.GAMES["_test"] = {"layout": "dat", "elf": "TEST.ELF", "entry_size": 0x30,
                            "dats": [("IMAGE", "DATA/IMAGE.DAT", self.va)]}

    va = 0x280000

    def test_dat_game(self):
        self.make_dat_disc()
        rows = self.run_extract()
        self.assertEqual([(r["file"], r["type"], r["checksum"]) for r in rows], [
            ("files/IMAGE/LOGO/title.TM2c", "tim2+tgcd", "ok"),
            ("files/IMAGE/LOGO/title.TM2", "tim2", "ok"),
            ("files/IMAGE/snd/jingle.vig", "vig", "ok"),
            ("hidden/IMAGE/000005.tm2", "tim2", ""),  # header, 1 + 1 + 2 sectors of files, then this
        ])
        out = self.root / "out"
        for rel in ("unpacked/IMAGE/LOGO/title.TM2c.tm2", "unpacked/IMAGE/LOGO/title.TM2c.png",
                    "files/IMAGE/LOGO/title.TM2.png", "files/IMAGE/snd/jingle.vig.wav"):
            self.assertTrue((out / rel).is_file(), rel)

    def test_dat_game_names_and_bad_checksum(self):
        self.make_dat_disc(corrupt=True)
        with self.assertRaises(SystemExit):
            self.run_extract(names="image/snd/*")
        with open(self.root / "out" / "manifest.csv", newline="") as fh:
            rows = list(csv.DictReader(fh))
        self.assertEqual([(r["file"], r["checksum"]) for r in rows], [("files/IMAGE/snd/jingle.vig", "BAD")])

    def test_filedata_game_without_terminator(self):
        blob, ipum = frameinfo_ipu()
        svag = struct.pack("<4s4I", b"Svag", 16, 44100, 1, 0) + bytes(P.SECTOR - 20) + adpcm_frame(1)
        archive = blob + bytes(P.SECTOR - len(blob)) + svag + bytes(-len(svag) % P.SECTOR)
        (self.root / "disc" / "DATA").mkdir(parents=True)
        (self.root / "disc" / "DATA" / "FILEDATA.BIN").write_bytes(archive)
        toc = toc_entry(1, 0, 1) + toc_entry(2, 1, 2) + toc_entry(1, 0x3C0040, 0)
        (self.root / "disc" / "TEST.ELF").write_bytes(elf_with(self.va, toc))
        P.GAMES["_test"] = {"elf": "TEST.ELF", "toc_va": self.va, "toc_count": 2, "terminated": False,
                            "archives": ["DATA/FILEDATA.BIN"]}
        rows = self.run_extract()
        self.assertEqual([(r["id"], r["type"], r["file"]) for r in rows],
                         [("0001", "ipu+frameinfo", "files/0001.fipu"), ("0002", "svag", "files/0002.svag")])
        self.assertEqual((self.root / "out" / "unpacked" / "0001.ipu").read_bytes(), ipum)
        self.assertTrue((self.root / "out" / "files" / "0002.wav").is_file())


if __name__ == "__main__":
    unittest.main()
