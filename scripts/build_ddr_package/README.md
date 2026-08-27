# build_ddr_package

IFS texture pack builder for DDR World Universal Modpack.

Converts a directory of PNG images into a valid IFS file (and optional ARC wrapper) that can be loaded by the game's BM2D texture system.

> **Optional / legacy.** Custom textures no longer require ARC packs. The supported
> path is **LayeredFS**: drop PNGs into `data_mods/` (mirroring a stock IFS's
> `_ifs/tex/` folder) and they're converted and injected at runtime — no packing,
> no touching the game's `data/` tree. See the README's "Custom Textures" and
> "LayeredFS" sections. This tool remains only for the niche case of build-time
> pre-baking an IFS/ARC; it is not part of the normal modding workflow.

## Install

```bash
pip install -r requirements.txt
```

## Usage

```bash
# Build IFS only
python -m build_ddr_package path/to/pngs -o output.ifs

# Build IFS + ARC wrapper (needed for game loading)
python -m build_ddr_package path/to/pngs -o output.ifs --arc
```

## Input

A directory containing PNG images. Subdirectories are flattened — all PNGs are collected recursively. Each PNG becomes a named texture accessible in-game via `resolveTexture("filename_without_extension")`.

## Output

- `.ifs` — IFS texture container with DXT5-compressed textures, KBin XML metadata
- `.arc` — ARC wrapper (if `--arc` flag used) that the game's archive manager can load

## How It Works

1. Collects all PNGs from the input directory
2. Converts each to DXT5 format (via Pillow's DDS encoder)
3. Applies word-swap for the game's big-endian format
4. Compresses with Konami LZ77
5. Generates `texturelist.xml` (as KBin), `version.xml`, `magic`
6. Packs into IFS format with proper manifest
7. Optionally wraps in ARC format

## Dependencies

- `Pillow` — PNG loading and DXT5 encoding
- `kbinxml` — Konami binary XML encoding
- `lxml` — XML processing (required by kbinxml)
- `tqdm` — Progress bars (optional, used by LZ77 compressor)

No dependency on `ifstools` — the LZ77 compressor is inlined (MIT licensed from ifstools).
