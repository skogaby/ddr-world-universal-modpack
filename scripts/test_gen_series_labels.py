"""Host-only tests for gen_series_labels.py (no game assets required)."""
import io
import json
import tempfile
import unittest
from pathlib import Path

import gen_series_labels as gen


class RenderTests(unittest.TestCase):
    def test_every_canonical_label_has_the_exact_canvas(self):
        for entry in gen.CANONICAL + gen.CANONICAL_GROUPS:
            for cols in gen.COLUMNS:
                img, _ = gen.render_label(gen.text_for(entry, cols), cols)
                self.assertEqual(img.mode, "RGBA")
                self.assertEqual(img.size, (gen.CANVAS_W[cols], gen.CANVAS_H), (entry.key, cols))

    def test_ink_stays_within_the_width_limit(self):
        texts = [gen.text_for(e, c) for e in gen.CANONICAL + gen.CANONICAL_GROUPS for c in gen.COLUMNS]
        texts += ["SUPERCALIFRAGILISTIC", "A VERY LONG SERIES NAME"]
        for text in texts:
            for cols in gen.COLUMNS:
                img, _ = gen.render_label(text, cols)
                bbox = img.getchannel("A").getbbox()
                self.assertIsNotNone(bbox, (text, cols))
                self.assertLessEqual(bbox[2], gen.INK_LIMIT[gen.CANVAS_W[cols]], (text, cols))

    def test_group_labels_stack_when_narrow(self):
        _, layout = gen.render_label("GROUP CLASSIC", 3)
        self.assertEqual(layout.mode, "stacked")
        self.assertEqual(layout.lines, ("GROUP", "CLASSIC"))
        _, layout = gen.render_label("GROUP GOLD", 1)
        self.assertEqual(layout.mode, "fit")

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
        self.assertEqual(block["num_group_columns"], 4)
        self.assertEqual(
            block["groups"],
            [
                {"texture": "group_gold", "series_start": 18, "series_end": 21},
                {"texture": "group_white", "series_start": 14, "series_end": 17},
                {"texture": "group_classic", "series_start": 1, "series_end": 13},
                {"texture": "no_flare", "series_start": 22, "series_end": 255},
            ],
        )
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

    def test_canonical_block_matches_the_model_fixture(self):
        fixture = (
            gen.REPO_ROOT / "src" / "mods" / "series_expansion" / "enhanced" / "testdata" / "canonical_enhanced.json"
        )
        stored = json.loads(fixture.read_text(encoding="utf-8"))
        self.assertEqual(stored, {"custom_series_enhanced": gen.canonical_config()})

    def test_cells_from_config(self):
        config = {
            "series_expansion": {
                "custom_series_enhanced": {
                    "num_columns": 2,
                    "groups": [
                        {"texture": "group_gold", "series_start": 18, "series_end": 21},
                        {"type": "BREAK", "thickness": 4},
                        {"texture": "ruby_tab", "label": "RUBY TAB", "series_start": 30},
                        {"texture": "mystery", "series_start": 1},
                    ],
                    "filters": [
                        {"label": "WORLD RUBY", "series_start": 30, "texture": "world_ruby"},
                        {"type": "BREAK", "thickness": 5},
                        {"thickness": 2},
                        {"label": "Bad", "series_start": 31, "texture": "Bad-Name"},
                        {"type": "BREAK", "thickness": 300},
                        {"label": "WORLD RUBY", "series_start": 30, "texture": "world_ruby"},
                    ],
                }
            }
        }
        cells, warnings = gen.cells_from_config(config)
        self.assertEqual(
            [(c.key, c.text) for c in cells],
            [("group_gold", "GROUP GOLD"), ("ruby_tab", "RUBY TAB"), ("world_ruby", "WORLD RUBY")],
        )
        # mystery (no label, no canonical text), Bad-Name, thickness 300
        self.assertEqual(len(warnings), 3, warnings)

    def test_cells_from_config_without_block(self):
        cells, warnings = gen.cells_from_config({"series_expansion": {}})
        self.assertEqual(cells, [])
        self.assertEqual(len(warnings), 1)

    def test_break_detection(self):
        self.assertTrue(gen.is_break({"type": "BREAK", "thickness": 5}))
        self.assertTrue(gen.is_break({"type": "break"}))
        self.assertTrue(gen.is_break({"thickness": 0}))
        self.assertFalse(gen.is_break({"texture": "a", "series_start": 1}))
        self.assertFalse(gen.is_break({"type": "FILTER"}))
        self.assertEqual(gen.break_thickness({"type": "BREAK"}), 0)
        self.assertIsNone(gen.break_thickness({"thickness": 2.5}))
        self.assertIsNone(gen.break_thickness({"thickness": True}))

    def test_write_labels_writes_every_width(self):
        with tempfile.TemporaryDirectory() as tmp:
            cells = [gen.Entry("world_ruby", "WORLD RUBY", 30, 30)]
            written = gen.write_labels(cells, Path(tmp))
            names = sorted(p.name for p in written)
            self.assertEqual(names, [f"sefi_version_world_ruby_{n}col.png" for n in gen.COLUMNS])


class LayoutTests(unittest.TestCase):
    def _tops(self, block):
        parsed, warnings = gen.parse_block(block)
        self.assertEqual(warnings, [])
        return [(kind, y) for kind, _, _, y in gen.layout(parsed)]

    def test_stock_tabs_then_cells(self):
        tops = self._tops({"num_columns": 3, "filters": gen.canonical_config()["filters"][:4]})
        self.assertEqual(tops[:3], [("stock", 0)] * 3)
        self.assertEqual(tops[3:], [("cell", 26)] * 3 + [("cell", 52)])

    def test_breaks_and_separator_mirror_the_model(self):
        # Same cases as model.rs config_tabs_wrap_breaks_and_separator.
        tab = lambda k: {"texture": "group_gold", "series_start": k}
        cell = lambda k: {"label": "A", "series_start": k, "texture": "a"}
        tops = self._tops({"num_columns": 5, "groups": [tab(1), tab(2), tab(3), tab(4)], "filters": [cell(1), cell(2)]})
        self.assertEqual([y for _, y in tops], [0, 0, 0, 26, 52, 52, 52])
        self.assertEqual(tops[4][0], "break")
        tops = self._tops({
            "num_columns": 5,
            "num_group_columns": 5,
            "groups": [tab(1), {"type": "BREAK", "thickness": 4}, tab(2), {"type": "BREAK", "thickness": 3}],
            "filters": [cell(1)],
        })
        self.assertEqual([y for _, y in tops], [0, 26, 30, 56, 59])


class PreviewTests(unittest.TestCase):
    def test_preview_without_stock_art(self):
        sheet = gen.build_preview(gen.canonical_config(), stock_dir=None)
        self.assertGreater(sheet.width, 5 * gen.PANEL_W)
        self.assertGreater(sheet.height, gen.PANEL_H)

    def test_preview_with_stock_tabs_and_breaks(self):
        block = {
            "num_columns": 2,
            "filters": [
                {"label": "WORLD", "series_start": 21, "texture": "world"},
                {"type": "BREAK", "thickness": 10},
                {"label": "A3", "series_start": 20, "texture": "a3"},
            ],
        }
        sheet = gen.build_preview(block, stock_dir=None)
        self.assertGreater(sheet.width, 5 * gen.PANEL_W)


class FontTests(unittest.TestCase):
    def test_missing_font_names_the_repo_relative_path(self):
        with self.assertRaises(SystemExit) as ctx:
            gen.load_font(gen.REPO_ROOT / "scripts" / "fonts" / "missing.otf", 15)
        self.assertIn("scripts/fonts/missing.otf", str(ctx.exception))


if __name__ == "__main__":
    unittest.main()
