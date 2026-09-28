"""Host-only tests for gen_series_labels.py (no game assets required)."""
import io
import json
import tempfile
import unittest
from pathlib import Path

import gen_series_labels as gen


class RenderTests(unittest.TestCase):
    def test_every_canonical_label_has_the_exact_canvas(self):
        for entry in gen.CANONICAL:
            for cols in gen.COLUMNS:
                img, _ = gen.render_label(gen.text_for(entry, cols), cols)
                self.assertEqual(img.mode, "RGBA")
                self.assertEqual(img.size, (gen.CANVAS_W[cols], gen.CANVAS_H), (entry.key, cols))

    def test_ink_stays_within_the_width_limit(self):
        texts = [gen.text_for(e, c) for e in gen.CANONICAL for c in gen.COLUMNS]
        texts += ["SUPERCALIFRAGILISTIC", "A VERY LONG SERIES NAME"]
        for text in texts:
            for cols in gen.COLUMNS:
                img, _ = gen.render_label(text, cols)
                bbox = img.getchannel("A").getbbox()
                self.assertIsNotNone(bbox, (text, cols))
                self.assertLessEqual(bbox[2], gen.INK_LIMIT[gen.CANVAS_W[cols]], (text, cols))

    def test_single_words_condense_instead_of_splitting(self):
        _, layout = gen.render_label("EXTREME", 5)
        self.assertEqual(layout.mode, "condensed")
        self.assertEqual(layout.lines, ("EXTREME",))

    def test_camel_case_words_stack_at_the_case_boundary(self):
        _, layout = gen.render_label("SuperNOVA2", 5)
        self.assertEqual(layout.mode, "stacked")
        self.assertEqual(layout.lines, ("Super", "NOVA2"))

    def test_short_labels_fit_unchanged(self):
        _, layout = gen.render_label("A3", 2)
        self.assertEqual(layout.mode, "fit")

    def test_rendering_is_deterministic(self):
        def encode(text, cols):
            buf = io.BytesIO()
            gen.render_label(text, cols)[0].save(buf, format="PNG")
            return buf.getvalue()

        self.assertEqual(encode("X3 VS 2ndMIX", 3), encode("X3 VS 2ndMIX", 3))


class ConfigTests(unittest.TestCase):
    def test_canonical_config_block(self):
        block = gen.canonical_config()
        self.assertEqual(block["num_columns"], 3)
        filters = block["filters"]
        self.assertEqual(len(filters), 20)
        self.assertEqual(filters[0], {"label": "WORLD", "series_start": 21, "series_end": 21, "texture": "world"})
        self.assertEqual(
            next(f for f in filters if f["texture"] == "2014"),
            {"label": "2014", "series_start": 15, "series_end": 16, "texture": "2014"},
        )
        self.assertEqual(filters[-1]["texture"], "1stmix")
        self.assertEqual((filters[-1]["series_start"], filters[-1]["series_end"]), (1, 1))
        self.assertEqual(json.loads(json.dumps(block)), block)

    def test_cells_from_config(self):
        config = {
            "series_expansion": {
                "custom_series_enhanced": {
                    "num_columns": 2,
                    "filters": [
                        {"label": "WORLD RUBY", "series_start": 30, "texture": "world_ruby"},
                        {"label": "Bad", "series_start": 31, "texture": "Bad-Name"},
                        {"label": "WORLD RUBY", "series_start": 30, "texture": "world_ruby"},
                    ],
                }
            }
        }
        cells, warnings = gen.cells_from_config(config)
        self.assertEqual([(c.key, c.text) for c in cells], [("world_ruby", "WORLD RUBY")])
        self.assertEqual(len(warnings), 1)

    def test_cells_from_config_without_block(self):
        cells, warnings = gen.cells_from_config({"series_expansion": {}})
        self.assertEqual(cells, [])
        self.assertEqual(len(warnings), 1)

    def test_write_labels_writes_every_width(self):
        with tempfile.TemporaryDirectory() as tmp:
            cells = [gen.Entry("world_ruby", "WORLD RUBY", 30, 30)]
            written = gen.write_labels(cells, Path(tmp))
            names = sorted(p.name for p in written)
            self.assertEqual(names, [f"sefi_version_world_ruby_{n}col.png" for n in gen.COLUMNS])


class PreviewTests(unittest.TestCase):
    def test_preview_without_stock_art(self):
        sheet = gen.build_preview(gen.CANONICAL, stock_dir=None)
        self.assertGreater(sheet.width, 5 * gen.PANEL_W)
        self.assertGreater(sheet.height, gen.PANEL_H)


class FontTests(unittest.TestCase):
    def test_missing_font_names_the_repo_relative_path(self):
        with self.assertRaises(SystemExit) as ctx:
            gen.load_font(gen.REPO_ROOT / "scripts" / "fonts" / "missing.otf", 15)
        self.assertIn("scripts/fonts/missing.otf", str(ctx.exception))


if __name__ == "__main__":
    unittest.main()
