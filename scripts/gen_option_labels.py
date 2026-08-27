#!/usr/bin/env python3
"""Generate option label, value-ribbon and preview-box PNGs for custom options.

Emits one texture set per language (see option_strings.AVAILABLE_LANGS; the
--lang flag restricts to one) into that language's IFS tex dir at
data_mods/custom_options/select_music_option_lang_<code>_v3_ifs/tex.
Display strings live in scripts/option_strings.py (data only); this module
owns fonts, layout metrics, and rendering.

Three texture families per language:

1. **Left-side row labels** — seop_item_<id>.png at 176x16 RGBA, black text,
   left-justified, in Inclusive Sans SemiBold. The option name (e.g. "VIDEO
   SIZE").
2. **Value-ribbon chips** — seop_op_<key>.png at 132x24 RGBA, teal (#00ffbd)
   text, horizontally centered, in Inclusive Sans SemiBold. The selected
   value's label (e.g. "FULL SCREEN"). Only NET-NEW values need generating;
   stock ribbons (seop_op_on/off/...) already exist in the game atlas.
3. **Preview-box explainers** — seop_image_<id>[_<value>].png at 368x172 RGBA,
   white body copy in Sen SemiBold. The in-game blurb describing what the
   option (or one specific value of it) does. See the PREVIEWS block below.

For (1) and (2), text wider than the usable width is condensed *horizontally
only* (height preserved), mirroring how the game squeezes its own over-long
labels. (3) word-wraps instead.

Requires: Pillow (pip install Pillow)
"""

import argparse
from pathlib import Path
from typing import NamedTuple, Optional

from PIL import Image, ImageDraw, ImageFont

from option_strings import (
    AVAILABLE_LANGS,
    HEADER_LABELS,
    LABELS,
    PREVIEWS,
    RIBBONS,
    SPLIT,
    TEMPLATES,
    VERBATIM_COPIES,
    WIDE,
    PreviewSpec,
    TemplateSpec,
)

SCRIPT_DIR = Path(__file__).resolve().parent

FONT_PATH = SCRIPT_DIR / "fonts" / "InclusiveSans-SemiBold.ttf"

# CJK fonts: Noto Sans JP/KR variable fonts (SIL OFL 1.1 — license files
# beside them). Weight applied via the wght axis: 600 for labels/headers/
# ribbons (visual match for Inclusive Sans SemiBold at these sizes), 500 for
# preview body copy (match for Sen SemiBold).
CJK_FONT_PATHS = {
    "ja": SCRIPT_DIR / "fonts" / "NotoSansJP[wght].ttf",
    "ko": SCRIPT_DIR / "fonts" / "NotoSansKR[wght].ttf",
}
CJK_LABEL_WEIGHT = 600
CJK_PREVIEW_WEIGHT = 500

# (string-table language key, IFS path code) — the game loads
# select_music_option_lang_<code>_v3.ifs for the player's language.
LANGS = [("en", "eng"), ("ja", "jpn"), ("ko", "kor")]


def out_dir(ifs_code: str) -> Path:
    """The tex dir one language's textures are written to."""
    return (
        SCRIPT_DIR.parent
        / "data_mods"
        / "custom_options"
        / f"select_music_option_lang_{ifs_code}_v3_ifs"
        / "tex"
    )


class FontSet(NamedTuple):
    """The loaded fonts one language renders with."""

    label: ImageFont.FreeTypeFont  # row labels + ribbons (FONT_SIZE)
    header: ImageFont.FreeTypeFont  # group headers (HEADER_FONT_SIZE)
    preview: ImageFont.FreeTypeFont  # preview body copy (PREVIEW_FONT_SIZE)


def load_fonts(lang_key: str) -> FontSet:
    """Load the FontSet for ``lang_key``. English uses the shipped Inclusive
    Sans / Sen pair; ja/ko use the Noto variable fonts at the weights above."""
    if lang_key == "en":
        return FontSet(
            label=ImageFont.truetype(str(FONT_PATH), FONT_SIZE),
            header=ImageFont.truetype(str(FONT_PATH), HEADER_FONT_SIZE),
            preview=ImageFont.truetype(str(PREVIEW_FONT_PATH), PREVIEW_FONT_SIZE),
        )
    path = CJK_FONT_PATHS.get(lang_key)
    if path is None or not path.exists():
        raise SystemExit(f"CJK font not found for {lang_key!r}: {path}")

    def var(size: float, weight: int) -> ImageFont.FreeTypeFont:
        font = ImageFont.truetype(str(path), size)
        font.set_variation_by_axes([weight])
        return font

    return FontSet(
        label=var(FONT_SIZE, CJK_LABEL_WEIGHT),
        header=var(HEADER_FONT_SIZE, CJK_LABEL_WEIGHT),
        preview=var(PREVIEW_FONT_SIZE, CJK_PREVIEW_WEIGHT),
    )

WIDTH, HEIGHT = 176, 16
# Symmetric left/right inset. The pad matches the stock labels' left edge:
# measured against the game's own rows, a pad of 5 put the custom text
# ~2 texture-px (~3-4 screen-px) further right than stock, so 3 lines it up.
SIDE_PAD = 3
LEFT_PAD = RIGHT_PAD = SIDE_PAD
USABLE_WIDTH = WIDTH - LEFT_PAD - RIGHT_PAD

# Pixel size for the label text. 16 matches the stock option-row labels'
# on-screen cap height (~11px once the 176x16 texture is scaled up to the
# row). Tune here if a target cabinet's option rows look off.
FONT_SIZE = 16

# Row in the 16px-tall canvas where the text baseline sits. All labels are
# anchored to this baseline so caps-only rows (e.g. AUTOPLAY) and rows with
# descenders (e.g. CHARACTER (P1)) line up — rather than each being centered
# on its own bounding box, which makes baselines drift between rows. At
# FONT_SIZE 16 this leaves ascenders ~13px above and descenders ~3px below,
# filling the canvas exactly.
BASELINE_Y = 13

# Scratch-buffer baseline; generous headroom above/below for any glyph.
_SCRATCH_W, _SCRATCH_H, _SCRATCH_BASELINE = 1024, 64, 48

# Black text on a transparent background.
TEXT_COLOR = (0, 0, 0, 255)

# ── Group-heading labels (UiKind::Header rows) ───────────────────────────
# Full-width labels for the non-selectable header rows the DLL injects on
# the MODS tab (src/services/custom_options — UiKind::Header; rendered by
# render_header, which hides the row's value box entirely). Same
# seop_item_<id> namespace as the row labels, but on a double-width
# canvas: the option_usr layer renders the bitmap at its natural size, so
# the wider art extends right across the (hidden) value area. Height
# matches the stock 16px label canvas — the bitmap sits in the row's text
# zone with visible margin on all four sides (a centered heading strip;
# maintainer-picked look 2026-08-15 after full-box variants bled into the
# next row: the label-zone origin sits below the row's box grid line, so
# a taller bitmap can't fill the box without overshooting it). Unlike the
# row labels the header canvas is OPAQUE — a dark blue bar with white
# centered text — because the texture IS the header's entire look.
HEADER_WIDTH, HEADER_HEIGHT = 352, 16
HEADER_USABLE_WIDTH = HEADER_WIDTH - 2 * SIDE_PAD
HEADER_BG_COLOR = (24, 40, 96, 255)  # dark blue
HEADER_TEXT_COLOR = (255, 255, 255, 255)  # white
# Header text is smaller than the row labels (~70% — maintainer-picked
# 2026-08-15) so the bar shows clear margin above and below the caps.
HEADER_FONT_SIZE = FONT_SIZE * 0.7
# Baseline row centering the smaller caps block in the 16px canvas: caps
# rise ~13px above the baseline at FONT_SIZE (so ~9px at 70%), leaving
# ~3px top / ~4px bottom margin with the baseline at 12.
HEADER_BASELINE_Y = 12

# ── Value-ribbon chips (right-side option-value labels) ──────────────────
# These render the option *value* shown in the selector (e.g. "FULL SCREEN"),
# in the game's flat, shared `seop_op_<key>` namespace. Stock values
# (`seop_op_on`, `seop_op_off`, ...) already exist in the game atlas and are
# NOT generated here; only net-new value labels need a texture. Unlike the
# left-side labels these are teal (#00ffbd), horizontally centered, on the
# stock ribbon dimensions (132x24).
#
# (Runtime-count options — the WebUI cosmetic pickers — need NO ribbons: they
# render as scalar rows whose value text goes through the game's native digit
# path. The old shared `seop_op_item_<NNN>` "ITEM #NNN" set is retired.)
RIBBON_WIDTH, RIBBON_HEIGHT = 132, 24
RIBBON_PAD = 3  # min inset on each side before condensing kicks in
RIBBON_USABLE_WIDTH = RIBBON_WIDTH - 2 * RIBBON_PAD
# Baseline within the 24px ribbon canvas. At FONT_SIZE 16 the caps run ~13px
# above the baseline; placing the baseline at 18 vertically centers the
# caps-height block (≈3px top / ≈3px bottom margin) in the taller chip.
RIBBON_BASELINE_Y = 18
RIBBON_TEXT_COLOR = (0, 0xFF, 0xBD, 255)  # #00ffbd

# ── Preview-box explainers (seop_image_*) ────────────────────────────────
# The 368x172 panel the game shows on the right of the options list while a
# row is focused: body copy explaining what the option does, optionally beside
# a screenshot. The game looks up `seop_image_<option_id>_<value_key>` for the
# focused value and falls back to `seop_image_<option_id>` (see
# `EnumValue::preview_key` in src/services/custom_options/api.rs and
# docs/option_preview_image_box.md).
#
# All the metrics below were reverse-measured off the hand-authored Photoshop
# originals in OUT_DIR (seop_image_autoplay_*, seop_image_premium_free_*,
# seop_image_customize_*), so generated panels sit pixel-flush with them:
# per-line baselines and per-line ink left edges were fit against every line of
# all 18 shipped panels, and the wrap widths were chosen as the values that
# reproduce every original's line breaks exactly.
PREVIEW_WIDTH, PREVIEW_HEIGHT = 368, 172

PREVIEW_FONT_PATH = SCRIPT_DIR / "fonts" / "Sen-SemiBold.ttf"

# Fractional on purpose: the originals' glyph advances fit Sen SemiBold at
# 13.5px within ±1px over every measured line — 13 is visibly too narrow and
# 14 too wide.
PREVIEW_FONT_SIZE = 13.5

PREVIEW_TEXT_COLOR = (255, 255, 255, 255)

# Pen x (text origin, NOT the ink edge) for a continuation line. Body copy
# lands its first ink column at x=13 in every original; the ~0.6px slack is
# the left side bearing.
PREVIEW_LEFT_PEN = 12.4
# Extra pen offset applied to the FIRST line of each paragraph. The originals
# indent it by ~2.5 space-widths; matched here as a flat pixel offset so it
# doesn't drift with the font size.
PREVIEW_INDENT = 10.0

# Baseline of the first line, then the baseline-to-baseline step within a
# paragraph. Both exact across all 18 originals (25, 41, 57, 73, 89, ...).
PREVIEW_FIRST_BASELINE = 25
PREVIEW_LINE_PITCH = 16
# Baseline step ACROSS a paragraph break — i.e. the blank line between two
# paragraphs is worth 13px, not a full 16px line. Also exact across every
# two-paragraph original (last line of para 1 at 41 → first of para 2 at 70).
PREVIEW_PARAGRAPH_PITCH = 29
# Deepest baseline that still leaves room for descenders inside the panel
# (Sen's descenders drop ~3px below the baseline at PREVIEW_FONT_SIZE). Copy
# that runs past it is a hard warning — trim it, the panel can't scroll.
PREVIEW_LAST_BASELINE = PREVIEW_HEIGHT - 4

# Rightmost pen x a line may reach, per layout. SPLIT keeps the copy clear of
# the dotted divider (whose ink is at x=183..184); WIDE runs the full panel,
# mirroring the ~13px left margin on the right.
PREVIEW_SPLIT_RIGHT = 173
PREVIEW_WIDE_RIGHT = 355

# The dotted-divider-only canvas used as the SPLIT base layer, lifted verbatim
# (byte-identical alpha) from the shipped originals — every split-view panel
# Konami/our artist authored carries the exact same two columns of dashes.
PREVIEW_DIVIDER_PATH = SCRIPT_DIR / "templates" / "seop_image_split_divider.png"

# Committed image assets: SPLIT-panel art crops (extracted once from the
# hand-authored originals — bounding-box crop of the ink right of the
# divider, x >= 186) and the verbatim per-language masters.
TEMPLATES_DIR = SCRIPT_DIR / "templates"



def render_strip(
    font: ImageFont.FreeTypeFont, text: str, color=TEXT_COLOR
) -> Image.Image:
    """Render ``text`` baseline-anchored and return an image cropped to its
    horizontal extent but keeping a fixed vertical frame, so the text
    baseline sits at a constant row across every label. ``color`` is the RGBA
    fill (defaults to black for the left-side labels). Antialiased alpha,
    transparent background. Returns None for empty input."""
    scratch = Image.new("RGBA", (_SCRATCH_W, _SCRATCH_H), (0, 0, 0, 0))
    draw = ImageDraw.Draw(scratch)
    # anchor="ls" = left edge, baseline — so the y we pass is the baseline row.
    draw.text((0, _SCRATCH_BASELINE), text, font=font, fill=color, anchor="ls")

    bbox = scratch.getbbox()
    if bbox is None:
        return None
    # Crop horizontally to the inked columns, but keep the full scratch height
    # so the baseline stays at _SCRATCH_BASELINE regardless of ascender/
    # descender presence in this particular label.
    return scratch.crop((bbox[0], 0, bbox[2], _SCRATCH_H))


def fit_width(strip: Image.Image, usable_width: int) -> Image.Image:
    """Condense ``strip`` horizontally if it exceeds ``usable_width``,
    keeping its height. This mirrors the game's own option rows: over-long
    text is squeezed on the X axis only, so height stays constant rather than
    shrinking uniformly."""
    if strip.width <= usable_width:
        return strip
    return strip.resize((usable_width, strip.height), Image.LANCZOS)


def render_label(font: ImageFont.FreeTypeFont, text: str) -> tuple[Image.Image, bool]:
    """Render a left-side option label: 176x16, black, left-justified at
    LEFT_PAD, baseline-aligned to BASELINE_Y. Returns (image, condensed)."""
    img = Image.new("RGBA", (WIDTH, HEIGHT), (0, 0, 0, 0))
    strip = render_strip(font, text)
    condensed = False
    if strip is not None:
        condensed = strip.width > USABLE_WIDTH
        strip = fit_width(strip, USABLE_WIDTH)
        # Align the scratch baseline (_SCRATCH_BASELINE) onto BASELINE_Y.
        img.alpha_composite(strip, (LEFT_PAD, BASELINE_Y - _SCRATCH_BASELINE))
    return img, condensed


def render_header_label(font: ImageFont.FreeTypeFont, text: str) -> tuple[Image.Image, bool]:
    """Render a group-heading label: HEADER_WIDTHxHEADER_HEIGHT, an opaque
    dark blue bar with white text at HEADER_FONT_SIZE, horizontally
    centered, baseline-aligned to HEADER_BASELINE_Y. Returns
    (image, condensed)."""
    img = Image.new("RGBA", (HEADER_WIDTH, HEADER_HEIGHT), HEADER_BG_COLOR)
    strip = render_strip(font, text, color=HEADER_TEXT_COLOR)
    condensed = False
    if strip is not None:
        condensed = strip.width > HEADER_USABLE_WIDTH
        strip = fit_width(strip, HEADER_USABLE_WIDTH)
        x = (HEADER_WIDTH - strip.width) // 2  # horizontal center
        img.alpha_composite(strip, (x, HEADER_BASELINE_Y - _SCRATCH_BASELINE))
    return img, condensed


def render_ribbon(font: ImageFont.FreeTypeFont, text: str) -> tuple[Image.Image, bool]:
    """Render a value-ribbon chip: 132x24, teal (#00ffbd), horizontally
    centered, baseline-aligned to RIBBON_BASELINE_Y. Returns (image,
    condensed)."""
    img = Image.new("RGBA", (RIBBON_WIDTH, RIBBON_HEIGHT), (0, 0, 0, 0))
    strip = render_strip(font, text, color=RIBBON_TEXT_COLOR)
    condensed = False
    if strip is not None:
        condensed = strip.width > RIBBON_USABLE_WIDTH
        strip = fit_width(strip, RIBBON_USABLE_WIDTH)
        x = (RIBBON_WIDTH - strip.width) // 2  # horizontal center
        img.alpha_composite(strip, (x, RIBBON_BASELINE_Y - _SCRATCH_BASELINE))
    return img, condensed


def preview_right_edge(layout: str) -> int:
    """Rightmost pen x a wrapped line may reach for ``layout``."""
    if layout == SPLIT:
        return PREVIEW_SPLIT_RIGHT
    if layout == WIDE:
        return PREVIEW_WIDE_RIGHT
    raise SystemExit(f"Unknown preview layout {layout!r} (expected {SPLIT}/{WIDE})")


# ── CJK line breaking (kinsoku shori) ────────────────────────────────────
# Characters that may break BEFORE/AFTER them when adjacent to other CJK
# break units. Japanese lines must not START with closing punctuation or
# small kana, nor END with an opening bracket. Korean is NOT broken per
# character (it wraps on spaces like English — Hangul is excluded from the
# CJK-char test below).
NO_BREAK_BEFORE = set("。、）」』！？ーぁぃぅぇぉっゃゅょァィゥェォッャュョ…・：；％%)]｝】》")
NO_BREAK_AFTER = set("（「『([｛【《")


def is_cjk_char(ch: str) -> bool:
    """True for characters that form their own break unit (Han, kana, CJK
    punctuation, fullwidth forms). Deliberately EXCLUDES Hangul — Korean
    wraps on spaces."""
    cp = ord(ch)
    return (
        0x3000 <= cp <= 0x30FF  # CJK punctuation, hiragana, katakana
        or 0x4E00 <= cp <= 0x9FFF  # CJK unified ideographs
        or 0xFF00 <= cp <= 0xFFEF  # full/half-width forms
        or 0x3400 <= cp <= 0x4DBF  # CJK extension A
    )


def break_units(text: str) -> list[tuple[str, bool]]:
    """Split ``text`` into (unit, preceded_by_space) break units: whitespace-
    delimited words for Latin/Korean, single characters for CJK runs, then
    merged so no unit starts with a NO_BREAK_BEFORE character or follows a
    NO_BREAK_AFTER character."""
    # A unit is space-preceded iff it starts a whitespace-delimited source
    # word (CJK units within a word carry no glue — they join bare).
    units: list[tuple[str, bool]] = []
    for word in text.split():
        word_units: list[str] = []
        chars = []
        for ch in word:
            if is_cjk_char(ch):
                if chars:
                    word_units.append("".join(chars))
                    chars = []
                word_units.append(ch)
            else:
                chars.append(ch)
        if chars:
            word_units.append("".join(chars))
        for j, u in enumerate(word_units):
            units.append((u, j == 0))
    # kinsoku merge
    merged: list[tuple[str, bool]] = []
    for unit, glue in units:
        if merged and not glue:
            prev_unit, prev_glue = merged[-1]
            if unit[0] in NO_BREAK_BEFORE or prev_unit[-1] in NO_BREAK_AFTER:
                merged[-1] = (prev_unit + unit, prev_glue)
                continue
        merged.append((unit, glue))
    return merged


def wrap_paragraph(
    font: ImageFont.FreeTypeFont, text: str, right_edge: int
) -> list[str]:
    """Greedy wrap ``text`` into lines that fit between the paragraph's pen x
    and ``right_edge``. The first line starts PREVIEW_INDENT further right,
    so it holds less — same as the originals.

    Latin/Korean text breaks at spaces (byte-identical to the pre-CJK
    behavior); Japanese breaks between CJK characters subject to the kinsoku
    sets above."""
    if not any(is_cjk_char(ch) for ch in text):
        # Legacy Latin path — kept verbatim so English output is unchanged.
        lines: list[str] = []
        current: list[str] = []
        for word in text.split():
            candidate = " ".join(current + [word])
            pen = PREVIEW_LEFT_PEN + (PREVIEW_INDENT if not lines else 0.0)
            if current and pen + font.getlength(candidate) > right_edge:
                lines.append(" ".join(current))
                current = [word]
            else:
                current.append(word)
        if current:
            lines.append(" ".join(current))
        return lines

    lines = []
    current = ""
    for unit, glue in break_units(text):
        joiner = " " if (glue and current) else ""
        candidate = current + joiner + unit
        pen = PREVIEW_LEFT_PEN + (PREVIEW_INDENT if not lines else 0.0)
        if current and pen + font.getlength(candidate) > right_edge:
            lines.append(current)
            current = unit
        else:
            current = candidate
    if current:
        lines.append(current)
    return lines


def render_preview(
    font: ImageFont.FreeTypeFont,
    spec: PreviewSpec,
    lang_key: str,
    divider: Optional[Image.Image],
) -> tuple[Image.Image, list[str]]:
    """Render one 368x172 preview panel in ``lang_key``. Returns
    (image, warnings)."""
    right_edge = preview_right_edge(spec.layout)
    warnings: list[str] = []

    img = Image.new("RGBA", (PREVIEW_WIDTH, PREVIEW_HEIGHT), (0, 0, 0, 0))
    if spec.layout == SPLIT:
        if divider is None:
            raise SystemExit(f"Divider template not found: {PREVIEW_DIVIDER_PATH}")
        img.alpha_composite(divider)
    if spec.art is not None:
        art_path = TEMPLATES_DIR / spec.art
        if not art_path.exists():
            raise SystemExit(f"Art asset not found: {art_path}")
        img.alpha_composite(Image.open(art_path).convert("RGBA"), spec.art_pos)

    draw = ImageDraw.Draw(img)
    baseline = float(PREVIEW_FIRST_BASELINE)
    for para_index, paragraph in enumerate(spec.paragraphs[lang_key]):
        if para_index:
            baseline += PREVIEW_PARAGRAPH_PITCH - PREVIEW_LINE_PITCH
        for line_index, line in enumerate(wrap_paragraph(font, paragraph, right_edge)):
            if line and line[0] in NO_BREAK_BEFORE:
                warnings.append(f"kinsoku violation — line starts with {line[0]!r}: {line!r}")
            pen = PREVIEW_LEFT_PEN + (PREVIEW_INDENT if not line_index else 0.0)
            overflow = pen + font.getlength(line) - right_edge
            if overflow > 0:
                warnings.append(f"line overflows by {overflow:.0f}px: {line!r}")
            if baseline > PREVIEW_LAST_BASELINE:
                warnings.append(f"line falls off the panel: {line!r}")
            # anchor="ls" = left edge, baseline — so y is the baseline row.
            draw.text(
                (pen, baseline), line, font=font, fill=PREVIEW_TEXT_COLOR, anchor="ls"
            )
            baseline += PREVIEW_LINE_PITCH

    return img, warnings


def render_template(
    font: ImageFont.FreeTypeFont,
    spec: TemplateSpec,
    lang_key: str,
    divider: Optional[Image.Image],
) -> Image.Image:
    """Render one customize preview-chrome template: dotted divider +
    pre-broken text lines (first line indented, 16px pitch from baseline 25 —
    the preview panels' grid) + the solid marker rectangle(s) the DLL parses
    at runtime for art placement. Marker geometry comes straight from the
    spec, so it is byte-identical across languages."""
    img = Image.new("RGBA", (PREVIEW_WIDTH, PREVIEW_HEIGHT), (0, 0, 0, 0))
    if divider is None:
        raise SystemExit(f"Divider template not found: {PREVIEW_DIVIDER_PATH}")
    img.alpha_composite(divider)

    draw = ImageDraw.Draw(img)
    baseline = float(PREVIEW_FIRST_BASELINE)
    for line_index, line in enumerate(spec.lines[lang_key]):
        pen = PREVIEW_LEFT_PEN + (PREVIEW_INDENT if not line_index else 0.0)
        overflow = pen + font.getlength(line) - PREVIEW_SPLIT_RIGHT
        if overflow > 0:
            raise SystemExit(
                f"template {spec.option} [{lang_key}] line overflows the "
                f"text column by {overflow:.0f}px: {line!r}"
            )
        if line and line[0] in NO_BREAK_BEFORE:
            raise SystemExit(
                f"template {spec.option} [{lang_key}] kinsoku violation: {line!r}"
            )
        draw.text(
            (pen, baseline), line, font=font, fill=PREVIEW_TEXT_COLOR, anchor="ls"
        )
        baseline += PREVIEW_LINE_PITCH

    for x, y, w, h, rgba in spec.markers:
        draw.rectangle((x, y, x + w - 1, y + h - 1), fill=tuple(rgba))
    return img


def preview_filename(spec: PreviewSpec) -> str:
    """seop_image_<option>_<value>.png, or seop_image_<option>.png when the
    spec is the option's single fallback panel."""
    stem = spec.option if spec.value is None else f"{spec.option}_{spec.value}"
    return f"seop_image_{stem}.png"


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Generate custom-option textures per language."
    )
    parser.add_argument(
        "--lang",
        choices=[key for key, _ in LANGS],
        default=None,
        help="Generate one language only (default: every AVAILABLE_LANGS entry)",
    )
    args = parser.parse_args()

    lang_keys = [args.lang] if args.lang else list(AVAILABLE_LANGS)
    for lang_key in lang_keys:
        if lang_key not in AVAILABLE_LANGS:
            raise SystemExit(f"Language {lang_key!r} has no string coverage yet")

    if not FONT_PATH.exists():
        raise SystemExit(f"Font not found: {FONT_PATH}")
    if PREVIEWS and not PREVIEW_FONT_PATH.exists():
        raise SystemExit(f"Font not found: {PREVIEW_FONT_PATH}")

    ifs_code_for = dict(LANGS)
    for lang_key in lang_keys:
        generate_language(lang_key, ifs_code_for[lang_key])


def generate_language(lang_key: str, ifs_code: str) -> None:
    """Generate every texture family for one language into its tex dir."""
    dest = out_dir(ifs_code)
    dest.mkdir(parents=True, exist_ok=True)
    fonts = load_fonts(lang_key)

    def text_for(table: dict, key: str) -> str:
        entry = table[key]
        if lang_key not in entry:
            raise SystemExit(f"Missing {lang_key!r} text for {key!r}")
        return entry[lang_key]

    for option_id in LABELS:
        img, condensed = render_label(fonts.label, text_for(LABELS, option_id))
        out_path = dest / f"seop_item_{option_id}.png"
        img.save(out_path)
        print(f"wrote {out_path.name}{' (condensed)' if condensed else ''}")

    for option_id in HEADER_LABELS:
        img, condensed = render_header_label(
            fonts.header, text_for(HEADER_LABELS, option_id)
        )
        out_path = dest / f"seop_item_{option_id}.png"
        img.save(out_path)
        print(f"wrote {out_path.name}{' (condensed)' if condensed else ''}")

    for ribbon_key in RIBBONS:
        img, condensed = render_ribbon(fonts.label, text_for(RIBBONS, ribbon_key))
        out_path = dest / f"seop_op_{ribbon_key}.png"
        img.save(out_path)
        print(f"wrote {out_path.name}{' (condensed)' if condensed else ''}")

    if PREVIEWS:
        divider = (
            Image.open(PREVIEW_DIVIDER_PATH).convert("RGBA")
            if PREVIEW_DIVIDER_PATH.exists()
            else None
        )
        for spec in PREVIEWS:
            if lang_key not in spec.paragraphs:
                raise SystemExit(
                    f"Missing {lang_key!r} text for preview {preview_filename(spec)}"
                )
            img, warnings = render_preview(fonts.preview, spec, lang_key, divider)
            out_path = dest / preview_filename(spec)
            img.save(out_path)
            print(f"wrote {out_path.name}")
            for warning in warnings:
                # CJK languages have no shipped precedent to eyeball against,
                # so layout problems are hard failures rather than warnings.
                if lang_key != "en":
                    raise SystemExit(f"{out_path.name}: {warning}")
                print(f"  WARNING: {warning}")

    if TEMPLATES:
        divider = (
            Image.open(PREVIEW_DIVIDER_PATH).convert("RGBA")
            if PREVIEW_DIVIDER_PATH.exists()
            else None
        )
        for spec in TEMPLATES.values():
            if lang_key not in spec.lines:
                raise SystemExit(
                    f"Missing {lang_key!r} text for template {spec.option}"
                )
            img = render_template(fonts.preview, spec, lang_key, divider)
            out_path = dest / f"seop_image_{spec.option}_TEMPLATE.png"
            img.save(out_path)
            print(f"wrote {out_path.name}")

    for tex_name, master in VERBATIM_COPIES.items():
        master_path = TEMPLATES_DIR / master
        if not master_path.exists():
            raise SystemExit(f"Master asset not found: {master_path}")
        out_path = dest / f"{tex_name}.png"
        out_path.write_bytes(master_path.read_bytes())
        print(f"wrote {out_path.name} (verbatim)")

    print(
        f"\n[{lang_key}] {len(LABELS)} label(s) + {len(HEADER_LABELS)} header(s) + "
        f"{len(RIBBONS)} ribbon(s) + {len(PREVIEWS)} preview(s) + "
        f"{len(TEMPLATES)} template(s) + {len(VERBATIM_COPIES)} verbatim "
        f"cop(ies) written to {dest}"
    )


if __name__ == "__main__":
    main()
