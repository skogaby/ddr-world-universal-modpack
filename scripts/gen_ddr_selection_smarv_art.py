#!/usr/bin/env python3
"""
Generate DDR SELECTION's S-Marvelous art for the legacy skins.

S-Marvelous' World art (``data_mods/s_marvelous/``) is a programmatic
recolour of World's own Marvelous textures. This script holds those recipes
and applies them to the Marvelous textures of DDR SELECTION's art sets:

  1..5  the eras (A3's legacy ``…000N`` packages);
  6     DDR A (A3's skin-0 ``…0000_v0`` packages);
  7     DDR A3 — the white cabinet's ``…0000_v2`` packages, shared with the
        gold cabinet's ``…0000_v1`` (DDR SELECTION skins 7 and 8): every
        ``_v1`` donor must be pixel-identical to its ``_v2`` twin, or set 7
        fails and writes nothing.

Recipes:

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
* ``violet_glow``    — sets 5 and 6's PURPLE SHADOW word: letters and dark
                      outline stock, the light glow / rim outside the outline
                      in the ALL PURPLE colour (its alpha fall-off kept); a
                      grey drop shadow stays grey.

Output, per art set N under ``data_mods/ddr_selection/s_marvelous/N/`` (the
layout mirrors ``data_mods/s_marvelous/``; T = the texture number, N for an
era and 0000 for a theme):

  dance_judge/smarvelous_{all_purple,purple_shadow}.png  (dance_judgeT_marvelous)
  dance_fullcombo/<region with 's' before its last token>.png
      (every dance_fullcombo texture whose last '_' token starts with
      'mar' — the DLL's splash rename rule: dafu_eff_mar -> dafu_eff_smar)
  dance_combo/smarvelous_{all_purple,purple_shadow}_{0..9,combo}.png
      (sets 4-7 only: dance_comboT_marvelous_*; sets 1-3 have no per-grade
      colour). The Judgement Color setting picks the variant, as for the
      word: ALL PURPLE recolours the digits, PURPLE SHADOW keeps them and
      gives their outline / glow the word's violet treatment (COMBO_SHADOW).

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
  python3 scripts/gen_ddr_selection_smarv_art.py [--skins 1,2,3,4,5,6,7]
      [--out DIR] [--sheet PNG]
  python3 scripts/gen_ddr_selection_smarv_art.py --check-world
  python3 scripts/gen_ddr_selection_smarv_art.py --review target/smarv_legacy_review

``--review DIR`` writes donor-vs-art review pages (one per art set, all of
them stacked, a words-only grid and a combo-sheets-only page) from the art
already under ``--out``.

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
# above `hi`: fully letter), the mode (maintainer, 2026-09-25) and, optional,
# a shade for the "outline" violet (its value scale; default 1.0 = the ALL
# PURPLE letters' colour):
#   "outline" — the outline / shadow turns violet and grows 1 px;
#   "glow"    — the outline stays; the light glow OUTSIDE it (found by
#               flood-filling from the image border through non-opaque and
#               light pixels — the dark outline is the wall) turns violet.
#   1 1st-5th     cream letters, navy outline
#   2 MAX-EXTREME white-to-yellow letters, brown outline + black drop shadow
#   3 SuperNOVA   silver letters, dark grey shadow
#   4 X           cream / gold letters, black outline with a soft blur
#   5 2013-A      peach letters, thin black outline, white glow, grey shadow
#   6 DDR A       cream letters (grey lower bevel), thick black outline whose
#                 lower edge is grey (lum ~0.3-0.4, so the wall sits higher),
#                 a white rim and a yellow glow outside it
#   7 DDR A3      cream letters with a thin light inner line, thick dark
#                 olive outline, faint yellow glow; the violet outline is
#                 darkened (maintainer, 2026-09-26: too light at 1.0)
OUTLINE = {
    1: (0.10, 0.70, "outline"),
    2: (0.42, 0.70, "outline"),
    3: (0.26, 0.42, "outline"),
    4: (0.15, 0.35, "outline"),
    5: (0.08, 0.30, "glow"),
    6: (0.40, 0.70, "glow"),
    7: (0.20, 0.45, "outline", 0.70),
}
# The combo sheet's PURPLE SHADOW (sets with per-grade sheets; maintainer,
# 2026-09-26): the word's recipe fields per set, over the eleven images with
# one violet for the whole sheet. X's digits keep their bold black outline
# and only the light glow around it turns violet (maintainer: consistent
# with the other sets), unlike X's word, which has no light glow and whose
# dark outline turns violet.
COMBO_SHADOW = {
    4: (0.15, 0.35, "glow"),
    5: (0.08, 0.30, "glow"),
    6: (0.40, 0.70, "glow"),
    7: (0.20, 0.45, "outline", 0.70),
}
COLOR_KEYS = ("all_purple", "purple_shadow")
COMBO_KEYS = [str(d) for d in range(10)] + ["combo"]
# "glow" mode: how light a glow pixel must be to turn violet (a linear ramp,
# so the drop shadow's dark pixels under the glow stay grey).
GLOW_RAMP = (0.35, 0.80)
# A3 coloured the combo by worst grade only on these sets — skins 4-5 and
# A3's own skin 0, i.e. the themes (`combo_math::single_sheet` is the
# complement).
GRADE_SHEET_SKINS = (4, 5, 6, 7)
# The themes' art sets: their packages' version suffix, and the twin
# generation that shares the set (DDR SELECTION's `targets::art_set`).
THEME_SUFFIX = {6: "_v0", 7: "_v2"}
THEME_TWIN = {7: "_v1"}
ART_SETS = (1, 2, 3, 4, 5, 6, 7)
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


def letter_violet(donors: list, all_purples: list, lo: float, hi: float, shade: float = 1.0) -> np.ndarray:
    """The outline violet: the median colour of the ALL PURPLE images'
    letter pixels (pooled over every image given), its value scaled by
    `shade`."""
    px = []
    for donor, all_purple in zip(donors, all_purples):
        _, alpha, _, w = _word_layers(donor, lo, hi)
        ap = np.asarray(all_purple.convert("RGBA")).astype(np.float64) / 255.0
        px.append(ap[..., :3][(alpha > 0.9) & (w >= 1)])
    return np.median(np.concatenate(px), axis=0) * shade


def violet_outline(
    donor: Image.Image,
    all_purple: Image.Image,
    lo: float,
    hi: float,
    shade: float = 1.0,
    violet: np.ndarray | None = None,
) -> tuple[Image.Image, tuple[int, int, int]]:
    """The legacy PURPLE SHADOW word: the stock letters over a violet
    outline one pixel thicker than the stock one (see OUTLINE). The violet
    is the median colour of the ALL PURPLE word's letters, its value scaled
    by `shade` (or `violet`, shared by a whole combo sheet). Returns the
    image and that colour."""
    a, alpha, lum, w = _word_layers(donor, lo, hi)
    stroke = (alpha > 0.9) & (w <= 0)
    if violet is None:
        violet = letter_violet([donor], [all_purple], lo, hi, shade)
    stroke_rgb = np.median(a[..., :3][stroke], axis=0) if stroke.any() else np.zeros(3)
    # An edge pixel is w * letter + (1 - w) * stroke colour: un-blend it so
    # the letter layer carries the letter colour, not the darkened mix.
    letter_rgb = np.clip((a[..., :3] - (1 - w[..., None]) * stroke_rgb) / np.maximum(w, 1e-3)[..., None], 0, 1)
    letter_rgb = np.where((w >= 1)[..., None], a[..., :3], letter_rgb)
    zero = np.zeros_like(alpha)
    rgb, out_a = _over(np.broadcast_to(violet, a[..., :3].shape), _dilate1(alpha * (1 - w)), a[..., :3], zero)
    rgb, out_a = _over(letter_rgb, alpha * w, rgb, out_a)
    return _to_image(rgb, out_a), tuple(int(round(c * 255)) for c in violet)


def combo_shadow(
    donor: Image.Image, all_purple: Image.Image, entry: tuple, violet: np.ndarray | None
) -> Image.Image:
    """One combo image's PURPLE SHADOW (COMBO_SHADOW `entry`; `violet` =
    the sheet's outline colour for the outline mode)."""
    lo, hi, mode, *_ = entry
    if mode == "glow":
        return violet_glow(donor, all_purple, lo, hi)[0]
    return violet_outline(donor, all_purple, lo, hi, violet=violet)[0]


# ── art sets ────────────────────────────────────────────────────────


def tex_number(art_set: int) -> int:
    """The %04d in the set's texture names: the era's own, 0 for a theme."""
    return 0 if art_set in THEME_SUFFIX else art_set


def package(kind: str, art_set: int, suffix: str | None = None) -> tuple[str, bool]:
    """(package name, fixed): an era's ``dance_judge000N`` goes through the
    probe's ``_v3`` / ``_v0`` / bare rungs; a theme's full ``…0000_vN`` name
    is opened as is (``suffix`` overrides the set's own generation)."""
    if art_set in THEME_SUFFIX:
        return f"{kind}0000{suffix or THEME_SUFFIX[art_set]}", True
    return f"{kind}{art_set:04d}", False


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


def arc_candidates(world: Path, a3: Path | None, base: str, fixed: bool = False) -> list[Path]:
    """The arcs the game's probe would open, in order: the A3 import
    (LayeredFS) first, World's data/ next, then the A3 install itself. A
    fixed name (a theme's ``…0000_vN``) is only its own arc."""
    if fixed:
        rels = [f"arc/bm2d/{base}.arc"]
    else:
        rels = [f"arc/bm2d/{base}_v3.arc", f"arc/bm2d/{base}_v0.arc", f"arc/bm2d/{base}.arc"]
    out = [world / "data_mods" / "ddr_selection_a3" / r for r in rels]
    out += [world / "data" / r for r in rels]
    if a3 is not None:
        out += [a3 / "data" / r for r in rels]
    return [p for p in out if p.is_file()]


def open_package(world: Path, a3: Path | None, base: str, tmp: Path, fixed: bool = False) -> Package | None:
    for path in arc_candidates(world, a3, base, fixed):
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


def open_set_package(kind: str, art_set: int, world: Path, a3: Path | None, tmp: Path) -> Package | None:
    """The set's donor package for ``kind``; for a set with a twin
    generation (7: ``_v2`` + ``_v1``) the twin is checked first — every
    Marvelous donor it has must be pixel-identical to the set's own, or the
    set is refused (its art would be wrong on one cabinet)."""
    name, fixed = package(kind, art_set)
    pkg = open_package(world, a3, name, tmp, fixed)
    twin_suffix = THEME_TWIN.get(art_set)
    if pkg is None or twin_suffix is None:
        return pkg
    twin_name, _ = package(kind, art_set, twin_suffix)
    twin = open_package(world, a3, twin_name, tmp, True)
    if twin is None:
        print(f"  [warn] set {art_set}: no {twin_name} to check against {name}")
        return pkg
    donors = [n for n in pkg.names() if n.endswith("_marvelous") or "_marvelous_" in n or fc_region_rename(n)]
    for n in donors:
        if n not in twin.names():
            raise SystemExit(f"FAIL set {art_set}: {twin_name} has no {n} ({name} does)")
        a, b = np.asarray(pkg.image(n)), np.asarray(twin.image(n))
        if a.shape != b.shape or not np.array_equal(a, b):
            raise SystemExit(
                f"FAIL set {art_set}: {twin_name}:{n} differs from {name}:{n} "
                f"({b.shape[1]}x{b.shape[0]} vs {a.shape[1]}x{a.shape[0]}) -- the two cabinets "
                "cannot share this set"
            )
    print(f"  [twin] {twin_name}: {len(donors)} Marvelous donor(s) pixel-identical to {name}")
    return pkg


def generate_skin(skin: int, world: Path, a3: Path | None, out: Path, tmp: Path, sheet: list) -> int:
    """Write every S-Marvelous texture for art set `skin`; returns the file
    count."""
    written = 0
    dest = out / str(skin)
    t = tex_number(skin)
    judge_name, _ = package("dance_judge", skin)
    fc_name, _ = package("dance_fullcombo", skin)
    combo_name, _ = package("dance_combo", skin)
    # Open (and twin-check) every donor before writing anything.
    judge = open_set_package("dance_judge", skin, world, a3, tmp)
    fc = open_set_package("dance_fullcombo", skin, world, a3, tmp)
    combo = open_set_package("dance_combo", skin, world, a3, tmp) if skin in GRADE_SHEET_SKINS else None

    if judge is None:
        print(f"  [warn] set {skin}: no {judge_name} package — word skipped")
    else:
        donor_name = f"dance_judge{t:04d}_marvelous"
        donor = judge.image(donor_name)
        (dest / "dance_judge").mkdir(parents=True, exist_ok=True)
        all_purple, _ = recolour(donor, "all_purple")
        path = dest / "dance_judge" / "smarvelous_all_purple.png"
        all_purple.save(path)
        print(f"  {tilde(path)} {all_purple.size}")
        lo, hi, mode, *rest = OUTLINE[skin]
        if mode == "glow":
            shadow, violet = violet_glow(donor, all_purple, lo, hi)
            what = f"violet glow {violet}, stock outline"
        else:
            shade = rest[0] if rest else 1.0
            shadow, violet = violet_outline(donor, all_purple, lo, hi, shade)
            what = f"violet outline {violet}, +1 px" + (f", shade {shade}" if shade != 1.0 else "")
        path = dest / "dance_judge" / "smarvelous_purple_shadow.png"
        shadow.save(path)
        print(f"  {tilde(path)} {shadow.size} ({what})")
        written += 2
        sheet.append((f"S{skin} {donor_name} -> all_purple", donor, all_purple))
        sheet.append((f"S{skin} {donor_name} -> purple_shadow", donor, shadow))

    if fc is None:
        print(f"  [warn] set {skin}: no {fc_name} package — splash skipped")
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
        if combo is None:
            print(
                f"  [warn] set {skin}: no readable {combo_name} (set 5: run the A3 import "
                "or pass --a3) — combo skipped"
            )
        else:
            donors = [combo.image(f"dance_combo{t:04d}_marvelous_{key}") for key in COMBO_KEYS]
            all_purples = [recolour(d, "all_purple")[0] for d in donors]
            entry = COMBO_SHADOW[skin]
            lo, hi, mode, *rest = entry
            violet = None
            if mode != "glow":
                violet = letter_violet(donors, all_purples, lo, hi, rest[0] if rest else 1.0)
            (dest / "dance_combo").mkdir(parents=True, exist_ok=True)
            for key, donor, all_purple in zip(COMBO_KEYS, donors, all_purples):
                shadow = combo_shadow(donor, all_purple, entry, violet)
                for color, img in zip(COLOR_KEYS, (all_purple, shadow)):
                    img.save(dest / "dance_combo" / f"smarvelous_{color}_{key}.png")
                    written += 1
                if key in ("0", "combo"):
                    sheet.append((f"S{skin} combo marvelous_{key} -> all_purple", donor, all_purple))
                    sheet.append((f"S{skin} combo marvelous_{key} -> purple_shadow", donor, shadow))
            what = mode if violet is None else f"{mode}, violet {tuple(int(round(c * 255)) for c in violet)}"
            print(
                f"  {tilde(dest / 'dance_combo')}/smarvelous_{{all_purple,purple_shadow}}_{{0..9,combo}}.png "
                f"(22 files, digits {donors[0].size}, word {donors[-1].size}; PURPLE SHADOW {what})"
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


# ── review pages ────────────────────────────────────────────────────

SET_NAMES = {
    1: "1st-5th",
    2: "MAX-EXTREME",
    3: "SuperNOVA",
    4: "X",
    5: "2013-A",
    6: "DDR A",
    7: "DDR A3 (White / Gold)",
}
BG, BAND, TILE, TEXT, DIM = (44, 45, 56, 255), (30, 31, 40, 255), (27, 28, 37, 255), (235, 235, 240, 255), (175, 175, 185, 255)
PAGE_W, PAD = 1624, 16


def _font(size: int):
    from PIL import ImageFont

    for name in ("Helvetica.ttc", "Arial.ttf", "DejaVuSans.ttf"):
        try:
            return ImageFont.truetype(name, size)
        except OSError:
            continue
    try:
        return ImageFont.load_default(size)
    except TypeError:  # Pillow < 10.1
        return ImageFont.load_default()


class Page:
    """A vertical stack of bands: titles, section headers and tile rows."""

    def __init__(self, width: int = PAGE_W):
        self.width = width
        self.parts: list[Image.Image] = []

    def band(self, text: str, size: int, fill=BAND, pad: int = 10) -> None:
        font = _font(size)
        h = size + 2 * pad
        im = Image.new("RGBA", (self.width, h), fill)
        ImageDraw.Draw(im).text((PAD, pad - 2), text, font=font, fill=TEXT)
        self.parts.append(im)

    def note(self, text: str) -> None:
        font = _font(13)
        im = Image.new("RGBA", (self.width, 22), BG)
        ImageDraw.Draw(im).text((PAD, 6), text, font=font, fill=DIM)
        self.parts.append(im)

    def tiles(self, items: list[tuple[str, Image.Image]], tile_w: int | None = None) -> None:
        """One or more rows of labelled tiles (wrapped at the page width);
        `tile_w` fixes the tile width (the image centred in it)."""
        font = _font(13)
        lab_h, gap = 22, 24
        rows: list[list[tuple[str, Image.Image, int]]] = [[]]
        x = PAD
        for label, img in items:
            w = max(tile_w or 0, img.width + 2 * PAD, int(font.getlength(label)) + 4)
            if rows[-1] and x + w > self.width - PAD:
                rows.append([])
                x = PAD
            rows[-1].append((label, img, w))
            x += w + gap
        for row in rows:
            h = lab_h + max(img.height for _, img, _ in row) + 2 * PAD + 8
            im = Image.new("RGBA", (self.width, h), BG)
            draw = ImageDraw.Draw(im)
            x = PAD
            for label, img, w in row:
                draw.text((x, 6), label, font=font, fill=DIM)
                th = img.height + 2 * PAD
                draw.rectangle((x, lab_h, x + w - 1, lab_h + th - 1), fill=TILE)
                im.alpha_composite(img, (x + (w - img.width) // 2, lab_h + PAD))
                x += w + gap
            self.parts.append(im)

    def image(self) -> Image.Image:
        h = sum(p.height for p in self.parts)
        out = Image.new("RGBA", (self.width, h), BG)
        y = 0
        for p in self.parts:
            out.alpha_composite(p, (0, y))
            y += p.height
        return out


def _scaled(img: Image.Image, k: float) -> Image.Image:
    return img.resize((max(1, round(img.width * k)), max(1, round(img.height * k))), Image.LANCZOS)


def _strip(images: list[Image.Image], gap: int = 4) -> Image.Image:
    w = sum(i.width for i in images) + gap * (len(images) - 1)
    h = max(i.height for i in images)
    out = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    x = 0
    for i in images:
        out.alpha_composite(i, (x, (h - i.height) // 2))
        x += i.width + gap
    return out


def write_review(sets: list[int], world: Path, a3: Path | None, art: Path, tmp: Path, review: Path) -> int:
    """Donor vs shipped-art review pages for `sets`, read from `art` (the
    generated folder — nothing is regenerated): `skinN_<name>.png` per set,
    `all_skins.png` (all pages stacked), `words_only.png` (stock | ALL
    PURPLE | PURPLE SHADOW per set) and `combos_only.png` (the same for the
    combo sheets of the per-grade sets)."""
    review.mkdir(parents=True, exist_ok=True)
    pages, words, combos = [], [], []
    for n in sets:
        t = tex_number(n)
        name = SET_NAMES[n]
        judge = open_set_package("dance_judge", n, world, a3, tmp)
        fc = open_set_package("dance_fullcombo", n, world, a3, tmp)
        combo = open_set_package("dance_combo", n, world, a3, tmp) if n in GRADE_SHEET_SKINS else None
        page = Page()
        title = f"Skin {n} — {name}" if n != 7 else "Set 7 — DDR A3 (skins 7 White and 8 Gold)"
        page.band(title, 26, fill=(26, 27, 35, 255), pad=14)
        d = art / str(n)
        if judge is not None:
            pkg_name, _ = package("dance_judge", n)
            stock = judge.image(f"dance_judge{t:04d}_marvelous")
            allp = Image.open(d / "dance_judge" / "smarvelous_all_purple.png").convert("RGBA")
            shadow = Image.open(d / "dance_judge" / "smarvelous_purple_shadow.png").convert("RGBA")
            page.band("Judgement word  (dance_judge/)", 17)
            page.tiles(
                [
                    (f"{pkg_name} MARVELOUS (stock)", stock),
                    ("S-Marvelous · ALL PURPLE", allp),
                    ("S-Marvelous · PURPLE SHADOW (default)", shadow),
                ]
            )
            page.tiles([("stock, 2x", _scaled(stock, 2)), ("PURPLE SHADOW, 2x", _scaled(shadow, 2))])
            words.append((f"Skin {n} — {name}:  stock  |  ALL PURPLE  |  PURPLE SHADOW", [stock, allp, shadow]))
        if fc is not None:
            page.band("S-MFC full-combo splash  (dance_fullcombo/)", 17)
            for region in fc.names():
                new = fc_region_rename(region)
                if new is None or not (d / "dance_fullcombo" / f"{new}.png").is_file():
                    continue
                page.tiles(
                    [
                        (f"stock {region}", fc.image(region)),
                        (f"S-MFC {new}.png", Image.open(d / "dance_fullcombo" / f"{new}.png").convert("RGBA")),
                    ]
                )
        if combo is not None and (d / "dance_combo").is_dir():
            keys = [str(k) for k in range(10)] + ["combo"]
            page.band("Combo sheet  (dance_combo/)  — shown while the combo is all S-Marvelous", 17)
            page.tiles([("stock marvelous sheet", _strip([combo.image(f"dance_combo{t:04d}_marvelous_{k}") for k in keys]))])
            rows = [("stock", _strip([combo.image(f"dance_combo{t:04d}_marvelous_{k}") for k in keys]))]
            for color, label in zip(COLOR_KEYS, ("ALL PURPLE", "PURPLE SHADOW")):
                files = [d / "dance_combo" / f"smarvelous_{color}_{k}.png" for k in keys]
                if all(f.is_file() for f in files):
                    strip = _strip([Image.open(f).convert("RGBA") for f in files])
                    page.tiles([(f"S-Marvelous sheet · {label}", strip)])
                    rows.append((label, strip))
            combos.append((f"Skin {n} — {name}", rows))
        img = page.image()
        slug = name.split(" (")[0].replace(" ", "-")
        path = review / f"skin{n}_{slug}.png"
        img.save(path)
        print(f"review: {tilde(path)} {img.size}")
        pages.append(img)
    if pages:
        w = max(p.width for p in pages)
        out = Image.new("RGBA", (w, sum(p.height + 8 for p in pages)), (20, 20, 26, 255))
        y = 0
        for p in pages:
            out.alpha_composite(p, (0, y))
            y += p.height + 8
        out.save(review / "all_skins.png")
        print(f"review: {tilde(review / 'all_skins.png')} {out.size}")
    if words:
        tile_w = 650
        page = Page(width=3 * tile_w + 2 * 24 + 2 * PAD)
        for label, imgs in words:
            k = min(1.8, (tile_w - 2 * PAD) / max(i.width for i in imgs))
            page.note(label)
            page.tiles([("", _scaled(i, k)) for i in imgs], tile_w=tile_w)
        img = page.image()
        img.save(review / "words_only.png")
        print(f"review: {tilde(review / 'words_only.png')} {img.size}")
    if combos:
        page = Page(width=1100)
        for label, rows in combos:
            page.note(f"{label}:  stock  |  ALL PURPLE  |  PURPLE SHADOW")
            for row_label, strip in rows:
                page.tiles([(row_label, strip)])
        img = page.image()
        img.save(review / "combos_only.png")
        print(f"review: {tilde(review / 'combos_only.png')} {img.size}")
    return 0


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
    ap.add_argument("--skins", default=",".join(str(n) for n in ART_SETS), help="art sets (1..7)")
    ap.add_argument("--sheet", type=Path, help="write a donor | generated contact sheet PNG here")
    ap.add_argument("--check-world", action="store_true")
    ap.add_argument(
        "--review",
        type=Path,
        help="write donor-vs-art review pages for --skins here from the art under --out (writes no art)",
    )
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
            print("error: --skins takes a comma list of art sets 1..7", file=sys.stderr)
            return 2
        if any(s not in ART_SETS for s in skins):
            print("error: --skins takes a comma list of art sets 1..7", file=sys.stderr)
            return 2
        if args.review:
            return write_review(skins, args.world, a3, args.out, tmp, args.review)
        sheet: list = []
        total = 0
        for skin in skins:
            print(f"set {skin}:")
            total += generate_skin(skin, args.world, a3, args.out, tmp, sheet)
        print(f"{total} file(s) written under {tilde(args.out)}")
        if args.sheet:
            write_sheet(sheet, args.sheet)
    return 0


if __name__ == "__main__":
    sys.exit(main())
