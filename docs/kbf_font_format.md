# KBF Font Format — Research Reference

Documentation of Konami's KBF (Konami Bitmap Font) format as used in DDR World for all in-game text rendering.

---

## Overview

DDR World renders text using bitmap font atlases in the KBF format. Each font consists of:
- One `.kbf` file — glyph metrics + atlas grid layout
- One or more `.N.dds` files — texture atlas pages (N = 0, 1, 2...)

Glyphs are packed in a fixed-size cell grid on the atlas. The KBF file maps character codes to glyph metrics. Atlas position is computed from the glyph's sequential index — there are no explicit UV coordinates in the file.

---

## Available Fonts

| Font | Glyphs | Atlas Size | Cell | Format | Use |
|------|--------|-----------|------|--------|-----|
| `2d_font_system` | 7,572 | 2048×2048 ×2 pages | 34×32 | DXT1 | Main UI font — full CJK + Latin. Used by `kt::BmpfontSimpleString`. |
| `2d_font_ui` | 7,484 | 2048×1136 ×1 | 17×18 | 8-bit luminance | Smaller UI font variant |
| `2d_font_ark_system` | 7,484 | 2048×1136 ×1 | 17×18 | 8-bit luminance | AVS framework system font (identical metrics to ui) |
| `2d_font_rival` | 198 | 512×404 ×1 | 37×25 | 8-bit luminance | Latin-only, large glyphs |
| `2d_font_player` | 219 | 1024×308 ×1 | 39×34 | 8-bit luminance | Player name display |
| `2d_font_songtitle_m` | 7,619 | 2048×2048 + 2048×1152 | 34×32 | 8-bit luminance | Song title (medium) |
| `2d_font_songtitle_s` | 7,625 | 2048×2048 + 2048×820 | 47×16 | 8-bit luminance | Song title (small) |

`2d_font_system` is the primary font — full Unicode coverage including CJK, DXT1 compressed, used for all standard UI text.

---

## KBF File Format

### Header (32 bytes)

```
Offset  Size  Field
+0x00   4     Magic: "kbf\0"
+0x04   4     Version: "00\0\0"
+0x08   4     Flags (0 or 1)
+0x0C   1     Max glyph width (informational)
+0x0D   1     Cell width (pixels) — atlas grid column stride
+0x0E   1     Cell width (duplicate, always == byte 0x0D)
+0x0F   1     Cell height (pixels) — atlas grid row stride
+0x10   1     Unknown metric
+0x11   1     Unknown metric
+0x12   1     Unknown metric
+0x13   1     Padding (0)
+0x14   4     Glyph count (uint32 LE)
+0x18   4     Offset to tail section (uint32 LE)
+0x1C   4     Offset to texture reference section (uint32 LE)
```

### Glyph Table (starts at offset 32, 16 bytes per entry)

```
Offset  Size  Field
+0x00   2     Character code (uint16 LE, Unicode)
+0x02   2     Padding (0)
+0x04   2     Padding (0)
+0x06   1     Glyph width (pixels on atlas)
+0x07   1     Glyph height (pixels on atlas)
+0x08   1     Bearing X (horizontal offset from pen position to glyph left edge)
+0x09   1     Bearing Y (vertical offset from baseline to glyph top edge)
+0x0A   1     Advance width (horizontal pen advance after this glyph)
+0x0B   5     Padding (0)
```

### Texture Reference Section (at offset from header +0x1C)

```
Offset  Size  Field
+0x00   4     Number of texture pages (uint32 LE)
+0x04   4     Offset to first string (relative to section start)
+0x08   4×N   String offsets for pages 1..N-1 (relative to section start)
...     var   Null-terminated ASCII texture filenames
```

---

## Atlas Position Formula

Glyphs are packed in a grid. Position is derived from the entry's sequential index:

```
cell_w = header[0x0D]    // cell width from KBF header
cell_h = header[0x0F]    // cell height from KBF header
cols = atlas_width / cell_w   // columns per atlas page (integer division)
cells_per_page = cols * (atlas_height / cell_h)

// For glyph at entry index N:
page = N / cells_per_page           // which DDS texture page
local_index = N % cells_per_page    // index within that page
col = local_index % cols
row = local_index / cols

// Pixel position on atlas (1px padding on top and left of each cell):
atlas_x = col * cell_w + 1
atlas_y = row * cell_h + 1

// UV coordinates:
u_left  = atlas_x / atlas_width
v_top   = atlas_y / atlas_height
u_right = (atlas_x + glyph_width) / atlas_width
v_bottom = (atlas_y + glyph_height) / atlas_height
```

**Verified** against `2d_font_rival` by cross-referencing KBF glyph entries with actual pixel positions in the DDS atlas. Every glyph's pixel data starts exactly at `(col * cell_w + 1, row * cell_h + 1)` and spans `(glyph_width × glyph_height)` pixels.

---

## Text Layout Using Bearing/Advance

When rendering a string, the bearing and advance values control character placement:

```
pen_x = start_x
baseline_y = start_y + ascent   // ascent ≈ cell_height or max bearing_y

for each character:
    glyph = lookup(charcode)
    draw_x = pen_x + glyph.bearing_x
    draw_y = baseline_y - glyph.bearing_y
    // Draw textured quad at (draw_x, draw_y) with size (glyph.width × glyph.height)
    pen_x += glyph.advance
```

Text alignment (left/center/right) is computed by measuring the total advance width of the string first, then adjusting `start_x` accordingly.
