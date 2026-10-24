"""Host-only tests for gen_stepmania_folder_art.py (no game assets required)."""
import io
import tempfile
import unittest
from pathlib import Path

from PIL import Image

import gen_series_labels as labels
import gen_stepmania_folder_art as art


def encode(img: Image.Image) -> bytes:
    buf = io.BytesIO()
    img.save(buf, format="PNG")
    return buf.getvalue()


def ink_bbox(img: Image.Image):
    return img.convert("RGBA").getchannel("A").getbbox()


class TextureTests(unittest.TestCase):
    def test_every_folder_texture_has_the_documented_size_and_mode(self):
        for name, (_, size, mode) in art.TEXTURES.items():
            img = art.render_texture(name)
            self.assertEqual(img.size, size, name)
            self.assertEqual(img.mode, mode, name)

    def test_the_seven_dogs_texture_names_map_one_to_one(self):
        dogs = sorted(
            p.name.replace("_dogs", "_stepmania")
            for p in (art.REPO_ROOT / "data_mods" / "custom_folders").rglob("*_dogs*.png")
        )
        self.assertEqual(len(dogs), 8)
        self.assertEqual(dogs, sorted(f"{name}.png" for name in art.TEXTURES))
        for name, (ifs_dir, _, _) in art.TEXTURES.items():
            ref = art.REPO_ROOT / "data_mods" / "custom_folders" / ifs_dir / "tex" / f"{name.replace('_stepmania', '_dogs')}.png"
            self.assertTrue(ref.is_file(), ref)

    def test_palette_textures_keep_transparency(self):
        for name in (
            "mufo_txt_folder_title_stepmania_on",
            "mufo_txt_folder_title_stepmania_off",
            "mufo_txt_folder_subtitle_stepmania_on",
            "mufo_txt_folder_subtitle_stepmania_off",
        ):
            img = art.render_texture(name)
            self.assertEqual(img.mode, "P", name)
            alpha = img.convert("RGBA").getchannel("A")
            lo, hi = alpha.getextrema()
            self.assertEqual(lo, 0, name)
            self.assertGreaterEqual(hi, 250, name)
            reloaded = Image.open(io.BytesIO(encode(img)))
            self.assertEqual(reloaded.mode, "P", name)
            self.assertIn("transparency", reloaded.info, name)

    def test_title_sits_in_the_lower_band_like_dogs(self):
        for state in ("on", "off"):
            bbox = ink_bbox(art.render_texture(f"mufo_txt_folder_title_stepmania_{state}"))
            self.assertIsNotNone(bbox)
            self.assertGreaterEqual(bbox[1], 60, state)
            self.assertLessEqual(bbox[3], 120, state)
            self.assertGreaterEqual(bbox[0], 8, state)
            self.assertLessEqual(bbox[2], 232, state)

    def test_on_and_off_variants_share_a_silhouette(self):
        for base in ("mufo_txt_folder_title_stepmania", "mufo_txt_folder_subtitle_stepmania"):
            on = art.render_texture(f"{base}_on").convert("RGBA").getchannel("A")
            off = art.render_texture(f"{base}_off").convert("RGBA").getchannel("A")
            self.assertEqual(on.getbbox(), off.getbbox(), base)

    def test_subtitle_is_centred_within_the_stock_rows(self):
        bbox = ink_bbox(art.render_texture("mufo_txt_folder_subtitle_stepmania_on"))
        self.assertGreaterEqual(bbox[1], 3)
        self.assertLessEqual(bbox[3], 22)
        self.assertLess(abs((bbox[0] + bbox[2]) / 2 - 120), 4)

    def test_info_banner_keeps_the_transparent_top_strip(self):
        img = art.render_texture("mufo_txt_folder_info_stepmania")
        alpha = img.getchannel("A")
        self.assertEqual(alpha.crop((0, 0, 488, art.INFO_TOP_CLEAR)).getextrema(), (0, 0))
        self.assertEqual(alpha.crop((0, art.INFO_TOP_CLEAR, 488, 104)).getextrema(), (255, 255))
        # White title and yellow text start at the stock x and stay on canvas.
        white = [x for x in range(488) for y in range(20, 60) if img.getpixel((x, y))[:3] == (255, 255, 255)]
        self.assertLess(min(white), 24)
        yellow = [
            (x, y)
            for x in range(488)
            for y in range(60, 104)
            if (lambda p: p[0] > 200 and p[1] > 180 and p[2] < 80)(img.getpixel((x, y)))
        ]
        self.assertGreater(len(yellow), 100)
        self.assertLess(min(x for x, _ in yellow), 24)
        self.assertLess(max(x for x, _ in yellow), 484)

    def test_card_strip_is_centred_and_two_lines(self):
        img = art.render_texture("muca_txt_folder_name_stepmania")
        bbox = ink_bbox(img)
        self.assertLess(abs((bbox[0] + bbox[2]) / 2 - 110), 4)
        self.assertGreaterEqual(bbox[1], 4)
        self.assertLessEqual(bbox[3], 54)
        # The second line is narrower than the first.
        width_at = lambda y: img.crop((0, y, 220, y + 1)).getchannel("A").getbbox()
        top_line, bottom_line = width_at(20), width_at(44)
        self.assertLess(bottom_line[2] - bottom_line[0], top_line[2] - top_line[0])

    def test_back_variants(self):
        on = art.render_texture("mufo_folder_back_stepmania_on")
        off = art.render_texture("mufo_folder_back_stepmania_off")
        self.assertEqual(on.mode, "RGB")
        self.assertEqual(off.mode, "P")
        grey = off.convert("RGB")
        r, g, b = grey.split()
        self.assertEqual(encode(r), encode(g))
        self.assertEqual(encode(g), encode(b))
        # Arrows are coloured when on: distinct hues in the receptor row.
        hues = {on.getpixel((x, 45)) for x in range(0, 240, 2) if max(on.getpixel((x, 45))) > 150}
        self.assertGreaterEqual(len(hues), 4)


class LabelTests(unittest.TestCase):
    def test_series_labels_use_the_shared_renderer(self):
        for cols in labels.COLUMNS:
            img = art.render_series_label(cols)
            self.assertEqual(img.mode, "RGBA")
            self.assertEqual(img.size, (labels.CANVAS_W[cols], labels.CANVAS_H))
            self.assertEqual(encode(img), encode(labels.render_label("STEPMANIA", cols)[0]))
            bbox = img.getchannel("A").getbbox()
            self.assertLessEqual(bbox[2], labels.INK_LIMIT[labels.CANVAS_W[cols]], cols)

    def test_legacy_label_is_the_two_column_canvas(self):
        self.assertEqual(art.render_series_label(2).size, (104, 20))


class OutputTests(unittest.TestCase):
    def test_write_all_lays_out_the_mod_folder(self):
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp)
            written = art.write_all(out)
            rel = sorted(p.relative_to(out).as_posix() for p in written)
            expected = sorted(
                [f"{ifs}/tex/{name}.png" for name, (ifs, _, _) in art.TEXTURES.items()]
                + ["select_music_option_v3_ifs/tex/sefi_version_stepmania.png"]
                + [f"series_labels/sefi_version_stepmania_{n}col.png" for n in labels.COLUMNS]
            )
            self.assertEqual(rel, expected)
            self.assertEqual(len(rel), 14)
            for path in written:
                with Image.open(path) as img:
                    img.load()
                    name = path.stem
                    if name in art.TEXTURES:
                        _, size, mode = art.TEXTURES[name]
                        self.assertEqual(img.size, size, name)
                        self.assertEqual(img.mode, mode, name)

    def test_rendering_is_deterministic(self):
        for name in art.TEXTURES:
            self.assertEqual(encode(art.render_texture(name)), encode(art.render_texture(name)), name)
        self.assertEqual(encode(art.render_series_label(3)), encode(art.render_series_label(3)))

    def test_committed_art_matches_the_generator(self):
        """The committed PNGs under data_mods/stepmania_packs are this
        script's output; regenerate (never hand-edit) when they differ."""
        out = art.DEFAULT_OUT_DIR
        if not out.is_dir():
            self.skipTest("no committed art")
        for name, (ifs, _, _) in art.TEXTURES.items():
            path = out / ifs / "tex" / f"{name}.png"
            self.assertTrue(path.is_file(), path)
            with Image.open(path) as committed:
                committed.load()
                self.assertEqual(encode(committed), encode(art.render_texture(name)), name)
        for cols in labels.COLUMNS:
            path = out / art.LABELS_DIR / labels.label_filename(art.KEY, cols)
            with Image.open(path) as committed:
                committed.load()
                self.assertEqual(encode(committed), encode(art.render_series_label(cols)), cols)


if __name__ == "__main__":
    unittest.main()
