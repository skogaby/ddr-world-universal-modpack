"""Host-only tests for gen_title_labels.py (no game assets required)."""
import io
import string
import tempfile
import unittest
from pathlib import Path

from PIL import Image

import gen_series_labels as series
import gen_title_labels as gen


class LabelTests(unittest.TestCase):
    def test_names_match_the_mod_layout(self):
        names = [name for name, _, _ in gen.labels()]
        self.assertEqual(len(names), 27)
        self.assertEqual(names[0], "sefi_title_a_5col.png")
        self.assertEqual(names[25], "sefi_title_z_5col.png")
        self.assertEqual(names[26], "sefi_title_other_2col.png")
        self.assertEqual(
            [n[len("sefi_title_"):-len("_5col.png")] for n in names[:26]],
            list(string.ascii_lowercase),
        )

    def test_every_label_has_the_exact_canvas_and_stays_in_the_ink_limit(self):
        for name, text, cols in gen.labels():
            img, layout = series.render_label(text, cols)
            width = series.CANVAS_W[cols]
            self.assertEqual(img.size, (width, series.CANVAS_H), name)
            bbox = img.getchannel("A").getbbox()
            self.assertIsNotNone(bbox, name)
            self.assertLessEqual(bbox[2], series.INK_LIMIT[width], name)
            self.assertEqual(layout.mode, "fit", name)

    def test_write_labels_writes_every_file(self):
        with tempfile.TemporaryDirectory() as tmp:
            written = gen.write_labels(Path(tmp))
            self.assertEqual(len(written), 27)
            for path in written:
                with Image.open(path) as img:
                    self.assertEqual(img.mode, "RGBA")
                    self.assertEqual(img.height, series.CANVAS_H)

    def test_rendering_is_deterministic(self):
        def encode():
            buf = io.BytesIO()
            series.render_label("OTHER", gen.OTHER_COLS)[0].save(buf, format="PNG")
            return buf.getvalue()

        self.assertEqual(encode(), encode())


class PreviewTests(unittest.TestCase):
    def test_preview_without_stock_art(self):
        sheet = gen.build_preview(stock_dir=None, scale=1)
        self.assertEqual(sheet.size, (gen.PANEL_W + 20, gen.PANEL_H + 20))


if __name__ == "__main__":
    unittest.main()
