# AFP Texture Pipeline Research

Research notes on how Konami's AFP animation system loads and renders textures, including the relationship between AFP templates, GE2D geo files, texture atlases, and UV coordinate mapping.

## AFP Template Loading

AFP templates are loaded from IFS containers at mount time. The game iterates `afplist.xml` and loads each AFP + BSI + geo file set.

### Loading sequence (observed via LayeredFS verbose logs)
1. Game opens `afplist.xml` from the IFS (kbin binary format)
2. For each `<afp>` entry, loads the AFP data file by MD5 of the AFP name
3. Loads the corresponding BSI file from `afp/bsi/` by the same MD5
4. For each geo shape index listed in the `<geo>` element, loads the geo file by MD5 of `{afp_name}_shape{index}`
5. All files are loaded through AVS filesystem hooks (`avs_fs_open`)

### AFP data format
- Stored scrambled in the IFS (BSI byte-swapping + string table cipher)
- BSI descrambling: 2-byte words encode offset/swap-type/loop-count, reverses groups of 2/4/8 bytes (self-inverse operation)
- String table cipher: rolling subtraction starting at 128, incrementing by 1 per byte (NOT simple XOR 0x80)
- After descrambling, the AP2 header contains the exported name at `string_table_offset + name_offset`
- The exported name is used to construct geo file names: `{exported_name}_shape{id}`

### Key addresses (libafp-win64.dll)
| Symbol | Address | Notes |
|--------|---------|-------|
| `afp_stream_do_create` | `0x180006f10` | Handles SWF/PFA/AP2 magic only — NOT GE2D |
| `%s_shape%d` format string | `0x1802085e8` | Constructs geo file names from exported name |
| Shape name lookup (`get_shape_id`) | `0x180024c10` | Calls gamemdx callback at `[0x180245180]` |
| `afp_draw_shape` | `0x180119950` | Shape rendering function |

## GE2D Geo File Format

GE2D files define shape geometry for AFP templates. Each shape contains vertex points, texture UV points, labels (texture region names), and draw parameters.

### Header (offsets from bemaniutils `geo.py`)
- Bytes 0-3: Magic (`D2EG` little-endian or `GE2D` big-endian)
- Bytes 20-31: Counts (vertex, tex, color, label, render_params) as u16
- Bytes 32-51: Offsets (vertex, tex, color, label, render_params) as u32

### Texture UV Coordinates

**Critical finding**: GE2D tex_points are **absolute UV coordinates in the atlas's coordinate space**, NOT normalized percentages.

The render params `flags` field controls UV interpretation:
- `flags & 0x40` (normalize flag): If set, tex_points are raw values that get normalized by the region metrics at render time
- `flags & 0x40` NOT set: tex_points are absolute UV coordinates where 0.0 = atlas origin, 1.0 = atlas edge

For the folder card shapes, `flags = 0x3` (drawable + has texture, NO normalize flag). This means:
- Tex_point `(0.234863, 0.733398)` maps to pixel `(0.234863 × 2048, 0.733398 × 1024)` = `(481, 751)` in the 2048×1024 atlas
- These coordinates are baked into the geo file and reference specific positions in the original texture atlas

**Implication for modding**: When cloning an AFP template with different textures, the new textures MUST be placed at the exact same pixel positions in an atlas of the same dimensions. The geo files' UV coordinates cannot be changed without modifying the binary GE2D data.

### Labels
- Stored as null-terminated strings, optionally obfuscated with XOR 0x80
- Obfuscation detection: if `(first_byte - 0x20) > 0x7F`, the string is obfuscated
- Labels reference texture region names in the texturelist (e.g., `mufo_folder_back_firststep_on`)
- The 6 folder-specific shape IDs (41, 44, 47, 50, 53, 56) contain texture name labels
- The 12 shared shape IDs (5, 8, 9, 12, 15, 18, 21, 29, 30, 59, 62, 63) have no folder-specific labels

## Texture Atlas System

### texturelist.xml structure
```xml
<texturelist compress="avslz">
  <texture format="argb8888rev" name="tex000" ...>
    <size __type="2u16">2048 1024</size>
    <image name="mufo_folder_back_firststep_on">
      <imgrect __type="4u16">1900 2380 656 984</imgrect>
      <uvrect __type="4u16">1902 2378 658 982</uvrect>
    </image>
    <!-- more images in same atlas -->
  </texture>
</texturelist>
```

### Key findings
- **Data storage**: Each image has its own data file in the IFS, stored by **plain filename** (e.g., `mufo_folder_back_firststep_on.png`), NOT by MD5 hash. This is different from AFP/geo files which use MD5 hashing.
- **Atlas grouping**: The `<texture>` block groups images into atlases. The atlas dimensions (`<size>`) control the UV coordinate space. Multiple images share the same atlas coordinate space.
- **imgrect/uvrect**: Values are doubled (the game divides by 2 when loading). `imgrect` defines the image boundary, `uvrect` is inset by 1 pixel for filtering.
- **Pixel position**: `imgrect 1900 2380 656 984` means the image occupies pixels (950, 328)-(1190, 492) in the atlas.

### Atlas cloning for custom textures
When creating custom textures that replace existing ones in an AFP template:
1. Read the source texturelist to find which atlases contain the source textures
2. Create new atlases of the **same dimensions** as the originals
3. Composite custom PNGs at the **same pixel positions** as the source textures
4. Convert to ARGB8888REV + AVSLZ
5. Register in texturelist with the same imgrect/uvrect values but new image/atlas names

This approach is data-driven — atlas dimensions and positions are read from the game's texturelist at runtime, not hardcoded.

## IFS Container Format

### Header
- Bytes 0-3: Signature `0x6CAD8F89` (big-endian)
- Bytes 4-5: Version (u16 BE)
- Bytes 6-7: ~Version (u16 BE, bitwise NOT of version)
- Bytes 8-11: Timestamp (u32 BE)
- Bytes 12-15: Tree size (u32 BE) — kbin memory size, NOT the data offset
- Bytes 16-19: Manifest end (u32 BE) — **this is the data blob start offset**
- Bytes 20-35: MD5 hash (16 bytes, if version > 1)

**Important**: The data blob offset is at byte 16, not byte 12. Byte 12 is the kbin tree memory size (used internally by the kbin decoder).

### Manifest
- kbin binary XML from header end (byte 36 for version > 1, byte 20 otherwise) to manifest_end
- Describes the file tree: sections (tex, geo, afp) containing file entries
- File entries: `<md5_hash __type="3s32">offset size timestamp</md5_hash>`
- Offsets are relative to manifest_end (data blob start)

### kbin tag name escaping
- Tags starting with a digit get `_` prefix: `8f5a...` → `_8f5a...`
- `_` in names becomes `__`, `.` becomes `_E`
- Must unescape when matching against computed MD5 hashes

## ARC Container Format

Simple container wrapping one or more files (typically IFS archives).

- Bytes 0-3: Magic `0x19751120` (little-endian)
- Bytes 4-7: Version (u32 LE)
- Bytes 8-11: File count (u32 LE)
- Bytes 12-15: Compression type (u32 LE)
- Cue table: file_count × 16-byte entries (path_offset, data_offset, decompressed_size, compressed_size)
- File data: optionally compressed with Konami LZ77 (same algorithm as AVSLZ)
