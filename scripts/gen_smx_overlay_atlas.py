#!/usr/bin/env python3
"""Generate the SMX touch-overlay button atlas.

Draws DDR Gold-cab-styled button faces (see ~/Desktop/buttons.png /
pinpad.jpeg references — silver convex menu buttons with near-black
rounded bevels, Kokushin-style charcoal pinpad keycaps with light
legends) into one RGBA atlas, plus per-shape pressed-highlight overlays
and a lamp-lit menu variant for the dimlamp crossfade.

Outputs:
  data_mods/smx_hardware/overlay_atlas.png   (the atlas, loaded at runtime)
  src/mods/smx_hardware/overlay_atlas.rs     (generated UV table)

Deterministic: same inputs => same bytes. Rerun after editing and commit
both outputs. Cells are drawn at 2-3x their display size for crispness.

Usage: python3 scripts/gen_smx_overlay_atlas.py
"""

from __future__ import annotations

import math
from dataclasses import dataclass
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter, ImageFont

SCRIPT_DIR = Path(__file__).resolve().parent
REPO = SCRIPT_DIR.parent
FONT_PATH = SCRIPT_DIR / "fonts" / "InclusiveSans-SemiBold.ttf"
# The PNG basename MUST equal the runtime texture stem: the engine's
# loose-PNG loader registers the texture under the file's basename, and
# asset_loader::resolve hashes that name (deploy #19 finding — a
# mismatched stem polls forever).
ATLAS_PNG = REPO / "data_mods" / "smx_hardware" / "smx_overlay_atlas.png"
ATLAS_RS = REPO / "src" / "mods" / "smx_hardware" / "overlay_atlas.rs"

# Cell sizes (px in the atlas; display sizes are 50x50 menu, 30x30 key,
# 120x30 utility).
MENU = 128
KEY = 96
UTIL_W, UTIL_H = 240, 60
PAD = 2  # transparent gutter between cells


# ── drawing helpers ──────────────────────────────────────────────────


def rounded_rect_mask(size: tuple[int, int], radius: int) -> Image.Image:
    m = Image.new("L", size, 0)
    d = ImageDraw.Draw(m)
    d.rounded_rectangle([0, 0, size[0] - 1, size[1] - 1], radius=radius, fill=255)
    return m


def vertical_gradient(size: tuple[int, int], top: tuple, bottom: tuple) -> Image.Image:
    """Linear top->bottom RGBA gradient."""
    w, h = size
    img = Image.new("RGBA", size)
    px = img.load()
    for y in range(h):
        t = y / max(h - 1, 1)
        c = tuple(int(top[i] + (bottom[i] - top[i]) * t) for i in range(4))
        for x in range(w):
            px[x, y] = c
    return img


def radial_glow(size: tuple[int, int], center_c: tuple, edge_c: tuple) -> Image.Image:
    """Radial gradient from center color to edge color."""
    w, h = size
    img = Image.new("RGBA", size)
    px = img.load()
    cx, cy = (w - 1) / 2, (h - 1) / 2
    maxd = math.hypot(cx, cy)
    for y in range(h):
        for x in range(w):
            t = min(math.hypot(x - cx, y - cy) / maxd, 1.0)
            px[x, y] = tuple(
                int(center_c[i] + (edge_c[i] - center_c[i]) * t) for i in range(4)
            )
    return img


def menu_button(size: int, lit: bool) -> Image.Image:
    """A Gold-cab menu button face: near-black rounded bevel ring around a
    convex silver diffuser face (lit variant: warm lamp glow)."""
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    radius = size * 12 // 128
    # Bevel ring: near-black with a subtle vertical sheen.
    bevel = vertical_gradient(
        (size, size), (38, 38, 42, 255), (16, 16, 18, 255)
    )
    img.paste(bevel, (0, 0), rounded_rect_mask((size, size), radius))

    # Face inset (the silver diffuser). Bevel width ~13% of the cell.
    inset = size * 17 // 128
    fs = size - 2 * inset
    fradius = max(fs * 10 // 94, 4)
    if lit:
        # Lamp glow through the diffuser: bright white with a SLIGHT
        # warm hue (deploy #19b iterations: pure gold read too yellow,
        # pure white too subtle at overlay opacity).
        face = radial_glow(
            (fs, fs), (255, 252, 238, 255), (238, 226, 188, 255)
        )
    else:
        # Convex brushed silver: light top-center falling to darker edges.
        face = radial_glow((fs, fs), (232, 233, 238, 255), (148, 152, 160, 255))
        # Specular band across the upper third.
        spec = Image.new("RGBA", (fs, fs), (0, 0, 0, 0))
        sd = ImageDraw.Draw(spec)
        sd.ellipse(
            [fs * -0.25, fs * -0.55, fs * 1.25, fs * 0.45],
            fill=(255, 255, 255, 90),
        )
        spec = spec.filter(ImageFilter.GaussianBlur(fs * 0.06))
        face = Image.alpha_composite(face, spec)
    # Soft inner shadow where the face meets the bevel.
    shadow = Image.new("RGBA", (fs, fs), (0, 0, 0, 0))
    sd = ImageDraw.Draw(shadow)
    sd.rounded_rectangle(
        [0, 0, fs - 1, fs - 1], radius=fradius, outline=(0, 0, 0, 110), width=max(fs // 32, 2)
    )
    shadow = shadow.filter(ImageFilter.GaussianBlur(fs * 0.02))
    face = Image.alpha_composite(face, shadow)
    img.paste(face, (inset, inset), rounded_rect_mask((fs, fs), fradius))
    return img


def menu_glow(size: int) -> Image.Image:
    """The lamp BLOOM halo: a soft warm-white radial glow drawn on a cell
    2x the button footprint (the button occupies the central half; the
    runtime inflates the quad so the halo spills past the bevel). Pure
    alpha falloff — reads through the overlay opacity where the lit face
    alone gets subtle."""
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    px = img.load()
    cx = cy = (size - 1) / 2
    maxd = size / 2
    for y in range(size):
        for x in range(size):
            t = min(math.hypot(x - cx, y - cy) / maxd, 1.0)
            # Flat-ish core (hidden behind the button) then smooth
            # quadratic falloff to nothing at the cell edge.
            if t < 0.38:
                a = 235
            else:
                k = 1.0 - (t - 0.38) / 0.62
                a = int(235 * k * k)
            px[x, y] = (255, 248, 222, a)
    return img


def keycap(size: int, legend: str, font: ImageFont.FreeTypeFont) -> Image.Image:
    """A Kokushin-style pinpad keycap: charcoal rounded cap, lighter top
    plateau, light-grey legend (empty legend = the blank key)."""
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    radius = size * 14 // 96
    # Cap body: dark charcoal, slightly lighter at top.
    body = vertical_gradient((size, size), (74, 75, 79, 255), (40, 41, 44, 255))
    img.paste(body, (0, 0), rounded_rect_mask((size, size), radius))
    # Top plateau: inset lighter face (keycap top), offset slightly up.
    inset = size * 10 // 96
    pw, ph = size - 2 * inset, size - 2 * inset - size * 4 // 96
    plateau = vertical_gradient((pw, ph), (92, 94, 99, 255), (58, 59, 63, 255))
    img.paste(
        plateau,
        (inset, inset),
        rounded_rect_mask((pw, ph), max(radius - 4, 4)),
    )
    if legend:
        d = ImageDraw.Draw(img)
        # Center the legend within the plateau.
        bbox = d.textbbox((0, 0), legend, font=font)
        tw, th = bbox[2] - bbox[0], bbox[3] - bbox[1]
        d.text(
            ((size - tw) / 2 - bbox[0], (size - size * 4 // 96 - th) / 2 - bbox[1]),
            legend,
            font=font,
            fill=(206, 208, 212, 255),
        )
    return img


def utility_button(
    w: int, h: int, legend: str, font: ImageFont.FreeTypeFont
) -> Image.Image:
    """A wide utility button (INSERT CARD / HIDE / SHOW): dark rounded
    rect, thin lighter edge, light legend."""
    img = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    radius = h * 14 // 60
    body = vertical_gradient((w, h), (58, 60, 66, 255), (30, 31, 35, 255))
    img.paste(body, (0, 0), rounded_rect_mask((w, h), radius))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle(
        [1, 1, w - 2, h - 2], radius=radius, outline=(120, 124, 132, 160), width=2
    )
    bbox = d.textbbox((0, 0), legend, font=font)
    tw, th = bbox[2] - bbox[0], bbox[3] - bbox[1]
    d.text(
        ((w - tw) / 2 - bbox[0], (h - th) / 2 - bbox[1]),
        legend,
        font=font,
        fill=(214, 216, 220, 255),
    )
    return img


def pressed_overlay(w: int, h: int, radius: int) -> Image.Image:
    """Per-shape pressed highlight: translucent grey rounded fill + a
    brighter rim, matching the button geometry so corners stay rounded."""
    img = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle([0, 0, w - 1, h - 1], radius=radius, fill=(210, 212, 218, 80))
    d.rounded_rectangle(
        [0, 0, w - 1, h - 1], radius=radius, outline=(235, 237, 240, 160), width=max(h // 24, 2)
    )
    return img


# ── atlas assembly ───────────────────────────────────────────────────


@dataclass
class Cell:
    name: str
    img: Image.Image
    x: int = 0
    y: int = 0


def build_cells() -> list[Cell]:
    key_font = ImageFont.truetype(str(FONT_PATH), KEY * 40 // 96)
    key_font_small = ImageFont.truetype(str(FONT_PATH), KEY * 30 // 96)
    util_font = ImageFont.truetype(str(FONT_PATH), UTIL_H * 26 // 60)

    cells = [
        Cell("menu_off", menu_button(MENU, lit=False)),
        Cell("menu_lit", menu_button(MENU, lit=True)),
        Cell("menu_glow", menu_glow(MENU * 2)),
        Cell("menu_pressed", pressed_overlay(MENU, MENU, MENU * 12 // 128)),
        Cell("key_pressed", pressed_overlay(KEY, KEY, KEY * 14 // 96)),
        Cell("util_pressed", pressed_overlay(UTIL_W, UTIL_H, UTIL_H * 14 // 60)),
    ]
    for legend in ["7", "8", "9", "4", "5", "6", "1", "2", "3", "0"]:
        cells.append(Cell(f"key_{legend}", keycap(KEY, legend, key_font)))
    cells.append(Cell("key_00", keycap(KEY, "00", key_font_small)))
    cells.append(Cell("key_blank", keycap(KEY, "", key_font)))
    cells.append(Cell("util_card", utility_button(UTIL_W, UTIL_H, "INSERT CARD", util_font)))
    cells.append(Cell("util_hide", utility_button(UTIL_W, UTIL_H, "HIDE OVERLAY", util_font)))
    cells.append(Cell("util_show", utility_button(UTIL_W, UTIL_H, "SHOW OVERLAY", util_font)))
    return cells


def pack(cells: list[Cell], atlas_w: int = 512) -> tuple[int, int]:
    """Shelf-pack the cells left-to-right, top-to-bottom."""
    x = y = shelf_h = 0
    for c in cells:
        w, h = c.img.size
        if x + w + PAD > atlas_w:
            x = 0
            y += shelf_h + PAD
            shelf_h = 0
        c.x, c.y = x, y
        x += w + PAD
        shelf_h = max(shelf_h, h)
    return atlas_w, y + shelf_h


def main() -> None:
    cells = build_cells()
    aw, ah = pack(cells)
    atlas = Image.new("RGBA", (aw, ah), (0, 0, 0, 0))
    for c in cells:
        atlas.paste(c.img, (c.x, c.y))
    ATLAS_PNG.parent.mkdir(parents=True, exist_ok=True)
    atlas.save(ATLAS_PNG)
    print(f"wrote {ATLAS_PNG} ({aw}x{ah})")

    # Generated Rust UV table (normalized rects, v down). The stem/path
    # constants derive from ATLAS_PNG so they can never diverge from the
    # actual file (the engine registers the texture under the basename).
    stem = ATLAS_PNG.stem
    rel_path = f"./data_mods/smx_hardware/{ATLAS_PNG.name}"
    lines = [
        "//! GENERATED by scripts/gen_smx_overlay_atlas.py -- do not edit.",
        "//! Normalized UV rects (u0, v0, u1, v1) into the overlay atlas.",
        "",
        "/// Atlas texture name stem — MUST equal the PNG basename (the",
        "/// engine's PngFileCallback registers textures under it).",
        f'pub const ATLAS_STEM: &str = "{stem}";',
        "/// Atlas path relative to the game working directory.",
        f'pub const ATLAS_PATH: &str = "{rel_path}";',
        "",
    ]
    for c in cells:
        w, h = c.img.size
        u0, v0 = c.x / aw, c.y / ah
        u1, v1 = (c.x + w) / aw, (c.y + h) / ah
        lines.append(
            f"pub const {c.name.upper()}: [f32; 4] = "
            f"[{u0:.6}, {v0:.6}, {u1:.6}, {v1:.6}];"
        )
    lines.append("")
    lines.append("/// Pinpad key cells indexed by the 10-key buffer index")
    lines.append("/// (0..=9 digits, 10 = \"00\", 11 = the blank decimal key).")
    lines.append("pub const KEY_CELLS: [[f32; 4]; 12] = [")
    for k in ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "00", "blank"]:
        lines.append(f"    KEY_{k.upper()},")
    lines.append("];")
    lines.append("")
    ATLAS_RS.write_text("\n".join(lines))
    print(f"wrote {ATLAS_RS}")

    # Preview sheet for eyeballing (not committed): buttons at display
    # size over a mid-grey background.
    preview = Image.new("RGBA", (560, 260), (96, 100, 108, 255))
    menu_off = cells[0].img.resize((58, 58), Image.LANCZOS)
    menu_lit = cells[1].img.resize((58, 58), Image.LANCZOS)
    menu_glow_img = next(c for c in cells if c.name == "menu_glow").img.resize(
        (116, 116), Image.LANCZOS
    )
    menu_prs = next(c for c in cells if c.name == "menu_pressed").img.resize(
        (58, 58), Image.LANCZOS
    )
    # Diamond (rotated) + unrotated + lit-with-bloom + pressed composite.
    diamond = menu_off.rotate(45, expand=True, resample=Image.BICUBIC)
    preview.alpha_composite(diamond, (20, 20))
    preview.alpha_composite(menu_off, (120, 30))
    # Lit + bloom: glow underneath (centered), then base, then lit face.
    preview.alpha_composite(menu_glow_img, (190 - 29, 30 - 29))
    preview.alpha_composite(menu_off, (190, 30))
    preview.alpha_composite(menu_lit, (190, 30))
    both = Image.alpha_composite(menu_off, menu_prs)
    preview.alpha_composite(both, (290, 30))
    for i, name in enumerate(["key_7", "key_8", "key_9", "key_00", "key_blank"]):
        cell = next(c for c in cells if c.name == name)
        key = cell.img.resize((36, 36), Image.LANCZOS)
        preview.alpha_composite(key, (20 + i * 44, 120))
    kp = next(c for c in cells if c.name == "key_pressed").img.resize((36, 36), Image.LANCZOS)
    k7 = next(c for c in cells if c.name == "key_7").img.resize((36, 36), Image.LANCZOS)
    preview.alpha_composite(Image.alpha_composite(k7, kp), (240, 120))
    for i, name in enumerate(["util_card", "util_hide", "util_show"]):
        cell = next(c for c in cells if c.name == name)
        util = cell.img.resize((120, 30), Image.LANCZOS)
        preview.alpha_composite(util, (20 + i * 132, 190))
    out = Path("/tmp/smx_overlay_preview.png")
    preview.save(out)
    print(f"wrote {out} (preview, not committed)")


if __name__ == "__main__":
    main()
