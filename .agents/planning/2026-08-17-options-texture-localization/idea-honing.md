# Idea Honing: Options Texture Localization (JPN/KOR)

Decision register for translating all injected custom-options textures to
Japanese and Korean, after first unifying all texture generation into
`scripts/gen_option_labels.py`. See `research/orientation.md` for the findings
behind each decision.

| ID | Decision | Why it matters | Recommendation | Status |
|----|----------|----------------|----------------|--------|
| D1 | DLL scope: per-language atlas build | jpn/kor textures don't function without it | Parameterize `asset_gen.rs` + `preview_gen.rs`; build all 3 languages unconditionally at init (disk-cached), fail-open per language | Accepted |
| D2 | Translation authorship & review | User-visible text in two languages the maintainer may not read | Agent-authored JA/KO, full string tables presented for review as a design appendix; stock-DDR style conventions | Accepted |
| D3 | JP/KR fonts | No current font covers CJK; licensing; visual match | Noto Sans JP + Noto Sans KR (OFL): w600 labels/ribbons/headers, w500 preview body. Game fonts ruled out for KR (zero Hangul in all 7 dumps); final sheet `research/font_final_choice.png` approved by maintainer | Accepted |
| D4 | Which textures get translated vs copied verbatim | Defines the full work surface | Translate labels/headers/ribbons/previews/templates; copy `seop_return` verbatim; keep `seop_tab_title_mods` "Modpack" wordmark Latin in all languages | Accepted |
| D5 | Hand-authored art take-over method | Unification requirement (part 1 of the ask) | Crop baked right-side art into `scripts/templates/*.png`; script composites art + divider + rendered text; ENG output must match shipped originals' layout (line breaks + baselines), not byte-identical | Accepted |
| D6 | Customize TEMPLATE generation | preview_gen's marker-rect contract must survive | Script generates all 9 `*_TEMPLATE.png` per language: translated baked text + marker rects at byte-identical coordinates/colors from one shared geometry table | Accepted |
| D7 | String table structure in the script | Maintainability of ~75 strings × 3 languages | Separate `scripts/option_strings.py` module: per-id dict with `en`/`ja`/`ko` fields; generator loops languages | Accepted |
| D8 | Translation style conventions | JP/KR game-UI text has strong genre conventions | Match stock DDR: katakana loans + Latin terms retained (STEP ZONE, ON/OFF, CSV, %), polite-neutral JP (です/ます), plain KR (-습니다 avoided in labels, used in body copy) | Accepted |
| D9 | Language build failure behavior | Cabinet robustness (missing jpn/kor ARC on some data dumps) | Fail-open per language: missing stock ARC/donor ⇒ WARN once, skip that language, others unaffected | Assumed |
| D10 | CJK label condensing | Overlong strings squeeze horizontally today | Keep existing horizontal-condense logic unchanged for all languages | Assumed |
| D11 | Repo layout of generated output | 3× texture sets in git | Commit all three `select_music_option_lang_{eng,jpn,kor}_v3_ifs/tex/` dirs, same as eng today | Assumed |
| D12 | Out of scope | Keep the feature bounded | `folder_expansion`'s own lang_eng constants, series/folder operator textures, mod-overlay-menu text (DLL-rendered), any in-game language *detection* | Assumed |

---

## D1 — DLL scope: per-language atlas build

**Question:** The ask framed this as script work, but `asset_gen.rs:40-43`
hardcodes the eng ARC/IFS/mod-path and `preview_gen.rs:49` hardcodes the eng
tex dir. Without DLL changes the jpn/kor folders are seen by LayeredFS but the
`seop_*` textures ride the auto-inject path (wrong UVs) and render broken.
How should the DLL build the per-language atlases?

**Recommendation:** Loop `["eng","jpn","kor"]` in `flush_label_atlas` (and the
preview chrome generation), building all three languages' atlases at init.
The build is already disk-cached (`generate_cloned_atlases_cached`), so warm
boots pay nothing; the game only ever opens the active language's IFS, so
runtime cost is unchanged. Rejected: reactive build keyed on which IFS the game
opens (timing-fragile — merged texturelist must exist before the open) and a
config knob for cabinet language (operator burden, and switching in test menu
would desync).

## D2 — Translation authorship & review

**Question:** Who writes and who checks the JA/KO strings?

**Recommendation:** Agent-authored, but the complete string table ships in the
design document for maintainer review before any texture is generated —
translations are the highest-blast-radius *content* in this feature even
though they're technically trivial to change later (regenerate + redeploy).

**Outcome (2026-08-17):** Design Appendix A tables reviewed and approved by
the maintainer as-shipped content; a native-speaker review pass is planned as
a later follow-up (string edits then are regenerate-and-redeploy only).

## D3 — JP/KR fonts

**Question:** Which fonts render the CJK text?

**Investigation (2026-08-17, see `research/fonts.md` + `research/font_comparison.png`):**
The maintainer dumped all 7 game fonts (KBF+DDS) to `data/font/`. Code-table
parse: **zero Hangul glyphs in every game font** (the JP-capable ones carry
ASCII + kana + ~6.7k kanji + fullwidth forms only) — the game-font route
cannot cover Korean. For JP, `2d_font_ui` is bitmap-locked to 17px, needs
`embedded_color` rendering + alpha recolor, and its converted ASCII `(` `)`
`%` glyphs are broken (fullwidth `（％）` works). Noto Sans JP/KR variable
fonts (OFL) render cleanly at all needed sizes/weights.

**Recommendation:** Noto Sans JP + Noto Sans KR — w600 for labels/ribbons/
headers, w500 for preview body copy. Pending maintainer's visual comparison
against the sample sheet (which includes a game-font JP row for reference).

**Additional context from maintainer:** cabinet supports per-user language
selection (multi-language testing is easy); stock jpn/kor ARCs confirmed
present in the game data (install root available locally via the
`DDR_WORLD_INSTALL` environment variable for offline verification).

## D4 — Translate vs copy

- **Translate:** 34 row labels, 4 headers, 4 ribbons, ~40 preview panels
  (script-generated + hand-authored take-overs), 9 customize TEMPLATEs.
- **Copy verbatim into jpn/kor dirs:** `seop_return.png` (icon, no text).
- **Keep Latin:** `seop_tab_title_mods.png` — "Modpack" is a brand wordmark
  with custom styling (teal + outline + floppy icon); stock DDR keeps such
  wordmarks Latin across languages. Copy verbatim. (If desired later, a
  styled-wordmark generator is a separable follow-up.)

## D5 — Hand-authored art take-over

The right-side art of the SPLIT panels (`premium_free` stage list,
`center_arrows_1p` screenshots ×2, `customize_movie_size` screenshots ×3) is
cropped once from the shipped PNGs into `scripts/templates/` and composited by
the script at the original coordinates. Text sides are re-rendered from
strings; the script's preview metrics were already fit to these exact originals
so ENG regeneration reproduces their layout. `autoplay_{on,off}` are text-only.
Acceptance: identical line breaks and baselines; not byte-identical.

## D6 — Customize TEMPLATEs

The 9 `seop_image_customize_*_TEMPLATE.png` files carry baked English text +
a solid green marker rectangle that `preview_gen.rs` parses at runtime for art
placement. The script takes over generating these: one shared geometry table
(marker rect x/y/w/h per option id, measured from the shipped templates) +
translated text per language. Marker rect coordinates and exact RGBA are
byte-identical across languages so the DLL's marker parsing behaves identically
regardless of language.

## D7 — String table structure

`scripts/option_strings.py`: a data-only module the generator imports.
Per-family dicts keyed by id, each entry `{"en": ..., "ja": ..., "ko": ...}`.
Rationale: 3 languages inline in the existing tuple lists would bloat
`gen_option_labels.py` past readability; a separate data module keeps the
generator logic diffable and the strings reviewable in one place.

## D8 — Style conventions

- JP: katakana loanwords where stock DDR uses them (オートプレイ, プレミアムフリー…),
  keep STEP ZONE / ON / OFF / CSV / % / P1 / P2 in Latin, polite body copy.
- KR: standard game-UI Korean, Latin retained for the same technical terms.
- Numerals stay Arabic everywhere.

---

**Readiness Confirmed 2026-08-17** — maintainer approved proceeding to design
with all decisions Accepted/Assumed (none Open). Design to include the full
JA/KO string tables as a reviewable appendix.

## Workflow notes (maintainer-set constraints)

- **No agent-run git operations** — the maintainer handles all commits/pushes.
- Multi-language cabinet testing is available (per-user language selection in
  the game itself); stock `select_music_option_lang_{jpn,kor}_v3.arc` confirmed
  present in the maintainer's game data (`DDR_WORLD_INSTALL` env var points at
  the install for local verification).

*(Question/answer/rationale detail for accepted decisions is appended as they
are accepted.)*
