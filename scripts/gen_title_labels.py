#!/usr/bin/env python3
"""Generate the per-letter MUSIC TITLE filter labels for the
Improved Song Title Sorting mod (src/mods/improved_song_title_sorting/).

The mod lays the MUSIC TITLE menu out as A..Z at the 42-px cell template
(five per row, one letter per cell), an OTHER cell at the 108-px template on
Z's row, then the stock kana lines (stock art, four per row). Only the new
cells need art:

  sefi_title_<a..z>_5col.png   32 x 20   (filter_switch_base05 label slot)
  sefi_title_other_2col.png   104 x 20   (filter_switch_base02 label slot)

written to data_mods/improved_song_title_sorting/title_labels/. The names must
match `layout::new_textures()` in the mod. Generated files -- edit this script
and regenerate; never hand-edit the PNGs.

The look (font, size, colour, baseline, ink limits) is shared with the VERSION
labels: rendering comes from gen_series_labels.render_label.

Modes:
  (default)            render the labels
  --preview [PATH]     write a contact sheet of the whole menu instead
                       (default target/title_labels_preview.png); uses the
                       stock kana-line art from a local stock extraction
                       (select_music_option_v3_ifs/tex/, untracked) when
                       present, rendered placeholders otherwise

Requires: Pillow (pip install Pillow)
"""

import argparse
import string
import sys
from pathlib import Path
from typing import Optional

from PIL import Image, ImageDraw

sys.path.insert(0, str(Path(__file__).resolve().parent))
from gen_series_labels import (  # noqa: E402  (shared renderer)
    CANVAS_H,
    CANVAS_W,
    REPO_ROOT,
    STOCK_TEX_DIR,
    render_label,
    rel,
)

DEFAULT_OUT_DIR = REPO_ROOT / "data_mods" / "improved_song_title_sorting" / "title_labels"
DEFAULT_PREVIEW = REPO_ROOT / "target" / "title_labels_preview.png"

# Cell templates (= cells per 216-px row); keep in sync with layout.rs.
LETTER_COLS = 5
OTHER_COLS = 2
KANA_COLS = 4
OTHER_TEXT = "OTHER"

# Stock kana lines, class order (label key, placeholder text for --preview).
KANA = [
    ("line_a", "ア行"), ("line_ka", "カ行"), ("line_sa", "サ行"), ("line_ta", "タ行"),
    ("line_na", "ナ行"), ("line_ha", "ハ行"), ("line_ma", "マ行"), ("line_ya", "ヤ行"),
    ("line_ra", "ラ行"), ("line_wa", "ワ行"),
]


def labels() -> list:
    """(file name, text, cols) for every mod-owned label, in menu order."""
    out = [(f"sefi_title_{c}_{LETTER_COLS}col.png", c.upper(), LETTER_COLS)
           for c in string.ascii_lowercase]
    out.append((f"sefi_title_other_{OTHER_COLS}col.png", OTHER_TEXT, OTHER_COLS))
    return out


def write_labels(out_dir: Path) -> list:
    """Render every label into `out_dir`; returns the written paths."""
    out_dir.mkdir(parents=True, exist_ok=True)
    written = []
    for name, text, cols in labels():
        img, layout = render_label(text, cols)
        path = out_dir / name
        img.save(path)
        written.append(path)
        if layout.mode != "fit":
            print(f"  {name}: {layout.mode} {layout.ratio:.2f}")
    return written


# ── Preview contact sheet ─────────────────────────────────────────────────
CELL_W = {1: 220, 2: 108, 3: 72, 4: 54, 5: 42}  # filter_switch_base0N
CELL_H = 26
PANEL_W, PANEL_H = 216, 266  # the filter item area
LABEL_X, LABEL_Y = 10, 3  # label canvas offset right of the check mark
PANEL_BG = (244, 246, 245, 255)
MARK_COLOR = (0, 72, 52, 255)


def _stock_image(stock_dir: Optional[Path], name: str) -> Optional[Image.Image]:
    if stock_dir is None:
        return None
    path = stock_dir / f"sefi_{name}.png"
    return Image.open(path).convert("RGBA") if path.is_file() else None


def _cell(label: Image.Image, cell_w: int, mark: Optional[Image.Image]) -> Image.Image:
    cell = Image.new("RGBA", (cell_w + LABEL_X, CELL_H), (0, 0, 0, 0))
    if mark is not None:
        cell.alpha_composite(mark, (0, 1))
    else:
        ImageDraw.Draw(cell).rectangle([3, 4, 8, 20], fill=MARK_COLOR)
    cell.alpha_composite(label, (LABEL_X, LABEL_Y))
    return cell


def build_preview(stock_dir: Optional[Path] = STOCK_TEX_DIR, scale: int = 3) -> Image.Image:
    """The filter item area as the game lays it out: letter rows, Z + OTHER,
    the 1-px row break, then the kana rows. Approximate chrome."""
    if stock_dir is not None and not Path(stock_dir).is_dir():
        stock_dir = None
    mark = _stock_image(stock_dir, "switch_mark_off")
    cells = [(render_label(text, cols)[0], cols) for _, text, cols in labels()]
    kana = []
    for key, text in KANA:
        art = _stock_image(stock_dir, f"title_{key}")
        kana.append((art if art is not None else render_label(text, KANA_COLS)[0], KANA_COLS))

    panel = Image.new("RGBA", (PANEL_W + 20, PANEL_H + 20), PANEL_BG)
    x = y = 0
    items: list = [*cells, None, *kana]  # None = the full-width 1-px row break
    for item in items:
        if item is None:
            x, y = 0, y + CELL_H + 1
            continue
        label, cols = item
        w = CELL_W[cols]
        if x + w > PANEL_W:
            x, y = 0, y + CELL_H
        panel.alpha_composite(_cell(label, w, mark), (4 + x, 4 + y))
        x += w
    ImageDraw.Draw(panel).rectangle([3, 3, 4 + PANEL_W, 4 + PANEL_H], outline=(0, 200, 160, 255))
    return panel.resize((panel.width * scale, panel.height * scale), Image.Resampling.LANCZOS)


# ── CLI ───────────────────────────────────────────────────────────────────
def main(argv=None) -> None:
    parser = argparse.ArgumentParser(
        description="Generate the per-letter MUSIC TITLE filter labels."
    )
    parser.add_argument("--preview", metavar="PATH", type=Path, nargs="?", const=DEFAULT_PREVIEW,
                        help=f"write a contact sheet instead of labels (default {rel(DEFAULT_PREVIEW)})")
    parser.add_argument("--out-dir", metavar="DIR", type=Path, default=DEFAULT_OUT_DIR,
                        help=f"label output directory (default {rel(DEFAULT_OUT_DIR)})")
    args = parser.parse_args(argv)

    if args.preview is not None:
        args.preview.parent.mkdir(parents=True, exist_ok=True)
        build_preview().save(args.preview)
        print(f"wrote {rel(args.preview)}")
        return

    written = write_labels(args.out_dir)
    print(f"wrote {len(written)} label(s) to {rel(args.out_dir)}")


if __name__ == "__main__":
    main()
