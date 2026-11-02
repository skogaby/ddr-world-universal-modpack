#!/usr/bin/env python3
"""Build the DDR Universe style-texture arc the Background Dancers LIGHTING STYLE values
STENCIL RAINBOW / THERMO / MERCURY / CANDY / BUBBLE / CRYSTAL / XRAY sample
(docs/ddr_universe_render_styles_research.md §4; shader `shaders/src/mdl_universe.hlsl`, s1).

Reads Universe's own ramp / sphere-map TGAs from the extracted x_data folder and writes one arc,
members `data/chara/bdfxuniverse/<stem>.dds` (A8R8G8B8, 3 mips — the stock texture shape,
`ktmdl_dump.write_dds_a8r8g8b8`). The DLL loads the arc with a song's dancers when the chosen
style needs one, then binds the texture into material slot 1 of the restyled dancer materials.
The stems are lower-case without `_` so the game's registry key (FNV-1 of the stem, lower-case,
`_` dropped) is FNV-1 of the stem itself — `shader_layout::StyleTexture::stem` must match.

Usage:
    gen_universe_style_textures.py [UNIVERSE_DIR] [--out ARC]
    (defaults: ~/Desktop/ddr_universe/extracted, data_mods/background_dancers/arc/bdfxuniverse.arc)
"""
import argparse
import os
import sys
import tempfile
from pathlib import Path

from PIL import Image

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import arc_tool  # noqa: E402
import ktmdl_dump as K  # noqa: E402

ARC_DIR = "bdfxuniverse"
# World stem -> Universe texture (x_data root). Keep in lockstep with shader_layout::StyleTexture.
TEXTURES = {
    "bdfxunirainbow": "rainbow.tga",
    "bdfxunithermo": "sphere_thermograph.tga",
    "bdfxunichrome": "sphmap2.tga",
    "bdfxunicrystal": "crystal.tga",
    "bdfxunixray": "xray1.tga",
}


def dds_bytes(path):
    img = Image.open(path).convert("RGBA")
    w, h = img.size
    raw = img.tobytes()
    rows = [raw[y * w * 4:(y + 1) * w * 4] for y in range(h)]
    return K.write_dds_a8r8g8b8(w, h, rows)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("universe_dir", nargs="?", default="~/Desktop/ddr_universe/extracted")
    ap.add_argument("--out", default=str(HERE.parent / "data_mods" / "background_dancers" / "arc" / (ARC_DIR + ".arc")))
    args = ap.parse_args()
    src = Path(os.path.expanduser(args.universe_dir))
    files = {f.lower(): f for f in os.listdir(src)}
    for stem in TEXTURES:
        key = K.texture_registry_key(stem)
        if stem != stem.lower() or "_" in stem:
            sys.exit("stem %r must be lower-case without '_' (registry key 0x%08x)" % (stem, key))
    with tempfile.TemporaryDirectory() as tmp:
        tex_dir = Path(tmp) / "data" / "chara" / ARC_DIR
        tex_dir.mkdir(parents=True)
        for stem, name in TEXTURES.items():
            if name.lower() not in files:
                sys.exit("%s not found in %s" % (name, src))
            data = dds_bytes(src / files[name.lower()])
            (tex_dir / (stem + ".dds")).write_bytes(data)
            print("%-16s <- %-24s %7d bytes, key 0x%08x" % (stem, name, len(data), K.texture_registry_key(stem)))
        out = Path(args.out)
        out.parent.mkdir(parents=True, exist_ok=True)
        rc = arc_tool.cmd_pack(Path(tmp) / "data", out)
        if rc:
            sys.exit(rc)
    print("wrote", os.path.relpath(out, HERE.parent) if out.is_relative_to(HERE.parent) else out)


if __name__ == "__main__":
    main()
