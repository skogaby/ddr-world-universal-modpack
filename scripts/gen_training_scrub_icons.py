#!/usr/bin/env python3
"""Generate the Training Mode FF/RW scrub indicator icons.

Two 128x128 RGBA PNGs — a fast-forward double-triangle (>>) and its
mirrored rewind twin (<<) — white fill with a dark outline so they read
on any gameplay background. Drawn 4x supersampled for smooth edges.

Output: data_mods/training_mode/tex/training_scrub_{ff,rw}.png
(loaded at runtime via asset_loader; stems must stay unique in the
ResourceManager namespace). Re-run from the repo root after style edits;
copy the results to the cabinet/install data_mods alongside the DLL.
"""

from PIL import Image, ImageDraw

SS = 4  # supersample factor
SIZE = 128
CANVAS = SIZE * SS

FILL = (255, 255, 255, 235)
OUTLINE = (10, 10, 10, 235)
OUTLINE_W = 3 * SS


def triangle(x0: float, width: float, top: float, bottom: float):
    """A right-pointing triangle spanning x0..x0+width."""
    mid_y = (top + bottom) / 2
    return [(x0, top), (x0 + width, mid_y), (x0, bottom)]


def render_ff() -> Image.Image:
    img = Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)
    top, bottom = 24 * SS, 104 * SS
    # ~40% of the back triangle hides behind the front one (maintainer
    # taste call 2026-08-15 — a tip-only occlusion read as unintentional).
    tri_w = 52 * SS
    overlap = int(tri_w * 0.40)
    x_first = (CANVAS - (2 * tri_w - overlap)) / 2
    for x0 in (x_first, x_first + tri_w - overlap):
        draw.polygon(
            triangle(x0, tri_w, top, bottom),
            fill=FILL,
            outline=OUTLINE,
            width=OUTLINE_W,
        )
    return img.resize((SIZE, SIZE), Image.Resampling.LANCZOS)


def main():
    out_dir = "data_mods/training_mode/tex"
    ff = render_ff()
    ff.save(f"{out_dir}/training_scrub_ff.png")
    # RW is the exact mirror.
    ff.transpose(Image.Transpose.FLIP_LEFT_RIGHT).save(f"{out_dir}/training_scrub_rw.png")
    print(f"wrote {out_dir}/training_scrub_ff.png + training_scrub_rw.png")


if __name__ == "__main__":
    main()
