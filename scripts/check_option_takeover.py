#!/usr/bin/env python3
"""Regression harness for the hand-authored texture take-over.

Compares the generator's output against a reference copy of the shipped
hand-authored originals, per the localization design's acceptance rule:
LAYOUT equivalence, not byte identity (the take-over re-renders text with the
same fonts/metrics, so antialiasing bytes differ but structure must not).

Per hand-authored file it checks:
- TEXT: the ink-row band structure of the text region (each rendered line's
  vertical extent, ±1 px) matches the original — same line count, same
  baselines, i.e. the greedy wrap reproduced the original's line breaks.
- ART (SPLIT panels): the art region right of the divider is byte-equal on
  RGB-over-alpha (the crop was extracted from these very files).
- MARKERS (templates): every marker rectangle is byte-equal, and marker
  geometry is identical across all generated languages.

Usage:
    python3 scripts/check_option_takeover.py --reference <dir-of-shipped-pngs>

Exit code 0 = all checks pass.
"""

import argparse
import sys
from pathlib import Path

import numpy as np
from PIL import Image

from option_strings import PREVIEWS, TEMPLATES

SCRIPT_DIR = Path(__file__).resolve().parent

# The hand-authored files taken over by the generator (everything else was
# already script-generated and is covered by the byte-identity gate).
TAKEOVER_PREVIEWS = [
    "seop_image_autoplay_on.png",
    "seop_image_autoplay_off.png",
    "seop_image_premium_free_on.png",
    "seop_image_premium_free_off.png",
    "seop_image_center_arrows_1p_on.png",
    "seop_image_center_arrows_1p_off.png",
    "seop_image_customize_movie_size_on.png",
    "seop_image_customize_movie_size_off.png",
    "seop_image_customize_movie_size_fullscreen.png",
]
VERBATIM = ["seop_return.png", "seop_tab_title_mods.png"]

# Text region: left of the divider for SPLIT panels; art starts at x>=186,
# so x<178 is text-only in every hand-authored panel (WIDE autoplay panels
# have text past 178 — for those compare the full width).
TEXT_X_LIMIT_SPLIT = 178


def eng_dir() -> Path:
    return (
        SCRIPT_DIR.parent
        / "data_mods"
        / "custom_options"
        / "select_music_option_lang_eng_v3_ifs"
        / "tex"
    )


def lang_dir(code: str) -> Path:
    return (
        SCRIPT_DIR.parent
        / "data_mods"
        / "custom_options"
        / f"select_music_option_lang_{code}_v3_ifs"
        / "tex"
    )


def row_mask(rgba: np.ndarray, xlim: int) -> np.ndarray:
    """Boolean per-row ink mask of the text region (alpha > 40)."""
    return rgba[:, :xlim, 3].max(axis=1) > 40


def dilate(mask: np.ndarray, r: int = 2) -> np.ndarray:
    out = mask.copy()
    for shift in range(1, r + 1):
        out[:-shift] |= mask[shift:]
        out[shift:] |= mask[:-shift]
    return out


def text_layout_matches(ref: np.ndarray, new: np.ndarray, xlim: int) -> list[str]:
    """Compare text layout between shipped and regenerated pixels.

    1. Row structure: the per-row ink masks, dilated by 2 rows, must be
       identical — tolerates +-2 rows of antialiasing jitter at band edges
       while catching any added/removed/moved line (a line is ~13 rows).
    2. Right edges: within each shared dilated band, the rightmost ink
       column must match within 3 px — catches a changed line break that
       kept the same row structure (a moved word shifts an edge by a word
       width).
    Returns a list of failure descriptions (empty = match).
    """
    problems = []
    rm, nm = row_mask(ref, xlim), row_mask(new, xlim)
    # Mutual containment under 2-row dilation: every shipped ink row must be
    # within 2 rows of regenerated ink and vice versa (tolerates rasterizer
    # jitter at band edges; an added/removed/moved LINE is ~13 rows and
    # cannot hide).
    if (rm & ~dilate(nm)).any() or (nm & ~dilate(rm)).any():
        problems.append(
            f"row structure differs (ref rows {np.where(rm)[0].min()}-"
            f"{np.where(rm)[0].max()}, new rows {np.where(nm)[0].min()}-"
            f"{np.where(nm)[0].max()})"
        )
        return problems
    # shared bands from the union mask
    rows = np.where(dilate(rm) | dilate(nm))[0]
    bands = []
    start = prev = int(rows[0])
    for r in rows[1:]:
        r = int(r)
        if r - prev > 1:
            bands.append((start, prev))
            start = r
        prev = r
    bands.append((start, prev))
    for y0, y1 in bands:
        ref_cols = np.where(ref[y0 : y1 + 1, :xlim, 3].max(axis=0) > 40)[0]
        new_cols = np.where(new[y0 : y1 + 1, :xlim, 3].max(axis=0) > 40)[0]
        re = int(ref_cols.max()) if len(ref_cols) else -1
        ne = int(new_cols.max()) if len(new_cols) else -1
        if abs(re - ne) > 3:
            problems.append(f"right edge differs in rows {y0}-{y1}: ref {re} new {ne}")
    return problems


CONTENT_FIXES = {"seop_image_customize_character_p2_TEMPLATE.png"}


def check(reference: Path) -> int:
    failures = 0
    gen = eng_dir()

    def load(p: Path) -> np.ndarray:
        return np.array(Image.open(p).convert("RGBA"))

    # 1) take-over preview panels: text layout + art region (layout/art from
    #    the specs, not pixel sniffing — WIDE text legitimately crosses the
    #    divider zone)
    specs = {
        f"seop_image_{p.option}_{p.value}.png": p
        for p in PREVIEWS
        if p.value is not None
    }
    for name in TAKEOVER_PREVIEWS:
        ref = load(reference / name)
        new = load(gen / name)
        spec = specs[name]
        is_split = spec.layout == "split"
        xlim = TEXT_X_LIMIT_SPLIT if is_split else ref.shape[1]
        for problem in text_layout_matches(ref, new, xlim):
            print(f"FAIL {name}: {problem}")
            failures += 1
        if spec.art is not None:
            # art region: alpha-visible pixels byte-equal (crop origin came
            # from these very files)
            ra, na = ref[:, 186:], new[:, 186:]
            mask = (ra[:, :, 3] > 0) | (na[:, :, 3] > 0)
            if not np.array_equal(ra[mask], na[mask]):
                print(f"FAIL {name}: art region differs")
                failures += 1

    # 2) templates: text bands (left of divider) + marker byte-equality +
    #    cross-language marker identity
    for tid, spec in TEMPLATES.items():
        name = f"seop_image_{tid}_TEMPLATE.png"
        ref = load(reference / name)
        new = load(gen / name)
        if name in CONTENT_FIXES:
            # content deliberately changed — structural check only
            rm, nm = row_mask(ref, TEXT_X_LIMIT_SPLIT), row_mask(new, TEXT_X_LIMIT_SPLIT)
            if not (len(np.where(rm)[0]) and len(np.where(nm)[0])):
                print(f"FAIL {name}: text missing after content fix")
                failures += 1
            elif abs(int(np.where(rm)[0].min()) - int(np.where(nm)[0].min())) > 2:
                print(f"FAIL {name}: text misplaced after content fix")
                failures += 1
        else:
            for problem in text_layout_matches(ref, new, TEXT_X_LIMIT_SPLIT):
                print(f"FAIL {name}: {problem}")
                failures += 1
        for x, y, w, h, rgba in spec.markers:
            want = np.array(rgba, dtype=np.uint8)
            region = new[y : y + h, x : x + w]
            if not (region == want).all():
                print(f"FAIL {name}: marker at {x},{y} not solid {tuple(rgba)}")
                failures += 1
            ref_region = ref[y : y + h, x : x + w]
            if not (ref_region == want).all():
                print(f"FAIL {name}: shipped original lacks marker at {x},{y}?!")
                failures += 1
        for code in ["eng", "jpn", "kor"]:
            p = lang_dir(code) / name
            if not p.exists():
                continue
            other = load(p)
            for x, y, w, h, rgba in spec.markers:
                if not np.array_equal(
                    other[y : y + h, x : x + w],
                    new[y : y + h, x : x + w],
                ):
                    print(f"FAIL {name}: marker differs between eng and {code}")
                    failures += 1

    # 3) verbatim copies byte-equal to the shipped originals
    for name in VERBATIM:
        if (reference / name).read_bytes() != (gen / name).read_bytes():
            print(f"FAIL {name}: verbatim copy differs from shipped original")
            failures += 1

    if failures == 0:
        n = len(TAKEOVER_PREVIEWS) + len(TEMPLATES) + len(VERBATIM)
        print(f"OK: all {n} take-over files reproduce the shipped layout")
    return failures


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--reference",
        required=True,
        type=Path,
        help="Directory holding the shipped hand-authored PNGs to compare against",
    )
    args = parser.parse_args()
    sys.exit(1 if check(args.reference) else 0)


if __name__ == "__main__":
    main()
