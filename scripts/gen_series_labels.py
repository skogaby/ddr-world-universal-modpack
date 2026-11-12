#!/usr/bin/env python3
"""Generate VERSION-filter cell labels for series_expansion's enhanced layout.

The enhanced VERSION menu (`series_expansion.custom_series_enhanced` in
mod-config.json) lays its cells out `num_columns` (1-5) per row and its GROUP
tabs `num_group_columns` per row, and each column count uses a different label
canvas. Every label is therefore rendered at all five widths and named
`sefi_version_<texture>_<N>col.png`, so switching layouts needs no new art. The
DLL picks the variant matching the configured column count at boot.

Outputs (default run): every canonical series (1stMIX ... WORLD) and every
canonical GROUP tab (GROUP GOLD / WHITE / CLASSIC, NO FLARE) at all five widths
into data_mods/custom_series/series_labels/. Generated files — edit the
CANONICAL / CANONICAL_GROUPS tables or the metrics here and regenerate; never
hand-edit the PNGs.

Modes:
  (default)            render the canonical labels
  --from-config PATH   render every distinct `texture` of a config's
                       custom_series_enhanced.filters (from its `label`) and
                       .groups (from an optional `label`, else the canonical
                       text for that texture)
  --emit-config        print the canonical custom_series_enhanced block
                       (paste it into series_expansion in a cabinet config)
  --preview [PATH]     write a contact sheet of all five cell layouts instead
                       of label files (default target/series_labels_preview.png);
                       with --from-config it previews that config's groups,
                       breaks and cells

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

# GROUP tabs (`groups[]`): a tab selects every cell whose series_start lies in
# its range. GOLD / WHITE / CLASSIC are the stock tabs' spans; NO FLARE covers
# the custom series the mod excludes from flare skill (22+).
CANONICAL_GROUPS = [
    Entry("group_gold", "GROUP GOLD", 18, 21),
    Entry("group_white", "GROUP WHITE", 14, 17),
    Entry("group_classic", "GROUP CLASSIC", 1, 13),
    Entry("no_flare", "NO FLARE", 22, 255),
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
    """The canonical custom_series_enhanced block: every CANONICAL_GROUPS tab
    (with the generated art; omit `groups` for the stock tab art), then one
    cell per series."""
    return {
        "num_group_columns": 4,
        "groups": [
            {
                "texture": e.key,
                "series_start": e.start,
                "series_end": e.end,
            }
            for e in CANONICAL_GROUPS
        ],
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


def is_break(item) -> bool:
    """A row break: `{"type": "BREAK", "thickness": N}` (`type` in any case;
    optional when `thickness` is present)."""
    if not isinstance(item, dict):
        return False
    kind = item.get("type")
    if kind is None:
        return "thickness" in item
    return isinstance(kind, str) and kind.lower() == "break"


def break_thickness(item: dict) -> Optional[int]:
    """A break's thickness in px (0-255), or None when invalid."""
    t = item.get("thickness", 0)
    if isinstance(t, bool) or not isinstance(t, int) or not 0 <= t <= 255:
        return None
    return t


KNOWN_TEXT = {e.key: e.text for e in CANONICAL + CANONICAL_GROUPS}


class Block(NamedTuple):
    """A parsed custom_series_enhanced block (lenient; the DLL validates)."""

    group_columns: int
    # None = the stock tabs; else [("tab", Entry) | ("break", px)]
    groups: Optional[list]
    columns: int
    # [("cell", Entry) | ("break", px)]
    filters: list


def _columns(value, default: int) -> int:
    return value if isinstance(value, int) and not isinstance(value, bool) and 1 <= value <= 5 else default


def _items(block: dict, name: str, warnings: list, need_label: bool) -> Optional[list]:
    raw = block.get(name)
    if not isinstance(raw, list):
        return None
    out = []
    for i, item in enumerate(raw):
        where = f"{name}[{i}]"
        if not isinstance(item, dict):
            warnings.append(f"{where}: not an object")
            continue
        if item.get("type") is not None and not is_break(item):
            warnings.append(f"{where}: unknown type {item.get('type')!r}")
            continue
        if is_break(item):
            t = break_thickness(item)
            if t is None:
                warnings.append(f"{where}: break thickness must be an integer 0-255")
            else:
                out.append(("break", t))
            continue
        key = item.get("texture")
        if not isinstance(key, str) or not TEXTURE_KEY_RE.match(key):
            warnings.append(f"{where}: texture {key!r} must match [a-z0-9_]+")
            continue
        label = item.get("label")
        if not isinstance(label, str) or not label.strip():
            if need_label or key not in KNOWN_TEXT:
                warnings.append(f"{where}: missing label" + ("" if need_label else f" (no canonical text for {key!r})"))
                continue
            label = KNOWN_TEXT[key]
        start = item.get("series_start", 0)
        end = item.get("series_end", start)
        out.append(("tab" if name == "groups" else "cell", Entry(key, label, start, end)))
    return out


def parse_block(block: dict) -> tuple:
    """(Block, warnings) for a custom_series_enhanced dict."""
    warnings = []
    groups = _items(block, "groups", warnings, need_label=False)
    if "groups" in block and groups is None:
        warnings.append("groups is not a list — stock GROUP tabs")
    filters = _items(block, "filters", warnings, need_label=True)
    if filters is None:
        warnings.append("filters is missing or not a list")
        filters = []
    parsed = Block(
        _columns(block.get("num_group_columns"), 3),
        groups,
        _columns(block.get("num_columns"), 2),
        filters,
    )
    return parsed, warnings


def cells_from_config(config: dict) -> tuple:
    """Entries for every distinct, valid `texture` in a mod-config's
    series_expansion.custom_series_enhanced groups and filters. Returns
    (entries, warnings)."""
    block = (config.get("series_expansion") or {}).get("custom_series_enhanced")
    if not isinstance(block, dict) or not isinstance(block.get("filters"), list):
        return [], ["no series_expansion.custom_series_enhanced.filters list in config"]
    parsed, warnings = parse_block(block)
    entries, seen = [], set()
    for kind, item in (parsed.groups or []) + parsed.filters:
        if kind == "break" or item.key in seen:
            continue
        seen.add(item.key)
        entries.append(item)
    return entries, warnings


# ── Preview contact sheet ─────────────────────────────────────────────────
CELL_W = {1: 220, 2: 108, 3: 72, 4: 54, 5: 42}  # filter_switch_base0N
CELL_H = 26
PANEL_W, PANEL_H = 216, 266  # the filter item area (GridPanel)
LABEL_X, LABEL_Y = 10, 3  # label canvas offset right of the check mark
PANEL_BG = (244, 246, 245, 255)
MARK_COLOR = (0, 72, 52, 255)
GROUPS = ("gold", "white", "classic")  # stock tab order


class Flow:
    """The item GridPanel's flow layout (mirrors the DLL's model::Flow): a
    child that would overflow the line starts the next one, below the
    tallest child of the previous line; breaks are full-width spacers."""

    def __init__(self):
        self.cursor = 0
        self.line_h = 0
        self.top = 0

    def place(self, w: int, h: int) -> tuple:
        if self.cursor + w > PANEL_W:
            self.top += self.line_h
            self.cursor = 0
            self.line_h = 0
        pos = (self.cursor, self.top)
        self.cursor += w
        self.line_h = max(self.line_h, h)
        return pos

    def joins_line(self, w: int) -> bool:
        return self.cursor > 0 and self.cursor + w <= PANEL_W


def layout(block: Block) -> list:
    """[(kind, payload, x, y)] in build order: tabs ("stock" name or Entry),
    the implicit separator, cells; breaks as ("break", px)."""
    flow, out = Flow(), []
    tab_w = CELL_W[block.group_columns]
    tabs: list = [("stock", g) for g in GROUPS] if block.groups is None else block.groups
    for kind, item in tabs:
        if kind == "break":
            out.append((kind, item, *flow.place(PANEL_W, item)))
        else:
            out.append((kind, item, *flow.place(tab_w, CELL_H)))
    if any(k != "break" for k, _ in tabs) and flow.joins_line(CELL_W[block.columns]):
        out.append(("break", 0, *flow.place(PANEL_W, 0)))
    for kind, item in block.filters:
        if kind == "break":
            out.append((kind, item, *flow.place(PANEL_W, item)))
        else:
            out.append((kind, item, *flow.place(CELL_W[block.columns], CELL_H)))
    return out


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


def _panel(block: Block, stock_dir: Optional[Path]) -> Image.Image:
    mark = _stock_image(stock_dir, "switch_mark_off")
    placed = layout(block)
    bottom = max((y + (item if kind == "break" else CELL_H) for kind, item, _, y in placed), default=0)
    height = max(bottom, PANEL_H) + 20
    panel = Image.new("RGBA", (PANEL_W + 20, height), PANEL_BG)
    first_cell_y = next((y for kind, _, _, y in placed if kind == "cell"), None)
    for kind, item, x, y in placed:
        if kind == "break":
            continue
        if kind == "stock":
            label = _stock_image(stock_dir, f"version_{item}")
            if label is None:
                label = render_label(f"GROUP\n{item.upper()}", block.group_columns)[0]
            cell_w = CELL_W[block.group_columns]
        elif kind == "tab":
            label = render_label(item.text, block.group_columns)[0]
            cell_w = CELL_W[block.group_columns]
        else:
            label = render_label(text_for(item, block.columns), block.columns)[0]
            cell_w = CELL_W[block.columns]
        panel.alpha_composite(_cell(label, cell_w, mark), (4 + x, 4 + y))
    draw = ImageDraw.Draw(panel)
    draw.rectangle([3, 3, 4 + PANEL_W, 4 + PANEL_H], outline=(0, 200, 160, 255))
    if first_cell_y is not None:
        # Tabs stay fixed above this line; cells scroll below it.
        draw.line([(0, 4 + first_cell_y), (2, 4 + first_cell_y)], fill=(40, 120, 220, 255))
    if bottom > PANEL_H:
        draw.line([(3, 4 + PANEL_H), (4 + PANEL_W, 4 + PANEL_H)], fill=(220, 40, 40, 255))
    return panel


def build_preview(block: dict, stock_dir: Optional[Path] = STOCK_TEX_DIR, scale: int = 3) -> Image.Image:
    """Contact sheet: the filter item area of `block` (a
    custom_series_enhanced dict) for every num_columns — tabs first, cells
    and breaks below; a red line marks where scrolling starts. Approximate
    (label offset, cell chrome), for layout review."""
    if stock_dir is not None and not Path(stock_dir).is_dir():
        stock_dir = None
    parsed, _ = parse_block(block)
    panels = [_panel(parsed._replace(columns=cols), stock_dir) for cols in COLUMNS]
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
                        help="render the textures named by a config's custom_series_enhanced groups and filters")
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

    entries = CANONICAL + CANONICAL_GROUPS
    block = canonical_config()
    if args.from_config is not None:
        try:
            config = json.loads(Path(args.from_config).read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as exc:
            raise SystemExit(f"cannot read {rel(args.from_config)}: {exc}")
        entries, warnings = cells_from_config(config)
        for warning in warnings:
            print(f"WARNING: {warning}", file=sys.stderr)
        block = (config.get("series_expansion") or {}).get("custom_series_enhanced") or {}

    if args.preview is not None:
        args.preview.parent.mkdir(parents=True, exist_ok=True)
        build_preview(block).save(args.preview)
        print(f"wrote {rel(args.preview)} ({len(entries)} label(s), 5 layouts)")
        return

    written = write_labels(entries, args.out_dir)
    print(f"wrote {len(written)} label(s) for {len(entries)} texture(s) to {rel(args.out_dir)}")


if __name__ == "__main__":
    main()
