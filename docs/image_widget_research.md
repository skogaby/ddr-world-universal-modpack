# Image Widget System — Research Documentation

---

## Table of Contents

1. [Sprite Rendering](#1-sprite-rendering)
2. [Texture Name Resolution](#2-texture-name-resolution)
3. [Archive & File System](#3-archive--file-system)
4. [BM2D / AFP System](#4-bm2d--afp-system)
5. [D3D9 Integration](#5-d3d9-integration)
6. [IFS Loading Pipeline](#6-ifs-loading-pipeline)
7. [Custom IFS Loading Pipeline](#7-custom-ifs-loading-pipeline)
8. [Custom Texture Pipeline](#8-custom-texture-pipeline)

---

## 1. Sprite Rendering

### agcs::Sprite Struct Layout

Flat struct at 0x70 bytes — no descriptor/render state indirection like text widgets.

| Offset | Type | Field | Notes |
|--------|------|-------|-------|
| +0x00 | ptr | vtable | → gamemdx.dll+0x367038 |
| +0x08-0x24 | — | base class fields | Zero-filled for standalone sprites |
| +0x28 | int32 | texture ID | ScreenCommandList handle, 0 = no texture |
| +0x2C | int32 | blend mode | 1 = alpha blend (see blend mode table) |
| +0x30 | float | X position | |
| +0x34 | float | Y position | |
| +0x38 | float | width | |
| +0x3C | float | height | |
| +0x40 | float | scale X | Default 1.0 |
| +0x44 | float | scale Y | Default 1.0 |
| +0x48 | float | anchor X | Pivot 0.0-1.0 |
| +0x4C | float | anchor Y | Pivot 0.0-1.0 |
| +0x50 | float | rotation | Radians |
| +0x54 | float | UV left | Normalized 0.0-1.0 |
| +0x58 | float | UV top | Normalized 0.0-1.0 |
| +0x5C | float | UV right | Normalized 0.0-1.0 |
| +0x60 | float | UV bottom | Normalized 0.0-1.0 |
| +0x64 | uint32 | color | **ABGR byte order** |

### Color Byte Order: ABGR (Confirmed)

- `0x80FF0000` → semi-transparent **blue** (not red)
- `0x800000FF` → semi-transparent **red**
- `0xFFFFFFFF` → opaque white
- Memory layout: `[A][B][G][R]` as little-endian uint32

### Alpha Blending Requires a Texture

When texture ID = 0, the sprite render writes ScreenCommandList command `0x100010` which bypasses alpha blending entirely. The blend mode switch only executes when a texture is bound.

**Workaround**: A glyph texture ID can be captured from the text rendering system and assigned to image widgets without their own texture. Tiny UVs (0→0.001) sample a single pixel to avoid font atlas artifacts.

### Sprite Vtable (gamemdx.dll+0x367038)

| Index | Offset | Function |
|-------|--------|----------|
| 0 | +0x201360 | destructor? |
| 5 | **+0x2015F0** | **render method** |

### Sprite Render Method (+0x2015F0)

Two sub-functions called in sequence:
1. `FUN_180201610(sprite)` — blend mode setup (switch on +0x2C) + texture bind (command 0x11 with +0x28)
2. `FUN_180201980(sprite)` — quad vertex computation from position/size/anchor/rotation, writes UV coords and color to ScreenCommandList

### Blend Modes (+0x2C)

| Value | Command ID | Description |
|-------|-----------|-------------|
| 0 | (custom) | No blend |
| 1 | 0x1220625 | Standard alpha blend |
| 2 | 0x1220225 | Additive |
| 3 | 0x1220265 | Additive variant |
| 4 | 0x1220129 | Multiply |
| 5 | 0x1220329 | (unnamed) |
| 6 | 0x1220526 | Screen |
| 7 | 0x1220226 | Overlay |
| 8 | 0x1220522 | (unnamed) |
| 9 | 0x1220622 | (unnamed) |
| 10 | 0x1220827 | (unnamed) |
| 11 | 0x1220728 | (unnamed) |
| 12 | 0x1220227 | (unnamed) |
| 13 | 0x1220228 | (unnamed) |
| 14 | 0x1220722 | (unnamed) |
| 15 | 0x1220822 | (unnamed) |
| 16 | 0x122012a | (unnamed) |

### Allocation

No factory needed — allocate 0x70 bytes, zero-fill, set vtable pointer, write fields. No internal vectors or ref-counted allocators.

---

## 2. Texture Name Resolution

### The get_bitmap_info Callback

**Location**: Function pointer at `libafp-win64.dll + 0x245178`

This is the game's texture name → ID resolver. It is a callback registered during initialization. The actual implementation lives in `libafputils-win64.dll + 0x36280`, called through a trampoline at a dynamically allocated address.

### Calling Convention

```c
// Signature: bool get_bitmap_info(BitmapInfo* out, const char* name)
// The callback pointer is read from libafp-win64.dll + 0x245178
```

### BitmapInfo Struct (16 bytes output)

```c
struct BitmapInfo {
    uint32_t textureId;    // +0x00: ScreenCommandList texture handle
    uint16_t atlasWidth;   // +0x04: texture atlas width in pixels
    uint16_t atlasHeight;  // +0x06: texture atlas height in pixels
    uint16_t pixelLeft;    // +0x08: sprite left edge on atlas
    uint16_t pixelTop;     // +0x0A: sprite top edge on atlas
    uint16_t pixelRight;   // +0x0C: sprite right edge (left + width)
    uint16_t pixelBottom;  // +0x0E: sprite bottom edge (top + height)
};
```

### Usage

```
1. Allocate 16-byte output buffer
2. Call get_bitmap_info(outBuf, textureName)
3. On success: read textureId, atlas dimensions, pixel UV rect
4. Convert pixel UVs to normalized 0-1 by dividing by atlas dimensions
```

### Texture Name Format

- **Bare asset name only** — no path, no extension
- `tex/deat_demo_title.png` inside an IFS → resolve as `"deat_demo_title"`
- The `tex/` prefix and `.png` extension are stripped by the IFS loader
- Names are case-sensitive

### Captured Examples

| Texture Name | Texture ID | Atlas Size | UV Rect (pixels) |
|-------------|-----------|------------|-------------------|
| `deat_demo_title` | 0xc888000 | 512×512 | [2, 586, 842, 898] |
| `deea_network_local_nopp` | 0xa870000 | 2048×1024 | [1370, 2714, 370, 730] |
| `deea_bt_decision_on` | 0xa870000 | 2048×1024 | [2, 1362, 1042, 1142] |
| `deea_bt_decision` | 0xa870000 | 2048×1024 | [2, 1362, 1146, 1246] |

**Key insight**: Multiple sprites share the same texture atlas (same texture ID). The UV rect identifies the specific sprite within the atlas.

### Textures Are Only Available After IFS Loading

Textures are only resolvable after their IFS file has been loaded by the game. IFS files load on demand when the game enters a screen that needs them. Testing confirmed:
- `deat_demo_title` → resolves only after attract mode starts (loads `demo_attract_v0.arc`)
- `deea_bt_decision` → resolves only when the attract screen with that UI is active
- `totally_fake_name` → always fails
- Textures from unloaded screens → fail until that screen is entered

---

## 3. Archive & File System

### Archive Manager Global

**Location**: `gamemdx.dll + 0x6b5cd8` (pointer to archive manager object)

### Archive Manager Structure

| Offset | Type | Description |
|--------|------|-------------|
| +0x08 | ptr | File table — array of 0x40-byte entries |
| +0x28 | ptr | Data table — array of 0xA0-byte entries |
| +0x78 | ptr | Pending load queue begin |
| +0x80 | ptr | Pending load queue end |
| +0x98 | ptr | Processing queue begin |
| +0xA0 | ptr | Processing queue end |
| +0xB8 | ptr | File callback list begin |
| +0xC0 | ptr | File callback list end |
| +0xD8 | uint32 | Free list head index |
| +0x150 | int32 | Lock count |
| +0x154 | int32 | Operation counter |

### File Table Entry (0x40 bytes)

| Offset | Type | Field |
|--------|------|-------|
| +0x00 | uint32 | FNV-1a hash of file path |
| +0x08 | ptr | Data pointer (file content or resource) |
| +0x14 | uint32 | Additional flags |
| +0x20 | int32 | Status (0=empty, 5=loaded, etc.) |
| +0x24 | int32 | Reference count |
| +0x28 | ptr | Resource object pointer |
| +0x30 | ptr | Callback object pointer |
| +0x3C | int32 | Free list next (-1 = end) |

### Data Table Entry (0xA0 bytes)

| Offset | Type | Field |
|--------|------|-------|
| +0x00 | len-prefixed string | Source (e.g., "local") |
| +0x10 | len-prefixed string | File path (e.g., "data/arc/startup.arc") |
| +0x8E | byte | Path offset within entry |
| +0x90 | string | Category/tag (e.g., "default") |

### FNV-1a Hash Function

```python
def fnv1a(path):
    h = 0x811c9dc5
    for c in path:
        h ^= ord(c)
        h = (h * 0x1000193) & 0xFFFFFFFF
    return h
```

### Key Functions

| Function | Offset | Description |
|----------|--------|-------------|
| `FUN_1801e7070` | +0x1E7070 | Open file by path — hashes path, creates entry |
| `FUN_1801e64c0` | +0x1E64C0 | Flush/process pending loads (blocking loop) |
| `FUN_1801e6290` | +0x1E6290 | Process individual file entries from queue |
| `FUN_1801e6e30` | +0x1E6E30 | Dispatch file to registered callback |
| `FUN_1801e76f0` | +0x1E76F0 | Register a file type callback |
| `FUN_1801e7860` | +0x1E7860 | Register extension → callback mapping |

### AVS Filesystem Mount

During boot (`Application::onBoot` at +0x20d0):
```c
XCnbrep700004b("/local/data", "/data", &DAT_1802bea64, 0);
```
This mounts the local `data/` directory to the virtual path `/data/`. Files at `data/arc/bm2d/foo.arc` become accessible as `data/arc/bm2d/foo.arc` in the virtual filesystem.

### Loaded File Types (1556 entries at runtime)

- `data/arc/*.arc` — archive bundles (loaded on demand per screen)
- `data/font/*.dds` — DDS font atlas textures
- `data/font/*.kbf` — font metadata
- `data/bm2d/*.ifs` — BM2D image/animation containers (extracted from arcs)
- `data/data/texture.db` — texture property database (loaded at boot, static)
- `data/shader/*.gsp` — GPU shader programs
- `data/sound/win/*.xgs` — sound banks
- `data/gamedata/*.xml` — game configuration

### Game Data Directory Layout

```
<root>
├── spice64.exe
├── gamemdx.dll
├── data/
│   ├── arc/
│   │   ├── startup.arc
│   │   ├── soundbanks.arc
│   │   ├── shader.arc
│   │   ├── arkdata.arc
│   │   └── bm2d/           ← 300+ arc files, one per screen/feature
│   │       ├── demo_attract_v0.arc
│   │       ├── common_shutter_v3.arc
│   │       ├── dance_judge0000_v0.arc
│   │       └── ...
│   ├── font/
│   ├── bm2d/                ← IFS files extracted from arcs at runtime
│   ├── shader/
│   └── data/
│       └── texture.db
```

---

## 4. BM2D / AFP System

### Module: libafp-win64.dll

The Flash-like animation engine. All functions are exported with real names.

### Key Exports

| Function | Description |
|----------|-------------|
| `afp_mc_load_bitmap(mcId, texName)` | Load texture by name into movie clip |
| `afp_mc_load_bitmap_from_info(...)` | Load texture with explicit info |
| `afp_mc_load_movie(...)` | Load movie clip |
| `afp_mc_set_param(mcId, paramId, ...)` | Set movie clip parameter |
| `afp_mc_get_param(mcId, paramId, out)` | Get movie clip parameter |
| `afp_mc_traversal(mcId, direction)` | Iterate movie clip siblings |
| `afp_mc_refer(mcId, childName)` | Resolve child by name |
| `afp_mc_op(mcId, opCode, label)` | Movie clip operation (play, stop, etc.) |
| `afp_id_is_valid(type, id)` | Check if AFP ID is valid |
| `afp_layer_play(layerId)` | Play animation layer |
| `afp_layer_set_attribute(layerId, attr, val)` | Set layer attribute |
| `afp_system_set_attribute(flags)` | Set system-wide attributes |

### afp_mc_load_bitmap Internals (libafp+0x3ae20)

```
1. Validate mcId (type check: (mcId >> 0x1b & 0xf) == 4)
2. Resolve mcId to internal object via FUN_180108b40
3. Call FUN_180024a50(outBuf, textureName, 0, 0) — texture name resolver
4. FUN_180024a50 internally:
   a. Copies name to local buffer, converts to lowercase
   b. Calls (*DAT_180245178)(outBuf, name) — the get_bitmap_info callback
   c. Returns 0 on success, 0xfffffffc on failure
5. If texture found, applies it to the movie clip object
```

### Module: libafputils-win64.dll

Lower-level texture/package management. 126 exports.

### Key Exports

| Function | Description |
|----------|-------------|
| `afpu_get_afp_bitmap_info` | The actual get_bitmap_info implementation |
| `afpu_get_texture_bind_id(texPtr, ctx)` | Get ScreenCommandList texture ID from texture pointer |
| `afpu_set_texture_info(...)` | Register texture info |
| `afpu_set_texture_detail_info(...)` | Register detailed texture info |
| `afpu_set_image(...)` | Register an image |
| `afpu_set_image_info(...)` | Register image info |
| `afpu_new_package(...)` | Create a new package |
| `afpu_get_image_id(...)` | Get image ID |
| `afpu_get_image_info(...)` | Get image info |
| `afpu_get_image_name(...)` | Get image name |
| `afpu_ngp_mounttable_open(...)` | Open mount table |
| `afpu_ngp_mount_package_read_data(...)` | Read package data |
| `afpu_change_texture_info(...)` | Modify existing texture info |
| `afpu_fontlib_set_get_bitmap_info_func(...)` | Register the bitmap info callback |

### Texture Bind ID System

`afpu_get_texture_bind_id(texturePtr, context)` converts a texture pointer to the 32-bit ScreenCommandList texture ID. The relationship between pointer and bind ID is not a simple formula — it is computed internally by the rendering system.

---

## 5. D3D9 Integration

### Modules

- `d3d9.dll` — Direct3D 9 runtime
- `d3dx9_43.dll` — D3D9 extensions (texture loading, etc.)

### D3D9 Device

**Location**: `gamemdx.dll + 0x6b5dc0` (pointer to D3D9 object)

The object at this address has a vtable in `d3d9.dll` at offset `+0x157108`. However, calling `CreateTexture` or `D3DXCreateTextureFromFileInMemoryEx` through it crashes with access violation at `0x40000005d`. Possible causes:
- The object might be a D3D9 swap chain or surface, not the device itself
- Or it is a proxy/wrapper that requires specific calling context
- Or thread affinity issues (D3D9 is single-threaded)

`D3DXCreateTextureFromFileInMemoryEx` is never called at runtime — the game creates textures through a different path (likely through the BM2D/AFP system's internal D3D9 integration).

### Texture ID Format

The 32-bit texture IDs used in ScreenCommandList command 0x11 are:
- NOT D3D9 pointers (too small for 64-bit, and don't match pointer values)
- NOT FNV-1a hashes of texture names
- NOT CRC32 hashes
- Appear to be sequentially allocated from a pool (values in 0x300000-0xC000000 range)
- Computed by `afpu_get_texture_bind_id` from the texture pointer

---

## 6. IFS Loading Pipeline

### Boot Sequence (Application::onBoot at +0x20d0)

```
1. Create screen graph system (DAT_1806b5d20)
2. Create resource manager (DAT_1806b5cf0)
3. Create archive manager (DAT_1806b5cd8)
4. Mount AVS filesystem: "/local/data" → "/data"
5. Register file type callbacks:
   [0] Generic file callback (vt=+0x3663b8)
   [1] agcs::ModelFileCallback (vt=+0x2becc8) — .mdl
   [2] agcs::DdsFileCallback (vt=+0x2bed00) — .dds
   [3] AnimeFileCallback (vt=+0x2beda8) — animation
   [4] PngFileCallback (vt=+0x2bede0) — .png
   [5] agcs::ShaderFileCallback (vt=+0x2bed38) — .gsp
   [6] agcs::Bm2dFileCallback (vt=+0x2bed70) — .ifs
   [7] Unknown (vt=+0x35dad0)
   [8] Unknown (vt=+0x35db08)
6. Load startup.arc
7. Load texture.db
8. Create render context (DAT_1806b5d40)
9. Initialize font system, sound system, etc.
```

### Bm2dFileCallback (IFS Handler)

**Vtable**: gamemdx.dll+0x2bed70

| Index | Offset | Function |
|-------|--------|----------|
| 0 | +0x2B10 | destructor |
| 1 | +0x1F02A0 | unknown |
| 2 | +0x1F02D0 | **getExtension()** → returns "ifs" |
| 3 | +0x1FA100 | returns 0 (stub) |
| 4 | +0x1F02E0 | **createTask(fileIndex)** — creates Bm2dFileTask |
| 5 | +0x1FC330 | unknown |

### IFS Loading Flow

```
1. Archive manager opens .arc file via FUN_1801e7070
2. Arc is extracted — IFS files become entries in the file table
3. FUN_1801e64c0 (flush) processes pending entries
4. FUN_1801e6290 dispatches each file to its registered callback
5. Bm2dFileCallback::createTask (vtable[4], +0x1F02E0):
   a. Allocates Bm2dFileTask (0x18 bytes)
   b. Calls FUN_1801f0020 which:
      - Reads file path from data table
      - Creates AsyncRegisterJob (0x68 bytes) with file data pointer and path
      - Submits job to screen graph system (DAT_1806b5d20) for async processing
6. The async job processes the IFS file:
   - Parses IFS container format
   - Creates D3D9 texture atlases
   - Registers sprite names in the BM2D texture registry
7. Sprites become resolvable via get_bitmap_info
```

### texture.db

**Path**: `data/data/texture.db` (loaded from startup.arc)
**Size**: 19264 bytes
**Format**: 12-byte header + 1604 entries × 12 bytes each

Header:
| Offset | Value | Description |
|--------|-------|-------------|
| +0x00 | 0x19751120 | Magic/hash |
| +0x04 | 0x00000002 | Version? |
| +0x08 | 0x00000644 | Entry count (1604) |

Entry format (12 bytes):
| Offset | Type | Description |
|--------|------|-------------|
| +0x00 | uint32 | Flags (0x315=common, 0x115=variant, 0x55=special) |
| +0x04 | uint32 | Hash (sorted ascending — used for binary search) |
| +0x08 | uint32 | Secondary hash (0 for many entries) |

**Purpose**: Static boot-time database for texture property validation/pre-allocation. NOT involved in texture name resolution. The hash function used is unknown (not FNV-1a, not CRC32). Likely not needed for custom textures.

---

## 7. Custom IFS Loading Pipeline

### Validated Approach: Piggyback Arc Loading

Custom IFS files can be loaded into the game's BM2D texture registry by hooking the archive manager's file open function (`FUN_1801e7070`). When the game opens any `.arc` file during a screen transition, the hook also opens the custom arc. The game's own flush processes both arcs together through the normal pipeline.

### Internal Loading Flow (FUN_1802587a0)

```
1. entry+0x08  = ifsName                              (e.g., "custom_mod.ifs")
2. entry+0x108 = "/dev/ram/link/" + ifsName            (AVS source path)
3. entry+0x208 = "/afp" + slotIndex + entry+0x108      (AVS mount point)
4. XCnbrep700004b(entry+0x208, entry+0x108, "imagefs", 0)  — mount IFS
5. afpu_ngp_read_data(entry+0x08, entry+0x208, 0)          — parse package
6. afpu_do_create_stream_all(pkgId, 1)                      — create D3D9 textures
7. XCnbrep700004c(entry+0x208)                              — unmount
```

**Package table**: `gamemdx.dll + 0xbbd330` — array of 256 entries × 0x318 bytes each.

### AVS Filesystem Types

| Type | Purpose | Example |
|------|---------|---------|
| `"link"` | Symlink/redirect to another AVS path | Boot: `/local/data` → `/data` |
| `"imagefs"` | Mount IFS container as filesystem | IFS loading |
| `"fs"` | (failed in testing) | — |
| `"arc"` | (failed in testing) | — |

### AVS Error Codes

| Code | Hex | Meaning |
|------|-----|---------|
| -2147024884 | 0x8007000C | Path not found / invalid source |
| -2147024877 | 0x80070013 | Invalid filesystem type or format |
| Positive | — | Success (mount handle) |

### Critical Finding: imagefs Only Reads from RAM Backend

The `imagefs` filesystem driver can only read IFS data from the AVS RAM filesystem (`/dev/ram/`). It cannot follow `link` mounts to read from disk. The game's pipeline works because:

1. Arc extractor writes raw IFS bytes to `/dev/ram/<name>.ifs/image.bin` (RAM)
2. A `link` is created: `/dev/ram/link/<name>.ifs` → `/dev/ram/<name>.ifs/image.bin`
3. `imagefs` mounts from `/dev/ram/link/<name>.ifs` — this works because the link target is in RAM

Attempts to link to disk-backed paths (e.g., `/data/custom_mod.ifs` via `/local/`) fail because `imagefs` cannot read through the disk-backed link.

### Arc Processing is Async

The arc extraction pipeline works in multiple stages:
1. `FUN_1801e6ff0(arcPath, tag)` — registers the arc file (returns file index)
2. `FUN_1801e64c0()` (flush) — blocking loop that processes pending files
3. Inside flush: `FUN_1801e5d30` + `FUN_1801e6290` process individual files
4. The Bm2dFileCallback creates an `AsyncRegisterJob` submitted to the screen graph system
5. The screen graph system processes the job asynchronously

Calling `flush()` from a hook crashes due to thread safety or reentrancy issues. The individual processing functions do not fully process the arc because the async job system needs the screen graph to tick.

### Working Solution: Piggyback on Game's Arc Loading

Hooking `FUN_1801e7070` (archive manager file open) and injecting the custom arc load when the game opens any `.arc` during a screen transition works reliably. The game's own flush processes both arcs together.

**Trigger**: Screen transitions (e.g., attract → mode select) cause the game to load new arcs. The hook piggybacks on the first arc load.

### Key Addresses

- `FUN_1801e7070` at `gamemdx+0x1e7070` — archive manager file open (hook target)
- `FUN_1801e6ff0` at `gamemdx+0x1e6ff0` — arc loading function
- `get_bitmap_info` callback at `libafp+0x245178` — texture name resolution

### Texture Availability Timing

- Textures are NOT available immediately at startup
- They become available after the first screen transition that loads arcs
- In normal gameplay flow: attract → P1 Start → mode select triggers arc loading
- After loading, textures remain available for the rest of the session

---

## 8. Custom Texture Pipeline

> **Historical / superseded.** This section documents the original ARC + `arc_load`
> texture pipeline, which has been removed from the hook DLL (the `asset_loader`
> service and `arc_load` signature no longer exist). The validated RE facts below
> remain accurate about `gamemdx.dll` itself, but the supported way to load custom
> textures now is **LayeredFS injection** into an IFS the game already opens — drop
> PNGs into `data_mods/<mod>/<ifs>_ifs/tex/`. See the project README's "Custom
> Textures" and "LayeredFS" sections. Kept here for binary-level reference.

### End-to-End Validated Pipeline

Custom PNG textures can be loaded into the game's BM2D texture registry via the following pipeline:

1. **Create IFS**: Pack textures into an IFS file (must use DXT5 format — `argb8888rev` crashes the game)
2. **Create ARC**: Wrap the IFS in a minimal ARC file (header + cue entry + path + IFS data)
3. **Place ARC**: Copy to `data/arc/bm2d/custom_mod.arc` in the game directory
4. **Hook**: Intercept `FUN_1801e7070` (archive manager file open) — when the game opens any `.arc`, also open the custom one
5. **Trigger**: Any screen transition that loads arcs (e.g., P1 Start from attract mode)
6. **Resolve**: Call `get_bitmap_info("texture_name")` to get texture ID + UVs
7. **Render**: Use the texture ID in a sprite widget

### Confirmed Test Results

A custom texture with a unique name (`frida_custom_test`) was created, packed into an IFS (DXT5 format), wrapped in an ARC, and placed in `data/arc/bm2d/`. After a screen transition:
- Texture resolved successfully: ID `0xf820000`, atlas 512×512, UV: [2, 788, 2, 132]
- The texture was genuinely new — not a leftover from any game IFS

### IFS Format Requirements

- DXT5 texture encoding is required — `argb8888rev` format crashes the game in `afpu_ngp_read_data`
- IFS files produced by the `ifstools` project render correctly; custom IFS generators may produce files that register names but render as transparent
- DXT5 encoding via Pillow's DDS save requires word-swap for the game's big-endian format
- LZ77 compression via the ifstools compressor
- KBin XML for texturelist.xml and version.xml
- IFS manifest with proper filename escaping (`.` → `_E`, `_` → `__`)
- ARC wrapper with correct header format

### Stream Creation Flags

`afpu_do_create_stream_all` is called with different flags depending on IFS content:
- Custom IFS (missing `afp` folder): `create_stream_all(pkgId, 0x1301)` — texture data may not upload to GPU
- Game IFS (with `afp` folder): `create_stream_all(pkgId, 0x10101)` — textures render correctly

The `afpu_new_package` function may set different flags based on what folders are present in the IFS.

### Hardcoded Offset Removal

All hardcoded offsets were converted to AOB signatures and runtime discovery:

| Address | Purpose | Resolution Method |
|---------|---------|-------------------|
| `+0x1e7070` | `arc_file_open` | Derived from `arc_load` CALL instruction |
| `+0x1e6ff0` | `arc_load` | AOB: `48 89 5c 24 08 48 89 74 24 10 57 48 83 ec 20 48 8b 3d ?? ?? ?? ?? 48 85 d2 48 8d 35 ?? ?? ?? ??` |
| `+0x6b5cd8` | `arc_manager` global | Derived from `arc_load` RIP-relative MOV at +0x0F |
| `+0x367038` | `sprite_vtable` | RTTI scan for `.?AVSprite@agcs@@`, walk back to vtable |
| `libafp+0x245178` | `get_bitmap_info` callback | Export tracing: `afp_mc_load_bitmap` → inner function → indirect call through global pointer |

Notes:
- The `arc_file_open` function is hooked by spice2x at runtime (replaced with a `jmp` trampoline). The address is derived from `arc_load`'s CALL instruction, which points to the original function address regardless of the trampoline.
- All libafputils functions (`afpu_get_texture_bind_id`, `afpu_do_create_stream_all`, etc.) are accessed via named export resolution — already version-agnostic.
