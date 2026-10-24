#!/usr/bin/env python3
"""Step 1 spike fixture generator (E1 / E3 / E5) — throwaway, NOT shipped.

Turns one StepMania song folder into a LayeredFS overlay mod folder that injects the song
into DDR World by hand, so the visibility contract can be proven on a cabinet before any
DLL code exists:

  <out>/mdb_apx/ssq/<code>.ssq
  <out>/sound/win/dance/<code>.xsb, <code>.xwb            (ddr-chart-tools CLI)
  <out>/arc/jacket/<code>_jk.arc                          (member data/jacket/<code>_jk.png, 512x512)
  <out>/arc/thumbnail/jacket_thumbnails_{ja,ua}_<S>.arc   (member data/jacket/thumbnail/<code>_tn.dds, 192x192 DXT1)
  <out>/arc/startup_arc/data/gamedata/musicdb.xml         (stock/base musicdb + appended <music> entries)

Clones (--clones N) append N extra entries with unique mcodes/basenames sharing the SSQ
(copied under each clone name) so the boot analysis has a chart per entry; they have no
audio and are for the E3 arena-headroom count only.

Usage:
  DDR_WORLD_INSTALL=... DDR_CHART_TOOLS=<path to ddr-chart-tools binary> \
    python3 make_spike.py "<pack>/<song folder>" [--out <mod dir>] [--code zab12c] \
      [--mcode 9000001] [--series 30] [--clones N] [--no-art] [--title T] [--artist A]

Requires Pillow. Prints paths ~-relative.
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

try:
    from PIL import Image, ImageFilter
except ImportError:  # pragma: no cover
    sys.exit("Pillow is required (pip install pillow)")

REPO = Path(__file__).resolve().parents[4]
sys.path.insert(0, str(REPO / "scripts"))
import arc_tool  # noqa: E402  (scripts/arc_tool.py)

SM_DIFF = {"beginner": 0, "easy": 1, "medium": 2, "hard": 3, "challenge": 4}
STYLE = {"dance-single": 0, "dance-double": 1}
RESERVED_MCODES = {0x931C, 0x9525, 0x939D, 0x9306, 0x94E7, 0x9733, 0x950C}


def tilde(p: Path | str) -> str:
    s = str(p)
    home = str(Path.home())
    return "~" + s[len(home):] if s.startswith(home) else s


# ----------------------------------------------------------------------------- MSD


def msd_tags(text: str) -> list[tuple[str, str]]:
    """Minimal MSD: `#TAG:value;` with `//` comments stripped. Values keep inner ':'."""
    text = re.sub(r"//[^\n]*", "", text)
    out = []
    for m in re.finditer(r"#([A-Za-z0-9_]+):([^;]*);", text, re.S):
        out.append((m.group(1).upper(), m.group(2)))
    return out


def parse_sm(path: Path) -> dict:
    raw = path.read_bytes()
    for enc in ("utf-8-sig", "shift_jis", "cp1252"):
        try:
            text = raw.decode(enc)
            break
        except UnicodeDecodeError:
            continue
    tags = msd_tags(text)
    song = {"title": "", "artist": "", "titletranslit": "", "artisttranslit": "",
            "music": "", "banner": "", "background": "", "jacket": "", "bpms": [], "charts": []}
    for k, v in tags:
        v = v.strip()
        if k in ("TITLE", "ARTIST", "TITLETRANSLIT", "ARTISTTRANSLIT", "MUSIC", "BANNER",
                 "BACKGROUND", "JACKET"):
            song[k.lower()] = v
        elif k == "BPMS":
            for seg in v.split(","):
                if "=" in seg:
                    try:
                        song["bpms"].append(float(seg.split("=")[1]))
                    except ValueError:
                        pass
        elif k == "NOTES":
            parts = [p.strip() for p in v.split(":")]
            if len(parts) >= 5:
                song["charts"].append({"stepstype": parts[0].lower(), "difficulty": parts[2].lower(),
                                       "meter": parts[3]})
    return song


# ----------------------------------------------------------------------------- art


def square_image(src: Path, size: int) -> Image.Image:
    im = Image.open(src)
    im.seek(0)
    im = im.convert("RGBA")
    w, h = im.size
    if abs(w - h) <= max(w, h) // 20:
        canvas = im.resize((size, size), Image.LANCZOS)
    elif w > h * 1.6:  # banner: blurred scaled copy behind the banner
        bg = im.resize((size, size), Image.LANCZOS).filter(ImageFilter.GaussianBlur(24))
        scale = size / w
        fg = im.resize((size, max(1, int(h * scale))), Image.LANCZOS)
        canvas = bg
        canvas.alpha_composite(fg, (0, (size - fg.size[1]) // 2))
    else:  # background-ish: centre crop
        side = min(w, h)
        im = im.crop(((w - side) // 2, (h - side) // 2, (w - side) // 2 + side, (h - side) // 2 + side))
        canvas = im.resize((size, size), Image.LANCZOS)
    flat = Image.new("RGB", (size, size), (0, 0, 0))
    flat.paste(canvas, mask=canvas.split()[3])
    return flat


def rgb565(r: int, g: int, b: int) -> int:
    return ((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3)


def bc1_encode(im: Image.Image) -> bytes:
    """Plain min/max-luminance BC1. Quality is irrelevant for the spike."""
    im = im.convert("RGB")
    w, h = im.size
    px = im.load()
    out = bytearray()
    for by in range(0, h, 4):
        for bx in range(0, w, 4):
            block = [px[bx + x, by + y] for y in range(4) for x in range(4)]
            lum = [0.299 * r + 0.587 * g + 0.114 * b for r, g, b in block]
            lo = block[lum.index(min(lum))]
            hi = block[lum.index(max(lum))]
            c0, c1 = rgb565(*hi), rgb565(*lo)
            if c0 < c1:
                c0, c1 = c1, c0
                lo, hi = hi, lo
            if c0 == c1:
                out += struct.pack("<HHI", c0, c1, 0)
                continue
            pal = [hi, lo,
                   tuple((2 * a + b) // 3 for a, b in zip(hi, lo)),
                   tuple((a + 2 * b) // 3 for a, b in zip(hi, lo))]
            idx = 0
            for i, p in enumerate(block):
                best = min(range(4), key=lambda k: sum((p[c] - pal[k][c]) ** 2 for c in range(3)))
                idx |= best << (2 * i)
            out += struct.pack("<HHI", c0, c1, idx)
    return bytes(out)


def dds_dxt1(im: Image.Image) -> bytes:
    """128-byte DDS header mirroring the stock thumbnails (flags 0x81007, linear size)."""
    w, h = im.size
    data = bc1_encode(im)
    hdr = bytearray(b"DDS ")
    hdr += struct.pack("<IIIIIII", 0x7C, 0x00081007, h, w, len(data), 0, 1)
    hdr += b"IMAGEMAGICK".ljust(44, b"\0")
    hdr += struct.pack("<II4sIIIII", 0x20, 0x4, b"DXT1", 0, 0, 0, 0, 0)
    hdr += struct.pack("<IIIII", 0x1000, 0, 0, 0, 0)
    assert len(hdr) == 128
    return bytes(hdr) + data


def pack_arc(members: dict[str, bytes], out: Path, store: bool = False) -> None:
    # arc_tool keeps the packed root's own name as the leading stored component, so
    # members must be laid out under a directory literally named `data`. `store`
    # disables LZ77: every stock PNG jacket member is stored raw (the engine's PNG
    # path does not decompress) while DDS members and thumbnails are compressed.
    with tempfile.TemporaryDirectory() as td:
        for rel, data in members.items():
            assert rel.startswith("data/"), rel
            p = Path(td) / rel
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_bytes(data)
        out.parent.mkdir(parents=True, exist_ok=True)
        saved = arc_tool.COMPRESS_THRESHOLD
        if store:
            arc_tool.COMPRESS_THRESHOLD = 1 << 40
        try:
            rc = arc_tool.cmd_pack(Path(td) / "data", out)
        finally:
            arc_tool.COMPRESS_THRESHOLD = saved
        if rc != 0:
            sys.exit(f"arc pack failed for {tilde(out)}")


# ----------------------------------------------------------------------------- musicdb


def xml_escape(s: str) -> str:
    return (s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")
            .replace('"', "&quot;"))


def clip_utf8(s: str, limit: int = 127) -> str:
    b = s.encode("utf-8")
    if len(b) <= limit:
        return s
    while limit > 0 and (b[limit] & 0xC0) == 0x80:
        limit -= 1
    return b[:limit].decode("utf-8", "ignore")


def yomi(s: str) -> str:
    s = s.lower()
    s = re.sub(r"\(([^)]*)\)", lambda m: "'" + re.sub(r"[^a-z0-9]", "", m.group(1)) + "'", s)
    return re.sub(r"[^a-z0-9']", "", s) or "stepmania"


def entry_xml(mcode: int, code: str, title: str, artist: str, bpmmax: int, series: int,
              levels: list[int]) -> str:
    assert len(code) <= 6 and re.fullmatch(r"[a-z0-9]+", code), code
    assert 0 < mcode < 0x8000_0000 and mcode not in RESERVED_MCODES
    assert len(levels) == 10
    limited = " ".join("0" if lv else "-1" for lv in levels)
    diff = " ".join(str(lv) for lv in levels)
    title = clip_utf8(title)
    artist = clip_utf8(artist)
    return (
        "  <music>\n"
        f'    <mcode __type="u32">{mcode}</mcode>\n'
        f"    <basename>{code}</basename>\n"
        f"    <title>{xml_escape(title)}</title>\n"
        f"    <title_yomi>{clip_utf8(yomi(title))}</title_yomi>\n"
        f"    <artist>{xml_escape(artist)}</artist>\n"
        f'    <bpmmax __type="u16">{bpmmax}</bpmmax>\n'
        f'    <series __type="u8">{series}</series>\n'
        f'    <limited_ary __type="s32" __count="10">{limited}</limited_ary>\n'
        f'    <diffLv __type="u8" __count="10">{diff}</diffLv>\n'
        "  </music>\n"
    )


def base_musicdb(install: Path, data_mods: Path, out_mod: Path) -> tuple[bytes, str]:
    """Prefer an existing operator startup_arc overlay (excluding our own), else stock."""
    for mod in sorted(data_mods.iterdir()):
        if mod.resolve() == out_mod.resolve() or mod.name.startswith("_"):
            continue
        cand = mod / "arc" / "startup_arc" / "data" / "gamedata" / "musicdb.xml"
        if cand.is_file():
            return cand.read_bytes(), f"overlay {tilde(cand)}"
    with tempfile.TemporaryDirectory() as td:
        subprocess.run([sys.executable, str(REPO / "scripts" / "unpack_arc.py"), "-f",
                        "data/gamedata/musicdb.xml", "-o", td,
                        str(install / "data" / "arc" / "startup.arc")],
                       check=True, capture_output=True)
        found = list(Path(td).rglob("musicdb.xml"))
        if not found:
            sys.exit("could not extract stock musicdb.xml from startup.arc")
        return found[0].read_bytes(), "stock startup.arc"


def existing_ids(xml: str) -> tuple[set[int], set[str]]:
    mcodes = {int(m) for m in re.findall(r'<mcode __type="u32">(\d+)</mcode>', xml)}
    codes = set(re.findall(r"<basename>([^<]+)</basename>", xml))
    return mcodes, codes


def base36(n: int, width: int) -> str:
    digits = "0123456789abcdefghijklmnopqrstuvwxyz"
    s = ""
    while n:
        s = digits[n % 36] + s
        n //= 36
    return s.rjust(width, "0")


# ----------------------------------------------------------------------------- main


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("song", type=Path, help="StepMania song folder (contains .sm/.ssc + audio)")
    ap.add_argument("--out", type=Path, help="mod folder (default <install>/data_mods/_spike_sm)")
    ap.add_argument("--code", default="zab12c")
    ap.add_argument("--mcode", type=int, default=9_000_001)
    ap.add_argument("--series", type=int, default=30)
    ap.add_argument("--clones", type=int, default=0)
    ap.add_argument("--no-art", action="store_true")
    ap.add_argument("--title")
    ap.add_argument("--artist")
    ap.add_argument("--fragment-only", type=Path, metavar="FILE",
                    help="write the <music> elements to FILE (for diagnostics.musicdb_fragment) "
                         "instead of a startup_arc overlay")
    ap.add_argument("--tools", default=os.environ.get("DDR_CHART_TOOLS"),
                    help="ddr-chart-tools binary (env DDR_CHART_TOOLS)")
    a = ap.parse_args()

    install = os.environ.get("DDR_WORLD_INSTALL")
    if not install:
        sys.exit("DDR_WORLD_INSTALL is not set")
    install = Path(install)
    data_mods = install / "data_mods"
    out = a.out or (data_mods / "_spike_sm")
    if not a.tools or not Path(a.tools).is_file():
        sys.exit("ddr-chart-tools binary not found: set DDR_CHART_TOOLS or --tools")
    if not re.fullmatch(r"[a-z0-9]{1,6}", a.code):
        sys.exit("--code must be 1-6 lowercase ASCII alphanumerics")

    simfiles = sorted(a.song.glob("*.ssc")) or sorted(a.song.glob("*.sm"))
    if not simfiles:
        sys.exit(f"no .ssc/.sm in {tilde(a.song)}")
    sim = simfiles[0]
    song = parse_sm(sim)
    audio = (a.song / song["music"]) if song["music"] else None
    if not audio or not audio.is_file():
        cands = [p for p in a.song.iterdir() if p.suffix.lower() in (".ogg", ".mp3", ".wav")]
        if not cands:
            sys.exit("no audio file found")
        audio = cands[0]

    # ---- charts -> slots
    levels = [0] * 10
    kept, dropped = [], []
    for ch in song["charts"]:
        st = STYLE.get(ch["stepstype"])
        d = SM_DIFF.get(ch["difficulty"])
        if st is None or d is None:
            dropped.append(f"{ch['stepstype']}/{ch['difficulty']}")
            continue
        slot = d + 5 * st
        if slot == 5:
            dropped.append("double beginner")
            continue
        try:
            meter = int(float(ch["meter"]))
        except ValueError:
            meter = 10
        levels[slot] = max(1, min(19, meter))
        kept.append(f"{ch['stepstype']}/{ch['difficulty']}={levels[slot]}")
    if not any(levels):
        sys.exit("no convertible chart")

    if out.exists():
        shutil.rmtree(out)
    out.mkdir(parents=True)

    # ---- convert via CLI
    with tempfile.TemporaryDirectory() as td:
        cmd = [a.tools, "--from-format", "SM5", "--to-format", "DDR", "--chartfile", str(sim),
               "--audiofile", str(audio), "--song-code", a.code, "--output-dir", td, "--overwrite", "-q"]
        r = subprocess.run(cmd, capture_output=True, text=True)
        if r.returncode != 0:
            sys.exit("ddr-chart-tools failed:\n" + r.stderr.replace(str(Path.home()), "~"))
        got = {p.suffix: p for p in Path(td).iterdir()}
        for ext, rel in ((".ssq", f"mdb_apx/ssq/{a.code}.ssq"), (".xsb", f"sound/win/dance/{a.code}.xsb"),
                         (".xwb", f"sound/win/dance/{a.code}.xwb")):
            if ext not in got:
                sys.exit(f"converter produced no {ext}")
            dst = out / rel
            dst.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(got[ext], dst)
        conv_notes = r.stderr.strip().replace(str(Path.home()), "~")

    # ---- art
    art_src = None
    if not a.no_art:
        for key in ("jacket", "banner", "background"):
            if song[key]:
                p = (a.song / song[key]).resolve()
                if p.is_file():
                    art_src = p
                    break
        if art_src is None:
            for p in sorted(a.song.iterdir()):
                if p.suffix.lower() in (".png", ".jpg", ".jpeg", ".gif", ".bmp"):
                    art_src = p
                    break
        if art_src is None:
            print("note: no art found; writing a flat placeholder")
        jk = square_image(art_src, 512) if art_src else Image.new("RGB", (512, 512), (40, 40, 120))
        with tempfile.TemporaryDirectory() as td:
            png = Path(td) / "jk.png"
            jk.convert("RGBA").save(png, "PNG")  # stock jackets are mostly 512x512 RGBA PNG, stored raw
            pack_arc({f"data/jacket/{a.code}_jk.png": png.read_bytes()},
                     out / "arc" / "jacket" / f"{a.code}_jk.arc", store=True)
        tn = dds_dxt1(jk.resize((192, 192), Image.LANCZOS))
        assert len(tn) == 18560, len(tn)
        for region in ("ja", "ua"):
            pack_arc({f"data/jacket/thumbnail/{a.code}_tn.dds": tn},
                     out / "arc" / "thumbnail" / f"jacket_thumbnails_{region}_{a.series}.arc")

    # ---- musicdb
    base, base_desc = base_musicdb(install, data_mods, out)
    xml = base.decode("utf-8")
    mcodes, codes = existing_ids(xml)
    title = a.title or song["titletranslit"] or song["title"] or sim.stem
    artist = a.artist or song["artisttranslit"] or song["artist"] or "StepMania"
    bpmmax = int(round(max(song["bpms"]))) if song["bpms"] else 120
    entries = []
    new_ids = [(a.mcode, a.code)]
    for i in range(1, a.clones + 1):
        new_ids.append((a.mcode + i, "z" + base36(i, 5)))
    for mc, code in new_ids:
        if mc in mcodes or code in codes:
            sys.exit(f"identity collision with base musicdb: mcode {mc} / basename {code}")
    entries.append(entry_xml(a.mcode, a.code, title, artist, bpmmax, a.series, levels))
    for mc, code in new_ids[1:]:
        entries.append(entry_xml(mc, code, f"CLONE {code}", artist, bpmmax, a.series, levels))
        shutil.copy2(out / "mdb_apx" / "ssq" / f"{a.code}.ssq", out / "mdb_apx" / "ssq" / f"{code}.ssq")
    if a.fragment_only:
        a.fragment_only.parent.mkdir(parents=True, exist_ok=True)
        a.fragment_only.write_bytes("".join(entries).encode("utf-8"))
        xml = "".join(entries)
        base_desc += f" (collision-checked only; fragment written to {tilde(a.fragment_only)})"
    else:
        end = xml.rfind("</mdb>")
        if end < 0:
            sys.exit("base musicdb has no </mdb>")
        xml = xml[:end] + "".join(entries) + xml[end:]
        mdb = out / "arc" / "startup_arc" / "data" / "gamedata" / "musicdb.xml"
        mdb.parent.mkdir(parents=True, exist_ok=True)
        mdb.write_bytes(xml.encode("utf-8"))

    # ---- summary
    print()
    print(f"Spike mod folder: {tilde(out)}")
    print(f"  song      : {tilde(sim)}")
    print(f"  audio     : {audio.name}")
    print(f"  code/mcode: {a.code} / {a.mcode}   series {a.series}   bpmmax {bpmmax}")
    print(f"  title     : {clip_utf8(title)!r}  artist {clip_utf8(artist)!r}")
    print(f"  charts    : kept {kept}  dropped {dropped}")
    print(f"  levels    : {levels}")
    print(f"  art       : {'NONE (E5)' if a.no_art else (tilde(art_src) if art_src else 'placeholder')}")
    print(f"  musicdb   : base = {base_desc}, +{len(entries)} entries, {len(xml.encode()):,} bytes")
    if conv_notes:
        print("  converter :", conv_notes.replace("\n", "\n              "))
    print()
    print("Cabinet checklist")
    print("  boot : grep log.txt for 'ME1529' (must be absent), 'FastBootup: boot pass complete -- processed N items'")
    print("         (N/5 ~= loaded songs; compare runs for E3), 'createList', and any WARN/ERROR")
    print(f"  wheel: song in VERSION filter 30 / ALL MUSIC; title {clip_utf8(title)!r}; thumbnail drawn? jacket on header?")
    print("  play : preview audible after settle; start; play to results; note any service error")
    print(f"  assets: grep log.txt for '{a.code}' to see SSQ/XSB/XWB/jacket opens via LayeredFS")
    if a.clones:
        print(f"  E3   : {a.clones} clones have SSQ only (no audio) — count loaded songs, do not play them")
    if a.no_art:
        print("  E5   : no jacket/thumbnail — record what the wheel card and header draw")
    return 0


if __name__ == "__main__":
    sys.exit(main())
