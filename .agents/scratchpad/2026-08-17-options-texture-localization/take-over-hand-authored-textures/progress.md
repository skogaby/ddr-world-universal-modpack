# Progress: take-over-hand-authored-textures

Updated: 2026-08-17
Status: Complete (uncommitted — maintainer commits)

## Checklist

- [x] Measurements: divider ink at x=183-184; shipped SPLIT panels' divider
      ALPHA byte-matches `scripts/templates/seop_image_split_divider.png`
      (RGB-under-transparent junk only); template markers all solid rects;
      template text on the standard baseline-25/pitch-16 grid
- [x] Extraction → `scripts/templates/`: 7 art crops (`*_art.png`, bbox of
      ink at x≥186, RGB-under-zero-alpha normalized) + 2 verbatim masters
      (`seop_return_master.png`, `seop_tab_title_mods_master.png`)
- [x] `option_strings.py`: 9 take-over PreviewSpecs (autoplay WIDE text-only;
      7 SPLIT with art/art_pos); `TemplateSpec` reshaped to explicit LINES
      (hand-chosen breaks narrower than the wrap width — pre-broken per
      language rather than auto-wrapped); 9 TEMPLATES entries with measured
      markers; `VERBATIM_COPIES`; character_p2 left→right fix
- [x] Generator: SPLIT art compositing in `render_preview`;
      `render_template` (divider + lines + exact solid marker fills);
      verbatim-copy family
- [x] Harness `scripts/check_option_takeover.py` (committed, reusable via
      `--reference`): text layout via mutual-containment of 2-row-dilated
      ink-row masks + per-band right edges (±3px); art region byte-equal on
      visible pixels; markers solid + byte-equal + cross-language identical;
      verbatim copies byte-equal. **OK: all 20 take-over files**
- [x] No regression: 71 previously-generated files byte-identical to the
      Step 2 baseline
- [x] Fresh-dir regeneration: full 91-file English set, byte-identical to
      the in-place run
- [x] Maintainer visual sheet: `takeover_diff_sheet.png` (this directory,
      SHIPPED vs REGEN for all 20 files)

## Deviations

- TemplateSpec uses explicit per-language LINES instead of auto-wrapped
  paragraphs (design said "translated baked text"; the shipped templates'
  hand-chosen breaks are narrower than any consistent wrap width — explicit
  lines reproduce EN faithfully and give JA/KO deterministic breaks).
  Conservative, recorded here; does not contradict the approved design's
  interfaces.
- Harness evolved from naive band-list comparison to dilated-mask mutual
  containment after two rounds of false positives from ±1px antialiasing
  jitter (visual side-by-side confirmed the renders were identical before
  the harness was adjusted; the final version still catches added/removed/
  moved lines and changed breaks).
- NO COMMIT per session instruction.

## Reference

- `shipped_reference/` (this directory): the 91 shipped eng PNGs snapshotted
  before the take-over overwrote them — the harness baseline.

Status: Complete (uncommitted — maintainer commits)
