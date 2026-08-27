# Stock jpn/kor Options IFS Verification

Date: 2026-08-17. Source: maintainer's game install (`DDR_WORLD_INSTALL` env
var), `data/arc/bm2d/select_music_option_lang_{eng,jpn,kor}_v3.arc`, extracted
with `scripts/unpack_arc.py` + `ifstools`.

## Donor availability — CONFIRMED

All donor textures used by `asset_gen.rs` exist in **all three** language
IFSes with **identical dimensions**:

| Donor | eng | jpn | kor |
|---|---|---|---|
| `seop_item_appearance` (row-label donor) | 176x16 | 176x16 | 176x16 |
| `seop_op_on` (ribbon donor) | 132x24 | 132x24 | 132x24 |
| `seop_image_scroll_speed` (preview donor) | 368x172 | 368x172 | 368x172 |
| `seop_tab_title_basic` (tab-title donor) | 124x30 | 124x30 | 124x30 |
| `seop_return` (stock-name replacement target) | 72x36 | 72x36 | 72x36 |

- `texturelist.xml` present in all three (per-language donor-clone can read
  each ARC's own stock texturelist the same way it reads eng's today).
- PNG name sets: jpn == eng exactly (210 names). kor = eng + one extra
  `seop_text` (a Korean-specific pre-rendered text sheet — consistent with the
  font finding that no game font carries Hangul; irrelevant to this feature).

## Stock localization style (calibrates D8)

Sampled stock labels/ribbons/previews (see analysis below; samples rendered
during research):

- **Labels are fully translated**, not romanized: ARROW VISIBILITY →
  「矢印の見え方」/ 「화살표 표시 방식」; SCROLL SPEED → 「スクロールスピード」/
  「스크롤 속도」; ARROW START LANE PREVIEW → 「矢印の開始レーン予告」/
  「화살표 시작 레인 예고」. JP freely mixes native terms (矢印) with katakana
  loans (スクロールスピード, レーン).
- **ON/OFF ribbons stay Latin** in jpn and kor — matches D8's "keep Latin
  technical terms".
- **Arabic numerals inline** (スクロールの速さを10ずつ / 속도를 10씩) — no
  fullwidth digits.
- **Body copy register**: JP uses polite です/ます (〜変更できます);
  KR uses polite -ㅂ니다 (〜있습니다). Matches D8 exactly.
- KR labels use visible inter-word spacing; JP labels have none.

## Stock layout metrics — identical across languages

Measured ink-row bands of `seop_image_scroll_speed.png` (text half, x<178):

| lang | line-to-line pitch | paragraph-break pitch |
|---|---|---|
| eng | 16 px | 26 px |
| jpn | 16 px | 25 px |
| kor | 16 px | 26 px |

⇒ Konami keeps **one layout metric set for all three languages** (CJK glyph
ink runs ~11px tall inside the 16px pitch, same as Latin caps). Our generator
can therefore reuse its existing PREVIEW_* metrics unchanged for JP/KR; only
the wrap-width measurement (font.getlength) naturally differs per font.
(Note: our script's PREVIEW_PARAGRAPH_PITCH is 29 — fitted to *our* shipped
panels, which differ slightly from this stock panel's 25-26; not a conflict,
ours stays as-is for consistency with the already-shipped English set.)

## Implications for the design

1. Per-language donor-clone is a straight parameterization: same donor names,
   same dimensions, per-language ARC path / IFS name / mod path / atlas prefix
   (`copt_mods_lang_{eng,jpn,kor}`; the fresh preview atlas prefix
   `copt_prev_NNN` must also become language-distinct to avoid name collisions
   if more than one language's package is ever resident).
2. No per-language canvas/metric changes needed in the generator.
3. Translation style D8 is validated against stock precedent.
