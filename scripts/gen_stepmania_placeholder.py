#!/usr/bin/env python3
"""Generate the StepMania pack loader's placeholder jacket.

`data_mods/stepmania_packs/placeholder_jacket.png` is the 512x512 RGBA jacket
the loader serves (`src/core/art/mod.rs`, embedded with `include_bytes!`) for
an injected song whose folder carries no usable art — no `#JACKET`, `#BANNER`
or `#BACKGROUND`, no file the name heuristics recognise and no pack-level
banner — or whose art fails to decode. It is deliberately simple: a dark
two-tone panel with the StepMania orange accent and a light up-arrow glyph,
drawn with PIL primitives only (no fonts, so the output is byte-stable across
machines).

The PNG is a committed artefact: change the constants here and regenerate;
never hand-edit it.

Usage:
  python3 scripts/gen_stepmania_placeholder.py [--out PATH]
"""

from __future__ import annotations

import argparse
import os
import sys

try:
    from PIL import Image, ImageDraw
except ImportError:  # pragma: no cover - tooling guard
    sys.exit("Pillow is required: pip install Pillow")

SIZE = 512
BACK_TOP = (24, 26, 34)
BACK_BOTTOM = (46, 50, 64)
ACCENT = (255, 138, 0)
ARROW = (232, 236, 244)
BAND = (18, 19, 25)

DEFAULT_OUT = os.path.join(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
    "data_mods",
    "stepmania_packs",
    "placeholder_jacket.png",
)


def tilde(path: str) -> str:
    home = os.path.expanduser("~")
    return "~" + path[len(home):] if path.startswith(home) else path


def render() -> Image.Image:
    img = Image.new("RGBA", (SIZE, SIZE), BACK_TOP + (255,))
    draw = ImageDraw.Draw(img)
    # Vertical two-tone gradient.
    for y in range(SIZE):
        t = y / (SIZE - 1)
        c = tuple(round(BACK_TOP[i] * (1 - t) + BACK_BOTTOM[i] * t) for i in range(3))
        draw.line([(0, y), (SIZE - 1, y)], fill=c + (255,))
    # Dark bands top and bottom with an orange rule under / over them.
    band = 56
    draw.rectangle([0, 0, SIZE - 1, band - 1], fill=BAND + (255,))
    draw.rectangle([0, SIZE - band, SIZE - 1, SIZE - 1], fill=BAND + (255,))
    draw.rectangle([0, band, SIZE - 1, band + 5], fill=ACCENT + (255,))
    draw.rectangle([0, SIZE - band - 6, SIZE - 1, SIZE - band - 1], fill=ACCENT + (255,))
    # Up-arrow glyph: a triangle head over a stem, both outlined in the accent.
    cx = SIZE // 2
    head_top, head_base, half = 120, 290, 130
    stem_w, stem_bottom = 76, 400
    head = [(cx, head_top), (cx + half, head_base), (cx - half, head_base)]
    stem = [cx - stem_w // 2, head_base - 2, cx + stem_w // 2, stem_bottom]
    draw.polygon(head, fill=ARROW + (255,), outline=ACCENT + (255,))
    draw.rectangle(stem, fill=ARROW + (255,), outline=ACCENT + (255,))
    # Thicken the outline for visibility at thumbnail size.
    draw.line(head + [head[0]], fill=ACCENT + (255,), width=6)
    draw.line(
        [(stem[0], stem[1] + 4), (stem[0], stem[3]), (stem[2], stem[3]), (stem[2], stem[1] + 4)],
        fill=ACCENT + (255,),
        width=6,
    )
    return img


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--out", default=DEFAULT_OUT, help="output PNG path")
    args = ap.parse_args()
    os.makedirs(os.path.dirname(os.path.abspath(args.out)), exist_ok=True)
    render().save(args.out, format="PNG", optimize=True)
    print(f"wrote {tilde(os.path.abspath(args.out))}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
