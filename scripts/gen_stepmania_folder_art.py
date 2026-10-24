#!/usr/bin/env python3
"""Generate the STEPMANIA folder and series art for the StepMania pack loader.

The loader registers a custom genre folder (key `stepmania`) and a custom
VERSION-filter series (texture `stepmania`) at boot — see
`folder_expansion::register_runtime_folder` / `series_expansion::
register_runtime_series`. Both mods resolve their art by name, so this script
renders every texture the two need from the repo's fonts into
data_mods/stepmania_packs/, laid out like the operator mod folders
`custom_folders` / `custom_series` (same IFS folder names):

  select_music_folder_v3_ifs/tex/
      mufo_txt_folder_title_stepmania_{on,off}.png   240x126  P (title text)
      mufo_folder_back_stepmania_on.png              240x164  RGB (card art)
      mufo_folder_back_stepmania_off.png             240x164  P (greyscale)
  select_music_folder_lang_eng_v3_ifs/tex/
      mufo_txt_folder_subtitle_stepmania_{on,off}.png 240x26  P
      mufo_txt_folder_info_stepmania.png             488x104  RGBA (info banner)
  select_music_card_lang_eng_v3_ifs/tex/
      muca_txt_folder_name_stepmania.png             220x60   RGBA (header strip)
  select_music_option_v3_ifs/tex/
      sefi_version_stepmania.png                     104x20   RGBA (legacy label)
  series_labels/
      sefi_version_stepmania_{1..5}col.png           enhanced-layout labels

Sizes, colour modes and text placement follow the reference `dogs` set in
data_mods/custom_folders/ (itself cut to the stock `firststep` textures): the
title sits in the lower band of its canvas, the subtitle is centred, the info
banner keeps the stock transparent 17-px top strip over a dark diagonal hatch
with a white title and yellow description at x=20, and the header strip is a
two-line outlined wordmark. "On" states use the StepMania orange accent;
"off" states are greyscale. The series labels come from gen_series_labels.py's
renderer, so they match the other VERSION cells exactly.

Generated files are committed artefacts — change the tables here and
regenerate; never hand-edit the PNGs.

Usage:
  python3 scripts/gen_stepmania_folder_art.py [--out-dir DIR]

Requires: Pillow (pip install Pillow)
"""

import argparse
import sys
from pathlib import Path
from typing import Dict, Optional, Tuple

from PIL import Image, ImageDraw, ImageFont

SCRIPT_DIR = Path(__file__).resolve().parent
sys.path.insert(0, str(SCRIPT_DIR))
import gen_series_labels as labels  # noqa: E402

REPO_ROOT = SCRIPT_DIR.parent
DEFAULT_OUT_DIR = REPO_ROOT / "data_mods" / "stepmania_packs"

KEY = "stepmania"
TITLE = "STEPMANIA"
SUBTITLE = "COMMUNITY SONG PACKS"
INFO_LINES = (
    "Songs from StepMania packs in stepmania/Songs —",
    "local play only, scores are not saved online",
)
CARD_LINE2 = "FOLDER"

FONT_PATH = labels.FONT_PATH  # FOT-TsukuGo Pro B, the series-label face
SUPERSAMPLE = 4

# ── Palette ────────────────────────────────────────────────────────────
ACCENT = (255, 138, 0, 255)  # StepMania orange
ACCENT_SUB = (255, 160, 20, 255)
WHITE = (255, 255, 255, 255)
OUTLINE = (39, 40, 40, 255)  # stock title outline
OFF_FILL = (205, 205, 205, 255)  # stock "off" title fill
OFF_OUTLINE = (36, 36, 36, 255)
OFF_SHADOW = (107, 107, 107, 255)
SUB_OFF = (221, 221, 221, 255)  # stock "off" subtitle fill
SUB_EDGE = (0, 0, 0, 110)  # stock subtitle edge (translucent black)
INFO_TITLE = WHITE
INFO_TEXT = (255, 227, 0, 255)  # stock info description yellow
INFO_BG = (24, 24, 24, 255)
INFO_STRIPE = (36, 37, 36, 255)
CARD_FILL = (235, 236, 228, 255)  # stock header-strip cream
CARD_SHADOW = ACCENT

# ── Canvases (stock sizes) ─────────────────────────────────────────────
TITLE_SIZE = (240, 126)
BACK_SIZE = (240, 164)
SUBTITLE_SIZE = (240, 26)
INFO_SIZE = (488, 104)
CARD_SIZE = (220, 60)
INFO_TOP_CLEAR = 17  # transparent rows above the banner (stock)

# Texture file name -> (IFS mod folder, expected size, expected PIL mode).
TEXTURES: Dict[str, Tuple[str, Tuple[int, int], str]] = {
    f"mufo_txt_folder_title_{KEY}_on": ("select_music_folder_v3_ifs", TITLE_SIZE, "P"),
    f"mufo_txt_folder_title_{KEY}_off": ("select_music_folder_v3_ifs", TITLE_SIZE, "P"),
    f"mufo_folder_back_{KEY}_on": ("select_music_folder_v3_ifs", BACK_SIZE, "RGB"),
    f"mufo_folder_back_{KEY}_off": ("select_music_folder_v3_ifs", BACK_SIZE, "P"),
    f"mufo_txt_folder_subtitle_{KEY}_on": ("select_music_folder_lang_eng_v3_ifs", SUBTITLE_SIZE, "P"),
    f"mufo_txt_folder_subtitle_{KEY}_off": ("select_music_folder_lang_eng_v3_ifs", SUBTITLE_SIZE, "P"),
    f"mufo_txt_folder_info_{KEY}": ("select_music_folder_lang_eng_v3_ifs", INFO_SIZE, "RGBA"),
    f"muca_txt_folder_name_{KEY}": ("select_music_card_lang_eng_v3_ifs", CARD_SIZE, "RGBA"),
}
LEGACY_LABEL = f"sefi_version_{KEY}"
LEGACY_LABEL_DIR = "select_music_option_v3_ifs"
LABELS_DIR = "series_labels"


# ── Text rendering ─────────────────────────────────────────────────────
def font(size: float) -> ImageFont.FreeTypeFont:
    return labels.load_font(FONT_PATH, size * SUPERSAMPLE)


def text_strip(
    text: str,
    size: float,
    fill,
    outline=None,
    outline_px: float = 0.0,
    shadow=None,
    shadow_offset: Tuple[int, int] = (0, 0),
    shear: float = 0.0,
) -> Tuple[Image.Image, int]:
    """Render `text` supersampled with an optional outline, drop shadow and
    italic shear, downsampled to 1x. Returns (RGBA strip, baseline row)."""
    ss = SUPERSAMPLE
    ft = font(size)
    left, top, right, bottom = ft.getbbox(text, anchor="ls")
    pad = int(round((outline_px + 2) * ss)) + max(abs(shadow_offset[0]), abs(shadow_offset[1])) * ss
    width = right - left + 2 * pad + int(abs(shear) * (bottom - top))
    height = bottom - top + 2 * pad
    baseline = pad - top
    img = Image.new("RGBA", (width, height), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)
    x0 = pad - left

    def put(dx, dy, colour, stroke):
        draw.text(
            (x0 + dx, baseline + dy),
            text,
            font=ft,
            fill=colour,
            anchor="ls",
            stroke_width=stroke,
            stroke_fill=colour,
        )

    stroke = int(round(outline_px * ss))
    if shadow is not None:
        put(shadow_offset[0] * ss, shadow_offset[1] * ss, shadow, stroke)
    if outline is not None and stroke > 0:
        put(0, 0, outline, stroke)
    put(0, 0, fill, 0)
    if shear:
        # Shear about the baseline so the text leans without shifting.
        img = img.transform(
            img.size,
            Image.Transform.AFFINE,
            (1, shear, -shear * baseline, 0, 1, 0),
            resample=Image.Resampling.BICUBIC,
        )
    out = img.resize((max(1, width // ss), max(1, height // ss)), Image.Resampling.LANCZOS)
    return out, baseline // ss


def paste_strip(canvas: Image.Image, strip: Image.Image, baseline: int, x: Optional[int], y_baseline: int) -> None:
    """Composite `strip` so its baseline lands on `y_baseline`; `x` None
    centres it horizontally."""
    if x is None:
        x = (canvas.width - strip.width) // 2
    canvas.alpha_composite(strip, (x, y_baseline - baseline))


def to_palette(img: Image.Image) -> Image.Image:
    """Quantise to a 256-colour palette, the mode the reference `dogs`
    title/subtitle/back-off textures use: an adaptive palette for opaque
    images, else an RGBA-palette octree quantisation (PNG gets a tRNS)."""
    if img.mode == "RGB":
        return img.convert("P", palette=Image.Palette.ADAPTIVE, colors=256)
    return img.quantize(colors=256, method=Image.Quantize.FASTOCTREE)


# ── Folder textures ────────────────────────────────────────────────────
def render_title(on: bool) -> Image.Image:
    """240x126 title: the wordmark in the lower band (like `dogs`), white with
    the stock dark outline and an orange shadow when on, grey tones when off."""
    img = Image.new("RGBA", TITLE_SIZE, (0, 0, 0, 0))
    if on:
        fill, outline, shadow = WHITE, OUTLINE, ACCENT
    else:
        fill, outline, shadow = OFF_FILL, OFF_OUTLINE, OFF_SHADOW
    strip, base = text_strip(TITLE, 33, fill, outline, 2.5, shadow, (3, 3))
    paste_strip(img, strip, base, None, 106)
    return to_palette(img)


def arrow(size: int, colour, edge, direction: str) -> Image.Image:
    """A dance arrow (`up`, `down`, `left`, `right`) of `size` px with a dark
    edge, anti-aliased by supersampling."""
    ss = SUPERSAMPLE
    s = size * ss
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)
    u = s / 48.0  # design unit: the arrow is drawn in a 48-unit box
    poly = [
        (24, 4), (45, 25), (35, 25), (35, 44), (13, 44), (13, 25), (3, 25),
    ]
    pts = [(x * u, y * u) for x, y in poly]
    draw.polygon(pts, fill=edge, outline=edge, width=int(3 * u))
    inner = [
        (24, 9), (38, 23), (30, 23), (30, 40), (18, 40), (18, 23), (10, 23),
    ]
    draw.polygon([(x * u, y * u) for x, y in inner], fill=colour)
    angle = {"up": 0, "left": 90, "down": 180, "right": 270}[direction]
    img = img.rotate(angle)
    return img.resize((size, size), Image.Resampling.LANCZOS)


def render_back(on: bool) -> Image.Image:
    """240x164 card art: a vertical dark gradient with a receptor row of four
    dance arrows in StepMania's quantisation colours (red/blue/green/yellow),
    and a darker band at the bottom under the title. Off = greyscale."""
    w, h = BACK_SIZE
    img = Image.new("RGBA", BACK_SIZE, (0, 0, 0, 255))
    px = img.load()
    for y in range(h):
        t = y / (h - 1)
        # navy -> near black, so the title band below the arrows stays dark
        r = int(28 + (14 - 28) * t)
        g = int(32 + (16 - 32) * t)
        b = int(56 + (28 - 56) * t)
        for x in range(w):
            # faint diagonal hatch, like the stock info banner
            lift = 8 if ((x + y) // 6) % 2 == 0 else 0
            px[x, y] = (r + lift, g + lift, b + lift, 255)
    colours = [(226, 60, 60, 255), (70, 130, 255, 255), (80, 205, 95, 255), (250, 200, 40, 255)]
    edge = (14, 14, 18, 255)
    size = 50
    gap = (w - 4 * size) // 5
    top = 20
    for i, (direction, colour) in enumerate(zip(("left", "down", "up", "right"), colours)):
        x = gap + i * (size + gap)
        img.alpha_composite(arrow(size, colour, edge, direction), (x, top))
    rgb = img.convert("RGB")
    if on:
        return rgb
    return to_palette(rgb.convert("L").convert("RGB"))


def render_subtitle(on: bool) -> Image.Image:
    """240x26 subtitle strip, centred, with the stock translucent edge."""
    img = Image.new("RGBA", SUBTITLE_SIZE, (0, 0, 0, 0))
    fill = ACCENT_SUB if on else SUB_OFF
    strip, base = text_strip(SUBTITLE, 14.5, fill, SUB_EDGE, 1.0)
    paste_strip(img, strip, base, None, 18)
    return to_palette(img)


def render_info() -> Image.Image:
    """488x104 info banner: transparent 17-px top strip, dark diagonal hatch,
    white title and two yellow description lines from x=20 (stock layout)."""
    w, h = INFO_SIZE
    img = Image.new("RGBA", INFO_SIZE, (0, 0, 0, 0))
    px = img.load()
    for y in range(INFO_TOP_CLEAR, h):
        for x in range(w):
            px[x, y] = INFO_STRIPE if ((x + y) % 7) < 3 else INFO_BG
    strip, base = text_strip(TITLE, 35, INFO_TITLE)
    paste_strip(img, strip, base, 19, 55)
    for i, line in enumerate(INFO_LINES):
        strip, base = text_strip(line, 14.5, INFO_TEXT)
        paste_strip(img, strip, base, 19, 74 + 15 * i)
    return img


def render_card() -> Image.Image:
    """220x60 header strip ("Change folder" card): two-line italic wordmark,
    cream fill, dark outline, orange shadow (the stock/`dogs` composition)."""
    img = Image.new("RGBA", CARD_SIZE, (0, 0, 0, 0))
    strip, base = text_strip(TITLE, 25, CARD_FILL, OUTLINE, 2.0, CARD_SHADOW, (3, 3), shear=0.18)
    paste_strip(img, strip, base, None, 31)
    strip, base = text_strip(CARD_LINE2, 13, CARD_FILL, OUTLINE, 1.5, CARD_SHADOW, (2, 2), shear=0.18)
    paste_strip(img, strip, base, None, 47)
    return img


RENDERERS = {
    f"mufo_txt_folder_title_{KEY}_on": lambda: render_title(True),
    f"mufo_txt_folder_title_{KEY}_off": lambda: render_title(False),
    f"mufo_folder_back_{KEY}_on": lambda: render_back(True),
    f"mufo_folder_back_{KEY}_off": lambda: render_back(False),
    f"mufo_txt_folder_subtitle_{KEY}_on": lambda: render_subtitle(True),
    f"mufo_txt_folder_subtitle_{KEY}_off": lambda: render_subtitle(False),
    f"mufo_txt_folder_info_{KEY}": render_info,
    f"muca_txt_folder_name_{KEY}": render_card,
}


def render_texture(name: str) -> Image.Image:
    img = RENDERERS[name]()
    _, size, mode = TEXTURES[name]
    assert img.size == size and img.mode == mode, (name, img.size, img.mode)
    return img


# ── Series labels ──────────────────────────────────────────────────────
def render_series_label(cols: int) -> Image.Image:
    """`sefi_version_stepmania` at a column count, via gen_series_labels."""
    img, _ = labels.render_label(TITLE, cols)
    return img


# ── Output ─────────────────────────────────────────────────────────────
def write_all(out_dir: Path) -> list:
    written = []
    for name, (ifs_dir, _, _) in TEXTURES.items():
        path = out_dir / ifs_dir / "tex" / f"{name}.png"
        path.parent.mkdir(parents=True, exist_ok=True)
        render_texture(name).save(path)
        written.append(path)
    legacy = out_dir / LEGACY_LABEL_DIR / "tex" / f"{LEGACY_LABEL}.png"
    legacy.parent.mkdir(parents=True, exist_ok=True)
    render_series_label(2).save(legacy)  # 104x20, the legacy label canvas
    written.append(legacy)
    label_dir = out_dir / LABELS_DIR
    label_dir.mkdir(parents=True, exist_ok=True)
    for cols in labels.COLUMNS:
        path = label_dir / labels.label_filename(KEY, cols)
        render_series_label(cols).save(path)
        written.append(path)
    return written


def main(argv=None) -> None:
    parser = argparse.ArgumentParser(description="Generate the STEPMANIA folder and series art.")
    parser.add_argument(
        "--out-dir",
        metavar="DIR",
        type=Path,
        default=DEFAULT_OUT_DIR,
        help=f"mod folder to write into (default {labels.rel(DEFAULT_OUT_DIR)})",
    )
    args = parser.parse_args(argv)
    written = write_all(args.out_dir)
    for path in written:
        print(f"  {labels.rel(path)}")
    print(f"wrote {len(written)} file(s) to {labels.rel(args.out_dir)}")


if __name__ == "__main__":
    main()
