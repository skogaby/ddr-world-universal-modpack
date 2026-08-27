# Font Research: CJK coverage for options texture localization

Date: 2026-08-17 (D3 investigation)

## Game font dump — KBF code-table analysis

The maintainer dumped the game's 7 bitmap fonts (KBF + DDS atlas pairs) to
`data/font/`. `scripts/kbf_to_font.py` (existing) converts them to sbix-strike
TTFs; the converted copies already in `scripts/fonts/` match the dump.

Direct parse of every `.kbf` code table (charcode field of each 16-byte
CharaInfo record):

| Font | Glyphs | ASCII | Kana | CJK kanji | Hangul (any block) | Fullwidth |
|---|---|---|---|---|---|---|
| 2d_font_ark_system | 7484 | 95 | 177 | 6682 | **0** | 163 |
| 2d_font_player | 219 | 95 | 0 | 0 | **0** | 0 |
| 2d_font_rival | 198 | 95 | 0 | 0 | **0** | 0 |
| 2d_font_songtitle_m | 7619 | 95 | 177 | 6692 | **0** | 163 |
| 2d_font_songtitle_s | 7625 | 95 | 177 | 6694 | **0** | 163 |
| 2d_font_system | 7572 | 95 | 177 | 6682 | **0** | 163 |
| 2d_font_ui | 7484 | 95 | 177 | 6682 | **0** | 163 |

Checked Hangul Syllables (U+AC00–D7A3), Hangul Jamo (U+1100–11FF), and Hangul
Compatibility Jamo (U+3130–318F): **zero glyphs in every font.** The game's
Korean UI text must come from somewhere else entirely (likely pre-rendered
into the kor texture atlases / a Korean build asset not in this dump).

⇒ The game-font route cannot cover Korean, full stop.

## Game-font (2d_font_ui) practical limits for JP

- Bitmap-only: `sbix` strike at exactly 17px; no outlines. Pillow renders it
  only at size 17 and only with `embedded_color=True` (glyphs are white+alpha
  PNGs; recolor via the alpha channel).
- **Several ASCII punctuation glyphs are broken in the conversion** — `(`,
  `)`, `%` render as "broken file" (empty sbix records); fullwidth `（％）`
  works. Mixed Latin/JP strings would need fullwidth substitution or fixes to
  `kbf_to_font.py`.
- 17px glyphs in a 16px label canvas / 16px preview line pitch: tight but
  workable for labels; body copy at 17px fits fewer chars/line than Sen 13.5
  and lines nearly touch.
- Upside: it IS the game's own UI face — exact style match for JP, pixel-crisp.

## Noto Sans JP / KR

- Sources: `google/fonts` repo, `ofl/notosansjp/NotoSansJP[wght].ttf` and
  `ofl/notosanskr/NotoSansKR[wght].ttf` — variable fonts (wght 100–900), SIL
  OFL 1.1 (redistributable in-repo). Pillow loads them and applies weight via
  `set_variation_by_axes([w])`.
- Static-instance TTFs (e.g. `NotoSansJP-SemiBold.ttf`) are also available if
  we prefer to commit fixed weights over variable files (~9.5 MB JP / ~10.4 MB
  KR each for the variable files; statics are similar since CJK glyf data
  dominates).
- Rendered cleanly at 16 / 13.5 px in all sampled weights (500/600/700) for
  both JP and KR, ASCII punctuation included.

## Comparison sheet

`font_comparison.png` (this directory): per-row — ENG current fonts, Noto JP
w500/600/700, game font 2d_font_ui, Noto KR w500/600/700; each row shows two
176x16 labels (on the light row chip), the 352x16 header bar, a 132x24 teal
ribbon, and a 368x172 preview panel with 4 lines of body copy at 2x zoom.

Sample strings used (draft translations):
- Label: 曲の再生速度 (%) / 곡 재생 속도 (%) ; オートプレイ / 자동 플레이
- Ribbon: フルスクリーン / 전체 화면
- Header: パワーユーザー設定 / 파워 유저 설정
- Preview body: assist-tick ON copy (4 lines)

## Assessment (for D3)

- Korean: Noto Sans KR is effectively the only candidate on the table.
- Japanese: Noto Sans JP w600 for labels/ribbons/headers and w500 for preview
  body sits visually close to the current Inclusive Sans SemiBold / Sen pair;
  the game font is an exact style match but adds fullwidth-punctuation
  workarounds and a second rendering path (embedded_color + recolor +
  17px-only) for one language.
- Using the same family pair for JP and KR keeps the three language sets
  visually consistent and the generator single-pathed.
