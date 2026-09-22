#!/usr/bin/env python3
"""Generate the Background Dancers RANDOM preview badge.

One 170x150 RGBA PNG shown in the options modal's preview box while
BACKGROUND DANCER / BACKGROUND STAGE holds the RANDOM value (the live 3D
preview needs a specific pick): a rounded dark panel in the preview
backdrop colour, a large die face (five pips) in the option-menu white with
a dark outline, and a "RANDOM" caption drawn geometrically (no font files).
Drawn 4x supersampled for smooth edges.

Output: data_mods/background_dancers/tex/preview_random.png (loaded at
runtime via asset_loader; the stem must stay unique in the ResourceManager
namespace). Re-run from the repo root after style edits; copy the result to
the cabinet/install data_mods alongside the DLL.
"""

from PIL import Image, ImageDraw

SS = 4  # supersample factor
W, H = 170, 150
CW, CH = W * SS, H * SS

# Matches `preview::layout::BACKDROP_ARGB` (0xFF0C0C14) so a RANDOM box and a
# live preview box share the same ground.
PANEL = (12, 12, 20, 255)
PANEL_EDGE = (60, 60, 80, 255)
FILL = (255, 255, 255, 240)
OUTLINE = (10, 10, 10, 240)
PIP = (12, 12, 20, 255)
CAPTION = (200, 200, 215, 235)


def rounded(draw: ImageDraw.ImageDraw, box, radius, fill, outline=None, width=0):
    draw.rounded_rectangle(box, radius=radius, fill=fill, outline=outline, width=width)


def die(draw: ImageDraw.ImageDraw, cx: float, cy: float, size: float):
    """A five-pip die face centred at (cx, cy), `size` wide."""
    half = size / 2
    rounded(
        draw,
        (cx - half, cy - half, cx + half, cy + half),
        radius=size * 0.18,
        fill=FILL,
        outline=OUTLINE,
        width=3 * SS,
    )
    r = size * 0.085
    off = size * 0.26
    for dx, dy in ((-off, -off), (off, -off), (0, 0), (-off, off), (off, off)):
        draw.ellipse((cx + dx - r, cy + dy - r, cx + dx + r, cy + dy + r), fill=PIP)


# 5x7 block glyphs for the caption (no TTF dependency).
GLYPHS = {
    "R": ["1110", "1001", "1001", "1110", "1010", "1001", "1001"],
    "A": ["0110", "1001", "1001", "1111", "1001", "1001", "1001"],
    "N": ["1001", "1101", "1101", "1011", "1011", "1001", "1001"],
    "D": ["1110", "1001", "1001", "1001", "1001", "1001", "1110"],
    "O": ["0110", "1001", "1001", "1001", "1001", "1001", "0110"],
    "M": ["10001", "11011", "10101", "10101", "10001", "10001", "10001"],
}


def caption(draw: ImageDraw.ImageDraw, text: str, cx: float, top: float, cell: float):
    widths = [len(GLYPHS[c][0]) for c in text]
    gap = 1.2
    total = sum(w for w in widths) * cell + (len(text) - 1) * gap * cell
    x = cx - total / 2
    for c, w in zip(text, widths):
        rows = GLYPHS[c]
        for ry, row in enumerate(rows):
            for rx, bit in enumerate(row):
                if bit == "1":
                    x0 = x + rx * cell
                    y0 = top + ry * cell
                    draw.rectangle((x0, y0, x0 + cell - 1, y0 + cell - 1), fill=CAPTION)
        x += (w + gap) * cell


# Vertical layout (in output pixels): the die + gap + caption block is
# centred so the top and bottom margins match (maintainer feedback
# 2026-09-22 — the first cut sat the caption 11 px from the bottom under a
# 32 px top margin).
DIE_SIZE = 62
CAPTION_CELL = 3.2
CAPTION_ROWS = 7
GAP = 14


def render() -> Image.Image:
    img = Image.new("RGBA", (CW, CH), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)
    rounded(
        draw,
        (0, 0, CW - 1, CH - 1),
        radius=8 * SS,
        fill=PANEL,
        outline=PANEL_EDGE,
        width=2 * SS,
    )
    caption_h = CAPTION_ROWS * CAPTION_CELL
    block_h = DIE_SIZE + GAP + caption_h
    top = (H - block_h) / 2
    die(draw, CW / 2, (top + DIE_SIZE / 2) * SS, DIE_SIZE * SS)
    caption(draw, "RANDOM", CW / 2, (top + DIE_SIZE + GAP) * SS, CAPTION_CELL * SS)
    return img.resize((W, H), Image.Resampling.LANCZOS)


def main():
    out = "data_mods/background_dancers/tex/preview_random.png"
    import os

    os.makedirs(os.path.dirname(out), exist_ok=True)
    render().save(out)
    print(f"wrote {out} ({W}x{H})")


if __name__ == "__main__":
    main()
