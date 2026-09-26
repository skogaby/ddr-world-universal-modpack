#!/usr/bin/env python3
"""
Generate DDR SELECTION's S-Marvelous art for the legacy skins.

S-Marvelous' World art (``data_mods/s_marvelous/``) is a programmatic
recolour of World's own Marvelous textures. This script holds those recipes
and applies them to the five legacy skins' Marvelous textures:

* ``all_purple``    — every pixel to hue 280°, saturation floor 150/255,
                      value x0.82 (World's combo digits, S-MFC splash, emblems
                      and the ALL PURPLE judgement word).
* ``purple_shadow`` — pixels with saturation < 0.12 (white fill, black
                      outline, grey anti-aliasing) kept; the rest to hue 280°,
                      saturation floor 0.55, value x0.90 (World's default
                      PURPLE SHADOW judgement word). Alpha is never changed.
* ``violet_outline`` — the legacy skins' PURPLE SHADOW word (maintainer,
                      2026-09-25): the stock letters kept, the dark outline /
                      shadow around them repainted in the skin's ALL PURPLE
                      letter colour and grown 1 px outward. Per-skin luminance
                      thresholds (``OUTLINE``) split letter from outline;
                      anti-aliased edge pixels are un-blended so the letters
                      keep a clean edge on the new colour.
* ``violet_glow``    — skin 5's PURPLE SHADOW word: letters and black outline
                      stock, the white glow outside the outline in the ALL
                      PURPLE colour (its alpha fall-off kept); the grey drop
                      shadow stays grey.

Output, per skin N under ``data_mods/ddr_selection/s_marvelous/N/`` (the
layout mirrors ``data_mods/s_marvelous/``):

  dance_judge/smarvelous_{all_purple,purple_shadow}.png  (dance_judge000N_marvelous)
  dance_fullcombo/<region with 's' before its last token>.png
      (every dance_fullcombo000N texture whose last '_' token starts with
      'mar' — the DLL's splash rename rule: dafu_eff_mar -> dafu_eff_smar)
  dance_combo/smarvelous_{0..9,combo}.png                (skins 4-5 only:
      dance_combo000N_marvelous_*; skins 1-3 have no per-grade colour)

Every file is written at its donor's imgrect size (the size ifstools
extracts). The DLL's donor-anchored atlas clone and per-image serving both
place the image at the donor's imgrect origin.

Inputs:
  World install  --world, else $DDR_WORLD_INSTALL (the folder holding data/)
  A3 install     --a3, else $DDR_A3_INSTALL — only needed for skin 5's combo
                 when the World install has no A3 import
                 (data_mods/ddr_selection_a3/); World's own
                 dance_combo0005_v0.arc is blanked.

Usage:
  python3 scripts/gen_ddr_selection_smarv_art.py [--skins 1,2,3,4,5]
      [--out DIR] [--sheet PNG]
  python3 scripts/gen_ddr_selection_smarv_art.py --check-world

``--check-world`` regenerates World's shipped S-Marvelous art from World's
_v3 donors (uvrect crop, as that art was made) and compares it with
``data_mods/s_marvelous/``, as a proof that the recipes here are the shipped
ones.

Requires: numpy, Pillow, ifstools (``pip install ifstools``);
scripts/unpack_arc.py next to this script.
"""
from __future__ import annotations

import argparse
import contextlib
import io
import os
import sys
import tempfile
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw

SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parent
sys.path.insert(0, str(SCRIPT_DIR))
from unpack_arc import ARC  # noqa: E402

try:
    from ifstools import IFS
except ImportError:  # pragma: no cover - environment check
    sys.exit("error: ifstools is not installed (pip install ifstools)")

HUE = 280.0 / 360.0
RECIPES = {
    # name: (neutral saturation kept below, saturation floor, value scale)
    "all_purple": (0.0, 150.0 / 255.0, 0.82),
    "purple_shadow": (0.12, 0.55, 0.90),
}
# The legacy PURPLE SHADOW word, per skin: the luminance band that separates
# the dark outline (at or below `lo`: fully outline) from the letters (at or
# above `hi`: fully letter), and the mode (maintainer, 2026-09-25):
#   "outline" — the outline / shadow turns violet and grows 1 px;
#   "glow"    — the outline stays; the light glow OUTSIDE it (found by
#               flood-filling from the image border through non-opaque and
#               light pixels — the dark outline is the wall) turns violet.
#   1 1st-5th     cream letters, navy outline
#   2 MAX-EXTREME white-to-yellow letters, brown outline + black drop shadow
#   3 SuperNOVA   silver letters, dark grey shadow
#   4 X           cream / gold letters, black outline with a soft blur
#   5 2013-A      peach letters, thin black outline, white glow, grey shadow
OUTLINE = {
    1: (0.10, 0.70, "outline"),
    2: (0.42, 0.70, "outline"),
    3: (0.26, 0.42, "outline"),
    4: (0.15, 0.35, "outline"),
    5: (0.08, 0.30, "glow"),
}
# "glow" mode: how light a glow pixel must be to turn violet (a linear ramp,
# so the drop shadow's dark pixels under the glow stay grey).
GLOW_RAMP = (0.35, 0.80)
# A3 coloured the combo by worst grade only on these skins
# (`combo_math::single_sheet` is the complement).
GRADE_SHEET_SKINS = (4, 5)
IFS_MAGIC = bytes([0x6C, 0xAD, 0x8F, 0x89])


def tilde(p: Path | str) -> str:
    """Print paths home-relative (never a username in logs)."""
    s = str(p)
    home = os.path.expanduser("~")
    return "~" + s[len(home):] if s.startswith(home) else s


# ── recipes ─────────────────────────────────────────────────────────


def _hsv(rgb: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    mx = rgb.max(-1)
    mn = rgb.min(-1)
    s = np.where(mx > 0, (mx - mn) / np.where(mx > 0, mx, 1), 0.0)
    return s, mx


def _rgb(h: float, s: np.ndarray, v: np.ndarray) -> np.ndarray:
    i = int(np.floor(h * 6)) % 6
    f = h * 6 - np.floor(h * 6)
    p, q, t = v * (1 - s), v * (1 - f * s), v * (1 - (1 - f) * s)
    return np.stack(
        [(v, q, p, p, t, v)[i], (t, v, v, q, p, p)[i], (p, p, t, v, v, q)[i]], -1
    )


def recolour(img: Image.Image, recipe: str) -> tuple[Image.Image, float]:
    """Apply `recipe`; returns the image and the fraction of visible pixels
    it changed (a PURPLE SHADOW over a grey word changes almost nothing)."""
    neutral, sat_floor, value = RECIPES[recipe]
    a = np.asarray(img.convert("RGBA")).astype(np.float64) / 255.0
    s, v = _hsv(a[..., :3])
    out = _rgb(HUE, np.maximum(s, sat_floor), v * value)
    kept = s < neutral
    rgb = np.where(kept[..., None], a[..., :3], out)
    res = np.concatenate([rgb, a[..., 3:]], -1)
    res8 = np.clip(np.rint(res * 255.0), 0, 255).astype(np.uint8)
    visible = a[..., 3] > 0.05
    changed = (np.abs(res8[..., :3].astype(int) - np.rint(a[..., :3] * 255).astype(int)).max(-1) > 2) & visible
    frac = float(changed.sum()) / float(max(visible.sum(), 1))
    return Image.fromarray(res8, "RGBA"), frac


def _dilate1(alpha: np.ndarray) -> np.ndarray:
    """Grow a coverage mask by one pixel (diagonals at 0.7 for a rounder
    edge)."""
    h, w = alpha.shape
    p = np.pad(alpha, 1)
    out = alpha.copy()
    for dy, dx, k in ((-1, 0, 1.0), (1, 0, 1.0), (0, -1, 1.0), (0, 1, 1.0),
                      (-1, -1, 0.7), (-1, 1, 0.7), (1, -1, 0.7), (1, 1, 0.7)):
        out = np.maximum(out, k * p[1 + dy:1 + dy + h, 1 + dx:1 + dx + w])
    return out


def _exterior(passable: np.ndarray) -> np.ndarray:
    """Pixels reachable from the image border through `passable` ones."""
    h, w = passable.shape
    seen = np.zeros_like(passable)
    stack = [(y, x) for y in range(h) for x in (0, w - 1) if passable[y, x]]
    stack += [(y, x) for x in range(w) for y in (0, h - 1) if passable[y, x]]
    for y, x in stack:
        seen[y, x] = True
    while stack:
        y, x = stack.pop()
        for ny, nx in ((y + 1, x), (y - 1, x), (y, x + 1), (y, x - 1)):
            if 0 <= ny < h and 0 <= nx < w and passable[ny, nx] and not seen[ny, nx]:
                seen[ny, nx] = True
                stack.append((ny, nx))
    return seen


def _over(top_rgb, top_a, bot_rgb, bot_a):
    out_a = top_a + bot_a * (1 - top_a)
    rgb = (top_rgb * top_a[..., None] + bot_rgb * bot_a[..., None] * (1 - top_a[..., None])) / np.maximum(
        out_a[..., None], 1e-6
    )
    return rgb, out_a


def _word_layers(donor: Image.Image, lo: float, hi: float):
    """(pixels, alpha, luminance, letter weight) of a word."""
    a = np.asarray(donor.convert("RGBA")).astype(np.float64) / 255.0
    alpha = a[..., 3]
    lum = 0.299 * a[..., 0] + 0.587 * a[..., 1] + 0.114 * a[..., 2]
    w = np.clip((lum - lo) / max(hi - lo, 1e-6), 0.0, 1.0)
    return a, alpha, lum, w


def _to_image(rgb: np.ndarray, alpha: np.ndarray) -> Image.Image:
    res = np.concatenate([rgb, alpha[..., None]], -1)
    return Image.fromarray(np.clip(np.rint(res * 255.0), 0, 255).astype(np.uint8), "RGBA")


def violet_glow(
    donor: Image.Image, all_purple: Image.Image, lo: float, hi: float
) -> tuple[Image.Image, tuple[int, int, int]]:
    """Skin 5's PURPLE SHADOW word: letters and outline stock, the light
    glow outside the outline recoloured to its ALL PURPLE colour (a light
    pixel's ALL PURPLE recolour, blended in by lightness — GLOW_RAMP — so the
    grey drop shadow under the glow stays grey). Alpha unchanged. Returns
    the image and the violet of a pure-white glow pixel."""
    a, alpha, lum, w = _word_layers(donor, lo, hi)
    ap = np.asarray(all_purple.convert("RGBA")).astype(np.float64) / 255.0
    halo = _exterior((alpha < 0.9) | (w > 0.5)) & (alpha >= 0.05)
    lo_g, hi_g = GLOW_RAMP
    g = np.clip((lum - lo_g) / (hi_g - lo_g), 0.0, 1.0)[..., None]
    rgb = np.where(halo[..., None], a[..., :3] * (1 - g) + ap[..., :3] * g, a[..., :3])
    white, _ = recolour(Image.new("RGBA", (1, 1), (255, 255, 255, 255)), "all_purple")
    return _to_image(rgb, alpha), tuple(white.getpixel((0, 0))[:3])


def violet_outline(
    donor: Image.Image, all_purple: Image.Image, lo: float, hi: float
) -> tuple[Image.Image, tuple[int, int, int]]:
    """The legacy PURPLE SHADOW word: the stock letters over a violet
    outline one pixel thicker than the stock one (see OUTLINE). The violet
    is the median colour of the ALL PURPLE word's letters. Returns the image
    and that colour."""
    a, alpha, lum, w = _word_layers(donor, lo, hi)
    ap = np.asarray(all_purple.convert("RGBA")).astype(np.float64) / 255.0
    letters = (alpha > 0.9) & (w >= 1)
    stroke = (alpha > 0.9) & (w <= 0)
    violet = np.median(ap[..., :3][letters], axis=0)
    stroke_rgb = np.median(a[..., :3][stroke], axis=0) if stroke.any() else np.zeros(3)
    # An edge pixel is w * letter + (1 - w) * stroke colour: un-blend it so
    # the letter layer carries the letter colour, not the darkened mix.
    letter_rgb = np.clip((a[..., :3] - (1 - w[..., None]) * stroke_rgb) / np.maximum(w, 1e-3)[..., None], 0, 1)
    letter_rgb = np.where((w >= 1)[..., None], a[..., :3], letter_rgb)
    zero = np.zeros_like(alpha)
    rgb, out_a = _over(np.broadcast_to(violet, a[..., :3].shape), _dilate1(alpha * (1 - w)), a[..., :3], zero)
    rgb, out_a = _over(letter_rgb, alpha * w, rgb, out_a)
    return _to_image(rgb, out_a), tuple(int(round(c * 255)) for c in violet)


# ── package reading ─────────────────────────────────────────────────


class Package:
    """One arc's IFS, opened with ifstools (textures decoded on demand)."""

    def __init__(self, arc_path: Path, tmp: Path):
        with contextlib.redirect_stdout(io.StringIO()):
            arc = ARC(arc_path.read_bytes(), decompress=True)
            member = next((n for n in arc.list_files() if n.endswith(".ifs")), None)
            data = arc.get_file(member) if member else None
        if member is None or not data or data[:4] != IFS_MAGIC:
            raise ValueError(f"{tilde(arc_path)}: no readable IFS member")
        self.ifs_path = tmp / Path(member).name
        self.ifs_path.write_bytes(data)
        self.ifs = IFS(str(self.ifs_path))
        self.tex = self.ifs.tree.folders["tex"].files
        self.arc_path = arc_path

    def names(self) -> list[str]:
        return sorted(n[:-4] for n in self.tex if n.endswith(".png"))

    def image(self, name: str, uv_crop: bool = False) -> Image.Image:
        f = self.tex[f"{name}.png"]
        raw = f.load(crop_to_uvrect=uv_crop) if uv_crop else f.load()
        return Image.open(io.BytesIO(raw)).convert("RGBA")


def arc_candidates(world: Path, a3: Path | None, base: str) -> list[Path]:
    """The arcs the game's probe would open, in order: the A3 import
    (LayeredFS) first, World's data/ next, then the A3 install itself."""
    rels = [f"arc/bm2d/{base}_v3.arc", f"arc/bm2d/{base}_v0.arc", f"arc/bm2d/{base}.arc"]
    out = [world / "data_mods" / "ddr_selection_a3" / r for r in rels]
    out += [world / "data" / r for r in rels]
    if a3 is not None:
        out += [a3 / "data" / r for r in rels]
    return [p for p in out if p.is_file()]


def open_package(world: Path, a3: Path | None, base: str, tmp: Path) -> Package | None:
    for path in arc_candidates(world, a3, base):
        try:
            pkg = Package(path, tmp)
            print(f"  [read] {tilde(path)}")
            return pkg
        except Exception as exc:  # blanked arc etc. — try the next one
            print(f"  [skip] {tilde(path)}: {exc}")
    return None


def fc_region_rename(region: str) -> str | None:
    """The DLL's splash rename rule (`assets.rs` / `legacy_logic.rs`)."""
    head, sep, tail = region.rpartition("_")
    if not sep or not tail.startswith("mar"):
        return None
    return f"{head}_s{tail}"


# ── generation ──────────────────────────────────────────────────────


def generate_skin(skin: int, world: Path, a3: Path | None, out: Path, tmp: Path, sheet: list) -> int:
    """Write every S-Marvelous texture for `skin`; returns the file count."""
    written = 0
    dest = out / str(skin)

    judge = open_package(world, a3, f"dance_judge{skin:04d}", tmp)
    if judge is None:
        print(f"  [warn] skin {skin}: no dance_judge{skin:04d} package — word skipped")
    else:
        donor_name = f"dance_judge{skin:04d}_marvelous"
        donor = judge.image(donor_name)
        (dest / "dance_judge").mkdir(parents=True, exist_ok=True)
        all_purple, _ = recolour(donor, "all_purple")
        path = dest / "dance_judge" / "smarvelous_all_purple.png"
        all_purple.save(path)
        print(f"  {tilde(path)} {all_purple.size}")
        lo, hi, mode = OUTLINE[skin]
        if mode == "glow":
            shadow, violet = violet_glow(donor, all_purple, lo, hi)
            what = f"violet glow {violet}, stock outline"
        else:
            shadow, violet = violet_outline(donor, all_purple, lo, hi)
            what = f"violet outline {violet}, +1 px"
        path = dest / "dance_judge" / "smarvelous_purple_shadow.png"
        shadow.save(path)
        print(f"  {tilde(path)} {shadow.size} ({what})")
        written += 2
        sheet.append((f"S{skin} {donor_name} -> all_purple", donor, all_purple))
        sheet.append((f"S{skin} {donor_name} -> purple_shadow", donor, shadow))

    fc = open_package(world, a3, f"dance_fullcombo{skin:04d}", tmp)
    if fc is None:
        print(f"  [warn] skin {skin}: no dance_fullcombo{skin:04d} package — splash skipped")
    else:
        for region in fc.names():
            new = fc_region_rename(region)
            if new is None:
                continue
            donor = fc.image(region)
            img, _ = recolour(donor, "all_purple")
            path = dest / "dance_fullcombo" / f"{new}.png"
            path.parent.mkdir(parents=True, exist_ok=True)
            img.save(path)
            written += 1
            print(f"  {tilde(path)} {img.size}")
            sheet.append((f"S{skin} {region}", donor, img))

    if skin in GRADE_SHEET_SKINS:
        combo = open_package(world, a3, f"dance_combo{skin:04d}", tmp)
        if combo is None:
            print(
                f"  [warn] skin {skin}: no readable dance_combo{skin:04d} (skin 5: run the A3 import "
                "or pass --a3) — combo skipped"
            )
        else:
            sizes = []
            for key in [str(d) for d in range(10)] + ["combo"]:
                donor = combo.image(f"dance_combo{skin:04d}_marvelous_{key}")
                img, _ = recolour(donor, "all_purple")
                path = dest / "dance_combo" / f"smarvelous_{key}.png"
                path.parent.mkdir(parents=True, exist_ok=True)
                img.save(path)
                written += 1
                sizes.append(img.size)
                if key in ("0", "combo"):
                    sheet.append((f"S{skin} combo marvelous_{key}", donor, img))
            print(
                f"  {tilde(dest / 'dance_combo')}/smarvelous_{{0..9,combo}}.png "
                f"(11 files, digits {sizes[0]}, word {sizes[-1]})"
            )
    return written


def write_sheet(rows: list, path: Path) -> None:
    """Donor | generated, one row per texture, on a dark background."""
    if not rows:
        return
    pad, label_w = 6, 300
    w = label_w + max(d.width for _, d, _ in rows) * 2 + pad * 3
    h = sum(max(d.height, g.height) + pad for _, d, g in rows) + pad
    sheet = Image.new("RGBA", (w, h), (32, 32, 40, 255))
    draw = ImageDraw.Draw(sheet)
    y = pad
    col = max(d.width for _, d, _ in rows)
    for label, donor, gen in rows:
        draw.text((pad, y + 4), label, fill=(230, 230, 230, 255))
        sheet.alpha_composite(donor, (label_w, y))
        sheet.alpha_composite(gen, (label_w + col + pad, y))
        y += max(donor.height, gen.height) + pad
    path.parent.mkdir(parents=True, exist_ok=True)
    sheet.save(path)
    print(f"contact sheet: {tilde(path)}")


def check_world(world: Path, tmp: Path) -> int:
    """Regenerate World's shipped art from World's _v3 donors (uvrect crop)
    and compare with data_mods/s_marvelous/."""
    shipped = REPO_ROOT / "data_mods" / "s_marvelous"
    cases = [
        ("dance_judge", "daju_marvelous", "all_purple", "dance_judge/smarvelous_all_purple.png"),
        ("dance_judge", "daju_marvelous", "purple_shadow", "dance_judge/smarvelous_purple_shadow.png"),
        ("dance_fullcombo", "dafu_eff_mar", "all_purple", "dance_fullcombo/dafu_eff_smar.png"),
        ("dance_fullcombo", "dafu_light_marvelous", "all_purple", "dance_fullcombo/dafu_light_smarvelous.png"),
        ("dance_fullcombo", "dafu_rocket_marvelous", "all_purple", "dance_fullcombo/dafu_rocket_smarvelous.png"),
        (
            "dance_fullcombo",
            "dafu_side_light_marvelous",
            "all_purple",
            "dance_fullcombo/dafu_side_light_smarvelous.png",
        ),
    ] + [
        ("dance_combo", f"daco_combo_marvelous_{d}", "all_purple", f"dance_combo/smarvelous_{d}.png")
        for d in range(10)
    ]
    pkgs: dict[str, Package] = {}
    bad = 0
    for base, donor_name, recipe, rel in cases:
        if base not in pkgs:
            pkg = open_package(world, None, f"{base}", tmp)
            if pkg is None:
                print(f"FAIL {base}: package not found")
                return 1
            pkgs[base] = pkg
        pkg = pkgs[base]
        want = np.asarray(Image.open(shipped / rel).convert("RGBA")).astype(int)
        # The shipped art was cut at the uvrect; some files are smaller
        # still (the combo digits) — compare at the top-left of the uvrect.
        img, _ = recolour(pkg.image(donor_name, uv_crop=True), recipe)
        got = np.asarray(img).astype(int)[: want.shape[0], : want.shape[1]]
        if got.shape != want.shape:
            print(f"FAIL {rel}: generated {got.shape[:2]} vs shipped {want.shape[:2]}")
            bad += 1
            continue
        vis = want[..., 3] > 12
        err = np.abs(got[..., :3] - want[..., :3])[vis]
        alpha = int(np.abs(got[..., 3] - want[..., 3]).max())
        ok = err.mean() < 1.0 and alpha == 0
        bad += 0 if ok else 1
        print(
            f"{'PASS' if ok else 'FAIL'} {rel}: mean |dRGB| {err.mean():.2f}, max {err.max()}, alpha max {alpha}"
        )
    print("check-world:", "OK" if bad == 0 else f"{bad} mismatch(es)")
    return 0 if bad == 0 else 1


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--world", type=Path, default=os.environ.get("DDR_WORLD_INSTALL"))
    ap.add_argument("--a3", type=Path, default=os.environ.get("DDR_A3_INSTALL"))
    ap.add_argument("--out", type=Path, default=REPO_ROOT / "data_mods" / "ddr_selection" / "s_marvelous")
    ap.add_argument("--skins", default="1,2,3,4,5")
    ap.add_argument("--sheet", type=Path, help="write a donor | generated contact sheet PNG here")
    ap.add_argument("--check-world", action="store_true")
    args = ap.parse_args()

    if args.world is None or not (args.world / "data" / "arc" / "bm2d").is_dir():
        print("error: set --world or DDR_WORLD_INSTALL to the World folder holding data/", file=sys.stderr)
        return 2
    a3 = args.a3 if args.a3 is not None and (args.a3 / "data").is_dir() else None

    with tempfile.TemporaryDirectory() as t:
        tmp = Path(t)
        if args.check_world:
            return check_world(args.world, tmp)
        try:
            skins = [int(s) for s in args.skins.split(",") if s.strip()]
        except ValueError:
            print("error: --skins takes a comma list of 1..5", file=sys.stderr)
            return 2
        if any(s < 1 or s > 5 for s in skins):
            print("error: --skins takes a comma list of 1..5", file=sys.stderr)
            return 2
        sheet: list = []
        total = 0
        for skin in skins:
            print(f"skin {skin}:")
            total += generate_skin(skin, args.world, a3, args.out, tmp, sheet)
        print(f"{total} file(s) written under {tilde(args.out)}")
        if args.sheet:
            write_sheet(sheet, args.sheet)
    return 0


if __name__ == "__main__":
    sys.exit(main())
