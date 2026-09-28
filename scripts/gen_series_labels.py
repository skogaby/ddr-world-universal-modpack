#!/usr/bin/env python3
"""Generate VERSION-filter cell labels for series_expansion's enhanced layout.

The enhanced VERSION menu (`series_expansion.custom_series_enhanced` in
mod-config.json) lays its cells out `num_columns` (1-5) per row, and each
column count uses a different label canvas. Every label is therefore rendered
at all five widths and named `sefi_version_<texture>_<N>col.png`, so switching
layouts needs no new art. The DLL picks the variant matching the configured
`num_columns` at boot.

Outputs (default run): every canonical series (1stMIX ... WORLD) at all five
widths into data_mods/custom_series/series_labels/. Generated files — edit the
CANONICAL table or the metrics here and regenerate; never hand-edit the PNGs.

Modes:
  (default)            render the canonical labels
  --from-config PATH   render every distinct `texture` of a config's
                       custom_series_enhanced.filters from its `label`
  --emit-config        print the canonical custom_series_enhanced block
                       (paste it into series_expansion in a cabinet config)
  --preview [PATH]     write a contact sheet of all five layouts instead of
                       label files (default target/series_labels_preview.png)

Look: FOT-TsukuGo Pro B, tuned against the stock sefi_version_* labels —
12-px caps on a baseline at y=16, a 0.90 base horizontal scale, fill #00B68C,
left-aligned. Ink stays within the stock per-width limits. Text that does not
fit is condensed horizontally down to MIN_RATIO, then stacked on two smaller
lines at a natural break (a space, or a lower->upper / digit->letter
boundary); single words keep condensing rather than split mid-word.

Requires: Pillow (pip install Pillow)
"""

import argparse
import json
import os
import re
import sys
from dataclasses import dataclass, field
from functools import lru_cache
from pathlib import Path
from typing import NamedTuple, Optional

from PIL import Image, ImageDraw, ImageFont

SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parent

FONT_PATH = SCRIPT_DIR / "fonts" / "FOT-TSUKUGOPRO-B.OTF"
DEFAULT_OUT_DIR = REPO_ROOT / "data_mods" / "custom_series" / "series_labels"
DEFAULT_PREVIEW = REPO_ROOT / "target" / "series_labels_preview.png"
# Untracked local extraction of the stock select_music_option_v3.ifs; used only
# to draw the stock GROUP tabs / check marks in --preview when present.
STOCK_TEX_DIR = REPO_ROOT / "select_music_option_v3_ifs" / "tex"

# ── Metrics ───────────────────────────────────────────────────────────────
COLUMNS = (1, 2, 3, 4, 5)
# Label canvas per column count (the stock filter_switch_base0N label slots).
CANVAS_W = {1: 220, 2: 104, 3: 64, 4: 44, 5: 32}
CANVAS_H = 20
# Rightmost inked column per canvas width, from the widest stock label of each
# size (version_maxex 98, cl_noplay 60, title_mno 42, rank_aaa 30). The canvas
# sits right of the cell's check mark, so its last pixels overrun the cell;
# stock art never inks them. 220 has no wide stock example: keep 4 px clear.
INK_LIMIT = {220: 216, 104: 98, 64: 60, 44: 42, 32: 30}
PAD_X = 1  # stock ink starts at x = 0..2

TEXT_COLOR = (0, 182, 140, 255)  # #00B68C, sampled from stock sefi_version_*
FONT_SIZE = 15  # 12-px caps in TsukuGo B
BASELINE_Y = 16  # stock caps occupy rows 4..16
# TsukuGo B at its natural width is ~12 % wider than the stock face
# (WORLD: 59 px vs 52); every strip is pre-scaled horizontally by this.
BASE_SCALE = 0.90
# Faux-bold stroke in pixels (rendered supersampled). Tried 0.375 against
# stock; the maintainer preferred the font's own weight (2026-09-27).
EMBOLDEN = 0.0
SUPERSAMPLE = 8
MIN_RATIO = 0.70  # condense down to this before stacking on two lines
STACK_FONT_SIZE = 10.5  # ~8-px caps, the stock SuperNOVA-/SuperNOVA2 scale
STACK_BASELINES = (9, 19)

# Scratch strip: tall enough for any glyph, baseline at a fixed row.
_SCRATCH_H, _SCRATCH_BASELINE = 64, 48


# ── Canonical series ──────────────────────────────────────────────────────
@dataclass(frozen=True)
class Entry:
    """One label: texture key, display text and the raw musicdb <series>
    range it stands for. `overrides` maps a column count to replacement
    text for that width; a "\\n" in any text forces a two-line stack there."""

    key: str
    text: str
    start: int
    end: int
    overrides: dict = field(default_factory=dict, compare=False, hash=False)


# Newest first, like the stock menu. Raw values follow the game's own
# per-series name table: 15 and 16 are both "DanceDanceRevolution 2014".
CANONICAL = [
    Entry("world", "WORLD", 21, 21),
    Entry("a3", "A3", 20, 20),
    Entry("a20plus", "A20 PLUS", 19, 19),
    Entry("a20", "A20", 18, 18),
    Entry("a", "A", 17, 17),
    Entry("2014", "2014", 15, 16),
    Entry("2013", "2013", 14, 14),
    Entry("x3", "X3 VS 2ndMIX", 13, 13),
    Entry("x2", "X2", 12, 12),
    Entry("x", "X", 11, 11),
    Entry("supernova2", "SuperNOVA2", 10, 10),
    Entry("supernova", "SuperNOVA", 9, 9),
    Entry("extreme", "EXTREME", 8, 8),
    Entry("max2", "MAX2", 7, 7),
    Entry("max", "MAX", 6, 6),
    Entry("5thmix", "5thMIX", 5, 5),
    Entry("4thmix", "4thMIX", 4, 4),
    Entry("3rdmix", "3rdMIX", 3, 3),
    Entry("2ndmix", "2ndMIX", 2, 2),
    Entry("1stmix", "1stMIX", 1, 1),
]

TEXTURE_KEY_RE = re.compile(r"^[a-z0-9_]+$")


def text_for(entry: Entry, cols: int) -> str:
    return entry.overrides.get(cols, entry.text)


def label_filename(key: str, cols: int) -> str:
    return f"sefi_version_{key}_{cols}col.png"


def rel(path: Path) -> str:
    """Repo-relative (or ~-relative) form of `path` for messages."""
    path = Path(path).resolve()
    try:
        return path.relative_to(REPO_ROOT).as_posix()
    except ValueError:
        home = Path(os.path.expanduser("~")).resolve()
        try:
            return "~/" + path.relative_to(home).as_posix()
        except ValueError:
            return path.name


# ── Rendering ─────────────────────────────────────────────────────────────
@lru_cache(maxsize=None)
def load_font(path: Path, size: float) -> ImageFont.FreeTypeFont:
    if not Path(path).is_file():
        raise SystemExit(f"font not found: {rel(Path(path))}")
    return ImageFont.truetype(str(path), size)


class Layout(NamedTuple):
    mode: str  # "fit" | "condensed" | "stacked"
    ratio: float  # horizontal squeeze applied to the widest line (1.0 = none)
    lines: tuple


def render_strip(text: str, size: float) -> Optional[Image.Image]:
    """Render `text` baseline-anchored, cropped to its ink columns, keeping a
    fixed _SCRATCH_H-tall frame with the baseline at _SCRATCH_BASELINE. The
    strip is pre-scaled horizontally by BASE_SCALE. None for blank text."""
    ss = SUPERSAMPLE
    font = load_font(FONT_PATH, size * ss)
    probe = font.getbbox(text, anchor="ls")
    width = max(1, int(probe[2] - min(0, probe[0])) + 4 * ss)
    scratch = Image.new("RGBA", (width, _SCRATCH_H * ss), (0, 0, 0, 0))
    ImageDraw.Draw(scratch).text(
        (-min(0, probe[0]), _SCRATCH_BASELINE * ss),
        text,
        font=font,
        fill=TEXT_COLOR,
        anchor="ls",
        stroke_width=round(EMBOLDEN * ss),
        stroke_fill=TEXT_COLOR,
    )
    bbox = scratch.getbbox()
    if bbox is None:
        return None
    scratch = scratch.crop((bbox[0], 0, bbox[2], _SCRATCH_H * ss))
    out_w = max(1, round(scratch.width * BASE_SCALE / ss))
    return scratch.resize((out_w, _SCRATCH_H), Image.Resampling.LANCZOS)


def fit_width(strip: Image.Image, avail: int) -> tuple:
    """Condense `strip` horizontally to `avail` px when wider (height kept)."""
    if strip.width <= avail:
        return strip, 1.0
    return strip.resize((avail, strip.height), Image.Resampling.LANCZOS), avail / strip.width


def natural_split(text: str) -> Optional[tuple]:
    """Two-line break for `text`: the space nearest the middle, else the
    lower->upper or digit->letter boundary nearest the middle. None for a
    single word with no such boundary (never split mid-word)."""
    if "\n" in text:
        top, bottom = text.split("\n", 1)
        return top, bottom
    mid = len(text) / 2
    spaces = [i for i, c in enumerate(text) if c == " "]
    if spaces:
        i = min(spaces, key=lambda i: abs(i - mid))
        return text[:i], text[i + 1 :]
    bounds = [
        i
        for i in range(1, len(text))
        if (text[i].isupper() and text[i - 1].islower())
        or (text[i].isalpha() and text[i - 1].isdigit())
    ]
    if bounds:
        i = min(bounds, key=lambda i: abs(i - mid))
        return text[:i], text[i:]
    return None


def _place(canvas: Image.Image, strip: Image.Image, x: int, baseline: int) -> None:
    """Composite `strip` so its baseline lands on `baseline`, clipped to the
    canvas height."""
    top = _SCRATCH_BASELINE - baseline
    window = strip.crop((0, top, strip.width, top + canvas.height))
    canvas.alpha_composite(window, (x, 0))


def render_label(text: str, cols: int) -> tuple:
    """Render one label for `cols` columns. Returns (RGBA image, Layout)."""
    width = CANVAS_W[cols]
    limit = INK_LIMIT[width]
    avail = limit - PAD_X
    img = Image.new("RGBA", (width, CANVAS_H), (0, 0, 0, 0))

    split = natural_split(text)
    single = render_strip(text.replace("\n", " "), FONT_SIZE)
    if single is None:
        return img, Layout("fit", 1.0, ())
    forced = "\n" in text
    ratio = avail / single.width
    if not forced and (ratio >= MIN_RATIO or split is None):
        strip, applied = fit_width(single, avail)
        _place(img, strip, PAD_X, BASELINE_Y)
        mode = "fit" if applied == 1.0 else "condensed"
        return img, Layout(mode, applied, (text,))

    top, bottom = split if split is not None else (text, "")
    worst = 1.0
    top_strip = render_strip(top, STACK_FONT_SIZE)
    bottom_strip = render_strip(bottom, STACK_FONT_SIZE)
    if top_strip is not None:
        top_strip, r = fit_width(top_strip, avail)
        worst = min(worst, r)
        _place(img, top_strip, PAD_X, STACK_BASELINES[0])
    if bottom_strip is not None:
        bottom_strip, r = fit_width(bottom_strip, avail)
        worst = min(worst, r)
        x = max(PAD_X, limit - bottom_strip.width)  # second line right-aligned
        _place(img, bottom_strip, x, STACK_BASELINES[1])
    return img, Layout("stacked", worst, (top, bottom))


def write_labels(entries, out_dir: Path) -> list:
    """Render every entry at every width into `out_dir`; returns the paths."""
    out_dir.mkdir(parents=True, exist_ok=True)
    written = []
    for entry in entries:
        for cols in COLUMNS:
            img, layout = render_label(text_for(entry, cols), cols)
            path = out_dir / label_filename(entry.key, cols)
            img.save(path)
            written.append(path)
            if layout.mode != "fit":
                print(f"  {path.name}: {layout.mode} {layout.ratio:.2f}")
    return written


# ── Config interplay ──────────────────────────────────────────────────────
def canonical_config() -> dict:
    """The canonical custom_series_enhanced block: one cell per series."""
    return {
        "num_columns": 3,
        "filters": [
            {
                "label": e.text,
                "series_start": e.start,
                "series_end": e.end,
                "texture": e.key,
            }
            for e in CANONICAL
        ],
    }


def cells_from_config(config: dict) -> tuple:
    """Entries for every distinct, valid `texture` in a mod-config's
    series_expansion.custom_series_enhanced.filters. Returns
    (entries, warnings)."""
    block = (config.get("series_expansion") or {}).get("custom_series_enhanced")
    if not isinstance(block, dict) or not isinstance(block.get("filters"), list):
        return [], ["no series_expansion.custom_series_enhanced.filters list in config"]
    entries, warnings, seen = [], [], set()
    for i, cell in enumerate(block["filters"]):
        if not isinstance(cell, dict):
            warnings.append(f"filters[{i}]: not an object")
            continue
        key, label = cell.get("texture"), cell.get("label")
        if not isinstance(key, str) or not TEXTURE_KEY_RE.match(key):
            warnings.append(f"filters[{i}]: texture {key!r} must match [a-z0-9_]+")
            continue
        if not isinstance(label, str) or not label.strip():
            warnings.append(f"filters[{i}]: missing label")
            continue
        if key in seen:
            continue
        seen.add(key)
        start = cell.get("series_start", 0)
        end = cell.get("series_end", start)
        entries.append(Entry(key, label, start, end))
    return entries, warnings


# ── Preview contact sheet ─────────────────────────────────────────────────
CELL_W = {1: 220, 2: 108, 3: 72, 4: 54, 5: 42}  # filter_switch_base0N
CELL_H = 26
PANEL_W, PANEL_H = 216, 266  # the filter item area; 9 cell rows below the tabs
LABEL_X, LABEL_Y = 10, 3  # label canvas offset right of the check mark
PANEL_BG = (244, 246, 245, 255)
MARK_COLOR = (0, 72, 52, 255)
GROUPS = ("gold", "white", "classic")  # stock tab order


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


def _panel(entries, cols: int, stock_dir: Optional[Path]) -> Image.Image:
    mark = _stock_image(stock_dir, "switch_mark_off")
    rows = -(-len(entries) // cols)
    height = max(CELL_H * (rows + 1), PANEL_H) + 20
    panel = Image.new("RGBA", (PANEL_W + 20, height), PANEL_BG)
    for g, group in enumerate(GROUPS):
        tab = _stock_image(stock_dir, f"version_{group}")
        if tab is None:
            tab = render_label(f"GROUP\n{group.upper()}", 3)[0]
        panel.alpha_composite(_cell(tab, 72, mark), (4 + g * 72, 4))
    for i, entry in enumerate(entries):
        label = render_label(text_for(entry, cols), cols)[0]
        x = 4 + (i % cols) * CELL_W[cols]
        y = 4 + CELL_H * (1 + i // cols)
        panel.alpha_composite(_cell(label, CELL_W[cols], mark), (x, y))
    draw = ImageDraw.Draw(panel)
    draw.rectangle([3, 3, 4 + PANEL_W, 4 + PANEL_H], outline=(0, 200, 160, 255))
    if CELL_H * (rows + 1) > PANEL_H:
        draw.line([(3, 4 + PANEL_H), (4 + PANEL_W, 4 + PANEL_H)], fill=(220, 40, 40, 255))
    return panel


def build_preview(entries, stock_dir: Optional[Path] = STOCK_TEX_DIR, scale: int = 3) -> Image.Image:
    """Contact sheet: the filter item area for every num_columns, tabs on the
    first line, `entries` in order below; a red line marks where scrolling
    starts. Approximate (label offset, cell chrome), for layout review."""
    if stock_dir is not None and not Path(stock_dir).is_dir():
        stock_dir = None
    panels = [_panel(entries, cols, stock_dir) for cols in COLUMNS]
    head = 18
    sheet_w = sum(p.width + 8 for p in panels)
    sheet_h = max(p.height for p in panels) + head
    sheet = Image.new("RGBA", (sheet_w, sheet_h), (60, 64, 66, 255))
    draw = ImageDraw.Draw(sheet)
    x = 0
    for cols, panel in zip(COLUMNS, panels):
        draw.text((x + 6, 3), f"num_columns = {cols}", fill=(255, 255, 255, 255))
        sheet.alpha_composite(panel, (x, head))
        x += panel.width + 8
    return sheet.resize((sheet_w * scale, sheet_h * scale), Image.Resampling.LANCZOS)


# ── CLI ───────────────────────────────────────────────────────────────────
def main(argv=None) -> None:
    parser = argparse.ArgumentParser(
        description="Generate VERSION-filter labels for series_expansion's enhanced layout."
    )
    parser.add_argument("--from-config", metavar="PATH", type=Path,
                        help="render the textures named by a config's custom_series_enhanced.filters")
    parser.add_argument("--emit-config", action="store_true",
                        help="print the canonical custom_series_enhanced block and exit")
    parser.add_argument("--preview", metavar="PATH", type=Path, nargs="?", const=DEFAULT_PREVIEW,
                        help=f"write a contact sheet instead of labels (default {rel(DEFAULT_PREVIEW)})")
    parser.add_argument("--out-dir", metavar="DIR", type=Path, default=DEFAULT_OUT_DIR,
                        help=f"label output directory (default {rel(DEFAULT_OUT_DIR)})")
    args = parser.parse_args(argv)

    if args.emit_config:
        body = json.dumps(canonical_config(), indent=2)
        print(f'"custom_series_enhanced": {body}')
        return

    entries = CANONICAL
    if args.from_config is not None:
        try:
            config = json.loads(Path(args.from_config).read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as exc:
            raise SystemExit(f"cannot read {rel(args.from_config)}: {exc}")
        entries, warnings = cells_from_config(config)
        for warning in warnings:
            print(f"WARNING: {warning}", file=sys.stderr)

    if args.preview is not None:
        args.preview.parent.mkdir(parents=True, exist_ok=True)
        build_preview(entries).save(args.preview)
        print(f"wrote {rel(args.preview)} ({len(entries)} cell(s), 5 layouts)")
        return

    written = write_labels(entries, args.out_dir)
    print(f"wrote {len(written)} label(s) for {len(entries)} texture(s) to {rel(args.out_dir)}")


if __name__ == "__main__":
    main()
