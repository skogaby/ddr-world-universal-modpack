#!/usr/bin/env python3
"""Host tests for scripts/build_packs.py (optional content packs).

Pins pack identity (ids, content ids), the shipped form (parity with
`pack_custom_models.run`), the marker, the zip layout, catalog validation and the
published-catalog composition — and that the committed fixture under
`updater/tests/fixtures/packs/` (which the updater's own tests read) is still what the
tool produces.

Run: (cd scripts && python3 -m unittest -q test_build_packs)
"""

import contextlib
import copy
import io
import json
import os
import tempfile
import unittest
import zipfile
from pathlib import Path

import build_packs as b
import pack_custom_models as p

FIXTURE = Path(__file__).resolve().parent.parent / "updater" / "tests" / "fixtures" / "packs"


def quiet(fn, *args, **kwargs):
    with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
        return fn(*args, **kwargs)


def write(path: Path, data: bytes = b"x") -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)


def make_assets(root: Path) -> None:
    """Two dancer sources and one stage source, with the shapes the scanner accepts."""
    d = root / "dancers" / "3rd MIX"
    write(d / "Afro" / "pl_ddr3afro00" / "pl_ddr3afro00.model", b"afro model")
    write(d / "Afro" / "pl_ddr3afro00" / "motion" / "a.anm", b"afro anm")
    write(d / "Afro" / "pl_ddr3afro00_head00" / "pl_ddr3afro00_head00.model", b"afro head")
    write(d / "Afro" / "chara_resources.rlist.txt", b"ddr3afro00,0\n")
    write(d / "Lady" / "pl_ddr3lady00" / "pl_ddr3lady00.model", b"lady model")
    write(d / "pl_ddr3flat00" / "pl_ddr3flat00.model", b"flat model")  # model directly in the source
    write(d / "notes" / "deep" / "readme.txt", b"kept verbatim")
    write(d / ".DS_Store", b"junk")
    write(root / "dancers" / "X & X2" / "Emi" / "pl_xemi00" / "pl_xemi00.model", b"emi")
    s = root / "stages" / "SUPERNOVA 1 & 2" / "Stage 01"
    write(s / "mapset_sn01" / "gm_sn01_floor" / "gm_sn01_floor.model", b"floor")
    write(s / "mapset_sn01" / "camera" / "main.camanm", b"cam")
    write(s / "mapset_sn01_g" / "gm_sn01_g_floor" / "x.model", b"gold, never packed")
    write(s / "map_resources.rlist.txt", b"sn01,ffffff\n")


def tree(root: Path) -> dict:
    return {
        f.relative_to(root).as_posix(): f.read_bytes()
        for f in sorted(root.rglob("*"))
        if f.is_file()
    }


class Identity(unittest.TestCase):
    def test_pack_ids(self):
        cases = {
            ("dancers", "3rd MIX"): "dancers-3rd-mix",
            ("stages", "SUPERNOVA 1 & 2"): "stages-supernova-1-2",
            ("dancers", "X & X2"): "dancers-x-x2",
            ("dancers", "HOTTSTPARTY 1-3"): "dancers-hottstparty-1-3",
            ("stages", "  --Caf\u00e9 Zone!! "): "stages-caf-zone",
        }
        for (kind, name), want in cases.items():
            self.assertEqual(b.pack_id(kind, name), want)
            self.assertRegex(want, b.ID_RE)
        with self.assertRaises(b.BuildError):
            b.pack_id("dancers", "\u30c0\u30f3\u30b5\u30fc")

    def test_content_id_depends_on_paths_and_hashes_only(self):
        files = {"b/2": (5, "bb"), "a/1": (3, "aa")}
        base = b.content_id(files)
        self.assertEqual(base, b.content_id(dict(reversed(list(files.items())))), "order")
        self.assertEqual(base, b.content_id({"b/2": (999, "bb"), "a/1": (3, "aa")}), "size is not hashed")
        self.assertNotEqual(base, b.content_id({"b/2": (5, "bc"), "a/1": (3, "aa")}))
        self.assertNotEqual(base, b.content_id({"b/3": (5, "bb"), "a/1": (3, "aa")}))
        import hashlib

        self.assertEqual(base, hashlib.sha256(b"a/1\naa\nb/2\nbb\n").hexdigest())


class Sources(unittest.TestCase):
    def test_find_sources_order_and_fields(self):
        with tempfile.TemporaryDirectory() as t:
            root = Path(t)
            make_assets(root)
            got = b.find_sources(root)
            self.assertEqual(
                [(s.kind, s.title, s.pack_id) for s in got],
                [
                    ("dancers", "3rd MIX", "dancers-3rd-mix"),
                    ("dancers", "X & X2", "dancers-x-x2"),
                    ("stages", "SUPERNOVA 1 & 2", "stages-supernova-1-2"),
                ],
            )
            self.assertEqual(got[0].root, "data_mods/custom_models/dancers/3rd MIX")
            self.assertEqual(got[0].marker, "data_mods/custom_models/.packs/dancers-3rd-mix.json")
            self.assertEqual([s.pack_id for s in b.select(got, ["dancers-x-x2"])], ["dancers-x-x2"])
            with self.assertRaises(b.BuildError):
                b.select(got, ["dancers-nope"])

    def test_refuses_content_outside_a_source_and_id_collisions(self):
        with tempfile.TemporaryDirectory() as t:
            root = Path(t)
            write(root / "dancers" / "pl_stray00" / "pl_stray00.model")
            with self.assertRaisesRegex(b.BuildError, "belongs to no pack"):
                b.find_sources(root)
        with tempfile.TemporaryDirectory() as t:
            root = Path(t)
            write(root / "dancers" / "chara_resources.rlist.txt")
            with self.assertRaisesRegex(b.BuildError, "belong to no pack"):
                b.find_sources(root)
        with tempfile.TemporaryDirectory() as t:
            root = Path(t)
            write(root / "dancers" / "X & X2" / "A" / "pl_a00" / "pl_a00.model")
            write(root / "dancers" / "X - X2" / "B" / "pl_b00" / "pl_b00.model")
            with self.assertRaisesRegex(b.BuildError, "both yield the pack id"):
                b.find_sources(root)
        with tempfile.TemporaryDirectory() as t:
            with self.assertRaisesRegex(b.BuildError, "neither dancers/ nor stages/"):
                b.find_sources(Path(t))


class ShippedForm(unittest.TestCase):
    def test_materialize_matches_the_release_packer(self):
        """A pack's files are exactly what `pack_custom_models.run` leaves for that
        source — the same arcs, sidecars and stray files — so the DLL sees the
        models it would have seen in a single-zip release."""
        with tempfile.TemporaryDirectory() as t:
            root, mirror, image = Path(t) / "assets", Path(t) / "mirror", Path(t) / "image"
            make_assets(root)
            self.assertEqual(quiet(p.run, root, mirror, False), 0)
            want = tree(mirror)
            got = {}
            for source in b.find_sources(root):
                built = b.materialize(source, image)
                for rel, (size, sha) in built.files.items():
                    self.assertTrue(rel.startswith(source.root + "/"), rel)
                    data = (image / rel).read_bytes()
                    self.assertEqual((len(data), b.sha256_file(image / rel)), (size, sha))
                    got[rel[len("data_mods/custom_models/"):]] = data
            self.assertEqual(got, want)
            self.assertNotIn("dancers/3rd MIX/.DS_Store", got)
            self.assertIn("dancers/3rd MIX/notes/deep/readme.txt", got)
            self.assertIn("dancers/3rd MIX/pl_ddr3flat00.arc", got)

    def test_items_count_bodies_and_stages_not_parts(self):
        with tempfile.TemporaryDirectory() as t:
            root, image = Path(t) / "assets", Path(t) / "image"
            make_assets(root)
            items = {s.pack_id: b.materialize(s, image).items for s in b.find_sources(root)}
            self.assertEqual(
                items, {"dancers-3rd-mix": 3, "dancers-x-x2": 1, "stages-supernova-1-2": 1}
            )

    def test_content_id_is_stable_and_sensitive(self):
        with tempfile.TemporaryDirectory() as t:
            root = Path(t) / "assets"
            make_assets(root)
            source = b.find_sources(root)[0]
            first = b.materialize(source, Path(t) / "i1").content_id
            for f in root.rglob("*"):
                os.utime(f, (1_000_000_000, 1_000_000_000))
            self.assertEqual(b.materialize(source, Path(t) / "i2").content_id, first)
            target = root / "dancers" / "3rd MIX" / "Afro" / "pl_ddr3afro00" / "motion" / "a.anm"
            target.write_bytes(b"afro anM")
            self.assertNotEqual(b.materialize(source, Path(t) / "i3").content_id, first)

    def test_existing_arc_beside_a_model_folder_is_an_error(self):
        with tempfile.TemporaryDirectory() as t:
            root = Path(t) / "assets"
            write(root / "dancers" / "S" / "A" / "pl_a00" / "pl_a00.model")
            write(root / "dancers" / "S" / "A" / "pl_a00.arc")
            with self.assertRaisesRegex(b.BuildError, "already exists beside"):
                b.materialize(b.find_sources(root)[0], Path(t) / "image")


class MarkerAndZip(unittest.TestCase):
    def test_marker_lists_every_file_and_zip_ends_with_it(self):
        with tempfile.TemporaryDirectory() as t:
            root, image = Path(t) / "assets", Path(t) / "image"
            make_assets(root)
            built = b.materialize(b.find_sources(root)[0], image)
            marker = json.loads(b.marker_bytes(built))
            self.assertEqual(
                (marker["schema"], marker["id"], marker["content_id"]),
                (1, "dancers-3rd-mix", built.content_id),
            )
            self.assertEqual(
                marker["files"],
                {rel: {"size": size, "sha256": sha} for rel, (size, sha) in built.files.items()},
            )
            self.assertNotIn(built.source.marker, marker["files"])

            zip_path = Path(t) / built.asset
            b.write_zip(built, image, zip_path)
            with zipfile.ZipFile(zip_path) as zf:
                names = zf.namelist()
                self.assertEqual(names[-1], built.source.marker)
                self.assertEqual(names[:-1], sorted(built.files, key=lambda n: n.encode()))
                self.assertEqual(zf.read(built.source.marker), b.marker_bytes(built))
                for zi in zf.infolist():
                    self.assertEqual(zi.date_time, b.ZIP_DATE)
                    self.assertEqual(zi.compress_type, zipfile.ZIP_DEFLATED)
                    self.assertFalse(zi.is_dir())
                for rel in built.files:
                    self.assertEqual(zf.read(rel), (image / rel).read_bytes())
            self.assertRegex(built.asset, r"^ddr-world-pack-dancers-3rd-mix-[0-9a-f]{12}\.zip$")
            self.assertTrue(built.asset.endswith(built.content_id[:12] + ".zip"))


class Commands(unittest.TestCase):
    def test_build_writes_zips_and_a_valid_catalog_and_skips_published_assets(self):
        with tempfile.TemporaryDirectory() as t:
            root, out = Path(t) / "assets", Path(t) / "out"
            make_assets(root)
            self.assertEqual(quiet(b.cmd_build, root, out, [], b.URL_BASE, None), 0)
            catalog = json.loads((out / "catalog.json").read_text())
            self.assertEqual(b.catalog_problems(catalog), [])
            self.assertEqual(
                [(e["id"], e["group"], e["title"]) for e in catalog["packs"]],
                [
                    ("dancers-3rd-mix", "Dancers", "3rd MIX"),
                    ("dancers-x-x2", "Dancers", "X & X2"),
                    ("stages-supernova-1-2", "Stages", "SUPERNOVA 1 & 2"),
                ],
            )
            for e in catalog["packs"]:
                z = out / e["asset"]
                self.assertEqual((z.stat().st_size, b.sha256_file(z)), (e["zip_size"], e["zip_sha256"]))
                self.assertEqual(e["url"], b.URL_BASE + e["asset"])
            self.assertFalse((out / ".work").exists())

            # A second build names the same assets; with one of them published,
            # its zip is not written and its zip fields are left open.
            out2 = Path(t) / "out2"
            published = catalog["packs"][0]["asset"]
            self.assertEqual(quiet(b.cmd_build, root, out2, [published], b.URL_BASE, None), 0)
            catalog2 = json.loads((out2 / "catalog.json").read_text())
            self.assertEqual([e["asset"] for e in catalog2["packs"]], [e["asset"] for e in catalog["packs"]])
            self.assertFalse((out2 / published).exists())
            self.assertEqual((catalog2["packs"][0]["zip_size"], catalog2["packs"][0]["zip_sha256"]), (None, None))
            self.assertTrue((out2 / catalog2["packs"][1]["asset"]).exists())
            self.assertNotEqual(b.catalog_problems(catalog2), [])
            self.assertEqual(b.catalog_problems(catalog2, allow_unbuilt=True), [])

    def test_stage_writes_files_and_markers_into_an_image(self):
        with tempfile.TemporaryDirectory() as t:
            root, image = Path(t) / "assets", Path(t) / "image"
            make_assets(root)
            self.assertEqual(quiet(b.cmd_stage, root, image, ["stages-supernova-1-2"]), 0)
            marker = json.loads(
                (image / "data_mods/custom_models/.packs/stages-supernova-1-2.json").read_text()
            )
            for rel, rec in marker["files"].items():
                self.assertEqual((image / rel).stat().st_size, rec["size"])
            self.assertFalse((image / "data_mods/custom_models/dancers").exists())

    def test_main_reports_input_errors_with_exit_code_2(self):
        with tempfile.TemporaryDirectory() as t:
            self.assertEqual(quiet(b.main, ["build", "--assets", t, "--out", str(Path(t) / "o")]), 2)


def valid_catalog() -> dict:
    return json.loads((FIXTURE / "out" / "catalog.json").read_text(encoding="utf-8"))


class CatalogValidation(unittest.TestCase):
    def test_the_fixture_catalog_is_valid(self):
        self.assertEqual(b.catalog_problems(valid_catalog()), [])

    def test_each_malformation_is_reported(self):
        def broken(edit):
            doc = valid_catalog()
            edit(doc)
            return b.catalog_problems(doc)

        def setp(key, value):
            return lambda d: d["packs"][0].__setitem__(key, value)

        cid = valid_catalog()["packs"][0]["content_id"]
        cases = {
            "schema": lambda d: d.__setitem__("schema", 2),
            "`packs` is not a list": lambda d: d.__setitem__("packs", {}),
            "`id`": setp("id", "Dancers_3rd"),
            "duplicate id": lambda d: d["packs"].append(copy.deepcopy(d["packs"][0])),
            "`title`": setp("title", ""),
            "`items`": setp("items", -1),
            "`root`": setp("root", "data_mods/custom_models/dancers/../../x"),
            "root`": setp("root", "data_mods/other/dancers/X"),
            "`root` must": setp("root", "data_mods/custom_models/dancers/A/B"),
            "`content_id`": setp("content_id", "abc"),
            "`asset` must": setp("asset", "ddr-world-pack-dancers-fixture-mix-000000000000.zip"),
            "https": setp("url", "http://example.invalid/" + valid_catalog()["packs"][0]["asset"]),
            "end with the asset": setp("url", "https://example.invalid/other.zip"),
            "`zip_size`": setp("zip_size", 0),
            "`zip_sha256`": setp("zip_sha256", cid.upper()),
            "`installed_size`": setp("installed_size", None),
        }
        for needle, edit in cases.items():
            problems = broken(edit)
            self.assertTrue(any(needle in x for x in problems), (needle, problems))
        self.assertEqual(b.catalog_problems([]), ["the catalog is not a JSON object"])

    def test_check_command_exit_codes(self):
        self.assertEqual(quiet(b.cmd_check, FIXTURE / "out" / "catalog.json", False), 0)
        with tempfile.TemporaryDirectory() as t:
            bad = Path(t) / "c.json"
            bad.write_text('{"schema": 1, "packs": [{}]}')
            self.assertEqual(quiet(b.cmd_check, bad, False), 1)
            bad.write_text("not json")
            self.assertEqual(quiet(b.cmd_check, bad, False), 1)


class PublishedCatalog(unittest.TestCase):
    def shelf(self, built: dict) -> dict:
        return {
            "tag_name": "packs",
            "assets": [
                {"name": "catalog.json", "size": 10, "digest": "sha256:" + "0" * 64, "state": "uploaded"}
            ]
            + [
                {
                    "name": e["asset"],
                    "size": 1000 + i,
                    "digest": "sha256:" + f"{i:x}" * 64,
                    "state": "uploaded",
                }
                for i, e in enumerate(built["packs"], 1)
            ],
        }

    def test_shelf_size_and_digest_win(self):
        built = valid_catalog()
        doc = b.published_catalog(built, self.shelf(built))
        self.assertEqual(b.catalog_problems(doc), [])
        self.assertEqual([(e["zip_size"], e["zip_sha256"]) for e in doc["packs"]], [(1001, "1" * 64), (1002, "2" * 64)])
        for got, src in zip(doc["packs"], built["packs"]):
            for key in ("id", "group", "title", "items", "root", "content_id", "asset", "url", "installed_size"):
                self.assertEqual(got[key], src[key])

    def test_unbuilt_packs_are_filled_from_the_shelf(self):
        built = valid_catalog()
        built["packs"][0]["zip_size"] = built["packs"][0]["zip_sha256"] = None
        doc = b.published_catalog(built, self.shelf(valid_catalog()))
        self.assertEqual(doc["packs"][0]["zip_size"], 1001)

    def test_missing_or_undigested_assets_fail(self):
        built = valid_catalog()
        shelf = self.shelf(built)
        shelf["assets"] = [a for a in shelf["assets"] if a["name"] != built["packs"][1]["asset"]]
        with self.assertRaisesRegex(b.BuildError, "is not on the shelf"):
            b.published_catalog(built, shelf)
        for digest in (None, "md5:abcd", "sha256:xyz"):
            shelf = self.shelf(built)
            shelf["assets"][1]["digest"] = digest
            with self.assertRaisesRegex(b.BuildError, "no sha256 digest"):
                b.published_catalog(built, shelf)
        shelf = self.shelf(built)
        shelf["assets"][1]["state"] = "open"
        with self.assertRaisesRegex(b.BuildError, "still uploading"):
            b.published_catalog(built, shelf)


class BaseImage(unittest.TestCase):
    """`base-image`: what a release build does to its staged image."""

    def image(self, root: Path) -> Path:
        image = root / "image"
        write(image / "ddr_world_hook.dll", b"dll")
        base = image / "data_mods" / "custom_models"
        write(base / "dancers" / "Custom" / "Miku" / "pl_miku00.arc", b"miku")
        write(base / "stages" / "Custom" / "Room" / "mapset_room00.arc", b"room")
        return image

    def published(self) -> bytes:
        return (FIXTURE / "out" / "catalog.json").read_bytes()

    def run_image(self, image, catalog_file=None, fetch=None):
        err = io.StringIO()
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(err):
            code = b.cmd_base_image(image, catalog_file, "https://example.invalid/catalog.json", fetch or (lambda url: self.published()))
        return code, err.getvalue()

    def test_embeds_the_published_catalog_byte_for_byte(self):
        with tempfile.TemporaryDirectory() as t:
            image = self.image(Path(t))
            code, warnings = self.run_image(image)
            self.assertEqual((code, warnings), (0, ""))
            self.assertEqual((image / "ddr_world_hook_packs.json").read_bytes(), self.published())

    def test_only_the_custom_source_stays_in_a_base_image(self):
        with tempfile.TemporaryDirectory() as t:
            image = self.image(Path(t))
            base = image / "data_mods" / "custom_models"
            write(base / "dancers" / "3rd MIX" / "Afro" / "pl_afro00.arc", b"stray copy")
            write(base / "dancers" / "pl_loose00" / "pl_loose00.model", b"stray model folder")
            write(base / "stages" / "notes.txt", b"stray file")
            code, warnings = self.run_image(image)
            self.assertEqual(code, 0)
            for name in ("dancers/3rd MIX", "dancers/pl_loose00", "stages/notes.txt"):
                self.assertIn(name, warnings)
            kept = sorted(p.relative_to(base).as_posix() for p in base.rglob("*") if p.is_file())
            self.assertEqual(
                kept,
                ["dancers/Custom/Miku/pl_miku00.arc", "stages/Custom/Room/mapset_room00.arc"],
            )

    def test_the_base_source_is_recognised_by_slug_not_spelling(self):
        # The DLL treats any folder whose slug is `custom` as the CUSTOM source.
        with tempfile.TemporaryDirectory() as t:
            image = Path(t) / "image"
            base = image / "data_mods" / "custom_models"
            write(base / "dancers" / "CUSTOM!" / "Teto" / "pl_teto00.arc", b"teto")
            self.assertEqual(b.prune_base_sources(image), [])
            self.assertTrue((base / "dancers" / "CUSTOM!" / "Teto" / "pl_teto00.arc").exists())

    def test_no_catalog_is_an_error_unless_one_is_supplied(self):
        def offline(url):
            raise OSError("network unreachable")

        with tempfile.TemporaryDirectory() as t:
            image = self.image(Path(t))
            with self.assertRaisesRegex(b.BuildError, "cannot download the pack catalog"):
                self.run_image(image, fetch=offline)
            self.assertFalse((image / "ddr_world_hook_packs.json").exists())

            supplied = Path(t) / "mine.json"
            supplied.write_bytes(self.published())
            code, warnings = self.run_image(image, catalog_file=supplied, fetch=offline)
            self.assertEqual(code, 0)
            self.assertIn("cannot be downloaded without credentials", warnings)
            self.assertEqual((image / "ddr_world_hook_packs.json").read_bytes(), self.published())

    def test_a_supplied_catalog_that_differs_from_the_published_one_is_flagged(self):
        with tempfile.TemporaryDirectory() as t:
            image = self.image(Path(t))
            supplied = Path(t) / "mine.json"
            supplied.write_bytes(self.published() + b"\n")
            code, warnings = self.run_image(image, catalog_file=supplied)
            self.assertEqual(code, 0)
            self.assertIn("differs from the published one", warnings)
            # Identical to the published one: no warning.
            supplied.write_bytes(self.published())
            self.assertEqual(self.run_image(image, catalog_file=supplied), (0, ""))

    def test_a_broken_catalog_fails_the_build(self):
        with tempfile.TemporaryDirectory() as t:
            image = self.image(Path(t))
            with self.assertRaisesRegex(b.BuildError, "is not JSON"):
                self.run_image(image, fetch=lambda url: b"<html>not found</html>")
            broken = json.loads(self.published())
            broken["packs"][0]["url"] = "http://example.invalid/x.zip"
            with self.assertRaisesRegex(b.BuildError, "is invalid"):
                self.run_image(image, fetch=lambda url: json.dumps(broken).encode())
            self.assertFalse((image / "ddr_world_hook_packs.json").exists())

    def test_an_image_the_installed_updater_would_refuse_fails_the_build(self):
        with tempfile.TemporaryDirectory() as t:
            image = self.image(Path(t))
            limit = b.BASE_IMAGE_LIMIT
            try:
                b.BASE_IMAGE_LIMIT = b.image_size(image) + len(self.published())
                with self.assertRaisesRegex(b.BuildError, "refuses"):
                    self.run_image(image)
                b.BASE_IMAGE_LIMIT += 1
                self.assertEqual(self.run_image(image)[0], 0)
            finally:
                b.BASE_IMAGE_LIMIT = limit


class CommittedFixture(unittest.TestCase):
    """`updater/tests/fixtures/packs/out` is what the tool makes of `…/assets`."""

    def test_fixture_is_reproduced(self):
        committed = valid_catalog()
        with tempfile.TemporaryDirectory() as t:
            out = Path(t) / "out"
            self.assertEqual(quiet(b.cmd_build, FIXTURE / "assets", out, [], b.URL_BASE, None), 0)
            rebuilt = json.loads((out / "catalog.json").read_text(encoding="utf-8"))
            # Deflate output may differ between zlib builds: compare everything but
            # the zip's own size and digest, then the zips entry by entry.
            volatile = ("zip_size", "zip_sha256")
            strip = lambda doc: [{k: v for k, v in e.items() if k not in volatile} for e in doc["packs"]]
            self.assertEqual(strip(rebuilt), strip(committed))
            for e in committed["packs"]:
                old = FIXTURE / "out" / e["asset"]
                self.assertEqual((old.stat().st_size, b.sha256_file(old)), (e["zip_size"], e["zip_sha256"]))
                with zipfile.ZipFile(old) as a, zipfile.ZipFile(out / e["asset"]) as c:
                    self.assertEqual(a.namelist(), c.namelist())
                    for name in a.namelist():
                        self.assertEqual(a.read(name), c.read(name), name)


if __name__ == "__main__":
    unittest.main()
