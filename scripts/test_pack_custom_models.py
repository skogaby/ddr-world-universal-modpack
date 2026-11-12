#!/usr/bin/env python3
"""Host tests for scripts/pack_custom_models.py (the release-time model packer).

The rules mirrored from the DLL are pinned against the same cases the Rust
suites use (`custom_content.rs` classify / folder_member_path tests,
`sources.rs` model-folder detection), plus the arc layout of
`core::arc::ArcArchive::to_bytes` and both driver modes on a synthetic tree.

Run: (cd scripts && python3 -m unittest -q test_pack_custom_models)
"""

import contextlib
import io
import struct
import tempfile
import unittest
from pathlib import Path

import pack_custom_models as p


def run_quiet(*args, **kwargs) -> int:
    """`p.run` with its report lines swallowed (the harness output stays clean)."""
    with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
        return p.run(*args, **kwargs)


def write(path: Path, data: bytes = b"x") -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)


class NameRules(unittest.TestCase):
    def test_classify_arc_name(self):
        cases = {
            "pl_peter00.arc": ("body", "peter00"),
            "PL_Peter00.ARC": ("body", "peter00"),
            "pl_peter00_head00.arc": ("part", "peter00"),
            "pl_emi00_face02.arc": ("part", "emi00"),
            "pl_peter_griffin00.arc": ("body", "peter_griffin00"),
            "pl_peter_griffin00_forearm00.arc": ("part", "peter_griffin00"),
            "pl_shadow00.arc": ("shadow", None),
            "mapset_griffin00.arc": ("stage", "griffin00"),
            "mapset_griffin00_g.arc": ("gold", "griffin00"),
            "mc_male.arc": ("other", None),
            "readme.txt": ("other", None),
            "pl_.arc": ("other", None),
            "pl_Bad Key.arc": ("other", None),
            # Non-ASCII is never lowered into a key (Rust to_ascii_lowercase).
            "pl_\u00c9mi00.arc": ("other", None),
        }
        for name, want in cases.items():
            self.assertEqual(p.classify_arc_name(name), want, name)

    def test_model_folder_names(self):
        for name in ("pl_teto00", "mapset_griffin00", "mapset_y_g", "pl_x_head00"):
            self.assertTrue(p.is_model_folder_name(name), name)
        for name in ("Peter Griffin", "textures_src", "pl_shadow00", "camera"):
            self.assertFalse(p.is_model_folder_name(name), name)

    def test_folder_member_path(self):
        body = ("body", "peter00")
        self.assertEqual(
            p.folder_member_path(*body, "pl_peter00", "pl_peter00.model"),
            "data/chara/pl_peter00/pl_peter00.model",
        )
        self.assertEqual(
            p.folder_member_path(*body, "pl_peter00", "data\\chara\\pl_peter00\\pl_peter00.model"),
            "data/chara/pl_peter00/pl_peter00.model",
        )
        self.assertEqual(
            p.folder_member_path("part", "peter00", "pl_peter00_head00", "pl_peter00_head00.model"),
            "data/chara/pl_peter00_head00/pl_peter00_head00.model",
        )
        stage = ("stage", "griffin00")
        self.assertEqual(
            p.folder_member_path(*stage, "mapset_griffin00", "gm_griffin00_room/gm_griffin00_room.model"),
            "data/map/gm_griffin00_room/gm_griffin00_room.model",
        )
        self.assertEqual(
            p.folder_member_path(*stage, "mapset_griffin00", "camera/griffin_st01.camanm"),
            "data/camera/griffin00/griffin_st01.camanm",
        )
        self.assertEqual(
            p.folder_member_path(*stage, "mapset_griffin00", "griffin_non01.CAMANM"),
            "data/camera/griffin00/griffin_non01.CAMANM",
        )


class ArcLayout(unittest.TestCase):
    def test_to_bytes_layout(self):
        data = p.arc_bytes({"b/two": b"22", "a/one": b"1" * 70})
        magic, version, count, comp = struct.unpack_from("<IIII", data, 0)
        self.assertEqual((magic, version, count, comp), (p.ARC_MAGIC, 1, 2, 0))
        entries = p.arc_entries(data)
        self.assertEqual([e[0] for e in entries], ["a/one", "b/two"])  # byte order
        for _, off, _ in entries:
            self.assertEqual(off % 64, 0)
        # String table right after the cue table; data at the next 64 boundary.
        self.assertEqual(struct.unpack_from("<I", data, 16)[0], 16 + 2 * 16)
        self.assertEqual(entries[0][1], 64)
        self.assertEqual(entries[1][1], 64 + 128)  # 70 bytes padded to 128
        self.assertEqual(len(data) % 64, 0)
        self.assertEqual(data[entries[0][1]:entries[0][1] + 70], b"1" * 70)


class Driver(unittest.TestCase):
    def tree(self, root: Path) -> None:
        d = root / "dancers"
        write(d / "pl_flat00" / "pl_flat00.model", b"flat")  # implicit CUSTOM
        write(d / "Friendly" / "pl_fr00" / "pl_fr00.model", b"fr")
        write(d / "Friendly" / "pl_fr00" / "motion" / "a.anm", b"anm")
        write(d / "Friendly" / "pl_fr00" / ".DS_Store", b"junk")
        write(d / "Friendly" / "chara_resources.rlist.txt", b"fr00, pl, F, A, 1, 0.75\n")
        write(d / "Src" / "Name" / "pl_deep00" / "pl_deep00.model", b"deep")
        # Too deep for the scanner: never packed.
        write(d / "Src" / "Name" / "Extra" / "pl_toodeep00" / "pl_toodeep00.model")
        s = root / "stages"
        write(s / "Room" / "mapset_room00" / "gm_room00_bg" / "gm_room00_bg.model", b"bg")
        write(s / "Room" / "mapset_room00" / "camera" / "room_st01.camanm", b"cam")
        write(s / "Room" / "mapset_room00_g" / "x.model")  # gold: left alone

    def test_find_model_folders(self):
        with tempfile.TemporaryDirectory() as t:
            root = Path(t)
            self.tree(root)
            got = sorted(f.relative_to(root).as_posix() for k in p.KINDS for f in p.find_model_folders(root / k))
            self.assertEqual(
                got,
                [
                    "dancers/Friendly/pl_fr00",
                    "dancers/Src/Name/pl_deep00",
                    "dancers/pl_flat00",
                    "stages/Room/mapset_room00",
                ],
            )

    def test_in_place(self):
        with tempfile.TemporaryDirectory() as t:
            root = Path(t)
            self.tree(root)
            self.assertEqual(run_quiet(root, None, dry_run=False), 0)
            arc = root / "dancers" / "Friendly" / "pl_fr00.arc"
            self.assertTrue(arc.is_file())
            self.assertFalse((root / "dancers" / "Friendly" / "pl_fr00").exists())
            self.assertTrue((root / "dancers" / "Friendly" / "chara_resources.rlist.txt").is_file())
            names = [e[0] for e in p.arc_entries(arc.read_bytes())]
            self.assertEqual(names, ["data/chara/pl_fr00/motion/a.anm", "data/chara/pl_fr00/pl_fr00.model"])
            stage = [e[0] for e in p.arc_entries((root / "stages" / "Room" / "mapset_room00.arc").read_bytes())]
            self.assertEqual(
                stage,
                ["data/camera/room00/room_st01.camanm", "data/map/gm_room00_bg/gm_room00_bg.model"],
            )
            self.assertTrue((root / "stages" / "Room" / "mapset_room00_g").is_dir())
            self.assertTrue((root / "dancers" / "Src" / "Name" / "Extra" / "pl_toodeep00").is_dir())
            # A second run finds nothing left to pack.
            self.assertEqual(run_quiet(root, None, dry_run=False), 0)

    def test_out_mirror(self):
        with tempfile.TemporaryDirectory() as t:
            src, dst = Path(t) / "src", Path(t) / "dst"
            self.tree(src)
            self.assertEqual(run_quiet(src, dst, dry_run=False), 0)
            # Source untouched; mirror holds arcs + every non-model file.
            self.assertTrue((src / "dancers" / "Friendly" / "pl_fr00").is_dir())
            self.assertTrue((dst / "dancers" / "Friendly" / "pl_fr00.arc").is_file())
            self.assertFalse((dst / "dancers" / "Friendly" / "pl_fr00").exists())
            self.assertTrue((dst / "dancers" / "Friendly" / "chara_resources.rlist.txt").is_file())
            self.assertTrue((dst / "dancers" / "pl_flat00.arc").is_file())
            self.assertFalse(list(dst.rglob(".DS_Store")))
            # Byte-identical to an in-place pack of the same content.
            self.assertEqual(
                (dst / "dancers" / "Friendly" / "pl_fr00.arc").read_bytes(),
                p.pack_folder(src / "dancers" / "Friendly" / "pl_fr00")[0],
            )

    def test_existing_arc_is_not_clobbered(self):
        with tempfile.TemporaryDirectory() as t:
            root = Path(t)
            write(root / "dancers" / "F" / "pl_a00" / "pl_a00.model", b"new")
            write(root / "dancers" / "F" / "pl_a00.arc", b"old")
            self.assertEqual(run_quiet(root, None, dry_run=False), 1)
            self.assertEqual((root / "dancers" / "F" / "pl_a00.arc").read_bytes(), b"old")
            self.assertTrue((root / "dancers" / "F" / "pl_a00").is_dir())

    def test_dry_run_writes_nothing(self):
        with tempfile.TemporaryDirectory() as t:
            root = Path(t)
            self.tree(root)
            before = sorted(x.relative_to(root).as_posix() for x in root.rglob("*"))
            self.assertEqual(run_quiet(root, None, dry_run=True), 0)
            self.assertEqual(sorted(x.relative_to(root).as_posix() for x in root.rglob("*")), before)


if __name__ == "__main__":
    unittest.main()
