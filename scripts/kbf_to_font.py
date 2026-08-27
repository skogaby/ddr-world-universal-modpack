#!/usr/bin/env python3
"""
kbf_to_font.py — Convert Konami Bitmap Font (KBF + DDS atlas) to TrueType font.

Produces a .ttf with:
  - Empty TrueType glyph outlines (zero-contour placeholders with correct
    advance widths, so text layout works in non-sbix-aware applications)
  - sbix bitmap strike embedding the native PNG glyphs at their native PPEM

Usage:
    python3 scripts/kbf_to_font.py assets/fonts/2d_font_rival
    python3 scripts/kbf_to_font.py assets/fonts/          # all fonts in dir
    python3 scripts/kbf_to_font.py assets/fonts/ -o output/

Requires: Pillow, fonttools (pip install Pillow fonttools)
"""

import argparse
import io
import os
import struct
import sys
from dataclasses import dataclass
from pathlib import Path

import numpy as np
from PIL import Image

from fontTools import ttLib
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib.tables.sbixStrike import Strike as SbixStrike
from fontTools.ttLib.tables.sbixGlyph import Glyph as SbixGlyph


# ── KBF parsing ──────────────────────────────────────────────────────

@dataclass
class KbfHeader:
    # Header fields at fixed offsets in the kbf file.
    #   0x0C line_height      (i8)
    #   0x0D monospace_width  (i8)
    #   0x0E chara_x_diff     (i8)
    #   0x0F chara_y_diff     (i8)
    #   0x10 upper_size       (i8) dots in top half above baseline
    #   0x11 middle_size      (i8) dots in bottom half above baseline
    #   0x12 lower_size       (i8) dots below baseline
    #   0x14 glyph_count      (u32)
    #   0x18 code_tbl_offset  (u32)
    #   0x1C texref_offset    (u32)
    line_height: int
    monospace_width: int
    chara_x_diff: int
    chara_y_diff: int
    upper_size: int
    middle_size: int
    lower_size: int
    glyph_count: int
    code_tbl_offset: int
    texref_offset: int

    @property
    def baseline_px(self) -> int:
        # Distance from the top of the cell to the baseline.
        return self.upper_size + self.middle_size


@dataclass
class Glyph:
    # CharaInfo record (16 bytes) at offset 0x20 + index * 0x10.
    #   0x00 code       (u32)
    #   0x04 reserved   (2 bytes)
    #   0x06 width      (i8)
    #   0x07 height     (i8)
    #   0x08 x_diff     (i8) horizontal offset from pen to glyph left edge
    #   0x09 y_diff     (i8) vertical offset from cell top to glyph top
    #   0x0A x_advance  (i8)
    #   0x0B has_sp     (u8) optional per-corner adjustment flag (ignored)
    #   0x0C sp_no      (u32) index into SpInfo table (ignored)
    charcode: int
    index: int
    width: int
    height: int
    x_diff: int
    y_diff: int
    advance: int


def parse_kbf(path: str) -> tuple[KbfHeader, list[Glyph], int]:
    data = Path(path).read_bytes()
    if len(data) < 32 or data[:4] != b'kbf\x00':
        raise ValueError(f"Not a valid KBF file: {path}")

    def s8(off: int) -> int:
        return struct.unpack_from('<b', data, off)[0]

    def u32(off: int) -> int:
        return struct.unpack_from('<I', data, off)[0]

    header = KbfHeader(
        line_height=s8(0x0C),
        monospace_width=s8(0x0D),
        chara_x_diff=s8(0x0E),
        chara_y_diff=s8(0x0F),
        upper_size=s8(0x10),
        middle_size=s8(0x11),
        lower_size=s8(0x12),
        glyph_count=u32(0x14),
        code_tbl_offset=u32(0x18),
        texref_offset=u32(0x1C),
    )

    glyphs = []
    for i in range(header.glyph_count):
        off = 0x20 + i * 0x10
        if off + 0x10 > len(data):
            break
        glyphs.append(Glyph(
            charcode=u32(off),
            index=i,
            width=s8(off + 6),
            height=s8(off + 7),
            x_diff=s8(off + 8),
            y_diff=s8(off + 9),
            advance=s8(off + 10),
        ))

    page_count = 1
    tro = header.texref_offset
    if 0 < tro and tro + 4 <= len(data):
        page_count = u32(tro)

    return header, glyphs, page_count


def load_atlas_pages(base_path: str, page_count: int) -> list[Image.Image]:
    pages = []
    for i in range(page_count):
        dds_path = f"{base_path}.{i}.dds"
        if not os.path.exists(dds_path):
            break
        img = Image.open(dds_path)
        if img.mode == 'RGBA':
            img = img.split()[0]
        elif img.mode != 'L':
            img = img.convert('L')
        pages.append(img)
    return pages


# ── Glyph bitmap extraction ─────────────────────────────────────────

def extract_glyph_bitmap(pages, glyph, header):
    if glyph.width == 0 or glyph.height == 0:
        return None

    # Cells are laid out on a fixed grid across the atlas. The grid pitch is
    # monospace_width × chara_y_diff. Each cell has a 1-pixel top/left margin
    # so glyphs do not bleed into neighbours under bilinear sampling.
    cell_w = header.monospace_width
    cell_h = header.chara_y_diff
    if cell_w <= 0 or cell_h <= 0:
        return None

    cols = pages[0].width // cell_w
    rows = pages[0].height // cell_h
    cells_per_page = cols * rows

    page_idx = glyph.index // cells_per_page
    if page_idx >= len(pages):
        return None

    local = glyph.index % cells_per_page
    col = local % cols
    row = local // cols
    ax = col * cell_w + 1
    ay = row * cell_h + 1
    return pages[page_idx].crop((ax, ay, ax + glyph.width, ay + glyph.height))


# ── Font building ────────────────────────────────────────────────────

def build_font(font_name, header, glyphs, pages, output_path):
    UPM = 1000
    # The baseline sits upper_size + middle_size pixels below the top of the
    # cell. Total cell height is line_height = baseline_px + lower_size.
    baseline_px = header.baseline_px
    cell_h = header.line_height if header.line_height > 0 else (baseline_px + header.lower_size)
    if cell_h <= 0:
        raise ValueError("Invalid line_height in KBF header")
    scale = UPM / cell_h

    ascent = round(baseline_px * scale)
    descent = round(header.lower_size * scale)

    # Build glyph name mapping
    glyph_names = ['.notdef']
    char_map = {}
    glyph_data = {}

    for g in glyphs:
        if g.charcode == 0:
            continue
        name = f"uni{g.charcode:04X}"
        if name in glyph_data:
            continue
        glyph_names.append(name)
        char_map[g.charcode] = name
        glyph_data[name] = (g, extract_glyph_bitmap(pages, g, header))

    print(f"  Building font: {len(glyph_names)} glyphs, UPM={UPM}, "
          f"ascent={ascent}, descent={descent}")

    fb = FontBuilder(UPM, isTTF=True)
    fb.setupGlyphOrder(glyph_names)
    fb.setupCharacterMap(char_map)

    # Empty TrueType outlines for every glyph. This keeps advance widths
    # correct for text-layout purposes while relying on the sbix strike for
    # actual rendering.
    glyph_outlines = {name: TTGlyphPen(None).glyph() for name in glyph_names}
    fb.setupGlyf(glyph_outlines)

    # Horizontal metrics. A small padding is added to the advance width to
    # preserve inter-glyph spacing that the 1-pixel atlas margin provides
    # in the original bitmap renderer.
    pad = round(2 * scale)
    notdef_adv = round(header.monospace_width * scale)
    hmtx = {'.notdef': (notdef_adv, 0)}
    for name, (g, _) in glyph_data.items():
        hmtx[name] = (round(g.advance * scale) + pad, 0)
    fb.setupHorizontalMetrics(hmtx)

    fb.setupHorizontalHeader(ascent=ascent, descent=-descent)

    # Name table entries required for recognition by macOS Font Book and
    # most other font browsers.
    ps_name = font_name.replace(' ', '-')
    fb.setupNameTable({"familyName": font_name, "styleName": "Regular"})
    nt = fb.font['name']
    for pid, eid, lid in [(1, 0, 0), (3, 1, 0x409)]:
        nt.setName("Converted from Konami KBF", 0, pid, eid, lid)
        nt.setName(f"{ps_name};1.0", 3, pid, eid, lid)
        nt.setName(f"{font_name} Regular", 4, pid, eid, lid)
        nt.setName("Version 1.0", 5, pid, eid, lid)
        nt.setName(ps_name, 6, pid, eid, lid)

    fb.setupOS2(
        sTypoAscender=ascent, sTypoDescender=-descent, sTypoLineGap=0,
        usWinAscent=ascent, usWinDescent=descent,
        sxHeight=round(ascent * 0.5), sCapHeight=round(ascent * 0.85),
    )
    fb.setupPost()

    # ── sbix bitmap strike ───────────────────────────────────────
    print("  Embedding bitmap strike (sbix)...")
    ppem = cell_h
    font = fb.font

    sbix_table = ttLib.newTable('sbix')
    sbix_table.version = 1
    sbix_table.flags = 1

    strike = SbixStrike()
    strike.ppem = ppem
    strike.resolution = 72
    strike.glyphs = {}

    embedded = 0
    for name in glyph_names:
        gobj = SbixGlyph()
        gobj.glyphName = name
        gobj.graphicType = 'png '

        if name == '.notdef' or name not in glyph_data:
            gobj.originOffsetX = 0
            gobj.originOffsetY = 0
            gobj.imageData = b''
            strike.glyphs[name] = gobj
            continue

        g, bmp = glyph_data[name]
        if bmp is None:
            gobj.originOffsetX = 0
            gobj.originOffsetY = 0
            gobj.imageData = b''
            strike.glyphs[name] = gobj
            continue

        if bmp.mode == 'L':
            arr = np.array(bmp)
            rgba_arr = np.zeros((*arr.shape, 4), dtype=np.uint8)
            rgba_arr[:, :, :3] = 255
            rgba_arr[:, :, 3] = arr
            rgba = Image.fromarray(rgba_arr, 'RGBA')
        else:
            rgba = bmp.convert('RGBA')

        buf = io.BytesIO()
        rgba.save(buf, format='PNG')

        # sbix originOffsetY is the signed distance from the glyph origin
        # (on the baseline) to the bottom-left corner of the PNG. For a
        # glyph whose bottom edge sits `n` pixels below the baseline (as
        # with descenders) this value is -n; glyphs resting on the
        # baseline have originOffsetY == 0.
        gobj.originOffsetX = g.x_diff
        gobj.originOffsetY = baseline_px - g.y_diff - g.height
        gobj.imageData = buf.getvalue()
        strike.glyphs[name] = gobj
        embedded += 1

    sbix_table.strikes = {ppem: strike}
    font['sbix'] = sbix_table
    print(f"  Embedded {embedded} glyph bitmaps at {ppem}px strike")

    fb.font.save(output_path)
    size_kb = os.path.getsize(output_path) / 1024
    print(f"  Saved: {output_path} ({size_kb:.0f} KB)")


# ── Main ─────────────────────────────────────────────────────────────

def process_font(base_path, output_dir):
    kbf_path = base_path + '.kbf'
    if not os.path.exists(kbf_path):
        print(f"Skipping {base_path}: no .kbf file")
        return

    name = os.path.basename(base_path)
    print(f"\nProcessing: {name}")

    header, glyphs, page_count = parse_kbf(kbf_path)
    print(f"  KBF: {header.glyph_count} glyphs, cell "
          f"{header.monospace_width}x{header.chara_y_diff}, "
          f"line={header.line_height}, baseline={header.baseline_px}, "
          f"{page_count} pages")

    pages = load_atlas_pages(base_path, page_count)
    if not pages:
        print(f"  ERROR: No atlas pages loaded")
        return
    print(f"  Loaded {len(pages)} atlas page(s): "
          f"{pages[0].width}x{pages[0].height} {pages[0].mode}")

    pretty = name.replace('2d_font_', 'DDR ').replace('_', ' ').title()
    os.makedirs(output_dir, exist_ok=True)
    build_font(pretty, header, glyphs, pages, os.path.join(output_dir, f"{name}.ttf"))


def main():
    parser = argparse.ArgumentParser(
        description="Convert KBF+DDS to TrueType with sbix bitmap strike (.ttf)"
    )
    parser.add_argument('input', help="Font base path (no ext) or directory")
    parser.add_argument('-o', '--output', default=None, help="Output directory")
    args = parser.parse_args()

    input_path = args.input.rstrip('/')
    if os.path.isdir(input_path):
        kbf_files = sorted(set(
            f.rsplit('.', 1)[0] for f in os.listdir(input_path) if f.endswith('.kbf')
        ))
        if not kbf_files:
            print(f"No .kbf files found in {input_path}")
            sys.exit(1)
        out_dir = args.output or os.path.join(input_path, 'converted')
        for name in kbf_files:
            process_font(os.path.join(input_path, name), out_dir)
    else:
        base = input_path.removesuffix('.kbf')
        out_dir = args.output or os.path.dirname(base) or '.'
        process_font(base, out_dir)

    print("\nDone!")


if __name__ == '__main__':
    main()
